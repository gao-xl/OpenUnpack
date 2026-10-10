use serde::{Deserialize, Serialize};

pub const WORKER_PROTOCOL_VERSION_V1: u32 = 1;
pub const WORKER_PROTOCOL_VERSION_V2: u32 = 2;
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord {
    pub relative_path: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub is_directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyFile {
    pub source_path: String,
    pub relative_path: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub is_directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
}

impl Default for WorkerCapabilities {
    fn default() -> Self {
        Self {
            backend_id: "7-Zip".into(),
            backend_version: "26.04".into(),
            read_metadata: true,
            extract_with_candidates: true,
            solid_optimization: true,
            max_batch_candidates: 128,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    pub candidate_passwords: Vec<String>,
    #[serde(default)]
    pub candidates: Vec<CandidateItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEvent {
    pub protocol_version: u32,
    pub request_id: String,
    pub kind: String,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub completed_bytes: u64,
    #[serde(default)]
    pub total_bytes: u64,
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
