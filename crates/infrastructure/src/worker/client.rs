use openunpack_protocol::{WorkerEvent, WorkerRequest};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, atomic::AtomicBool},
};

static DIAGNOSTICS: OnceLock<Mutex<VecDeque<WorkerDiagnostic>>> = OnceLock::new();
static EXIT_RECEIPTS: OnceLock<Mutex<HashMap<String, WorkerExitReceipt>>> = OnceLock::new();
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

        match crate::engine::execute_native_worker(request.clone(), progress, cancel).await {
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

pub fn process_identity_exited(_pid: u32, _creation: u64) -> Result<bool, String> {
    Ok(true)
}
