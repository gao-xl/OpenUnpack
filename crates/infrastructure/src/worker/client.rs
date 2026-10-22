use openunpack_protocol::{WorkerEvent, WorkerRequest};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, atomic::AtomicBool},
};
static UNCONFIRMED: OnceLock<Mutex<Vec<(u32, u64)>>> = OnceLock::new();
static DIAGNOSTICS: OnceLock<Mutex<std::collections::VecDeque<WorkerDiagnostic>>> = OnceLock::new();
static EXIT_RECEIPTS: OnceLock<Mutex<std::collections::HashMap<String, WorkerExitReceipt>>> =
    OnceLock::new();
static RETAINED_RECEIPTS: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct WorkerExitReceipt {
    pub processes: Vec<(u32, u64)>,
    pub exit_confirmed: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerDiagnostic {
    pub kind: String,
    pub version: u32,
    pub operation: String,
    pub phase: String,
    pub error_code: String,
    pub exception_type: String,
    pub h_result: Option<i32>,
    pub native_code: Option<i32>,
}

fn parse_diagnostics(bytes: &[u8]) -> Vec<WorkerDiagnostic> {
    bytes
        .split(|byte| *byte == b'\n')
        .take(8)
        .filter_map(|line| {
            if line.len() > 512 {
                return None;
            }
            let record: WorkerDiagnostic = serde_json::from_slice(line).ok()?;
            if record.kind != "openunpackDiagnostic"
                || record.version != 1
                || !matches!(
                    record.operation.as_str(),
                    "startup" | "extract" | "inspect" | "probe" | "copy" | "readback" | "unknown"
                )
                || !matches!(
                    record.phase.as_str(),
                    "request"
                        | "metadata"
                        | "decode"
                        | "transfer"
                        | "sourceZone"
                        | "targetZone"
                        | "mitigation"
                        | "cleanup"
                        | "unknown"
                )
                || !matches!(
                    record.error_code.as_str(),
                    "ProtocolError"
                        | "ResourceLimit"
                        | "Unsupported"
                        | "Corrupt"
                        | "MissingVolume"
                        | "UnsafePath"
                        | "IoError"
                        | "Cancelled"
                        | "HardeningFailed"
                        | "HardeningUnavailable"
                        | "NeedsPassword"
                        | "PasswordMismatchOrCorrupt"
                )
                || record.exception_type.len() > 64
                || !record
                    .exception_type
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return None;
            }
            Some(record)
        })
        .collect()
}

#[derive(Clone)]
struct OperationIdentity {
    version: u32,
    request_id: String,
    operation_id: Option<String>,
    generation: Option<u64>,
}
impl OperationIdentity {
    fn accepts_ready(&self, event: &WorkerEvent) -> bool {
        if event.protocol_version != 1 || event.kind != "ready" || !event.request_id.is_empty() {
            return false;
        }
        self.version == 1
            || event.hello.as_ref().is_some_and(|hello| {
                hello.protocol_version == 2
                    && hello.major == 2
                    && hello.capabilities.backend_id == "7-Zip"
                    && hello.capabilities.max_batch_candidates == 128
                    && !hello.capabilities.extract_with_candidates
                    && !hello.capabilities.solid_optimization
            })
    }
    fn matches(&self, event: &WorkerEvent) -> bool {
        event.protocol_version == self.version
            && event.request_id == self.request_id
            && event.hello.is_none()
            && (self.version == 1
                || (event.operation_id == self.operation_id
                    && event.generation == self.generation
                    && event.matched_password.is_none()))
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn diagnostics_reject_credentials_and_unbounded_or_unrecognised_output() {
        let valid = br#"{"kind":"openunpackDiagnostic","version":1,"operation":"inspect","phase":"metadata","errorCode":"ResourceLimit","exceptionType":"ArchiveException","hResult":-1,"nativeCode":null}"#;
        assert_eq!(parse_diagnostics(valid).len(), 1);
        let mut leaked: serde_json::Value = serde_json::from_slice(valid).unwrap();
        leaked["password"] = "synthetic-secret".into();
        assert!(parse_diagnostics(&serde_json::to_vec(&leaked).unwrap()).is_empty());
        leaked.as_object_mut().unwrap().remove("password");
        leaked["operation"] = r"C:\private\synthetic-secret".into();
        assert!(parse_diagnostics(&serde_json::to_vec(&leaked).unwrap()).is_empty());
        assert!(parse_diagnostics(&vec![b'x'; 513]).is_empty());
        let repeated = (0..20)
            .flat_map(|_| valid.iter().copied().chain([b'\n']))
            .collect::<Vec<_>>();
        assert_eq!(parse_diagnostics(&repeated).len(), 8);
    }
    #[test]
    fn v2_requires_truthful_hello_and_keeps_v1_ready_compatibility() {
        let identity = OperationIdentity {
            version: 2,
            request_id: "request".into(),
            operation_id: Some("operation".into()),
            generation: Some(3),
        };
        let mut ready = WorkerEvent::default();
        ready.kind = "ready".into();
        assert!(!identity.accepts_ready(&ready));
        assert!(
            OperationIdentity {
                version: 1,
                ..identity.clone()
            }
            .accepts_ready(&ready)
        );
        ready.hello = Some(openunpack_protocol::worker::WorkerHello::default());
        assert!(identity.accepts_ready(&ready));
        ready.hello.as_mut().unwrap().major = 3;
        assert!(!identity.accepts_ready(&ready));
        ready.hello.as_mut().unwrap().major = 2;
        ready
            .hello
            .as_mut()
            .unwrap()
            .capabilities
            .extract_with_candidates = true;
        assert!(!identity.accepts_ready(&ready));
        ready
            .hello
            .as_mut()
            .unwrap()
            .capabilities
            .extract_with_candidates = false;
        ready.request_id = "unexpected".into();
        assert!(!identity.accepts_ready(&ready));
    }
    #[test]
    fn v2_rejects_late_generations_operations_and_returned_secrets() {
        let identity = OperationIdentity {
            version: 2,
            request_id: "request".into(),
            operation_id: Some("operation".into()),
            generation: Some(3),
        };
        let mut event = WorkerEvent::default();
        event.protocol_version = 2;
        event.request_id = "request".into();
        event.operation_id = Some("operation".into());
        event.generation = Some(3);
        event.kind = "completed".into();
        assert!(identity.matches(&event));
        let mut older = event.clone();
        older.generation = Some(2);
        assert!(!identity.matches(&older));
        let mut abandoned = event.clone();
        abandoned.operation_id = Some("old-operation".into());
        assert!(!identity.matches(&abandoned));
        let mut echoed = event.clone();
        echoed.matched_password = Some("synthetic".into());
        assert!(!identity.matches(&echoed));
        event.protocol_version = 1;
        assert!(!identity.matches(&event));
    }
}

#[derive(Clone)]
pub struct WorkerClient {
    pub executable: PathBuf,
    pub engine: PathBuf,
}

/// Admission notifications are local Host state, never Worker protocol events.
pub type ResourceWaitObserver = Arc<dyn Fn(bool) + Send + Sync>;

impl WorkerClient {
    pub fn register_no_process(&self, request_id: &str) -> Result<(), String> {
        self.register_receipt(request_id, false)
    }
    pub fn retain_request_receipt(&self, request_id: &str) -> Result<(), String> {
        self.register_receipt(request_id, true)
    }
    fn register_receipt(&self, request_id: &str, retain: bool) -> Result<(), String> {
        if request_id.is_empty() || request_id.len() > 200 {
            return Err("WorkerProtocolError".into());
        }
        {
            let mut receipts = EXIT_RECEIPTS
                .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut retained = RETAINED_RECEIPTS
                .get_or_init(|| Mutex::new(std::collections::HashSet::new()))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if receipts
                .get(request_id)
                .is_some_and(|receipt| !receipt.exit_confirmed)
            {
                return Err("WorkerProtocolError".into());
            }
            if receipts.len() >= 256 {
                receipts.retain(|id, receipt| !receipt.exit_confirmed || retained.contains(id));
            }
            if receipts.len() >= 256 {
                return Err("ResourceLimit".into());
            }
            receipts.insert(
                request_id.to_string(),
                WorkerExitReceipt {
                    processes: vec![],
                    exit_confirmed: true,
                },
            );
            if retain {
                retained.insert(request_id.to_string());
            }
        }
        Ok(())
    }

