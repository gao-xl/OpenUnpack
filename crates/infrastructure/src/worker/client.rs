use openunpack_protocol::{WorkerEvent, WorkerRequest};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, atomic::AtomicBool},
};

static DIAGNOSTICS: OnceLock<Mutex<VecDeque<WorkerDiagnostic>>> = OnceLock::new();
static EXIT_RECEIPTS: OnceLock<Mutex<HashMap<String, WorkerExitReceipt>>> = OnceLock::new();
static RECEIPT_ORDER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static RETAINED_RECEIPTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

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

#[derive(Clone)]
pub struct WorkerClient {
    pub executable: PathBuf,
    pub engine: PathBuf,
}

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
        let mut receipts = EXIT_RECEIPTS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut retained = RETAINED_RECEIPTS
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut order = RECEIPT_ORDER
            .get_or_init(|| Mutex::new(VecDeque::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // First pass: clean up confirmed receipts that are not explicitly retained
        if receipts.len() >= 256 {
            receipts.retain(|id, receipt| !receipt.exit_confirmed || retained.contains(id));
            order.retain(|id| receipts.contains_key(id));
        }
        // Second pass: FIFO eviction if still at or above capacity (ISSUE-H03)
        // Evict the oldest non-retained confirmed receipt first, or oldest retained if needed,
        // ensuring we never block operations with ResourceLimit due to an in-memory receipt table.
        while receipts.len() >= 256 {
            // Find oldest candidate in order
            let mut evicted_id = None;
            for (idx, id) in order.iter().enumerate() {
                if !retained.contains(id) {
                    evicted_id = Some((idx, id.clone()));
                    break;
                }
            }
            if let Some((idx, id)) = evicted_id {
                order.remove(idx);
                receipts.remove(&id);
            } else if let Some(oldest) = order.pop_front() {
                retained.remove(&oldest);
                receipts.remove(&oldest);
            } else {
                break;
            }
        }

        if !receipts.contains_key(request_id) {
            order.push_back(request_id.to_string());
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
        Ok(())
    }

    pub fn request_exit_receipt(&self, request_id: &str) -> Option<WorkerExitReceipt> {
        let receipts = EXIT_RECEIPTS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        receipts.get(request_id).cloned()
    }

    pub fn forget_confirmed_request(&self, request_id: &str) {
        let mut receipts = EXIT_RECEIPTS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if receipts.get(request_id).is_some_and(|r| r.exit_confirmed) {
            receipts.remove(request_id);
            RETAINED_RECEIPTS
                .get_or_init(|| Mutex::new(HashSet::new()))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(request_id);
            RECEIPT_ORDER
                .get_or_init(|| Mutex::new(VecDeque::new()))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|id| id != request_id);
        }
    }

    pub fn recent_diagnostics(&self) -> Vec<WorkerDiagnostic> {
        DIAGNOSTICS
            .get_or_init(|| Mutex::new(VecDeque::new()))
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
        _concurrency: u32,
        _on_wait: Option<ResourceWaitObserver>,
    ) -> Result<Vec<bool>, String> {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err("Cancelled".into());
        }
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
        let res: Vec<bool> = crate::engine::preview_output_exclusions(paths, patterns);
        Ok(res)
    }

    pub fn unconfirmed_processes(&self) -> Vec<(u32, u64)> {
        Vec::new()
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
        _concurrency: u32,
        _on_wait: Option<ResourceWaitObserver>,
    ) -> Result<WorkerEvent, String> {
        self.register_no_process(&request.request_id)?;
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            let mut ev = openunpack_protocol::WorkerEvent::default();
            ev.protocol_version = request.protocol_version;
            ev.request_id = request.request_id.clone();
            ev.operation_id = request.operation_id.clone();
            ev.generation = request.generation;
            ev.kind = "cancelled".into();
            ev.error_code = "Cancelled".into();
            ev.message = "Cancelled".into();
            return Ok(ev);
        }

        // Aborting the future that awaits this call is the outermost cancel
        // signal there is: nothing else can reach into a `spawn_blocking` copy
        // loop. Arm the flag for the duration of the work so a dropped future
        // still stops the engine and releases the progress callback.
        let mut abort_guard = CancelOnAbort(Some(cancel.clone()));
        let outcome = crate::engine::execute_native_worker(request.clone(), progress, cancel).await;
        abort_guard.0 = None;
        match outcome {
            Ok(event) => Ok(event),
            Err(err) if err == "Cancelled" => {
                let mut ev = openunpack_protocol::WorkerEvent::default();
                ev.protocol_version = request.protocol_version;
                ev.request_id = request.request_id.clone();
                ev.operation_id = request.operation_id.clone();
                ev.generation = request.generation;
                ev.kind = "cancelled".into();
                ev.error_code = "Cancelled".into();
                ev.message = "Cancelled".into();
                Ok(ev)
            }
            Err(err) => {
                let mut ev = openunpack_protocol::WorkerEvent::default();
                ev.protocol_version = request.protocol_version;
                ev.request_id = request.request_id.clone();
                ev.operation_id = request.operation_id.clone();
                ev.generation = request.generation;
                ev.kind = "error".into();
                let code = err.split(':').next().unwrap_or(&err).trim().to_string();
                let code = match code.as_str() {
                    "CorruptArchive" | "ExtractionFailed" => "Corrupt".to_string(),
                    other => other.to_string(),
                };
                ev.error_code = code;
                ev.message = err;
                Ok(ev)
            }
        }
    }
}

