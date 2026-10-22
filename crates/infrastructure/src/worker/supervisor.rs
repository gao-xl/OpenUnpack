use openunpack_application::{ApplicationError, SessionHandle, WorkerSupervisor};
use openunpack_protocol::{WorkerEvent, WorkerRequest};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

struct ActiveSession {
    job_id: String,
    cancelled: Arc<AtomicBool>,
    terminated: Arc<AtomicBool>,
    completion: Mutex<SessionCompletion>,
}

struct SessionCompletion {
    thread: Option<thread::JoinHandle<Result<(), String>>>,
    result: Option<Result<(), String>>,
}

impl SessionCompletion {
    fn join_finished(&mut self) {
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
            && let Some(thread) = self.thread.take()
        {
            self.result = Some(match thread.join() {
                Ok(result) => result,
                Err(_) => Err("WorkerSessionPanickedExitUnconfirmed".into()),
            });
        }
    }
}

pub struct LocalWorkerSupervisor {
    pub worker_executable: PathBuf,
    pub engine_library: PathBuf,
    sessions: Mutex<HashMap<String, Arc<ActiveSession>>>,
}

impl LocalWorkerSupervisor {
    pub fn new(worker_executable: PathBuf, engine_library: PathBuf) -> Self {
        Self {
            worker_executable,
            engine_library,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn terminate_with_timeout(
        &self,
        handle: &SessionHandle,
        timeout: Duration,
    ) -> Result<(), ApplicationError> {
        let session = {
            let guard = self
                .sessions
                .lock()
                .map_err(|_| ApplicationError::WorkerUnavailable("会话锁已损坏".into()))?;
            guard
                .get(&handle.session_id)
                .cloned()
                .ok_or_else(|| ApplicationError::WorkerUnavailable("未找到活动会话".into()))?
        };
        if session.job_id != handle.job_id {
            return Err(ApplicationError::WorkerUnavailable(
                "会话与任务不匹配".into(),
            ));
        }
        session.cancelled.store(true, Ordering::SeqCst);
        session.terminated.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + timeout;
        loop {
            // A concurrent caller observes the same completion. No caller may infer
            // success from a JoinHandle temporarily owned by another caller.
            match session.completion.try_lock() {
                Ok(mut completion) => {
                    completion.join_finished();
                    match &completion.result {
                        Some(Ok(())) => break,
                        Some(Err(error)) => {
                            return Err(ApplicationError::WorkerUnavailable(error.clone()));
                        }
                        None => {}
                    }
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(ApplicationError::WorkerUnavailable(
                        "会话完成状态锁已损坏，退出未确认".into(),
                    ));
                }
                Err(std::sync::TryLockError::WouldBlock) => {}
            }
            if Instant::now() >= deadline {
                // Retain the JoinHandle: a later retry can confirm a delayed exit.
                return Err(ApplicationError::WorkerUnavailable(
                    "WorkerExitUnconfirmed".into(),
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
        let mut guard = self
            .sessions
            .lock()
            .map_err(|_| ApplicationError::WorkerUnavailable("会话锁已损坏".into()))?;
        if guard
            .get(&handle.session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &session))
        {
            guard.remove(&handle.session_id);
        }
        Ok(())
    }
}

impl WorkerSupervisor for LocalWorkerSupervisor {
    fn is_available(&self) -> bool {
        self.worker_executable.is_file() && self.engine_library.is_file()
    }

    fn start_session(
        &self,
        job_id: &str,
    ) -> Result<(SessionHandle, Sender<WorkerRequest>, Receiver<WorkerEvent>), ApplicationError>
    {
        // Reclaim naturally completed sessions while retaining uncertain exits.
        {
            let mut guard = self
                .sessions
                .lock()
                .map_err(|_| ApplicationError::WorkerUnavailable("会话锁已损坏".into()))?;
            guard.retain(|_, session| {
                if let Ok(mut completion) = session.completion.try_lock() {
                    completion.join_finished();
                    return !matches!(completion.result, Some(Ok(())));
                }
                true
            });
        }
        let (req_tx, req_rx) = channel::<WorkerRequest>();
        let (evt_tx, evt_rx) = channel::<WorkerEvent>();

        let session_id = uuid::Uuid::new_v4().to_string();
        let session = SessionHandle {
            session_id: session_id.clone(),
            job_id: job_id.to_string(),
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let terminated = Arc::new(AtomicBool::new(false));

        let executable = self.worker_executable.clone();
        let engine = self.engine_library.clone();
        let thread_cancelled = cancelled.clone();
        let thread_terminated = terminated.clone();

        let worker_thread = thread::Builder::new()
            .name(format!("worker-session-{}", job_id))
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        return Err(format!("创建异步运行时失败: {}", e));
                    }
                };

                loop {
                    if thread_terminated.load(Ordering::SeqCst)
                        || thread_cancelled.load(Ordering::SeqCst)
                    {
                        break;
                    }
                    let req = match req_rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(req) => req,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    if thread_terminated.load(Ordering::SeqCst)
                        || thread_cancelled.load(Ordering::SeqCst)
                    {
                        break;
                    }
                    if !executable.is_file() || !engine.is_file() {
                        let mut failed = WorkerEvent::default();
                        failed.protocol_version = req.protocol_version;
                        failed.request_id = req.request_id.clone();
                        failed.kind = "failed".into();
                        failed.operation_id = req.operation_id.clone();
                        failed.generation = req.generation;
                        failed.message = format!("Worker 或 7z 引擎不存在: {:?}", executable);
                        failed.error_code = "WorkerUnavailable".into();
                        let _ = evt_tx.send(failed);
                        continue;
                    }

                    let client = super::client::WorkerClient {
                        executable: executable.clone(),
                        engine: engine.clone(),
                    };

                    let cancel_flag = thread_cancelled.clone();
                    let evt_tx_clone = evt_tx.clone();
                    let progress = Arc::new(move |evt: &WorkerEvent| {
                        let _ = evt_tx_clone.send(evt.clone());
                    });

                    match rt.block_on(client.run(req.clone(), cancel_flag, progress)) {
                        Ok(event) => {
                            let _ = evt_tx.send(event);
                        }
                        Err(err) => {
                            let exit_unconfirmed = err == "WorkerExitUnconfirmed";
                            let mut failed = WorkerEvent::default();
                            failed.protocol_version = req.protocol_version;
                            failed.request_id = req.request_id.clone();
                            failed.kind = "failed".into();
                            failed.operation_id = req.operation_id.clone();
                            failed.generation = req.generation;
                            failed.message = err.clone();
                            failed.error_code = err;
                            let _ = evt_tx.send(failed);
                            if exit_unconfirmed {
                                return Err("WorkerExitUnconfirmed".into());
                            }
                        }
                    }
                }
                Ok(())
            })
            .map_err(|e| {
                ApplicationError::WorkerUnavailable(format!("无法启动会话工作线程: {}", e))
            })?;

        self.sessions
            .lock()
            .map_err(|_| ApplicationError::WorkerUnavailable("会话锁已损坏".into()))?
            .insert(
                session_id,
                Arc::new(ActiveSession {
                    job_id: job_id.to_string(),
                    cancelled,
                    terminated,
                    completion: Mutex::new(SessionCompletion {
                        thread: Some(worker_thread),
                        result: None,
                    }),
                }),
            );

        Ok((session, req_tx, evt_rx))
    }

    fn request_stop(&self, handle: &SessionHandle) -> Result<(), ApplicationError> {
        let guard = self
            .sessions
            .lock()
            .map_err(|_| ApplicationError::WorkerUnavailable("会话锁已损坏".into()))?;
        if let Some(session) = guard.get(&handle.session_id) {
            if session.job_id != handle.job_id {
                return Err(ApplicationError::WorkerUnavailable(
                    "会话与任务不匹配".into(),
                ));
            }
            session.cancelled.store(true, Ordering::SeqCst);
            Ok(())
        } else {
            Err(ApplicationError::WorkerUnavailable(format!(
                "未找到活动会话: {}",
                handle.session_id
            )))
        }
    }

    fn terminate(&self, handle: &SessionHandle) -> Result<(), ApplicationError> {
        self.terminate_with_timeout(handle, Duration::from_secs(55))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminate_joins_idle_session_and_disconnects_request_channel() {
        let supervisor = LocalWorkerSupervisor::new(
            PathBuf::from("missing-worker"),
            PathBuf::from("missing-engine"),
        );
        let (handle, requests, events) = supervisor.start_session("idle").unwrap();
        supervisor.terminate(&handle).unwrap();
        assert!(supervisor.sessions.lock().unwrap().is_empty());
        assert!(matches!(
            events.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ));
        assert!(
            requests
                .send(
                    serde_json::from_value(serde_json::json!({
                        "protocolVersion": 1, "requestId": "after-terminate", "operation": "copy"
                    }))
                    .unwrap()
                )
                .is_err()
        );
    }

    #[test]
    fn uncertain_exit_is_retained_and_cannot_report_success() {
        let supervisor = LocalWorkerSupervisor::new(PathBuf::new(), PathBuf::new());
        let handle = SessionHandle {
            session_id: "uncertain".into(),
            job_id: "job".into(),
        };
        supervisor.sessions.lock().unwrap().insert(
            handle.session_id.clone(),
            Arc::new(ActiveSession {
                job_id: handle.job_id.clone(),
                cancelled: Arc::new(AtomicBool::new(false)),
                terminated: Arc::new(AtomicBool::new(false)),
                completion: Mutex::new(SessionCompletion {
                    thread: Some(thread::spawn(|| Err("WorkerExitUnconfirmed".into()))),
                    result: None,
                }),
            }),
        );
        assert!(supervisor.terminate(&handle).is_err());
        assert!(
            supervisor
                .sessions
                .lock()
                .unwrap()
                .contains_key(&handle.session_id)
        );
        assert!(supervisor.terminate(&handle).is_err());
    }

    fn blocked_session(supervisor: &LocalWorkerSupervisor) -> (SessionHandle, Sender<()>) {
        let handle = SessionHandle {
            session_id: "blocked".into(),
            job_id: "blocked-job".into(),
        };
        let (release, wait) = channel();
        supervisor.sessions.lock().unwrap().insert(
            handle.session_id.clone(),
            Arc::new(ActiveSession {
                job_id: handle.job_id.clone(),
                cancelled: Arc::new(AtomicBool::new(false)),
                terminated: Arc::new(AtomicBool::new(false)),
                completion: Mutex::new(SessionCompletion {
                    thread: Some(thread::spawn(move || {
                        wait.recv().unwrap();
                        Ok(())
                    })),
                    result: None,
                }),
            }),
        );
        (handle, release)
    }

    #[test]
    fn concurrent_terminate_never_reports_success_while_thread_is_still_running() {
        let supervisor = Arc::new(LocalWorkerSupervisor::new(PathBuf::new(), PathBuf::new()));
        let (handle, release) = blocked_session(&supervisor);
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut callers = vec![];
        for _ in 0..2 {
            let supervisor = supervisor.clone();
            let handle = handle.clone();
            let barrier = barrier.clone();
            callers.push(thread::spawn(move || {
                barrier.wait();
                supervisor.terminate_with_timeout(&handle, Duration::from_millis(60))
            }));
        }
        barrier.wait();
        for caller in callers {
            assert!(caller.join().unwrap().is_err(), "no exit has occurred yet");
        }
        assert!(
            supervisor
                .sessions
                .lock()
                .unwrap()
                .contains_key(&handle.session_id)
        );
        // A different job can register while this one waits for exit.
        let (other, _, _) = supervisor.start_session("other").unwrap();
        supervisor.terminate(&other).unwrap();
        release.send(()).unwrap();
        supervisor.terminate(&handle).unwrap();
        assert!(supervisor.sessions.lock().unwrap().is_empty());
    }

    #[test]
    fn absent_join_handle_is_not_an_exit_confirmation() {
        let supervisor = LocalWorkerSupervisor::new(PathBuf::new(), PathBuf::new());
        let handle = SessionHandle {
            session_id: "owner-has-handle".into(),
            job_id: "job".into(),
        };
        supervisor.sessions.lock().unwrap().insert(
            handle.session_id.clone(),
            Arc::new(ActiveSession {
                job_id: handle.job_id.clone(),
                cancelled: Arc::new(AtomicBool::new(false)),
                terminated: Arc::new(AtomicBool::new(false)),
                completion: Mutex::new(SessionCompletion {
                    thread: None,
                    result: None,
                }),
            }),
        );
        assert!(
            supervisor
                .terminate_with_timeout(&handle, Duration::from_millis(5))
                .is_err()
        );
        assert!(
            supervisor
                .sessions
                .lock()
                .unwrap()
                .contains_key(&handle.session_id)
        );
    }
}
