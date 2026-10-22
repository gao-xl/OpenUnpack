use openunpack_protocol::{
    HistoryPageQuery, PasswordInput, SelectiveExtractRequest, Settings, SourceGroup,
    ToolboxExecuteRequest, ToolboxOptions,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldError {
    pub field: String,
    pub reason: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
pub enum Scope {
    #[serde(rename = "jobs.read")]
    JobsRead,
    #[serde(rename = "jobs.write")]
    JobsWrite,
    #[serde(rename = "files.read")]
    FilesRead,
    #[serde(rename = "files.write")]
    FilesWrite,
    #[serde(rename = "files.delete")]
    FilesDelete,
    #[serde(rename = "vault.read")]
    VaultRead,
    #[serde(rename = "vault.write")]
    VaultWrite,
    #[serde(rename = "vault.reveal")]
    VaultReveal,
    #[serde(rename = "settings.read")]
    SettingsRead,
    #[serde(rename = "settings.write")]
    SettingsWrite,
    #[serde(rename = "desktop.interactive")]
    DesktopInteractive,
}

impl Scope {
    pub const ALL: [Self; 11] = [
        Self::JobsRead,
        Self::JobsWrite,
        Self::FilesRead,
        Self::FilesWrite,
        Self::FilesDelete,
        Self::VaultRead,
        Self::VaultWrite,
        Self::VaultReveal,
        Self::SettingsRead,
        Self::SettingsWrite,
        Self::DesktopInteractive,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::JobsRead => "读取任务和历史",
            Self::JobsWrite => "管理任务和恢复结果",
            Self::FilesRead => "读取批准目录中的文件",
            Self::FilesWrite => "修改批准目录中的文件",
            Self::FilesDelete => "删除文件或清理源压缩包",
            Self::VaultRead => "读取密码名称和分组",
            Self::VaultWrite => "管理和导入密码",
            Self::VaultReveal => "读取密码明文、复制及导出密码",
            Self::SettingsRead => "读取设置",
            Self::SettingsWrite => "修改设置",
            Self::DesktopInteractive => "打开文件夹和显示文件选择窗口",
        }
    }
}

/// All existing desktop business operations. No API-key administration is
/// reachable through this enum. Sensitive request values are never Debug.
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "command",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ApiCommand {
    Snapshot,
    QueueSnapshot,
    ReadSettings,
    History,
    HistoryPage {
        query: HistoryPageQuery,
    },
    JobDetails {
        job_id: String,
    },
    ScanSources {
        paths: Vec<String>,
        request_id: Option<String>,
    },
    CancelScan {
        request_id: String,
    },
    PreviewExclusions {
        paths: Vec<String>,
        patterns: Vec<String>,
        #[serde(default = "input_scope")]
        scope: String,
        request_id: Option<String>,
    },
    CancelPreviewExclusions {
        request_id: String,
    },
    AddJobs {
        sources: Vec<SourceGroup>,
        options: Settings,
    },
    StartJobs,
    StartSelectedJobs {
        job_ids: Vec<String>,
    },
    PauseJobs,
    CancelJobs {
        job_id: Option<String>,
    },
    RetryJob {
        job_id: String,
        #[serde(default)]
        partial: bool,
        password: Option<String>,
        #[serde(default)]
        preserve_options: bool,
    },
    RetryNode {
        job_id: String,
        node_id: String,
        #[serde(default)]
        preserve_options: bool,
    },
    RemoveJob {
        job_id: String,
    },
    ClearCompleted,
    ClearHistory,
    RecoverJob {
        job_id: String,
        action: String,
    },
    Passwords,
    SavePassword {
        input: PasswordInput,
    },
    DeletePassword {
        id: String,
    },
    RevealPassword {
        id: String,
    },
    CopyPassword {
        id: String,
    },
    BulkPasswordAction {
        ids: Vec<String>,
        action: String,
        #[serde(default)]
        groups: Vec<String>,
    },
    PreviewImport {
        text: String,
        #[serde(default)]
        group: String,
        #[serde(default = "text_format")]
        format: String,
    },
    ConfirmImport {
        text: String,
        #[serde(default)]
        group: String,
        #[serde(default = "text_format")]
        format: String,
        token: String,
    },
    ExportBackup {
        path: String,
        passphrase: String,
    },
    PreviewRestoreBackup {
        path: String,
        passphrase: String,
    },
    ConfirmRestoreBackup {
        path: String,
        passphrase: String,
        token: String,
    },
    SaveSettings {
        settings: Settings,
    },
    InspectArchive {
        path: String,
        password: Option<String>,
        inspect_id: String,
    },
    CancelInspectArchive {
        inspect_id: String,
    },
    ExtractSelective {
        request: SelectiveExtractRequest,
    },
    OpenPath {
        path: String,
    },
    PickFiles {
        #[serde(default)]
        folder: bool,
    },
    PickDirectory,
    PickSaveBackup,
    PickOpenBackup,
    ReadImportFile {
        path: String,
        #[serde(default = "utf8")]
        encoding: String,
    },
    PreviewToolboxOperation {
        options: ToolboxOptions,
    },
    ExecuteToolboxOperation {
        request: ToolboxExecuteRequest,
    },
    AddArchive {
        path: String,
        #[serde(default)]
        output_dir: Option<String>,
        #[serde(default)]
        password: Option<String>,
        #[serde(default)]
        delete_source: Option<bool>,
        #[serde(default = "default_true")]
        auto_start: bool,
    },
    SubmitInput {
        job_id: String,
        #[serde(default = "default_password_type")]
        input_type: String,
        value: String,
    },
}

fn default_true() -> bool {
    true
}
fn default_password_type() -> String {
    "password".into()
}

fn input_scope() -> String {
    "input".into()
}
fn text_format() -> String {
    "txt".into()
}
fn utf8() -> String {
    "utf-8".into()
}

impl ApiCommand {
    pub const NAMES: &'static [&'static str] = &[
        "snapshot",
        "queue_snapshot",
        "read_settings",
        "history",
        "history_page",
        "job_details",
        "scan_sources",
        "cancel_scan",
        "preview_exclusions",
        "cancel_preview_exclusions",
        "add_jobs",
        "start_jobs",
        "start_selected_jobs",
        "pause_jobs",
        "cancel_jobs",
        "retry_job",
        "retry_node",
        "remove_job",
        "clear_completed",
        "clear_history",
        "recover_job",
        "passwords",
        "save_password",
        "delete_password",
        "reveal_password",
        "copy_password",
        "bulk_password_action",
        "preview_import",
        "confirm_import",
        "export_backup",
        "preview_restore_backup",
        "confirm_restore_backup",
        "save_settings",
        "inspect_archive",
        "cancel_inspect_archive",
        "extract_selective",
        "open_path",
        "pick_files",
        "pick_directory",
        "pick_save_backup",
        "pick_open_backup",
        "read_import_file",
        "preview_toolbox_operation",
        "execute_toolbox_operation",
        "add_archive",
        "submit_input",
    ];
    pub const UNIT_NAMES: &'static [&'static str] = &[
        "snapshot",
        "queue_snapshot",
        "read_settings",
        "history",
        "start_jobs",
        "pause_jobs",
        "clear_completed",
        "clear_history",
        "passwords",
        "pick_directory",
        "pick_save_backup",
        "pick_open_backup",
    ];
    pub fn scopes(&self) -> &'static [Scope] {
        use Scope::*;
        match self {
            Self::Snapshot => &[JobsRead, VaultRead, SettingsRead],
            Self::QueueSnapshot
            | Self::History
            | Self::HistoryPage { .. }
            | Self::JobDetails { .. } => &[JobsRead],
            Self::ReadSettings => &[SettingsRead],
            Self::ScanSources { .. }
            | Self::CancelScan { .. }
            | Self::PreviewExclusions { .. }
            | Self::CancelPreviewExclusions { .. }
            | Self::InspectArchive { .. }
            | Self::CancelInspectArchive { .. }
            | Self::ReadImportFile { .. } => &[FilesRead],
            Self::AddJobs { .. } => &[JobsWrite, FilesRead, FilesWrite],
            Self::StartJobs
            | Self::StartSelectedJobs { .. }
            | Self::PauseJobs
            | Self::CancelJobs { .. }
            | Self::RetryJob { .. }
            | Self::RetryNode { .. }
            | Self::RemoveJob { .. }
            | Self::ClearCompleted
            | Self::ClearHistory
            | Self::RecoverJob { .. } => &[JobsWrite],
            Self::Passwords => &[VaultRead],
            Self::SavePassword { .. }
            | Self::DeletePassword { .. }
            | Self::BulkPasswordAction { .. }
            | Self::PreviewImport { .. }
            | Self::ConfirmImport { .. } => &[VaultWrite],
            Self::RevealPassword { .. } => &[VaultReveal],
            Self::CopyPassword { .. } => &[VaultReveal, DesktopInteractive],
            Self::ExportBackup { .. } => &[VaultReveal, FilesWrite],
            Self::PreviewRestoreBackup { .. } | Self::ConfirmRestoreBackup { .. } => {
                &[VaultWrite, FilesRead]
            }
            Self::SaveSettings { .. } => &[SettingsWrite],
            Self::OpenPath { .. }
            | Self::PickFiles { .. }
            | Self::PickDirectory
            | Self::PickSaveBackup
            | Self::PickOpenBackup => &[DesktopInteractive],
            Self::PreviewToolboxOperation { .. } => &[FilesRead],
            Self::ExecuteToolboxOperation { .. } => &[FilesRead, FilesWrite],
            Self::ExtractSelective { .. } => &[FilesRead, FilesWrite],
            Self::AddArchive { .. } => &[JobsWrite, FilesRead, FilesWrite],
            Self::SubmitInput { .. } => &[JobsWrite],
        }
    }

    pub fn requires_file_delete(&self) -> bool {
        match self {
            Self::AddJobs { options, .. } | Self::SaveSettings { settings: options } => {
                options.source_cleanup != "keep"
            }
            Self::ExecuteToolboxOperation { request } => request.options.remove_empty_folders,
            Self::AddArchive { delete_source, .. } => delete_source.unwrap_or(false),
            _ => false,
        }
    }

    pub fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut errors = Vec::new();
        match self {
            Self::StartSelectedJobs { job_ids } => {
                let mut unique = std::collections::HashSet::new();
                if job_ids.is_empty() || job_ids.len() > 1000 {
                    errors.push(FieldError::new(
                        "jobIds",
                        "jobIds 必须包含 1 至 1000 个不重复的非空任务 ID。",
                    ));
                } else {
                    for (index, id) in job_ids.iter().enumerate() {
                        if id.trim().is_empty() {
                            errors.push(FieldError::new(
                                format!("jobIds[{index}]"),
                                "任务 ID 不能为空。",
                            ));
                        } else if !unique.insert(id) {
                            errors.push(FieldError::new(
                                format!("jobIds[{index}]"),
                                "任务 ID 不能重复。",
                            ));
                        }
                    }
                }
            }
            Self::JobDetails { job_id }
            | Self::RemoveJob { job_id }
            | Self::RetryJob { job_id, .. } => {
                if !is_safe_identifier(job_id) {
                    errors.push(FieldError::new(
                        "jobId",
                        "任务 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。",
                    ));
                }
            }
            Self::RetryNode {
                job_id, node_id, ..
            } => {
                if !is_safe_identifier(job_id) {
                    errors.push(FieldError::new(
                        "jobId",
                        "任务 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。",
                    ));
                }
                if node_id.trim().is_empty() || node_id.len() > 64 {
                    errors.push(FieldError::new(
                        "nodeId",
                        "节点 ID 必须是非空且不超过 64 字符。",
                    ));
                }
            }
            Self::RecoverJob { job_id, action } => {
                if !is_safe_identifier(job_id) {
                    errors.push(FieldError::new(
                        "jobId",
                        "任务 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。",
                    ));
                }
                if !matches!(
                    action.as_str(),
                    "retry" | "writeback" | "cleanup" | "verify"
                ) {
                    errors.push(FieldError::new(
                        "action",
                        "恢复操作必须为 retry、writeback、cleanup 或 verify 之一。",
                    ));
                }
            }
            Self::ScanSources { paths, request_id } => {
                if paths.len() > 1000 {
                    errors.push(FieldError::new(
                        "paths",
                        "扫描输入位置不能超过 1000 个路径。",
                    ));
                } else {
                    for (index, path) in paths.iter().enumerate() {
                        check_abs_path(path, &format!("paths[{index}]"), &mut errors);
                    }
                }
                if let Some(req_id) = request_id
                    && !is_safe_identifier(req_id)
                {
                    errors.push(FieldError::new(
                        "requestId",
                        "请求 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。",
                    ));
                }
            }
            Self::CancelScan { request_id } | Self::CancelPreviewExclusions { request_id } => {
                if !is_safe_identifier(request_id) {
                    errors.push(FieldError::new(
                        "requestId",
                        "请求 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。",
                    ));
                }
            }
            Self::InspectArchive {
                path, inspect_id, ..
            } => {
                check_abs_path(path, "path", &mut errors);
                if !is_safe_identifier(inspect_id) {
                    errors.push(FieldError::new(
                        "inspectId",
                        "检查会话 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。",
                    ));
                }
            }
            Self::CancelInspectArchive { inspect_id } => {
                if !is_safe_identifier(inspect_id) {
                    errors.push(FieldError::new(
                        "inspectId",
                        "检查会话 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。",
                    ));
                }
            }
            Self::ExtractSelective { request } => {
                check_abs_path(&request.archive_path, "request.archivePath", &mut errors);
                if let Some(destination) = request
                    .destination_dir
                    .as_deref()
                    .filter(|path| !path.trim().is_empty())
                {
                    check_abs_path(destination, "request.destinationDir", &mut errors);
                }
                if request.entry_paths.is_empty() || request.entry_paths.len() > 10_000 {
                    errors.push(FieldError::new(
                        "request.entryPaths",
                        "所选归档条目必须为1至10000项。",
                    ));
                }
                for (index, path) in request.entry_paths.iter().enumerate() {
                    if path.is_empty()
                        || path.len() > 510
                        || path.chars().any(char::is_control)
                        || path.starts_with(['/', '\\'])
                        || path.contains(':')
                        || path
                            .split(['/', '\\'])
                            .any(|part| part == ".." || part == ".")
                    {
                        errors.push(FieldError::new(
                            format!("request.entryPaths[{index}]"),
                            "归档条目必须是安全的相对路径。",
                        ));
                    }
                }
            }
            Self::PreviewExclusions {
                paths,
                patterns,
                request_id,
                ..
            } => {
                if paths.len() > 1000 {
                    errors.push(FieldError::new(
                        "paths",
                        "排除预览位置不能超过 1000 个路径。",
                    ));
                } else {
                    for (index, path) in paths.iter().enumerate() {
                        check_abs_path(path, &format!("paths[{index}]"), &mut errors);
                    }
                }
                if patterns.len() > 500 {
                    errors.push(FieldError::new("patterns", "排除规则不能超过 500 项。"));
                }
                if let Some(req_id) = request_id
                    && !is_safe_identifier(req_id)
                {
                    errors.push(FieldError::new(
                        "requestId",
                        "请求 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。",
                    ));
                }
            }
            Self::AddJobs { sources, .. } => {
                if sources.len() > 1000 {
                    errors.push(FieldError::new("sources", "一次最多添加 1000 个任务源。"));
                } else {
                    for (index, source) in sources.iter().enumerate() {
                        check_abs_path(
                            &source.path,
                            &format!("sources[{index}].path"),
                            &mut errors,
                        );
                    }
                }
            }
            Self::DeletePassword { id }
            | Self::RevealPassword { id }
            | Self::CopyPassword { id } => {
                if id.trim().is_empty() || id.len() > 64 {
                    errors.push(FieldError::new(
                        "id",
                        "密码 ID 必须是非空且不超过 64 字符。",
                    ));
                }
            }
            Self::BulkPasswordAction { ids, action, .. } => {
                if ids.is_empty() || ids.len() > 500 {
                    errors.push(FieldError::new(
                        "ids",
                        "批量密码操作必须包含 1 至 500 个密码 ID。",
                    ));
                }
                if action.trim().is_empty() || action.len() > 32 {
                    errors.push(FieldError::new("action", "批量操作类型不能为空。"));
                }
            }
            Self::PreviewImport { text, .. } => {
                if text.trim().is_empty() {
                    errors.push(FieldError::new("text", "导入文本内容不能为空。"));
                } else if text.len() > 10 * 1024 * 1024 {
                    errors.push(FieldError::new("text", "导入文本内容不能超过 10 MiB。"));
                }
            }
            Self::ConfirmImport { text, token, .. } => {
                if text.trim().is_empty() {
                    errors.push(FieldError::new("text", "导入文本内容不能为空。"));
                }
                if token.trim().is_empty() || token.len() > 64 {
                    errors.push(FieldError::new(
                        "token",
                        "确认令牌必须是非空且不超过 64 字符。",
                    ));
                }
            }
            Self::ExportBackup { path, passphrase } => {
                check_abs_path(path, "path", &mut errors);
                if passphrase.len() < 4 || passphrase.len() > 1024 {
                    errors.push(FieldError::new(
                        "passphrase",
                        "备份保护口令必须为 4 至 1024 个字符。",
                    ));
                }
            }
            Self::PreviewRestoreBackup { path, passphrase } => {
                check_abs_path(path, "path", &mut errors);
                if passphrase.is_empty() {
                    errors.push(FieldError::new("passphrase", "还原口令不能为空。"));
                }
            }
            Self::ConfirmRestoreBackup {
                path,
                passphrase,
                token,
            } => {
                check_abs_path(path, "path", &mut errors);
                if passphrase.is_empty() {
                    errors.push(FieldError::new("passphrase", "还原口令不能为空。"));
                }
                if token.trim().is_empty() || token.len() > 64 {
                    errors.push(FieldError::new(
                        "token",
                        "确认令牌必须是非空且不超过 64 字符。",
                    ));
                }
            }
            Self::OpenPath { path } | Self::ReadImportFile { path, .. } => {
                check_abs_path(path, "path", &mut errors);
            }
            Self::PreviewToolboxOperation { options } => {
                check_abs_path(&options.source_directory, "sourceDirectory", &mut errors);
            }
            Self::ExecuteToolboxOperation { request } => {
                check_abs_path(
                    &request.options.source_directory,
                    "sourceDirectory",
                    &mut errors,
                );
                if let Some(token) = &request.token
                    && (token.trim().is_empty() || token.len() > 64)
                {
                    errors.push(FieldError::new(
                        "token",
                        "操作令牌必须是非空且不超过 64 字符。",
                    ));
                }
            }
            Self::SaveSettings { settings } => {
                if settings.concurrency < 1 || settings.concurrency > 64 {
                    errors.push(FieldError::new(
                        "concurrency",
                        "并发数必须介于 1 至 64 之间。",
                    ));
                }
                if settings.max_depth > 32 {
                    errors.push(FieldError::new("maxDepth", "嵌套解压最大深度不能超过 32。"));
                }
                if !settings.output_root.trim().is_empty() {
                    check_abs_path(&settings.output_root, "outputRoot", &mut errors);
                }
                if !settings.cache_root.trim().is_empty() {
                    check_abs_path(&settings.cache_root, "cacheRoot", &mut errors);
                }
            }
            Self::AddArchive {
                path, output_dir, ..
            } => {
                check_abs_path(path, "path", &mut errors);
                if let Some(out) = output_dir
                    && !out.trim().is_empty()
                {
                    check_abs_path(out, "outputDir", &mut errors);
                }
            }
            Self::SubmitInput { job_id, value, .. } => {
                if !is_safe_identifier(job_id) {
                    errors.push(FieldError::new(
                        "jobId",
                        "任务 ID 必须是 1 至 64 位的字母、数字、连字符或下划线。",
                    ));
                }
                if value.is_empty() {
                    errors.push(FieldError::new("value", "输入内容不能为空。"));
                }
            }
            _ => {}
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn is_safe_identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn check_abs_path(path: &str, field: &str, errors: &mut Vec<FieldError>) {
    if path.trim().is_empty() {
        errors.push(FieldError::new(field, "路径不能为空。"));
    } else if path.chars().any(char::is_control) {
        errors.push(FieldError::new(field, "路径不能包含控制字符。"));
    } else if !std::path::Path::new(path).is_absolute() {
        errors.push(FieldError::new(field, "路径必须为绝对路径。"));
    }
}