/// Sets the job's cancel flag when the awaiting future is dropped.
///
/// The native engine runs copies on `spawn_blocking`, which cannot be aborted
/// by dropping its handle. Without this guard an aborted `run` keeps copying
/// the whole archive and keeps the progress callback (and everything it
/// captures) alive until the copy finishes on its own.
struct CancelOnAbort(Option<std::sync::Arc<std::sync::atomic::AtomicBool>>);

impl Drop for CancelOnAbort {
    fn drop(&mut self) {
        if let Some(flag) = &self.0 {
            flag.store(true, std::sync::atomic::Ordering::Release);
        }
    }
}

#[cfg(windows)]
pub fn process_identity_exited(pid: u32, creation: u64) -> Result<bool, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE: u32 = 259;

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return Ok(true);
    }
    let mut created = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let has_times =
        unsafe { GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user) } != 0;
    if !has_times {
        unsafe { CloseHandle(handle) };
        return Ok(true);
    }
    let proc_created = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    if proc_created != creation {
        unsafe { CloseHandle(handle) };
        return Ok(true);
    }
    let mut exit_code: u32 = 0;
    let has_exit = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0;
    unsafe { CloseHandle(handle) };
    if has_exit {
        Ok(exit_code != STILL_ACTIVE)
    } else {
        Ok(true)
    }
}

#[cfg(not(windows))]
pub fn process_identity_exited(_pid: u32, _creation: u64) -> Result<bool, String> {
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_receipts_fifo_eviction() {
        let client = WorkerClient {
            executable: PathBuf::new(),
            engine: PathBuf::new(),
        };

        // Register 300 requests (exceeding 256 limit)
        for i in 0..300 {
            let req_id = format!("req-{}", i);
            let res = client.register_no_process(&req_id);
            assert!(
                res.is_ok(),
                "Request {} should succeed without ResourceLimit error",
                i
            );
        }

        // The oldest requests should have been evicted by FIFO
        assert!(client.request_exit_receipt("req-0").is_none());
        assert!(client.request_exit_receipt("req-43").is_none());
        // The most recent requests should still be present
        assert!(client.request_exit_receipt("req-299").is_some());
        assert!(client.request_exit_receipt("req-298").is_some());
    }

    #[test]
    fn test_process_identity_exited() {
        // Non-existent PID must report exited
        assert!(process_identity_exited(999_999_999, 12345).unwrap());

        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::FILETIME;
            use windows_sys::Win32::System::Threading::{
                GetCurrentProcess, GetCurrentProcessId, GetProcessTimes,
            };
            let pid = unsafe { GetCurrentProcessId() };
            let mut created = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            assert_ne!(
                unsafe {
                    GetProcessTimes(
                        GetCurrentProcess(),
                        &mut created,
                        &mut exit,
                        &mut kernel,
                        &mut user,
                    )
                },
                0
            );
            let proc_created =
                (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);

            // Mismatched creation time on running process must report exited (PID reused)
            assert!(process_identity_exited(pid, proc_created + 100).unwrap());

            // Exact match on currently running process must report NOT exited
            assert!(!process_identity_exited(pid, proc_created).unwrap());
        }
    }
}
