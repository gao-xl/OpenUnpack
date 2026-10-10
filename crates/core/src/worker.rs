use crate::types::{WorkerEvent, WorkerRequest};
use std::{path::PathBuf, sync::{Arc,Mutex,OnceLock, atomic::AtomicBool}};
static UNCONFIRMED:OnceLock<Mutex<Vec<(u32,u64)>>>=OnceLock::new();

#[derive(Clone)]
pub struct WorkerClient {
    pub executable: PathBuf,
    pub engine: PathBuf,
}

impl WorkerClient {
    pub fn unconfirmed_processes(&self)->Vec<(u32,u64)>{UNCONFIRMED.get_or_init(||Mutex::new(vec![])).lock().map(|v|v.clone()).unwrap_or_else(|_|vec![(0,0)])}
    pub async fn run(&self, request: WorkerRequest, cancel: Arc<AtomicBool>,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>) -> Result<WorkerEvent, String> {
        #[cfg(windows)]
        { windows::run(self, request, cancel, progress).await }
        #[cfg(not(windows))]
        { let _ = (request, cancel, progress); Err("WorkerUnsupportedPlatform".into()) }
    }
}

pub fn process_identity_exited(pid:u32,creation:u64)->Result<bool,String>{
    #[cfg(windows)]{windows::process_identity_exited(pid,creation)}
    #[cfg(not(windows))]{let _=(pid,creation);Err("WorkerUnsupportedPlatform".into())}
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{ffi::c_void, mem::size_of, os::windows::io::AsRawHandle, process::Stdio,
        ptr::null, sync::atomic::Ordering, time::Duration};
    use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::windows::named_pipe::{ClientOptions, NamedPipeClient},
        process::{Child, Command}, sync::{mpsc, oneshot, watch},
        time::{Instant, interval, sleep, timeout}};
    use windows_sys::Win32::{Foundation::{CloseHandle, HANDLE,FILETIME,GetLastError,ERROR_INVALID_PARAMETER,WAIT_OBJECT_0,WAIT_TIMEOUT}, Storage::FileSystem::SECURITY_IDENTIFICATION,
        System::{JobObjects::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
            JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE},
            Pipes::GetNamedPipeServerProcessId, Threading::{CREATE_NO_WINDOW,GetProcessTimes,OpenProcess,WaitForSingleObject,PROCESS_QUERY_LIMITED_INFORMATION,PROCESS_SYNCHRONIZE}}};
    use zeroize::{Zeroize, Zeroizing};

    const MAX_MESSAGE: usize = 8 * 1024 * 1024;

    fn creation_time(handle:HANDLE)->Result<u64,String>{unsafe{let mut created:FILETIME=std::mem::zeroed();let mut exit:FILETIME=std::mem::zeroed();let mut kernel:FILETIME=std::mem::zeroed();let mut user:FILETIME=std::mem::zeroed();if GetProcessTimes(handle,&mut created,&mut exit,&mut kernel,&mut user)==0{return Err("WorkerProcessIdentityUnavailable".into());}Ok((created.dwHighDateTime as u64)<<32|created.dwLowDateTime as u64)}}
    pub(super) fn process_identity_exited(pid:u32,creation:u64)->Result<bool,String>{
        if pid==0||creation==0{return Err("WorkerProcessIdentityUnavailable".into());}
        unsafe {
            let handle=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION|PROCESS_SYNCHRONIZE,0,pid);
            if handle.is_null(){return if GetLastError()==ERROR_INVALID_PARAMETER{Ok(true)}else{Err("WorkerProcessIdentityUnavailable".into())};}
            let identity=creation_time(handle);let signal=WaitForSingleObject(handle,0);CloseHandle(handle);
            match identity{Ok(actual)if actual!=creation=>Ok(true),Ok(_)=>match signal{WAIT_OBJECT_0=>Ok(true),WAIT_TIMEOUT=>Ok(false),_=>Err("WorkerProcessIdentityUnavailable".into())},Err(e)=>Err(e)}
        }
    }

    struct JobGuard(HANDLE);
    // A job HANDLE can be used/closed from any thread. It owns no borrowed memory.
    unsafe impl Send for JobGuard {}
    impl Drop for JobGuard { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
    impl JobGuard {
        fn attach(child: &Child) -> Result<Self, String> {
            unsafe {
                let handle = CreateJobObjectW(null(), null());
                if handle.is_null() { return Err("WorkerJobObjectUnavailable".into()); }
                let guard = Self(handle);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(handle, JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const c_void, size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32) == 0
                    || child.raw_handle().is_none_or(|process| AssignProcessToJobObject(handle, process as HANDLE) == 0) {
                    return Err("WorkerJobObjectUnavailable".into());
                }
                Ok(guard)
            }
        }
    }

    pub(super) async fn run(client: &WorkerClient, mut request: WorkerRequest, cancel: Arc<AtomicBool>,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>) -> Result<WorkerEvent, String> {
        if !client.executable.is_absolute() || !client.engine.is_absolute()
            || !client.executable.is_file() || !client.engine.is_file() {
            return Err("WorkerUnavailable".into());
        }
        let prefix = format!("openunpack-{}", uuid::Uuid::new_v4());
        let mut child = Command::new(&client.executable)
            .arg("--pipe-prefix").arg(&prefix).arg("--parent-pid").arg(std::process::id().to_string())
            .arg("--engine").arg(&client.engine).creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true)
            .spawn().map_err(|_| "WorkerStartFailed".to_string())?;
        let _job = match JobGuard::attach(&child) {
            Ok(job) => job,
            Err(error) => { confirm_exit(&mut child, true).await?; return Err(error); }
        };
        let worker_pid = match child.id() {
            Some(pid) => pid,
            None => { confirm_exit(&mut child, true).await?; return Err("WorkerStartFailed".into()); }
        };
        let serialized = serde_json::to_vec(&request).map_err(|_| "WorkerProtocolError".to_string());
        if let Some(secret) = request.password.as_mut() { secret.zeroize(); }
        request.password = None;
        let mut result = match serialized {
            Ok(bytes) if bytes.len() <= MAX_MESSAGE => {
                let bytes = Zeroizing::new(bytes);
                communicate(&mut child, &prefix, worker_pid, &request.request_id, bytes, cancel, progress).await
            }
            _ => Err("WorkerProtocolError".into())
        };
        confirm_exit(&mut child, result.is_err()).await?;
        if let Ok(ref mut event) = result {
            if let Some(manifest_path) = event.manifest_path.take() {
                let p = std::path::Path::new(&manifest_path);
                if p.exists() {
                    let bytes = std::fs::read(p).map_err(|_| "WorkerManifestReadFailed".to_string())?;
                    event.files = serde_json::from_slice(&bytes).map_err(|_| "WorkerManifestCorrupted".to_string())?;
                    let _ = std::fs::remove_file(p);
                }
            }
        }
        result
    }

    async fn confirm_exit(child: &mut Child, force: bool) -> Result<(), String> {
        let identity=(child.id().unwrap_or(0),child.raw_handle().and_then(|h|creation_time(h as HANDLE).ok()).unwrap_or(0));
        if force { let _ = child.start_kill(); }
        if let Ok(Ok(_)) = timeout(Duration::from_secs(10), child.wait()).await { return Ok(()); }
        let _ = child.start_kill();
        match timeout(Duration::from_secs(10), child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            _ => {if let Ok(mut registry)=UNCONFIRMED.get_or_init(||Mutex::new(vec![])).lock(){if !registry.contains(&identity){registry.push(identity);}}Err("WorkerExitUnconfirmed".into())},
        }
    }

    async fn connect(child: &mut Child, name: &str, pid: u32) -> Result<NamedPipeClient, String> {
        let started = Instant::now();
        loop {
            match ClientOptions::new().read(true).write(true).security_qos_flags(SECURITY_IDENTIFICATION).open(name) {
                Ok(pipe) => {
                    let mut server = 0;
                    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut server) } == 0 || server != pid {
                        return Err("WorkerPipeIdentityMismatch".into());
                    }
                    return Ok(pipe);
                }
                Err(error) if matches!(error.raw_os_error(), Some(2 | 231)) => {
                    if child.try_wait().map_err(|_| "WorkerProcessUnavailable")?.is_some() {
                        return Err("WorkerStartFailed".into());
                    }
                    if started.elapsed() >= Duration::from_secs(30) { return Err("WorkerConnectTimeout".into()); }
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
        }).await.map_err(|_| "WorkerControlTimeout".to_string())?
            .map_err(|_| "WorkerControlFailed".to_string())
    }

    async fn read_event(pipe: &mut NamedPipeClient, pending: &mut Vec<u8>) -> Result<WorkerEvent, String> {
        let mut searched = 0;
        loop {
            if let Some(relative_end) = pending[searched..].iter().position(|b| *b == b'\n') {
                let end = searched + relative_end;
                if end > MAX_MESSAGE { return Err("WorkerProtocolError".into()); }
                let event = serde_json::from_slice(&pending[..end]).map_err(|_| "WorkerProtocolError".to_string())?;
                pending.drain(..=end);
                return Ok(event);
            }
            if pending.len() > MAX_MESSAGE { return Err("WorkerProtocolError".into()); }
            searched = pending.len();
            let mut buffer = [0u8; 4096];
            let available = (MAX_MESSAGE + 1 - pending.len()).min(buffer.len());
            let count = pipe.read(&mut buffer[..available]).await.map_err(|_| "WorkerDisconnected".to_string())?;
            if count == 0 { return Err("WorkerDisconnected".into()); }
            pending.extend_from_slice(&buffer[..count]);
        }
    }

    struct ControlCommand { bytes: Vec<u8>, ack: oneshot::Sender<Result<(), String>> }
    fn command(request_id: &str, operation: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({ "protocolVersion": 1, "requestId": request_id, "operation": operation }))
            .expect("fixed control fields are serializable")
    }

    async fn control_loop(mut pipe: NamedPipeClient, mut commands: mpsc::Receiver<ControlCommand>,
        request_id: String, cancel: Arc<AtomicBool>) -> Result<(), String> {
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
                        write_message(&mut pipe, &command(&request_id, "cancel")).await?;
                        cancellation_sent = true;
                    }
                }
            }
        }
    }

    async fn communicate(child: &mut Child, prefix: &str, pid: u32, request_id: &str,
        bytes: Zeroizing<Vec<u8>>, cancel: Arc<AtomicBool>, progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>)
        -> Result<WorkerEvent, String> {
        let mut control = connect(child, &format!(r"\\.\pipe\{prefix}-control"), pid).await?;
        let mut events = connect(child, &format!(r"\\.\pipe\{prefix}-events"), pid).await?;
        let mut pending = Vec::new();
        let ready = timeout(Duration::from_secs(30), read_event(&mut events, &mut pending)).await
            .map_err(|_| "WorkerReadyTimeout".to_string())??;
        if ready.protocol_version != 1 || ready.kind != "ready" || !ready.request_id.is_empty() {
            return Err("WorkerProtocolError".into());
        }
        write_message(&mut control, &bytes).await?;
        drop(bytes);
        let (command_tx, command_rx) = mpsc::channel(4);
        let mut controller = tokio::spawn(control_loop(control, command_rx, request_id.into(), cancel.clone()));
        let (event_tx, mut event_rx) = mpsc::channel(64);
        let reader = tokio::spawn(async move {
            loop {
                let event = read_event(&mut events, &mut pending).await;
                let failed = event.is_err();
                if event_tx.send(event).await.is_err() || failed { return; }
            }
        });
        let (latest_tx, mut latest_rx) = watch::channel::<Option<WorkerEvent>>(None);
        let observer = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(200));
            loop {
                ticker.tick().await;
                if latest_rx.has_changed().unwrap_or(false) {
                    let event = latest_rx.borrow_and_update().clone();
                    // Callbacks are invoked outside any lock; UI delivery must remain nonblocking.
                    if let Some(event) = event { progress(&event); }
                }
            }
        });
        let mut ticker = interval(Duration::from_millis(200));
        let mut last_response = Instant::now();
        let mut cancellation_started = None;
        let mut unresponsive = false;
        let result = loop {
            tokio::select! {
                event = event_rx.recv() => {
                    let event = match event {
                        Some(Ok(event)) => event,
                        Some(Err(error)) => break Err(error),
                        None => break Err("WorkerDisconnected".into()),
                    };
                    if event.protocol_version != 1 || event.request_id != request_id {
                        break Err("WorkerProtocolError".into());
                    }
                    last_response = Instant::now();
                    match event.kind.as_str() {
                        "heartbeat" => {},
                        "progress" => { let _ = latest_tx.send(Some(event)); },
                        "completed" | "error" | "cancelled" => {
                            if unresponsive { break Err("WorkerUnresponsive".into()); }
                            let (ack_tx, ack_rx) = oneshot::channel();
                            let exit = ControlCommand { bytes: command(request_id, "exit"), ack: ack_tx };
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
                    if last_response.elapsed() >= Duration::from_secs(120) && !cancel.load(Ordering::Acquire) {
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
        reader.abort(); controller.abort(); observer.abort();
        // The OS JobObject remains alive until run() confirms exit. Aborted tasks cannot
        // retain pipe handles indefinitely; none owns the job HANDLE.
        result
    }
}
