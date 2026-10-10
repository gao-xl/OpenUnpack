use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordInput {
    pub id: Option<String>,
    pub name: String,
    pub secret: Option<String>,
    pub groups: Vec<String>,
    pub notes: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordView {
    pub id: String, pub name: String, pub groups: Vec<String>, pub notes: String,
    pub enabled: bool, pub success_count: u64, pub last_success: Option<String>, pub revision: u64,
}
#[derive(Clone)]
pub struct Candidate {
    pub id: String, pub revision: u64, pub label: String, pub secret: String,
}
impl Drop for Candidate { fn drop(&mut self) { use zeroize::Zeroize; self.secret.zeroize(); } }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub token: String, pub new_count: usize, pub duplicate_count: usize,
    pub error_count: usize, pub rows: Vec<ImportRow>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRow { pub line: usize, pub name: String, pub group: String, pub status: String, pub message: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult { pub added: usize, pub duplicates: usize }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceGroup {
    pub path: String, pub display_name: String, pub volumes: Vec<String>,
    pub size: u64, pub missing_volume: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult { pub sources: Vec<SourceGroup>, pub notices: Vec<ScanNotice> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanNotice { pub path: String, pub message: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub output_root: String, pub conflict_policy: String, pub password_group: String,
    pub cache_enabled: bool, pub cache_input: bool, pub cache_root: String,
    pub cache_quota_bytes: u64, pub min_free_bytes: u64,
    pub expand_nested: bool, pub max_depth: u32, pub verification: String,
    pub max_entries: u64, pub max_expanded_bytes: u64, pub recursive_scan: bool,
    pub input_exclusions: Vec<String>, pub output_exclusions: Vec<String>,
    pub small_first: bool, pub theme: String, pub history_limit: usize,
    pub always_on_top: bool, pub completion_sound: bool,
    pub minimize_to_tray: bool,
    pub unwrap_single_root: bool, pub source_cleanup: String,
}
impl Default for Settings {
    fn default() -> Self { Self {
        output_root: String::new(), conflict_policy: "rename".into(), password_group: String::new(),
        cache_enabled: false, cache_input: true, cache_root: String::new(),
        cache_quota_bytes: 20 * 1024u64.pow(3), min_free_bytes: 1024u64.pow(3),
        expand_nested: true, max_depth: 2, verification: "standard".into(),
        max_entries: 100_000, max_expanded_bytes: 100 * 1024u64.pow(3),
        recursive_scan: true, input_exclusions: vec![], output_exclusions: vec![],
        small_first: false, theme: "system".into(), history_limit: 500,
        always_on_top: false, completion_sound: false,
        minimize_to_tray: false,
        unwrap_single_root: false, source_cleanup: "keep".into(),
    } }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: String, pub source: SourceGroup, pub options: Settings,
    pub status: String, pub stage: String, pub progress: f64, pub message: String,
    pub attempts: u64, pub output_path: Option<String>, pub verification_scope: String,
    pub created_at: String, pub finished_at: Option<String>, pub workspace: Option<String>,
    pub nodes: Vec<NodeView>, pub stage_times: BTreeMap<String, u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    pub id: String, pub parent_id: Option<String>, pub name: String, pub depth: u32,
    pub status: String, pub stage: String, pub progress: f64, pub attempts: u64,
    pub message: String, pub password_label: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord {
    pub relative_path: String, pub size: u64, pub sha256: String,
    #[serde(default)] pub is_directory: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyFile {
    pub source_path: String, pub relative_path: String, pub size: u64,
    pub sha256: String, #[serde(default)] pub is_directory: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRequest {
    pub protocol_version: u32, pub request_id: String, pub operation: String,
    #[serde(default)] pub archive_path: String, #[serde(default)] pub destination_directory: String,
    pub password: Option<String>, #[serde(default)] pub allowed_volumes: Vec<String>,
    #[serde(default)] pub max_entries: u64, #[serde(default)] pub max_total_bytes: u64,
    #[serde(default)] pub max_written_bytes: u64, #[serde(default)] pub min_free_bytes: u64,
    #[serde(default)] pub files: Vec<CopyFile>, #[serde(default)] pub verification: String,
    #[serde(default)] pub exclusions: Vec<String>,
    #[serde(default)] pub candidate_passwords: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEvent {
    pub protocol_version: u32, pub request_id: String, pub kind: String,
    #[serde(default)] pub completed_bytes: u64, #[serde(default)] pub total_bytes: u64,
    #[serde(default)] pub message: String, #[serde(default)] pub error_code: String,
    #[serde(default)] pub files: Vec<FileRecord>, #[serde(default)] pub manifest_path: Option<String>,
    #[serde(default)] pub password_used: bool,
    #[serde(default)] pub matched_password: Option<String>,
    #[serde(default)] pub probe_status: Option<String>,
    #[serde(default)] pub fingerprint: Option<String>, #[serde(default)] pub verification_scope: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessEvent {
    pub event_id: String, pub password_id: String, pub password_revision: u64,
    pub fingerprint: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Publication {
    pub id: String, pub job_id: String, pub temporary_path: String, pub final_path: String,
    pub manifest_hash: String, pub state: String, pub files: Vec<CopyFile>,
    pub verification: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub jobs: Vec<JobView>, pub passwords: Vec<PasswordView>, pub settings: Settings,
    pub running: bool, pub paused: bool, pub vault_error: Option<String>,
    pub data_directory: String, pub engine_available: bool,
    pub recoverable_count: usize, pub cache_used_bytes: u64,
}
