use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DESKTOP_API_VERSION: u32 = 1;
pub const MAX_CONCURRENT_JOBS: u32 = 5;

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordInput {
    pub id: Option<String>,
    pub name: String,
    pub secret: Option<String>,
    pub groups: Vec<String>,
    pub notes: String,
    pub enabled: bool,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordView {
    pub id: String,
    pub name: String,
    pub groups: Vec<String>,
    pub notes: String,
    pub enabled: bool,
    pub success_count: u64,
    pub last_success: Option<String>,
    pub revision: u64,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRow {
    pub line: usize,
    pub name: String,
    pub group: String,
    pub status: String,
    pub message: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub token: String,
    pub new_count: usize,
    pub duplicate_count: usize,
    pub error_count: usize,
    pub rows: Vec<ImportRow>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub added: usize,
    pub duplicates: usize,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceGroup {
    pub path: String,
    pub display_name: String,
    pub volumes: Vec<String>,
    pub size: u64,
    pub missing_volume: bool,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanNotice {
    pub path: String,
    pub message: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub sources: Vec<SourceGroup>,
    pub notices: Vec<ScanNotice>,
}

fn default_concurrency() -> u32 {
    1
}

fn default_auto_start_on_add() -> bool {
    true
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub output_root: String,
    pub conflict_policy: String,
    pub password_group: String,
    pub cache_enabled: bool,
    pub cache_policy: String,
    pub cache_input: bool,
    pub cache_root: String,
    pub cache_quota_bytes: u64,
    pub min_free_bytes: u64,
    pub expand_nested: bool,
    pub max_depth: u32,
    pub verification: String,
    pub max_entries: u64,
    pub max_expanded_bytes: u64,
    pub recursive_scan: bool,
    pub input_exclusions: Vec<String>,
    pub output_exclusions: Vec<String>,
    #[serde(default)]
    pub output_inclusions: Vec<String>,
    pub small_first: bool,
    pub theme: String,
    pub history_limit: usize,
    pub always_on_top: bool,
    pub completion_sound: bool,
    pub minimize_to_tray: bool,
    pub unwrap_single_root: bool,
    pub source_cleanup: String,
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
    pub engine_threads: u32,
    pub worker_priority: String,
    pub salvage_corrupted: bool,
    pub auto_fix_encoding: bool,
    pub clean_empty_dirs: bool,
    #[serde(default = "default_deduplicate_hardlinks")]
    pub deduplicate_hardlinks: bool,
    #[serde(default = "default_auto_start_on_add")]
    pub auto_start_on_add: bool,
    #[serde(default = "default_post_extraction_action")]
    pub post_extraction_action: String,
    #[serde(default)]
    pub on_complete: Option<String>,
    #[serde(default)]
    pub watched_folders: Vec<String>,
}

fn default_deduplicate_hardlinks() -> bool {
    true
}

fn default_post_extraction_action() -> String {
    "none".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_root: String::new(),
            conflict_policy: "rename".into(),
            password_group: String::new(),
            cache_enabled: false,
            cache_policy: "auto".into(),
            cache_input: true,
            cache_root: String::new(),
            cache_quota_bytes: 20 * 1024u64.pow(3),
            min_free_bytes: 1024u64.pow(3),
            expand_nested: true,
            max_depth: 2,
            verification: "standard".into(),
            max_entries: 100_000,
            max_expanded_bytes: 100 * 1024u64.pow(3),
            recursive_scan: true,
            input_exclusions: vec![],
            output_exclusions: vec![],
            output_inclusions: vec![],
            small_first: false,
            theme: "system".into(),
            history_limit: 500,
            always_on_top: false,
            completion_sound: false,
            minimize_to_tray: false,
            unwrap_single_root: false,
            source_cleanup: "keep".into(),
            concurrency: 1,
            engine_threads: 0,
            worker_priority: "belowNormal".into(),
            salvage_corrupted: true,
            auto_fix_encoding: true,
            clean_empty_dirs: true,
            deduplicate_hardlinks: true,
            auto_start_on_add: true,
            post_extraction_action: "none".into(),
            on_complete: None,
            watched_folders: vec![],
        }
    }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub depth: u32,
    pub status: String,
    pub stage: String,
    pub progress: f64,
    pub attempts: u64,
    pub message: String,
    pub password_label: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_second: Option<f64>,
    pub remaining_seconds: Option<f64>,
    pub updated_at: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: String,
    pub source: SourceGroup,
    pub options: Settings,
    pub status: String,
    pub stage: String,
    pub progress: f64,
    pub message: String,
    pub attempts: u64,
    pub output_path: Option<String>,
    pub verification_scope: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub workspace: Option<String>,
    pub nodes: Vec<NodeView>,
    pub stage_times: BTreeMap<String, u64>,
    #[serde(default)]
    pub transfer_progress: Option<TransferProgress>,
}

/// Bounded history rows omit node trees and frozen extraction settings.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSummary {
    pub id: String,
    pub source: SourceGroup,
    pub status: String,
    pub stage: String,
    pub progress: f64,
    pub message: String,
    pub attempts: u64,
    pub output_path: Option<String>,
    pub verification_scope: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub workspace: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPageQuery {
    pub offset: usize,
    pub limit: usize,
    pub status: String,
    pub search: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobHistoryPage {
    pub items: Vec<JobSummary>,
    pub offset: usize,
    pub limit: usize,
    pub filtered_total: usize,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub recoverable: usize,
    pub clear_allowed: bool,
    pub session_id: String,
    #[serde(with = "decimal_revision")]
    pub revision: u64,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub jobs: Vec<JobView>,
    pub passwords: Vec<PasswordView>,
    pub settings: Settings,
    pub running: bool,
    pub paused: bool,
    pub vault_error: Option<String>,
    pub data_directory: String,
    pub engine_available: bool,
    pub recoverable_count: usize,
    pub cache_used_bytes: u64,
    #[serde(default)]
    pub session_id: String,
    #[serde(default, with = "decimal_revision")]
    pub revision: u64,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotState {
    pub running: bool,
    pub paused: bool,
    pub vault_error: Option<String>,
    pub data_directory: String,
    pub engine_available: bool,
    pub recoverable_count: usize,
    pub cache_used_bytes: u64,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDelta {
    pub jobs_changed: Vec<JobView>,
    pub jobs_removed: Vec<String>,
    pub passwords_changed: Vec<PasswordView>,
    pub passwords_removed: Vec<String>,
    pub settings: Option<Settings>,
    pub state: Option<SnapshotState>,
}

impl SnapshotDelta {
    pub fn apply_to(&self, base: &AppSnapshot, session_id: String, revision: u64) -> AppSnapshot {
        let removed_jobs: std::collections::HashSet<&str> =
            self.jobs_removed.iter().map(|s| s.as_str()).collect();
        let changed_jobs: std::collections::HashMap<&str, &JobView> = self
            .jobs_changed
            .iter()
            .map(|j| (j.id.as_str(), j))
            .collect();

        let mut jobs = Vec::new();
        let mut seen_jobs = std::collections::HashSet::new();
        for job in &base.jobs {
            if removed_jobs.contains(job.id.as_str()) {
                continue;
            }
            if let Some(changed) = changed_jobs.get(job.id.as_str()) {
                jobs.push((*changed).clone());
            } else {
                jobs.push(job.clone());
            }
            seen_jobs.insert(job.id.as_str());
        }
        for job in &self.jobs_changed {
            if !seen_jobs.contains(job.id.as_str()) {
                jobs.push(job.clone());
            }
        }

        let removed_pwds: std::collections::HashSet<&str> =
            self.passwords_removed.iter().map(|s| s.as_str()).collect();
        let changed_pwds: std::collections::HashMap<&str, &PasswordView> = self
            .passwords_changed
            .iter()
            .map(|p| (p.id.as_str(), p))
            .collect();

        let mut passwords = Vec::new();
        let mut seen_pwds = std::collections::HashSet::new();
        for pwd in &base.passwords {
            if removed_pwds.contains(pwd.id.as_str()) {
                continue;
            }
            if let Some(changed) = changed_pwds.get(pwd.id.as_str()) {
                passwords.push((*changed).clone());
            } else {
                passwords.push(pwd.clone());
            }
            seen_pwds.insert(pwd.id.as_str());
        }
        for pwd in &self.passwords_changed {
            if !seen_pwds.contains(pwd.id.as_str()) {
                passwords.push(pwd.clone());
            }
        }

        let settings = self
            .settings
            .clone()
            .unwrap_or_else(|| base.settings.clone());

        let running = self
            .state
            .as_ref()
            .map(|s| s.running)
            .unwrap_or(base.running);
        let paused = self.state.as_ref().map(|s| s.paused).unwrap_or(base.paused);
        let vault_error = self
            .state
            .as_ref()
            .map(|s| s.vault_error.clone())
            .unwrap_or_else(|| base.vault_error.clone());
        let data_directory = self
            .state
            .as_ref()
            .map(|s| s.data_directory.clone())
            .unwrap_or_else(|| base.data_directory.clone());
        let engine_available = self
            .state
            .as_ref()
            .map(|s| s.engine_available)
            .unwrap_or(base.engine_available);
        let recoverable_count = self
            .state
            .as_ref()
            .map(|s| s.recoverable_count)
            .unwrap_or(base.recoverable_count);
        let cache_used_bytes = self
            .state
            .as_ref()
            .map(|s| s.cache_used_bytes)
            .unwrap_or(base.cache_used_bytes);

        AppSnapshot {
            jobs,
            passwords,
            settings,
            running,
            paused,
            vault_error,
            data_directory,
            engine_available,
            recoverable_count,
            cache_used_bytes,
            session_id,
            revision,
        }
    }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotUpdate {
    pub session_id: String,
    #[serde(with = "decimal_revision")]
    pub revision: u64,
    pub full: Option<AppSnapshot>,
    pub delta: Option<SnapshotDelta>,
}

/// Decimal text avoids loss of revision precision in JavaScript snapshots.
mod decimal_revision {
    pub fn serialize<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|c| c.is_ascii_digit())
        {
            return Err(serde::de::Error::custom("invalid decimal revision"));
        }
        value.parse().map_err(serde::de::Error::custom)
    }
    use serde::Deserialize;
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateInvalidated {
    pub session_id: String,
    pub revision: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgressUpdate {
    pub job_id: String,
    pub stage: String,
    pub progress: f64,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_second: f64,
    pub remaining_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_file: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedPasswordEvent {
    pub job_id: String,
    pub archive_path: String,
    pub timeout_seconds: u32,
    pub input_type: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntryView {
    pub path: String,
    pub size: u64,
    #[serde(default = "archive_size_known")]
    pub size_known: bool,
    pub is_directory: bool,
    pub is_encrypted: bool,
    pub raw_path: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInspectionView {
    pub archive_path: String,
    pub total_entries: usize,
    pub total_size: u64,
    #[serde(default = "archive_size_known")]
    pub total_size_known: bool,
    pub encrypted_entries: usize,
    pub entries: Vec<ArchiveEntryView>,
    pub detected_encoding: String,
}

fn archive_size_known() -> bool {
    true
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectiveExtractRequest {
    pub archive_path: String,
    pub entry_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default)]
    pub flatten: bool,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectiveExtractResult {
    pub output_directory: String,
    pub extracted_files: Vec<String>,
    pub total_bytes: u64,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolboxKind {
    NormalizeExtensions,
    BatchRename,
    FlattenDirectory,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictStrategy {
    Rename,
    Skip,
    Error,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolboxAction {
    Rename,
    Move,
    Skip,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePatternRule {
    pub search: String,
    pub replace: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub numbering_start: Option<u32>,
    pub numbering_digits: Option<u32>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxOptions {
    pub kind: ToolboxKind,
    pub source_directory: String,
    pub target_directory: Option<String>,
    pub recursive: bool,
    pub remove_empty_folders: bool,
    pub conflict_strategy: ConflictStrategy,
    pub rename_rule: Option<RenamePatternRule>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxItemPlan {
    pub source_path: String,
    pub relative_path: String,
    pub proposed_name: String,
    pub proposed_path: String,
    pub size: u64,
    pub action: ToolboxAction,
    pub has_conflict: bool,
    pub conflict_reason: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxPreviewResult {
    pub total_scanned: usize,
    pub total_planned: usize,
    pub conflicts_count: usize,
    pub plans: Vec<ToolboxItemPlan>,
    #[serde(default)]
    pub token: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxExecuteRequest {
    pub options: ToolboxOptions,
    pub approved_paths: Option<Vec<String>>,
    #[serde(default)]
    pub token: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolboxExecuteResult {
    pub success_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub cleaned_folders_count: usize,
    pub errors: Vec<String>,
}

impl Drop for PasswordInput {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        if let Some(secret) = self.secret.as_mut() {
            secret.zeroize();
        }
    }
}

impl std::fmt::Debug for PasswordInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the secret: Debug output may reach logs or panic messages.
        f.debug_struct("PasswordInput")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("secret", &self.secret.as_deref().map(|_| "[REDACTED]"))
            .field("groups", &self.groups)
            .field("notes", &self.notes)
            .field("enabled", &self.enabled)
            .finish()
    }
}