    pub fn request_exit_receipt(&self, request_id: &str) -> Option<WorkerExitReceipt> {
        let mut receipts = EXIT_RECEIPTS
            .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let receipt = receipts.get_mut(request_id)?;
        if !receipt.exit_confirmed
            && !receipt.processes.is_empty()
            && receipt
                .processes
                .iter()
                .all(|(pid, created)| process_identity_exited(*pid, *created).unwrap_or(false))
        {
            receipt.exit_confirmed = true;
        }
        Some(receipt.clone())
    }
    pub fn forget_confirmed_request(&self, request_id: &str) {
        let mut receipts = EXIT_RECEIPTS
            .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if receipts
            .get(request_id)
            .is_some_and(|receipt| receipt.exit_confirmed)
        {
            receipts.remove(request_id);
            RETAINED_RECEIPTS
                .get_or_init(|| Mutex::new(std::collections::HashSet::new()))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(request_id);
        }
    }
    pub fn recent_diagnostics(&self) -> Vec<WorkerDiagnostic> {
        DIAGNOSTICS
            .get_or_init(|| Mutex::new(std::collections::VecDeque::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
    pub async fn preview_output_rules(
        &self,
        paths: &[String],
        patterns: &[String],
    ) -> Result<Vec<bool>, String> {
        self.preview_output_rules_with_resources(
            paths,
            patterns,
            Arc::new(AtomicBool::new(false)),
            1,
            None,
        )
        .await
    }

    pub async fn preview_output_rules_with_resources(
        &self,
        paths: &[String],
        patterns: &[String],
        cancel: Arc<AtomicBool>,
        concurrency: u32,
        on_wait: Option<ResourceWaitObserver>,
    ) -> Result<Vec<bool>, String> {
        if paths.len() > 1000
            || patterns.len() > 100
            || paths.iter().any(|path| path.len() > 32768)
            || patterns.iter().any(|pattern| pattern.len() > 512)
        {
            return Err("RulePreviewLimit".into());
        }
        let bytes = serde_json::to_vec(&serde_json::json!({"paths": paths, "patterns": patterns}))
            .map_err(|_| "RulePreviewInvalid")?;
        if bytes.len() > 1024 * 1024 {
            return Err("RulePreviewLimit".into());
        }
        #[cfg(windows)]
        {
            windows::preview_output_rules(self, bytes, paths.len(), cancel, concurrency, on_wait)
                .await
        }
        #[cfg(not(windows))]
        {
            let _ = (bytes, cancel, concurrency, on_wait);
            Err("WorkerUnsupportedPlatform".into())
        }
    }
    // A poisoned lock only means another thread panicked while holding it; the registry is
    // plain data, so serve its real contents instead of inventing a (0,0) sentinel process.
    pub fn unconfirmed_processes(&self) -> Vec<(u32, u64)> {
        let mut registry = UNCONFIRMED
            .get_or_init(|| Mutex::new(vec![]))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        registry.retain(|(pid, created)| !process_identity_exited(*pid, *created).unwrap_or(false));
        (*registry).clone()
    }
    pub async fn run(
        &self,
        request: WorkerRequest,
        cancel: Arc<AtomicBool>,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    ) -> Result<WorkerEvent, String> {
        self.run_with_resources(request, cancel, progress, 1, None)
            .await
    }

    pub async fn run_with_resources(
        &self,
        request: WorkerRequest,
        cancel: Arc<AtomicBool>,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
        concurrency: u32,
        on_wait: Option<ResourceWaitObserver>,
    ) -> Result<WorkerEvent, String> {
        self.register_no_process(&request.request_id)?;
        #[cfg(windows)]
        {
            windows::run(self, request, cancel, progress, concurrency, on_wait).await
        }
        #[cfg(not(windows))]
        {
            let _ = (request, cancel, progress, concurrency, on_wait);
            Err("WorkerUnsupportedPlatform".into())
        }
    }
}

pub fn process_identity_exited(pid: u32, creation: u64) -> Result<bool, String> {
    #[cfg(windows)]
    {
        windows::process_identity_exited(pid, creation)
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, creation);
        Err("WorkerUnsupportedPlatform".into())
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        ffi::c_void, mem::size_of, os::windows::io::AsRawHandle, process::Stdio, ptr::null,
        sync::atomic::Ordering, time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::windows::named_pipe::{ClientOptions, NamedPipeClient},
        process::{Child, Command},
        sync::{mpsc, oneshot, watch},
        time::{Instant, interval, sleep, timeout},
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, GetLastError, HANDLE, WAIT_OBJECT_0,
            WAIT_TIMEOUT,
        },
        Storage::FileSystem::SECURITY_IDENTIFICATION,
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
                JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
                JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_JOB_MEMORY,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_UILIMIT_DESKTOP,
                JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
                JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES,
                JOB_OBJECT_UILIMIT_READCLIPBOARD, JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS,
                JOB_OBJECT_UILIMIT_WRITECLIPBOARD, JOBOBJECT_BASIC_UI_RESTRICTIONS,
                JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JobObjectBasicUIRestrictions, JobObjectCpuRateControlInformation,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject,
            },
            Pipes::GetNamedPipeServerProcessId,
            SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
            Threading::{
                CREATE_NO_WINDOW, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, SetPriorityClass, WaitForSingleObject,
            },
        },
    };
    use zeroize::{Zeroize, Zeroizing};

    const MAX_MESSAGE: usize = 8 * 1024 * 1024;
    const IDLE_PRIORITY_CLASS: u32 = 0x00000040;
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x00004000;
    const NORMAL_PRIORITY_CLASS: u32 = 0x00000020;

    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;
    const MIN_WORKER_MEMORY: u64 = 256 * MIB;
    // JobMemoryLimit is stored in pages. Round down before reserving so the
    // queried OS limit and accounting are identical on supported Windows x64.
    const MEMORY_GRANULARITY: u64 = 64 * 1024;
    const RESOURCE_POLL: Duration = Duration::from_millis(200);

    #[derive(Clone, Copy)]
    struct MemoryStatus {
        total_physical: u64,
        available_physical: u64,
        // Conservative current-process commit allowance, not system commit total.
        available_commit: u64,
    }

    fn memory_status() -> Result<MemoryStatus, String> {
        let mut status = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 || status.ullTotalPhys == 0 {
            return Err("WorkerResourceProbeUnavailable".into());
        }
        Ok(MemoryStatus {
            total_physical: status.ullTotalPhys,
            available_physical: status.ullAvailPhys,
            available_commit: status.ullAvailPageFile,
        })
    }

    type MemoryProbe = Arc<dyn Fn() -> Result<MemoryStatus, String> + Send + Sync>;
    type ExitProbe = Arc<dyn Fn(u32, u64) -> bool + Send + Sync>;

    #[derive(Default)]
    struct ResourceState {
        next_id: u64,
        waiters: std::collections::VecDeque<u64>,
        leases: std::collections::HashMap<u64, MemoryReservation>,
    }
    struct MemoryReservation {
        bytes: u64,
        identity: Option<(u32, u64)>,
        quarantined: bool,
    }
    struct ResourcePool {
        state: Mutex<ResourceState>,
        changed: tokio::sync::Notify,
        memory_probe: MemoryProbe,
        exit_probe: ExitProbe,
    }
    static RESOURCE_POOL: OnceLock<Arc<ResourcePool>> = OnceLock::new();

