use serde::{Deserialize, Serialize};

pub const WORKER_PROTOCOL_VERSION_V1: u32 = 1;
pub const WORKER_PROTOCOL_VERSION_V2: u32 = 2;
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

fn known_size() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord {
    pub relative_path: String,
    pub size: u64,
    #[serde(default = "known_size")]
    pub size_known: bool,
    pub sha256: String,
    #[serde(default)]
    pub is_directory: bool,
    #[serde(default)]
    pub is_encrypted: bool,
    #[serde(default)]
    pub is_corrupted: bool,
}

impl Default for FileRecord {
    fn default() -> Self {
        Self {
            relative_path: String::new(),
            size: 0,
            size_known: true,
            sha256: String::new(),
            is_directory: false,
            is_encrypted: false,
            is_corrupted: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyFile {
    pub source_path: String,
    pub relative_path: String,
    pub size: u64,
    #[serde(default = "known_size")]
    pub size_known: bool,
    pub sha256: String,
    #[serde(default)]
    pub is_directory: bool,
    #[serde(default)]
    pub is_corrupted: bool,
}

impl Default for CopyFile {
    fn default() -> Self {
        Self {
            source_path: String::new(),
            relative_path: String::new(),
            size: 0,
            size_known: true,
            sha256: String::new(),
            is_directory: false,
            is_corrupted: false,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateItem {
    pub candidate_id: String,
    pub source: String,
    #[serde(default)]
    pub revision: u64,
    pub label: String,
    pub secret: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkerCapabilities {
    pub backend_id: String,
    pub backend_version: String,
    pub read_metadata: bool,
    pub extract_with_candidates: bool,
    pub solid_optimization: bool,
    pub max_batch_candidates: usize,
    pub backends: Vec<WorkerBackend>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkerBackend {
    pub backend_id: String,
    pub backend_version: String,
    pub formats: Vec<String>,
    pub read_metadata: bool,
    pub encrypted_archives: bool,
    pub unknown_expanded_size: bool,
    pub verification_scopes: Vec<String>,
}

impl Default for WorkerCapabilities {
    fn default() -> Self {
        Self {
            backend_id: "7-Zip".into(),
            backend_version: "26.04".into(),
            read_metadata: true,
            extract_with_candidates: false,
            solid_optimization: false,
            max_batch_candidates: 128,
            backends: vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerHello {
    pub protocol_version: u32,
    pub major: u32,
    pub minor: u32,
    pub capabilities: WorkerCapabilities,
}

impl Default for WorkerHello {
    fn default() -> Self {
        Self {
            protocol_version: WORKER_PROTOCOL_VERSION_V2,
            major: 2,
            minor: 0,
            capabilities: WorkerCapabilities::default(),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRequest {
    pub protocol_version: u32,
    pub request_id: String,
    pub operation: String,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub archive_path: String,
    #[serde(default)]
    pub destination_directory: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub allowed_volumes: Vec<String>,
    #[serde(default)]
    pub max_entries: u64,
    #[serde(default)]
    pub max_total_bytes: u64,
    #[serde(default)]
    pub max_written_bytes: u64,
    #[serde(default)]
    pub min_free_bytes: u64,
    #[serde(default)]
    pub files: Vec<CopyFile>,
    #[serde(default)]
    pub verification: String,
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default)]
    pub inclusions: Vec<String>,
    #[serde(default)]
    pub candidate_passwords: Vec<String>,
    #[serde(default)]
    pub candidates: Vec<CandidateItem>,
    #[serde(default)]
    pub engine_threads: u32,
    #[serde(default)]
    pub worker_priority: String,
    #[serde(default)]
    pub salvage_corrupted: bool,
}

impl Default for WorkerRequest {
    fn default() -> Self {
        Self {
            protocol_version: WORKER_PROTOCOL_VERSION_V1,
            request_id: String::new(),
            operation: String::new(),
            operation_id: None,
            generation: None,
            archive_path: String::new(),
            destination_directory: String::new(),
            password: None,
            allowed_volumes: vec![],
            max_entries: 0,
            max_total_bytes: 0,
            max_written_bytes: 0,
            min_free_bytes: 0,
            files: vec![],
            verification: String::new(),
            exclusions: vec![],
            inclusions: vec![],
            candidate_passwords: vec![],
            candidates: vec![],
            engine_threads: 0,
            worker_priority: String::new(),
            salvage_corrupted: false,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEvent {
    pub protocol_version: u32,
    pub request_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hello: Option<WorkerHello>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub completed_bytes: u64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default = "known_size")]
    pub total_bytes_known: bool,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub error_code: String,
    #[serde(default)]
    pub files: Vec<FileRecord>,
    #[serde(default)]
    pub manifest_path: Option<String>,
    #[serde(default)]
    pub manifest_id: Option<String>,
    #[serde(default)]
    pub manifest_sha256: Option<String>,
    #[serde(default)]
    pub manifest_byte_length: Option<u64>,
    #[serde(default)]
    pub password_used: bool,
    #[serde(default)]
    pub matched_password: Option<String>,
    #[serde(default)]
    pub matched_candidate_id: Option<String>,
    #[serde(default)]
    pub probe_status: Option<String>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub verification_scope: String,
}

impl Default for WorkerEvent {
    fn default() -> Self {
        Self {
            protocol_version: WORKER_PROTOCOL_VERSION_V1,
            request_id: String::new(),
            kind: String::new(),
            hello: None,
            operation_id: None,
            generation: None,
            completed_bytes: 0,
            total_bytes: 0,
            total_bytes_known: true,
            message: String::new(),
            error_code: String::new(),
            files: vec![],
            manifest_path: None,
            manifest_id: None,
            manifest_sha256: None,
            manifest_byte_length: None,
            password_used: false,
            matched_password: None,
            matched_candidate_id: None,
            probe_status: None,
            fingerprint: None,
            verification_scope: String::new(),
        }
    }
}

impl Drop for CandidateItem {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
}

impl std::fmt::Debug for CandidateItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the secret: Debug output may reach logs or panic messages.
        f.debug_struct("CandidateItem")
            .field("candidate_id", &self.candidate_id)
            .field("source", &self.source)
            .field("revision", &self.revision)
            .field("label", &self.label)
            .field(
                "secret",
                &if self.secret.is_empty() {
                    "empty"
                } else {
                    "[REDACTED]"
                },
            )
            .finish()
    }
}

impl Drop for WorkerRequest {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        if let Some(secret) = self.password.as_mut() {
            secret.zeroize();
        }
        for secret in &mut self.candidate_passwords {
            secret.zeroize();
        }
        // `candidates` needs no extra pass: CandidateItem wipes its own secret on drop.
    }
}

impl std::fmt::Debug for WorkerRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render credential fields: Debug output may reach logs or panic messages.
        f.debug_struct("WorkerRequest")
            .field("protocol_version", &self.protocol_version)
            .field("request_id", &self.request_id)
            .field("operation", &self.operation)
            .field("operation_id", &self.operation_id)
            .field("generation", &self.generation)
            .field("archive_path", &self.archive_path)
            .field("destination_directory", &self.destination_directory)
            .field("password", &self.password.as_deref().map(|_| "[REDACTED]"))
            .field("allowed_volumes", &self.allowed_volumes)
            .field("max_entries", &self.max_entries)
            .field("max_total_bytes", &self.max_total_bytes)
            .field("max_written_bytes", &self.max_written_bytes)
            .field("min_free_bytes", &self.min_free_bytes)
            .field("files", &self.files)
            .field("verification", &self.verification)
            .field("exclusions", &self.exclusions)
            .field(
                "candidate_passwords",
                &self
                    .candidate_passwords
                    .iter()
                    .map(|_| "[REDACTED]")
                    .collect::<Vec<_>>(),
            )
            .field("candidates", &self.candidates)
            .field("engine_threads", &self.engine_threads)
            .field("worker_priority", &self.worker_priority)
            .field("salvage_corrupted", &self.salvage_corrupted)
            .finish()
    }
}

impl Drop for WorkerEvent {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        // v2 rejects echoed secrets before use; wipe defensively in case a
        // protocol-violating payload is parsed and then discarded.
        if let Some(secret) = self.matched_password.as_mut() {
            secret.zeroize();
        }
    }
}

impl std::fmt::Debug for WorkerEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the echoed credential: Debug output may reach logs or panic messages.
        f.debug_struct("WorkerEvent")
            .field("protocol_version", &self.protocol_version)
            .field("request_id", &self.request_id)
            .field("kind", &self.kind)
            .field("hello", &self.hello)
            .field("operation_id", &self.operation_id)
            .field("generation", &self.generation)
            .field("completed_bytes", &self.completed_bytes)
            .field("total_bytes", &self.total_bytes)
            .field("message", &self.message)
            .field("error_code", &self.error_code)
            .field("files", &self.files)
            .field("manifest_path", &self.manifest_path)
            .field("manifest_id", &self.manifest_id)
            .field("manifest_sha256", &self.manifest_sha256)
            .field("manifest_byte_length", &self.manifest_byte_length)
            .field("password_used", &self.password_used)
            .field(
                "matched_password",
                &self.matched_password.as_deref().map(|_| "[REDACTED]"),
            )
            .field("matched_candidate_id", &self.matched_candidate_id)
            .field("probe_status", &self.probe_status)
            .field("fingerprint", &self.fingerprint)
            .field("verification_scope", &self.verification_scope)
            .finish()
    }
}