    impl ResourcePool {
        fn shared() -> Arc<Self> {
            RESOURCE_POOL
                .get_or_init(|| {
                    Self::new(
                        Arc::new(memory_status),
                        Arc::new(|pid, created| {
                            process_identity_exited(pid, created).unwrap_or(false)
                        }),
                    )
                })
                .clone()
        }
        fn new(memory_probe: MemoryProbe, exit_probe: ExitProbe) -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new(ResourceState::default()),
                changed: tokio::sync::Notify::new(),
                memory_probe,
                exit_probe,
            })
        }
        fn reap_confirmed_exits(&self) {
            // Identity queries cannot run while holding the admission mutex.
            let abandoned: Vec<_> = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .leases
                .iter()
                .filter_map(|(id, reservation)| {
                    reservation
                        .quarantined
                        .then_some(reservation.identity)
                        .flatten()
                        .map(|identity| (*id, identity))
                })
                .collect();
            let exited: Vec<_> = abandoned
                .into_iter()
                .filter(|(_, (pid, created))| (self.exit_probe)(*pid, *created))
                .collect();
            if exited.is_empty() {
                return;
            }
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (id, identity) in exited {
                if state
                    .leases
                    .get(&id)
                    .is_some_and(|lease| lease.quarantined && lease.identity == Some(identity))
                {
                    state.leases.remove(&id);
                }
            }
            drop(state);
            self.changed.notify_waiters();
        }
        async fn acquire(
            self: &Arc<Self>,
            concurrency: u32,
            cancel: &AtomicBool,
            on_wait: Option<ResourceWaitObserver>,
        ) -> Result<MemoryLease, String> {
            let mut ticket = ResourceTicket::new(self.clone())?;
            let mut notice = WaitNotice {
                observer: on_wait,
                waiting: false,
            };
            let mut probe_failures = 0;
            loop {
                if cancel.load(Ordering::Acquire) {
                    return Err("Cancelled".into());
                }
                self.reap_confirmed_exits();
                match (self.memory_probe)() {
                    Ok(memory) => {
                        probe_failures = 0;
                        let capacity = (memory.total_physical / 2).min(8 * GIB);
                        if capacity < MIN_WORKER_MEMORY {
                            return Err("ResourceLimit".into());
                        }
                        let parallelism =
                            concurrency.clamp(1, openunpack_protocol::MAX_CONCURRENT_JOBS);
                        let desired = (capacity / parallelism as u64)
                            .clamp(MIN_WORKER_MEMORY, 4 * GIB)
                            .min(capacity);
                        let headroom = (memory.total_physical / 10).max(512 * MIB);
                        let mut state = self
                            .state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let reserved = state
                            .leases
                            .values()
                            .fold(0u64, |sum, lease| sum.saturating_add(lease.bytes));
                        // Adapt only a new reservation. Requiring the whole
                        // nominal 4 GiB would strand default-one on an 8 GiB PC.
                        let bytes = desired
                            .min(capacity.saturating_sub(reserved))
                            .min(memory.available_physical.saturating_sub(headroom))
                            .min(memory.available_commit.saturating_sub(512 * MIB));
                        let bytes = bytes / MEMORY_GRANULARITY * MEMORY_GRANULARITY;
                        if state.waiters.front() == Some(&ticket.id)
                            && state.leases.len()
                                < openunpack_protocol::MAX_CONCURRENT_JOBS as usize
                            && bytes >= MIN_WORKER_MEMORY
                            && reserved.saturating_add(bytes) <= capacity
                            && memory.available_physical >= headroom.saturating_add(bytes)
                            && memory.available_commit >= (512 * MIB).saturating_add(bytes)
                        {
                            state.waiters.pop_front();
                            state.leases.insert(
                                ticket.id,
                                MemoryReservation {
                                    bytes,
                                    identity: None,
                                    quarantined: false,
                                },
                            );
                            ticket.queued = false;
                            drop(state);
                            self.changed.notify_waiters();
                            let lease = MemoryLease {
                                pool: self.clone(),
                                id: ticket.id,
                                bytes,
                                confirmed: false,
                            };
                            notice.set(false);
                            return Ok(lease);
                        }
                    }
                    Err(_) => {
                        probe_failures += 1;
                        if probe_failures >= 3 {
                            return Err("WorkerResourceProbeUnavailable".into());
                        }
                    }
                }
                notice.set(true);
                // The timer observes external memory recovery and cancellation,
                // including a lost notification immediately before this select.
                tokio::select! {
                    _ = self.changed.notified() => {},
                    _ = sleep(RESOURCE_POLL) => {},
                }
            }
        }
    }

    struct ResourceTicket {
        pool: Arc<ResourcePool>,
        id: u64,
        queued: bool,
    }
    impl ResourceTicket {
        fn new(pool: Arc<ResourcePool>) -> Result<Self, String> {
            let mut state = pool
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let id = state.next_id.checked_add(1).ok_or("ResourceLimit")?;
            state.next_id = id;
            state.waiters.push_back(id);
            drop(state);
            Ok(Self {
                pool,
                id,
                queued: true,
            })
        }
    }
    impl Drop for ResourceTicket {
        fn drop(&mut self) {
            if self.queued {
                self.pool
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .waiters
                    .retain(|id| *id != self.id);
                self.pool.changed.notify_waiters();
            }
        }
    }
    struct WaitNotice {
        observer: Option<ResourceWaitObserver>,
        waiting: bool,
    }
    impl WaitNotice {
        fn set(&mut self, waiting: bool) {
            if self.waiting != waiting {
                self.waiting = waiting;
                if let Some(observer) = &self.observer {
                    observer(waiting);
                }
            }
        }
    }
    impl Drop for WaitNotice {
        fn drop(&mut self) {
            self.set(false);
        }
    }
    struct MemoryLease {
        pool: Arc<ResourcePool>,
        id: u64,
        bytes: u64,
        confirmed: bool,
    }
    impl MemoryLease {
        fn bind(&mut self, identity: (u32, u64)) {
            if let Some(reservation) = self
                .pool
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .leases
                .get_mut(&self.id)
            {
                reservation.identity = Some(identity);
            }
        }
        fn confirm_exit(&mut self) {
            self.confirmed = true;
        }
    }
    impl Drop for MemoryLease {
        fn drop(&mut self) {
            let identity = self
                .pool
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .leases
                .get(&self.id)
                .and_then(|reservation| reservation.identity);
            let released = self.confirmed
                || identity.is_none()
                || identity.is_some_and(|(pid, created)| (self.pool.exit_probe)(pid, created));
            let mut state = self
                .pool
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if released {
                state.leases.remove(&self.id);
            } else if let Some(reservation) = state.leases.get_mut(&self.id) {
                // JobGuard close requests termination; it does not prove exit.
                reservation.quarantined = true;
            }
            drop(state);
            self.pool.changed.notify_waiters();
        }
    }

    fn resolve_priority_flags(priority: &str) -> u32 {
        match priority.to_ascii_lowercase().as_str() {
            "idle" => IDLE_PRIORITY_CLASS,
            "normal" => NORMAL_PRIORITY_CLASS,
            _ => BELOW_NORMAL_PRIORITY_CLASS,
        }
    }

    pub(super) async fn preview_output_rules(
        client: &WorkerClient,
        bytes: Vec<u8>,
        count: usize,
        cancel: Arc<AtomicBool>,
        concurrency: u32,
        on_wait: Option<ResourceWaitObserver>,
    ) -> Result<Vec<bool>, String> {
        if !client.executable.is_absolute() || !client.executable.is_file() {
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && let (Some(paths), Some(patterns)) =
                    (json["paths"].as_array(), json["patterns"].as_array())
            {
                let pat_strings: Vec<String> = patterns
                    .iter()
                    .filter_map(|p| p.as_str().map(String::from))
                    .collect();
                let res: Vec<bool> = paths
                    .iter()
                    .map(|p| {
                        let path_str = p.as_str().unwrap_or("");
                        crate::engine::matches_exclusion(path_str, &pat_strings)
                    })
                    .collect();
                if res.len() == count {
                    return Ok(res);
                }
            }
            return Err("WorkerUnavailable".into());
        }
        let mut lease = ResourcePool::shared()
            .acquire(concurrency, &cancel, on_wait)
            .await?;
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }
        let request_id = uuid::Uuid::new_v4().to_string();
        client.register_no_process(&request_id)?;
        let worker_dir = client
            .executable
            .parent()
            .unwrap_or(std::path::Path::new(r"C:\Windows"));
        // Async pipes let cancellation terminate a preview after admission too;
        // a timed-out blocking reader must not outlive its process ownership.
        let mut child = Command::new(&client.executable)
            .current_dir(worker_dir)
            .arg("--preview-output-rules")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "WorkerStartFailed")?;
        let mut exit_guard = ProcessExitGuard::new(&child, request_id);
        lease.bind(exit_guard.identity);
        let _job = match JobGuard::attach_limited(&child, lease.bytes) {
            Ok(job) => job,
            Err(error) => {
                confirm_exit(&mut child, true).await?;
                exit_guard.confirm();
                lease.confirm_exit();
                return Err(error);
            }
        };
        let mut result = Vec::new();
        let outcome = {
            let communicate = async {
                let mut stdin = child.stdin.take().ok_or("RulePreviewIoError")?;
                let stdout = child.stdout.take().ok_or("RulePreviewIoError")?;
                stdin
                    .write_all(&bytes)
                    .await
                    .map_err(|_| "RulePreviewIoError")?;
                stdin.shutdown().await.map_err(|_| "RulePreviewIoError")?;
                drop(stdin);
                stdout
                    .take(8193)
                    .read_to_end(&mut result)
                    .await
                    .map_err(|_| "RulePreviewIoError")?;
                if result.len() > 8192 {
                    return Err("RulePreviewLimit".to_string());
                }
                let status = child.wait().await.map_err(|_| "WorkerProcessUnavailable")?;
                if !status.success() {
                    return Err("RulePreviewInvalid".to_string());
                }
                Ok(())
            };
            tokio::select! {
                result = timeout(Duration::from_secs(10), communicate) => {
                    result.map_err(|_| "RulePreviewTimeout".to_string()).and_then(|r| r)
                },
                _ = wait_for_cancel(&cancel) => Err("Cancelled".into()),
            }
        };
        confirm_exit(&mut child, outcome.is_err()).await?;
        exit_guard.confirm();
        lease.confirm_exit();
        outcome?;
        if result.len() > 8192 {
            return Err("RulePreviewLimit".into());
        }
        let parsed: Vec<bool> =
            serde_json::from_slice(&result).map_err(|_| "RulePreviewInvalid")?;
        if parsed.len() != count {
            return Err("RulePreviewInvalid".into());
        }
        Ok(parsed)
    }

    async fn wait_for_cancel(cancel: &AtomicBool) {
        while !cancel.load(Ordering::Acquire) {
            sleep(RESOURCE_POLL).await;
        }
    }

    fn creation_time(handle: HANDLE) -> Result<u64, String> {
        unsafe {
            let mut created: FILETIME = std::mem::zeroed();
            let mut exit: FILETIME = std::mem::zeroed();
            let mut kernel: FILETIME = std::mem::zeroed();
            let mut user: FILETIME = std::mem::zeroed();
            if GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user) == 0 {
                return Err("WorkerProcessIdentityUnavailable".into());
            }
            Ok((created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64)
        }
    }
    pub(super) fn process_identity_exited(pid: u32, creation: u64) -> Result<bool, String> {
        if pid == 0 || creation == 0 {
            return Err("WorkerProcessIdentityUnavailable".into());
        }
        unsafe {
            let handle = OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            );
            if handle.is_null() {
                return if GetLastError() == ERROR_INVALID_PARAMETER {
                    Ok(true)
                } else {
                    Err("WorkerProcessIdentityUnavailable".into())
                };
            }
            let identity = creation_time(handle);
            let signal = WaitForSingleObject(handle, 0);
            CloseHandle(handle);
            match identity {
                Ok(actual) if actual != creation => Ok(true),
                Ok(_) => match signal {
                    WAIT_OBJECT_0 => Ok(true),
                    WAIT_TIMEOUT => Ok(false),
                    _ => Err("WorkerProcessIdentityUnavailable".into()),
                },
                Err(e) => Err(e),
            }
        }
    }

    struct JobGuard(HANDLE);
    // A job HANDLE can be used/closed from any thread. It owns no borrowed memory.
    unsafe impl Send for JobGuard {}
    impl Drop for JobGuard {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    impl JobGuard {
        fn attach_raw(process: HANDLE, memory_limit: u64) -> Result<Self, String> {
            unsafe {
                let handle = CreateJobObjectW(null(), null());
                if handle.is_null() {
                    return Err("WorkerJobObjectUnavailable".into());
                }
                let guard = Self(handle);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                    | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
                    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                    | JOB_OBJECT_LIMIT_JOB_MEMORY;
                // The engine is an in-process DLL. Disallow descendants so a
                // verified Worker exit also proves this entire Job is empty.
                limits.BasicLimitInformation.ActiveProcessLimit = 1;
                limits.JobMemoryLimit =
                    usize::try_from(memory_limit).map_err(|_| "WorkerJobObjectUnavailable")?;
                if SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) == 0
                {
                    return Err("WorkerJobObjectUnavailable".into());
                }
                let mut actual_limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                if QueryInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &mut actual_limits as *mut _ as *mut c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                ) == 0
                    || actual_limits.JobMemoryLimit != limits.JobMemoryLimit
                    || actual_limits.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_JOB_MEMORY
                        == 0
                    || actual_limits.BasicLimitInformation.LimitFlags
                        & JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                        == 0
                    || actual_limits.BasicLimitInformation.ActiveProcessLimit != 1
                {
                    return Err("WorkerJobObjectUnavailable".into());
                }
                // Each worker has a hard CPU ceiling as well as its memory and
                // elapsed-time budgets. Requested parallelism remains independent.
                let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = std::mem::zeroed();
                cpu.ControlFlags =
                    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
                cpu.Anonymous.CpuRate = 5000; // 50%, in hundredths of a percent
                if SetInformationJobObject(
                    handle,
                    JobObjectCpuRateControlInformation,
                    &cpu as *const _ as *const c_void,
                    size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                ) == 0
                {
                    return Err("WorkerJobObjectUnavailable".into());
                }
                let mut actual: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = std::mem::zeroed();
                if QueryInformationJobObject(
                    handle,
                    JobObjectCpuRateControlInformation,
                    &mut actual as *mut _ as *mut c_void,
                    size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                ) == 0
                    || actual.ControlFlags != cpu.ControlFlags
                    || actual.Anonymous.CpuRate != 5000
                {
                    return Err("WorkerJobObjectUnavailable".into());
                }
                let ui_restrictions = JOBOBJECT_BASIC_UI_RESTRICTIONS {
                    UIRestrictionsClass: JOB_OBJECT_UILIMIT_HANDLES
                        | JOB_OBJECT_UILIMIT_READCLIPBOARD
                        | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
                        | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
                        | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                        | JOB_OBJECT_UILIMIT_GLOBALATOMS
                        | JOB_OBJECT_UILIMIT_DESKTOP
                        | JOB_OBJECT_UILIMIT_EXITWINDOWS,
                };
                let _ = SetInformationJobObject(
                    handle,
                    JobObjectBasicUIRestrictions,
                    &ui_restrictions as *const _ as *const c_void,
                    size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
                );
                if AssignProcessToJobObject(handle, process) == 0 {
                    return Err("WorkerJobObjectUnavailable".into());
                }
                Ok(guard)
            }
        }
        fn attach_limited(child: &Child, memory_limit: u64) -> Result<Self, String> {
            match child.raw_handle() {
                Some(h) => Self::attach_raw(h as HANDLE, memory_limit),
                None => Err("WorkerJobObjectUnavailable".into()),
            }
        }
        #[cfg(test)]
        fn attach(child: &Child) -> Result<Self, String> {
            Self::attach_limited(child, 4 * GIB)
        }
    }

    pub(super) async fn run(
        client: &WorkerClient,
        mut request: WorkerRequest,
        cancel: Arc<AtomicBool>,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
        concurrency: u32,
        on_wait: Option<ResourceWaitObserver>,
    ) -> Result<WorkerEvent, String> {
        if !matches!(request.protocol_version, 1 | 2) || request.request_id.is_empty() {
            return Err("WorkerProtocolError".into());
        }
        let is_probe = request.operation == "probe";
        if request.protocol_version == 2 {
            request
                .operation_id
                .get_or_insert_with(|| uuid::Uuid::new_v4().to_string());
            request.generation.get_or_insert(1);
            if request
                .operation_id
                .as_ref()
                .is_none_or(|id| id.is_empty() || id.len() > 200)
                || request.generation == Some(0)
            {
                return Err("WorkerProtocolError".into());
            }
        }
        let identity = OperationIdentity {
            version: request.protocol_version,
            request_id: request.request_id.clone(),
            operation_id: request.operation_id.clone(),
            generation: request.generation,
        };
        let use_native_engine = std::env::var("OPENUNPACK_ENGINE").as_deref() == Ok("native")
            || !client.executable.is_file()
            || !client.engine.is_file();

        if use_native_engine {
            return crate::engine::execute_native_worker(request, progress, cancel).await;
        }
        let mut lease = ResourcePool::shared()
            .acquire(concurrency, &cancel, on_wait)
            .await?;
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }
        let prefix = format!("openunpack-{}", uuid::Uuid::new_v4());
        let priority_flag = resolve_priority_flags(&request.worker_priority);
        let worker_dir = client
            .executable
            .parent()
            .unwrap_or(std::path::Path::new(r"C:\Windows"));
        let mut child = Command::new(&client.executable)
            .current_dir(worker_dir)
            .arg("--pipe-prefix")
            .arg(&prefix)
            .arg("--parent-pid")
            .arg(std::process::id().to_string())
            .arg("--engine")
            .arg(&client.engine)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "WorkerStartFailed".to_string())?;
        // A dropped/panicking operation cannot await Child::wait. Keep the actual
        // process identity recoverable until exit has been observed separately.
        let mut exit_guard = ProcessExitGuard::new(&child, request.request_id.clone());
        lease.bind(exit_guard.identity);
        let mut diagnostic_reader = DiagnosticReader::start(&mut child);
        let _job = match JobGuard::attach_limited(&child, lease.bytes) {
            Ok(job) => job,
            Err(error) => {
                confirm_exit(&mut child, true).await?;
                exit_guard.confirm();
                lease.confirm_exit();
                return Err(error);
            }
        };
        let worker_pid = match child.id() {
            Some(pid) => pid,
            None => {
                confirm_exit(&mut child, true).await?;
                exit_guard.confirm();
                lease.confirm_exit();
                return Err("WorkerStartFailed".into());
            }
        };
        let serialized =
            serde_json::to_vec(&request).map_err(|_| "WorkerProtocolError".to_string());
        if let Some(secret) = request.password.as_mut() {
            secret.zeroize();
        }
        request.password = None;
        for secret in &mut request.candidate_passwords {
            secret.zeroize();
        }
        for candidate in &mut request.candidates {
            candidate.secret.zeroize();
        }
        let mut result = match serialized {
            Ok(bytes) if bytes.len() <= MAX_MESSAGE => {
                let bytes = Zeroizing::new(bytes);
                communicate(
                    &mut child,
                    &prefix,
                    worker_pid,
                    &identity,
                    bytes,
                    RunOptions {
                        priority_flag,
                        is_probe,
                        cancel,
                        progress,
                    },
                )
                .await
            }
            _ => Err("WorkerProtocolError".into()),
        };
        confirm_exit(&mut child, result.is_err()).await?;
        exit_guard.confirm();
        lease.confirm_exit();
        diagnostic_reader.finish().await;
        if let Ok(ref mut event) = result {
            crate::worker::manifest::consume(&request, event)?;
        }
        result
    }

    struct DiagnosticReader(Option<tokio::task::JoinHandle<Zeroizing<Vec<u8>>>>);
    impl DiagnosticReader {
        fn start(child: &mut Child) -> Self {
            Self(child.stderr.take().map(|stderr| {
                tokio::spawn(async move {
                    let mut bytes = Zeroizing::new(Vec::new());
                    let _ = stderr.take(16 * 1024).read_to_end(&mut bytes).await;
                    bytes
                })
            }))
        }
        async fn finish(&mut self) {
            let Some(mut task) = self.0.take() else {
                return;
            };
            if let Ok(Ok(bytes)) = timeout(Duration::from_millis(500), &mut task).await {
                let mut records = DIAGNOSTICS
                    .get_or_init(|| Mutex::new(std::collections::VecDeque::new()))
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for record in parse_diagnostics(&bytes) {
                    if records.len() >= 64 {
                        records.pop_front();
                    }
                    records.push_back(record);
                }
            } else {
                task.abort();
            }
        }
    }
    impl Drop for DiagnosticReader {
        fn drop(&mut self) {
            if let Some(task) = self.0.take() {
                task.abort();
            }
        }
    }

    struct ProcessExitGuard {
        identity: (u32, u64),
        request_id: String,
        confirmed: bool,
    }

    impl ProcessExitGuard {
        fn new(child: &Child, request_id: String) -> Self {
            let guard = Self {
                identity: (
                    child.id().unwrap_or(0),
                    child
                        .raw_handle()
                        .and_then(|h| creation_time(h as HANDLE).ok())
                        .unwrap_or(0),
                ),
                request_id,
                confirmed: false,
            };
            guard.record();
            guard
        }
        fn record(&self) {
            EXIT_RECEIPTS
                .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(
                    self.request_id.clone(),
                    WorkerExitReceipt {
                        processes: vec![self.identity],
                        exit_confirmed: self.confirmed,
                    },
                );
        }
        fn confirm(&mut self) {
            self.confirmed = true;
            self.record();
        }
    }

    impl Drop for ProcessExitGuard {
        fn drop(&mut self) {
            if !self.confirmed {
                self.confirmed = matches!(
                    process_identity_exited(self.identity.0, self.identity.1),
                    Ok(true)
                );
                // Keep this request's identity even if termination already completed.
                self.record();
                if !self.confirmed {
                    register_unconfirmed(self.identity);
                }
            }
        }
    }

    fn register_unconfirmed(identity: (u32, u64)) {
        let mut registry = UNCONFIRMED
            .get_or_init(|| Mutex::new(vec![]))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !registry.contains(&identity) {
            registry.push(identity);
        }
    }

    async fn confirm_exit(child: &mut Child, force: bool) -> Result<(), String> {
        let identity = (
            child.id().unwrap_or(0),
            child
                .raw_handle()
                .and_then(|h| creation_time(h as HANDLE).ok())
                .unwrap_or(0),
        );
        if force {
            let _ = child.start_kill();
        }
        if let Ok(Ok(_)) = timeout(Duration::from_secs(10), child.wait()).await {
            return Ok(());
        }
        let _ = child.start_kill();
        match timeout(Duration::from_secs(10), child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            _ => {
                // Recover a poisoned registry instead of silently dropping the leak record:
                // losing an entry would hide a process that exited without confirmation.
                register_unconfirmed(identity);
                Err("WorkerExitUnconfirmed".into())
            }
        }
    }

    async fn connect(child: &mut Child, name: &str, pid: u32) -> Result<NamedPipeClient, String> {
        let started = Instant::now();
        loop {
            match ClientOptions::new()
                .read(true)
                .write(true)
                .security_qos_flags(SECURITY_IDENTIFICATION)
                .open(name)
            {
                Ok(pipe) => {
                    let mut server = 0;
                    if unsafe {
                        GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut server)
                    } == 0
                        || server != pid
                    {
                        return Err("WorkerPipeIdentityMismatch".into());
                    }
                    return Ok(pipe);
                }
                Err(error) if matches!(error.raw_os_error(), Some(2 | 231)) => {
                    if child
                        .try_wait()
                        .map_err(|_| "WorkerProcessUnavailable")?
                        .is_some()
                    {
                        return Err("WorkerStartFailed".into());
                    }
                    if started.elapsed() >= Duration::from_secs(30) {
                        return Err("WorkerConnectTimeout".into());
                    }
                    sleep(Duration::from_millis(50)).await;
                }
                Err(_) => return Err("WorkerPipeUnavailable".into()),
            }
        }
    }

    async fn write_message(pipe: &mut NamedPipeClient, bytes: &[u8]) -> Result<(), String> {
        timeout(Duration::from_secs(3), async {
            pipe.write_all(bytes).await?;
            pipe.write_all(b"\n").await?;
            pipe.flush().await
        })
        .await
        .map_err(|_| "WorkerControlTimeout".to_string())?
        .map_err(|_| "WorkerControlFailed".to_string())
    }

    async fn read_event(
        pipe: &mut NamedPipeClient,
        pending: &mut Vec<u8>,
    ) -> Result<WorkerEvent, String> {
        let mut searched = 0;
        loop {
            if let Some(relative_end) = pending[searched..].iter().position(|b| *b == b'\n') {
                let end = searched + relative_end;
                if end > MAX_MESSAGE {
                    return Err("WorkerProtocolError".into());
                }
                let event = serde_json::from_slice(&pending[..end])
                    .map_err(|_| "WorkerProtocolError".to_string())?;
                pending.drain(..=end);
                return Ok(event);
            }
            if pending.len() > MAX_MESSAGE {
                return Err("WorkerProtocolError".into());
            }
            searched = pending.len();
            let mut buffer = [0u8; 4096];
            let available = (MAX_MESSAGE + 1 - pending.len()).min(buffer.len());
            let count = pipe
                .read(&mut buffer[..available])
                .await
                .map_err(|_| "WorkerDisconnected".to_string())?;
            if count == 0 {
                return Err("WorkerDisconnected".into());
            }
            pending.extend_from_slice(&buffer[..count]);
        }
    }

    struct ControlCommand {
        bytes: Vec<u8>,
        ack: oneshot::Sender<Result<(), String>>,
    }
    fn command(identity: &OperationIdentity, operation: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "protocolVersion": identity.version,
            "requestId": identity.request_id, "operation": operation,
            "operationId": identity.operation_id, "generation": identity.generation }))
        .expect("fixed control fields are serializable")
    }

    async fn control_loop(
        mut pipe: NamedPipeClient,
        mut commands: mpsc::Receiver<ControlCommand>,
        identity: OperationIdentity,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let mut ticker = interval(Duration::from_millis(100));
        let mut cancellation_sent = false;
        loop {
            tokio::select! {
                message = commands.recv() => match message {
                    Some(message) => {
                        let result = write_message(&mut pipe, &message.bytes).await;
                        let failed = result.is_err();
                        let _ = message.ack.send(result);
                        if failed { return Err("WorkerControlFailed".into()); }
                    }
                    None => return Ok(()),
                },
                _ = ticker.tick() => {
                    if cancel.load(Ordering::Acquire) && !cancellation_sent {
                        write_message(&mut pipe, &command(&identity, "cancel")).await?;
                        cancellation_sent = true;
                    }
                }
            }
        }
    }

    struct RunOptions {
        priority_flag: u32,
        is_probe: bool,
        cancel: Arc<AtomicBool>,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    }

    struct AbortTasks(Vec<tokio::task::AbortHandle>);
    impl Drop for AbortTasks {
        fn drop(&mut self) {
            // Dropping a JoinHandle detaches the task. Cancelling the outer
            // request must also release every pipe and progress callback owner.
            for task in &self.0 {
                task.abort();
            }
        }
    }

    async fn communicate(
        child: &mut Child,
        prefix: &str,
        pid: u32,
        identity: &OperationIdentity,
        bytes: Zeroizing<Vec<u8>>,
        options: RunOptions,
    ) -> Result<WorkerEvent, String> {
        let RunOptions {
            priority_flag,
            is_probe,
            cancel,
            progress,
        } = options;
        let mut control = connect(child, &format!(r"\\.\pipe\{prefix}-control"), pid).await?;
        let mut events = connect(child, &format!(r"\\.\pipe\{prefix}-events"), pid).await?;
        let mut pending = Vec::new();
        let ready = timeout(
            Duration::from_secs(30),
            read_event(&mut events, &mut pending),
        )
        .await
        .map_err(|_| "WorkerReadyTimeout".to_string())??;
        if !identity.accepts_ready(&ready) {
            return Err("WorkerProtocolError".into());
        }
        if let Some(raw) = child.raw_handle() {
            unsafe {
                SetPriorityClass(raw as HANDLE, priority_flag);
            }
        }
        write_message(&mut control, &bytes).await?;
        drop(bytes);
        let mut child_tasks = AbortTasks(Vec::with_capacity(3));
        let (command_tx, command_rx) = mpsc::channel(4);
        let mut controller = tokio::spawn(control_loop(
            control,
            command_rx,
            identity.clone(),
            cancel.clone(),
        ));
        child_tasks.0.push(controller.abort_handle());
        let (event_tx, mut event_rx) = mpsc::channel(64);
        let reader = tokio::spawn(async move {
            loop {
                let event = read_event(&mut events, &mut pending).await;
                let failed = event.is_err();
                if event_tx.send(event).await.is_err() || failed {
                    return;
                }
            }
        });
        child_tasks.0.push(reader.abort_handle());
        let (latest_tx, mut latest_rx) = watch::channel::<Option<WorkerEvent>>(None);
        let observer = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(200));
            loop {
                ticker.tick().await;
                match latest_rx.has_changed() {
                    Ok(true) => {
                        let event = latest_rx.borrow_and_update().clone();
                        // Callbacks are invoked outside any lock; UI delivery must remain nonblocking.
                        if let Some(event) = event {
                            progress(&event);
                        }
                    }
                    Ok(false) => {}
                    Err(_) => return,
                }
            }
        });
        child_tasks.0.push(observer.abort_handle());
        let mut ticker = interval(Duration::from_millis(200));
        let operation_started = Instant::now();
        let mut last_response = Instant::now();
        let mut cancellation_started = None;
        let mut unresponsive = false;
        let max_duration = if is_probe {
            Duration::from_secs(300)
        } else {
            Duration::from_secs(7200)
        };
        let result = loop {
            tokio::select! {
                event = event_rx.recv() => {
                    let event = match event {
                        Some(Ok(event)) => event,
                        Some(Err(error)) => break Err(error),
                        None => break Err("WorkerDisconnected".into()),
                    };
                    if !identity.matches(&event) {
                        break Err("WorkerProtocolError".into());
                    }
                    last_response = Instant::now();
                    match event.kind.as_str() {
                        "heartbeat" => {},
                        "progress" => { let _ = latest_tx.send(Some(event)); },
                        "completed" | "error" | "cancelled" => {
                            if unresponsive { break Err("WorkerUnresponsive".into()); }
                            let (ack_tx, ack_rx) = oneshot::channel();
                            let exit = ControlCommand { bytes: command(identity, "exit"), ack: ack_tx };
                            if command_tx.send(exit).await.is_err() { break Err("WorkerControlFailed".into()); }
                            match timeout(Duration::from_secs(4), ack_rx).await {
                                Ok(Ok(Ok(()))) => break Ok(event),
                                _ => break Err("WorkerControlFailed".into()),
                            }
                        }
                        _ => break Err("WorkerProtocolError".into()),
                    }
                }
                result = &mut controller => {
                    break Err(result.ok().and_then(Result::err).unwrap_or_else(|| "WorkerControlFailed".into()));
                }
                _ = ticker.tick() => {
                    if (last_response.elapsed() >= Duration::from_secs(120) || operation_started.elapsed() >= max_duration) && !cancel.load(Ordering::Acquire) {
                        unresponsive = true;
                        cancel.store(true, Ordering::Release);
                    }
                    if cancel.load(Ordering::Acquire) {
                        let started = cancellation_started.get_or_insert_with(Instant::now);
                        if started.elapsed() >= Duration::from_secs(10) {
                            if unresponsive {
                                break Err("WorkerUnresponsive".into());
                            } else {
                                break Err("WorkerCancellationTimedOut".into());
                            }
                        }
                    }
                }
            }
        };
        reader.abort();
        controller.abort();
        observer.abort();
        // The OS JobObject remains alive until run() confirms exit. Aborted tasks cannot
        // retain pipe handles indefinitely; none owns the job HANDLE.
        result
    }
    #[cfg(test)]
    mod resource_tests {
        use super::*;
        use std::os::windows::process::CommandExt;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[tokio::test]
        async fn resource_aborted_task_group_releases_all_child_captures() {
            let mut tasks = AbortTasks(Vec::with_capacity(3));
            let mut captures = Vec::new();
            let mut started = Vec::new();
            for _ in 0..3 {
                let marker = Arc::new(());
                captures.push(Arc::downgrade(&marker));
                let (ready, observed) = oneshot::channel();
                started.push(observed);
                let child = tokio::spawn(async move {
                    let _marker = marker;
                    let _ = ready.send(());
                    std::future::pending::<()>().await;
                });
                tasks.0.push(child.abort_handle());
                drop(child); // The abort owner, rather than JoinHandle, owns cancellation.
            }
            for observed in started {
                observed.await.unwrap();
            }
            assert!(captures.iter().all(|marker| marker.upgrade().is_some()));
            drop(tasks);
            timeout(Duration::from_secs(2), async {
                while captures.iter().any(|marker| marker.upgrade().is_some()) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("all three child tasks must release captures after outer cancellation");
        }

        #[tokio::test]
        async fn resource_aborted_owner_keeps_process_bytes_until_exit_confirmation() {
            let exited = Arc::new(AtomicBool::new(false));
            let exit_state = exited.clone();
            let pool = ResourcePool::new(
                Arc::new(|| Ok(roomy())),
                Arc::new(move |pid, created| {
                    pid == 42 && created == 100 && exit_state.load(Ordering::Acquire)
                }),
            );
            let mut lease = pool
                .acquire(1, &AtomicBool::new(false), None)
                .await
                .unwrap();
            lease.bind((42, 100));
            let other = pool
                .acquire(1, &AtomicBool::new(false), None)
                .await
                .unwrap();
            let (started, ready) = oneshot::channel();
            let owner = tokio::spawn(async move {
                let _lease = lease;
                let _ = started.send(());
                std::future::pending::<()>().await;
            });
            ready.await.unwrap();
            owner.abort();
            assert!(matches!(owner.await, Err(error) if error.is_cancelled()));
            assert_eq!(reservations(&pool), (2, 8 * GIB, 0));
            let cancel = AtomicBool::new(false);
            let next = pool.acquire(1, &cancel, None);
            tokio::pin!(next);
            assert!(timeout(Duration::from_millis(20), &mut next).await.is_err());
            exited.store(true, Ordering::Release);
            pool.changed.notify_waiters();
            let resumed = timeout(Duration::from_secs(1), next)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(reservations(&pool), (2, 8 * GIB, 0));
            drop(resumed);
            drop(other);
            assert_eq!(reservations(&pool), (0, 0, 0));
        }

        fn roomy() -> MemoryStatus {
            MemoryStatus {
                total_physical: 16 * GIB,
                available_physical: 12 * GIB,
                available_commit: 16 * GIB,
            }
        }
        fn isolated(memory: Arc<Mutex<MemoryStatus>>) -> Arc<ResourcePool> {
            ResourcePool::new(
                Arc::new(move || Ok(*memory.lock().unwrap())),
                Arc::new(|_, _| false),
            )
        }
        fn reservations(pool: &ResourcePool) -> (usize, u64, usize) {
            let state = pool.state.lock().unwrap();
            (
                state.leases.len(),
                state.leases.values().map(|r| r.bytes).sum(),
                state.waiters.len(),
            )
        }
        async fn wait_until_waiting(pool: &ResourcePool, count: usize) {
            timeout(Duration::from_secs(2), async {
                while reservations(pool).2 != count {
                    sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("resource waiter registration");
        }

        #[tokio::test]
        async fn resource_five_light_leases_overlap_and_sixth_waits() {
            let pool = isolated(Arc::new(Mutex::new(roomy())));
            let cancel = AtomicBool::new(false);
            let mut leases = Vec::new();
            for _ in 0..5 {
                leases.push(pool.acquire(5, &cancel, None).await.unwrap());
            }
            let share = leases[0].bytes;
            assert!(share <= 8 * GIB / 5 && 8 * GIB / 5 - share < MEMORY_GRANULARITY);
            assert_eq!(share % MEMORY_GRANULARITY, 0);
            assert_eq!(reservations(&pool), (5, share * 5, 0));
            let next = pool.acquire(5, &cancel, None);
            tokio::pin!(next);
            assert!(timeout(Duration::from_millis(20), &mut next).await.is_err());
            drop(leases.pop());
            let sixth = timeout(Duration::from_secs(1), next)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(reservations(&pool).0, 5);
            assert!(reservations(&pool).1 <= 8 * GIB);
            drop(sixth);
            drop(leases);
            assert_eq!(reservations(&pool), (0, 0, 0));
        }

        #[tokio::test]
        async fn resource_concurrency_changes_only_new_lease_limits() {
            let pool = isolated(Arc::new(Mutex::new(roomy())));
            let cancel = AtomicBool::new(false);
            let original = pool.acquire(1, &cancel, None).await.unwrap();
            let small = pool.acquire(5, &cancel, None).await.unwrap();
            assert_eq!(original.bytes, 4 * GIB);
            let share = small.bytes;
            assert!(share <= 8 * GIB / 5 && 8 * GIB / 5 - share < MEMORY_GRANULARITY);
            let reduced_parallelism = pool.acquire(1, &cancel, None).await.unwrap();
            assert_eq!(original.bytes, 4 * GIB);
            assert_eq!(small.bytes, share);
            assert_eq!(
                reduced_parallelism.bytes,
                8 * GIB - original.bytes - small.bytes
            );
            assert_eq!(reservations(&pool).1, 8 * GIB);
        }

        #[tokio::test]
        async fn resource_eight_gib_default_adapts_to_physical_and_commit_room() {
            let memory = Arc::new(Mutex::new(MemoryStatus {
                total_physical: 8 * GIB,
                available_physical: 3 * GIB,
                available_commit: 8 * GIB,
            }));
            let pool = isolated(memory.clone());
            let cancel = AtomicBool::new(false);
            let lease = pool.acquire(1, &cancel, None).await.unwrap();
            assert_eq!(
                lease.bytes,
                (3 * GIB - 8 * GIB / 10) / MEMORY_GRANULARITY * MEMORY_GRANULARITY
            );
            drop(lease);
            memory.lock().unwrap().available_commit = GIB;
            let lease = pool.acquire(1, &cancel, None).await.unwrap();
            assert_eq!(lease.bytes, 512 * MIB);
        }

        #[tokio::test]
        async fn resource_low_physical_and_commit_room_resume_without_owner_completion() {
            let memory = Arc::new(Mutex::new(MemoryStatus {
                available_physical: 100 * MIB,
                ..roomy()
            }));
            let pool = isolated(memory.clone());
            let child_pool = pool.clone();
            let waiting =
                tokio::spawn(
                    async move { child_pool.acquire(5, &AtomicBool::new(false), None).await },
                );
            wait_until_waiting(&pool, 1).await;
            assert_eq!(reservations(&pool).0, 0);
            *memory.lock().unwrap() = MemoryStatus {
                available_commit: 100 * MIB,
                ..roomy()
            };
            sleep(Duration::from_millis(230)).await;
            assert!(!waiting.is_finished());
            *memory.lock().unwrap() = roomy();
            // No notification: the periodic probe must detect external recovery.
            let lease = timeout(Duration::from_secs(1), waiting)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(lease.bytes <= 8 * GIB / 5 && 8 * GIB / 5 - lease.bytes < MEMORY_GRANULARITY);
        }

        #[tokio::test]
        async fn resource_cancel_and_abandoned_waiter_release_fifo_and_notice() {
            let memory = Arc::new(Mutex::new(MemoryStatus {
                available_physical: 0,
                ..roomy()
            }));
            let pool = isolated(memory.clone());
            let cancel = Arc::new(AtomicBool::new(false));
            let notices = Arc::new(Mutex::new(Vec::new()));
            let recorded = notices.clone();
            let observer: ResourceWaitObserver =
                Arc::new(move |waiting| recorded.lock().unwrap().push(waiting));
            let first_pool = pool.clone();
            let first_cancel = cancel.clone();
            let first =
                tokio::spawn(
                    async move { first_pool.acquire(5, &first_cancel, Some(observer)).await },
                );
            wait_until_waiting(&pool, 1).await;
            let second_pool = pool.clone();
            let second =
                tokio::spawn(
                    async move { second_pool.acquire(5, &AtomicBool::new(false), None).await },
                );
            wait_until_waiting(&pool, 2).await;
            let started = Instant::now();
            cancel.store(true, Ordering::Release);
            assert_eq!(
                timeout(Duration::from_millis(450), first)
                    .await
                    .unwrap()
                    .unwrap()
                    .err()
                    .unwrap(),
                "Cancelled"
            );
            assert!(started.elapsed() < Duration::from_millis(450));
            assert_eq!(*notices.lock().unwrap(), vec![true, false]);
            assert_eq!(reservations(&pool).2, 1);
            second.abort();
            match second.await {
                Err(error) => assert!(error.is_cancelled()),
                Ok(_) => panic!("abandoned resource waiter unexpectedly completed"),
            }
            assert_eq!(reservations(&pool), (0, 0, 0));
            *memory.lock().unwrap() = roomy();
            assert!(pool.acquire(5, &AtomicBool::new(false), None).await.is_ok());
        }

        #[tokio::test]
        async fn resource_fifo_head_is_admitted_before_later_requests() {
            let memory = Arc::new(Mutex::new(MemoryStatus {
                available_physical: 0,
                ..roomy()
            }));
            let pool = isolated(memory.clone());
            let first_pool = pool.clone();
            let first =
                tokio::spawn(
                    async move { first_pool.acquire(1, &AtomicBool::new(false), None).await },
                );
            wait_until_waiting(&pool, 1).await;
            let second_pool = pool.clone();
            let second =
                tokio::spawn(
                    async move { second_pool.acquire(1, &AtomicBool::new(false), None).await },
                );
            wait_until_waiting(&pool, 2).await;
            let first_id = *pool.state.lock().unwrap().waiters.front().unwrap();
            *memory.lock().unwrap() = roomy();
            pool.changed.notify_waiters();
            let a = first.await.unwrap().unwrap();
            let b = second.await.unwrap().unwrap();
            assert_eq!(a.id, first_id);
            assert!(a.id < b.id);
            assert_eq!(reservations(&pool).1, 8 * GIB);
        }

        #[tokio::test]
        async fn resource_probe_failure_is_bounded_and_transient_probe_can_recover() {
            let calls = Arc::new(AtomicUsize::new(0));
            let count = calls.clone();
            let pool = ResourcePool::new(
                Arc::new(move || {
                    count.fetch_add(1, Ordering::SeqCst);
                    Err("synthetic probe error".into())
                }),
                Arc::new(|_, _| false),
            );
            let error = timeout(
                Duration::from_secs(2),
                pool.acquire(1, &AtomicBool::new(false), None),
            )
            .await
            .unwrap()
            .err()
            .unwrap();
            assert_eq!(error, "WorkerResourceProbeUnavailable");
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            assert_eq!(reservations(&pool), (0, 0, 0));
            let calls = Arc::new(AtomicUsize::new(0));
            let count = calls.clone();
            let pool = ResourcePool::new(
                Arc::new(move || {
                    if count.fetch_add(1, Ordering::SeqCst) < 2 {
                        Err("transient".into())
                    } else {
                        Ok(roomy())
                    }
                }),
                Arc::new(|_, _| false),
            );
            assert!(
                timeout(
                    Duration::from_secs(2),
                    pool.acquire(1, &AtomicBool::new(false), None)
                )
                .await
                .unwrap()
                .is_ok()
            );
        }

        #[tokio::test]
        async fn resource_unconfirmed_exit_keeps_bytes_until_exact_identity_exits() {
            let exited = Arc::new(Mutex::new(std::collections::HashSet::new()));
            let observed = exited.clone();
            let pool = ResourcePool::new(
                Arc::new(|| Ok(roomy())),
                Arc::new(move |pid, created| observed.lock().unwrap().contains(&(pid, created))),
            );
            let cancel = AtomicBool::new(false);
            let mut first = pool.acquire(1, &cancel, None).await.unwrap();
            let second = pool.acquire(1, &cancel, None).await.unwrap();
            first.bind((42, 100));
            drop(first);
            assert_eq!(reservations(&pool).1, 8 * GIB);
            exited.lock().unwrap().insert((42, 101));
            pool.reap_confirmed_exits();
            assert_eq!(reservations(&pool).1, 8 * GIB);
            exited.lock().unwrap().insert((42, 100));
            pool.reap_confirmed_exits();
            assert_eq!(reservations(&pool), (1, 4 * GIB, 0));
            drop(second); // No process was spawned: start errors release immediately.
            assert_eq!(reservations(&pool), (0, 0, 0));
            let mut confirmed = pool.acquire(1, &cancel, None).await.unwrap();
            confirmed.bind((43, 0)); // Even an unavailable identity needs explicit exit confirmation.
            confirmed.confirm_exit();
            drop(confirmed);
            assert_eq!(reservations(&pool), (0, 0, 0));
        }

        fn test_name(function: &str) -> String {
            format!("{}::{function}", module_path!().split_once("::").unwrap().1)
        }

        #[test]
        fn resource_public_batch_inspect_preview_share_budget() {
            let name = test_name("resource_public_batch_inspect_preview_share_budget");
            if std::env::var("OPENUNPACK_RESOURCE_ENTRY_TEST")
                .ok()
                .as_deref()
                != Some(&name)
            {
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", &name, "--nocapture", "--test-threads=1"])
                    .env("OPENUNPACK_RESOURCE_ENTRY_TEST", &name)
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "isolated entry test: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                return;
            }
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let pool = isolated(Arc::new(Mutex::new(roomy())));
                    assert!(RESOURCE_POOL.set(pool.clone()).is_ok());
                    let cancel = AtomicBool::new(false);
                    let mut leases = Vec::new();
                    for _ in 0..5 {
                        leases.push(pool.acquire(5, &cancel, None).await.unwrap());
                    }
                    // Existing absolute files pass validation; the full pool must
                    // prevent these test executables ever being spawned as Workers.
                    let executable = std::env::current_exe().unwrap();
                    let client = WorkerClient {
                        executable: executable.clone(),
                        engine: executable,
                    };
                    let mut operations = Vec::new();
                    let mut cancellations = Vec::new();
                    for operation in ["extract", "inspect", "preview"] {
                        let client = client.clone();
                        let cancel = Arc::new(AtomicBool::new(false));
                        cancellations.push(cancel.clone());
                        operations.push(tokio::spawn(async move {
                            if operation == "preview" {
                                client
                                    .preview_output_rules_with_resources(&[], &[], cancel, 5, None)
                                    .await
                                    .map(|_| ())
                            } else {
                                let mut request = WorkerRequest::default();
                                request.request_id = uuid::Uuid::new_v4().to_string();
                                request.operation = operation.into();
                                client
                                    .run_with_resources(request, cancel, Arc::new(|_| {}), 5, None)
                                    .await
                                    .map(|_| ())
                            }
                        }));
                    }
                    wait_until_waiting(&pool, 3).await;
                    assert_eq!(reservations(&pool).0, 5);
                    for cancel in cancellations {
                        cancel.store(true, Ordering::Release);
                    }
                    for operation in operations {
                        assert_eq!(
                            timeout(Duration::from_secs(1), operation)
                                .await
                                .unwrap()
                                .unwrap()
                                .unwrap_err(),
                            "Cancelled"
                        );
                    }
                    assert_eq!(reservations(&pool).2, 0);
                    drop(leases);
                    assert_eq!(reservations(&pool), (0, 0, 0));
                });
        }

        #[tokio::test]
        async fn resource_native_job_enforces_memory_and_denies_child_processes() {
            use std::io::Read;
            let name = test_name("resource_native_job_enforces_memory_and_denies_child_processes");
            if std::env::var("OPENUNPACK_RESOURCE_CHILD_TEST")
                .ok()
                .as_deref()
                == Some(&name)
            {
                std::io::stdin().read_exact(&mut [0u8]).unwrap();
                assert!(
                    std::process::Command::new("cmd.exe")
                        .args(["/d", "/c", "exit", "0"])
                        .creation_flags(CREATE_NO_WINDOW)
                        .spawn()
                        .is_err(),
                    "Worker Job must deny descendants"
                );
                return;
            }
            let pool = isolated(Arc::new(Mutex::new(roomy())));
            let mut lease = pool
                .acquire(5, &AtomicBool::new(false), None)
                .await
                .unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture", "--test-threads=1"])
                .env("OPENUNPACK_RESOURCE_CHILD_TEST", &name)
                .creation_flags(CREATE_NO_WINDOW)
                .kill_on_drop(true)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut exit = ProcessExitGuard::new(&child, uuid::Uuid::new_v4().to_string());
            lease.bind(exit.identity);
            let job = JobGuard::attach_limited(&child, lease.bytes).unwrap();
            // attach_limited verifies both queried JobMemoryLimit and ActiveProcessLimit.
            let mut input = child.stdin.take().unwrap();
            input.write_all(&[1]).await.unwrap();
            drop(input);
            let result = timeout(Duration::from_secs(10), child.wait())
                .await
                .unwrap()
                .unwrap();
            let mut diagnostics = Vec::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_end(&mut diagnostics)
                .await
                .unwrap();
            assert!(
                result.success(),
                "restricted child: {}",
                String::from_utf8_lossy(&diagnostics)
            );
            exit.confirm();
            lease.confirm_exit();
            drop(job);
            drop(lease);
            assert_eq!(reservations(&pool), (0, 0, 0));
        }
    }

    #[cfg(test)]
    mod process_tests {
        use super::*;

        #[tokio::test]
        async fn exit_receipts_keep_request_ownership_and_observe_late_exit() {
            let client = WorkerClient {
                executable: PathBuf::new(),
                engine: PathBuf::new(),
            };
            let first_id = uuid::Uuid::new_v4().to_string();
            let second_id = uuid::Uuid::new_v4().to_string();
            let mut first = Command::new("cmd.exe")
                .args(["/d", "/c", "exit", "0"])
                .creation_flags(CREATE_NO_WINDOW)
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let first_guard = ProcessExitGuard::new(&first, first_id.clone());
            let first_identity = first_guard.identity;
            timeout(Duration::from_secs(10), first.wait())
                .await
                .unwrap()
                .unwrap();
            drop(first_guard); // Exit may have happened before the future was dropped.
            let first_receipt = client.request_exit_receipt(&first_id).unwrap();
            assert!(first_receipt.exit_confirmed);
            assert_eq!(first_receipt.processes, vec![first_identity]);

            let mut second = Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Start-Sleep -Seconds 20",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let second_guard = ProcessExitGuard::new(&second, second_id.clone());
            let second_identity = second_guard.identity;
            let job = JobGuard::attach(&second).unwrap(); // Includes CPU configuration/readback.
            drop(second_guard);
            let second_receipt = client.request_exit_receipt(&second_id).unwrap();
            assert!(!second_receipt.exit_confirmed);
            assert_eq!(second_receipt.processes, vec![second_identity]);
            assert_ne!(first_identity, second_identity);
            assert!(
                client
                    .request_exit_receipt(&first_id)
                    .unwrap()
                    .exit_confirmed
            );
            second.start_kill().unwrap();
            timeout(Duration::from_secs(10), second.wait())
                .await
                .unwrap()
                .unwrap();
            drop(job);
            assert!(
                client
                    .request_exit_receipt(&second_id)
                    .unwrap()
                    .exit_confirmed
            );
            assert!(!client.unconfirmed_processes().contains(&second_identity));
            let retained_id = uuid::Uuid::new_v4().to_string();
            client.retain_request_receipt(&retained_id).unwrap();
            for _ in 0..260 {
                client
                    .register_no_process(&uuid::Uuid::new_v4().to_string())
                    .unwrap();
            }
            assert!(
                client
                    .request_exit_receipt(&retained_id)
                    .unwrap()
                    .exit_confirmed
            );
            client.forget_confirmed_request(&retained_id);
            client.forget_confirmed_request(&first_id);
            client.forget_confirmed_request(&second_id);
        }
    }
}
