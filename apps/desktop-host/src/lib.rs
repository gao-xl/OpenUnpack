mod clipboard;
mod local_api;
use openunpack_core::coordinator::Coordinator;
use openunpack_protocol::{
    AppSnapshot, ArchiveInspectionView, HistoryPageQuery, ImportPreview, ImportResult,
    JobHistoryPage, JobView, PasswordInput, PasswordView, ScanResult, SelectiveExtractRequest,
    SelectiveExtractResult, Settings, SnapshotUpdate, SourceGroup, SuccessEvent,
    ToolboxExecuteRequest, ToolboxExecuteResult, ToolboxOptions, ToolboxPreviewResult,
};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{
    Builder, Emitter, Manager, State, WebviewWindow, WindowEvent, async_runtime::spawn_blocking,
    generate_context, generate_handler,
};
use tokio::sync::broadcast::error::RecvError;

pub type Window = WebviewWindow;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x00000400;
const MAX_IMPORT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Default)]
pub struct NativeAccess {
    readable_entries: HashSet<PathBuf>,
    readable_directories: HashSet<PathBuf>,
    writable_directories: HashSet<PathBuf>,
    backup_read_files: HashSet<PathBuf>,
    backup_write_files: HashSet<PathBuf>,
    native_drop_paths: HashSet<String>,
}

impl NativeAccess {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant_read(&mut self, path: PathBuf, is_directory: bool) {
        if is_directory {
            self.readable_directories.insert(path);
        } else {
            self.readable_entries.insert(path);
        }
    }
    pub fn grant_write_dir(&mut self, path: PathBuf) {
        self.writable_directories.insert(path);
    }
    pub fn grant_backup_read(&mut self, path: PathBuf) {
        self.backup_read_files.insert(path);
    }
    pub fn grant_backup_write(&mut self, path: PathBuf) {
        self.backup_write_files.insert(path);
    }
    pub fn is_readable(&self, target: &Path) -> bool {
        self.readable_entries.contains(target)
            || self
                .readable_directories
                .iter()
                .any(|b| path_contains_or_equal(b, target))
    }
    pub fn is_writable_dir(&self, target: &Path) -> bool {
        self.writable_directories
            .iter()
            .any(|b| path_contains_or_equal(b, target))
    }
    pub fn is_backup_read_allowed(&self, target: &Path) -> bool {
        self.backup_read_files.contains(target)
    }
    pub fn is_backup_write_allowed(&self, target: &Path) -> bool {
        self.backup_write_files.contains(target)
    }
}

pub struct AppState {
    pub coordinator: Arc<Coordinator>,
    pub vault_use_cases: Arc<openunpack_application::VaultUseCases>,
    pub sys_env: Arc<dyn openunpack_application::SystemEnvironment>,
    pub access: Arc<Mutex<NativeAccess>>,
    pub _lock_file: File,
    scans: Mutex<HashMap<String, Arc<AtomicBool>>>,
    clipboard_sequence: Mutex<Option<u32>>,
    exiting: AtomicBool,
    minimize_to_tray: AtomicBool,
}

/// Compatibility adapter exposing the production Coordinator's vault through the Application VaultStore port.
struct CoordinatorVaultStore(Arc<Coordinator>);

impl openunpack_application::VaultStore for CoordinatorVaultStore {
    fn list_entries(&self) -> Result<Vec<PasswordView>, openunpack_application::ApplicationError> {
        self.0
            .vault()
            .map_err(openunpack_application::ApplicationError::VaultError)?
            .views()
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn add_entry(
        &self,
        label: &str,
        secret: &str,
    ) -> Result<PasswordView, openunpack_application::ApplicationError> {
        self.upsert_entry(PasswordInput {
            id: None,
            name: label.to_string(),
            secret: Some(secret.to_string()),
            groups: vec![],
            notes: String::new(),
            enabled: true,
        })
    }

    fn delete_entry(&self, id: &str) -> Result<(), openunpack_application::ApplicationError> {
        let coord = self.0.clone();
        let id_owned = id.to_string();
        coord
            .update_visible_state(|| coord.vault()?.delete(&id_owned))
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn load_decrypt_candidates(
        &self,
    ) -> Result<Vec<openunpack_domain::DomainCandidate>, openunpack_application::ApplicationError>
    {
        let vault = self
            .0
            .vault()
            .map_err(openunpack_application::ApplicationError::VaultError)?;
        let counts: HashMap<String, u64> = vault
            .views()
            .map_err(openunpack_application::ApplicationError::VaultError)?
            .into_iter()
            .map(|view| (view.id, view.success_count))
            .collect();
        Ok(vault
            .candidates(None, None)
            .map_err(openunpack_application::ApplicationError::VaultError)?
            .into_iter()
            .map(|candidate| openunpack_domain::DomainCandidate {
                id: candidate.id.clone(),
                revision: candidate.revision,
                origin: openunpack_domain::CandidateOrigin::VaultEntry,
                label: candidate.label.clone(),
                secret: candidate.secret.clone(),
                success_count: counts.get(&candidate.id).copied().unwrap_or_default(),
            })
            .collect())
    }

    fn record_success(
        &self,
        event: &openunpack_application::PasswordSuccessEvent,
    ) -> Result<(), openunpack_application::ApplicationError> {
        if event.origin != openunpack_domain::CandidateOrigin::VaultEntry {
            return Ok(());
        }
        let coord = self.0.clone();
        let event_clone = SuccessEvent {
            event_id: event.event_id.clone(),
            password_id: event.candidate_id.clone(),
            password_revision: event.candidate_revision,
            fingerprint: event.fingerprint.clone(),
        };
        coord
            .update_visible_state(|| coord.vault()?.record_success(&event_clone))
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn import_passwords(
        &self,
        passwords: &[String],
    ) -> Result<usize, openunpack_application::ApplicationError> {
        if passwords.iter().any(|p| p.contains(['\r', '\n', '\0'])) {
            return Err(openunpack_application::ApplicationError::InvalidInput(
                "列表中的每个密码必须是单行原文".into(),
            ));
        }
        let text = zeroize::Zeroizing::new(passwords.join("\n"));
        let coord = self.0.clone();
        let vault = coord
            .vault()
            .map_err(openunpack_application::ApplicationError::VaultError)?;
        let preview = vault
            .preview_import(&text, "txt", "")
            .map_err(openunpack_application::ApplicationError::VaultError)?;
        if preview.error_count > 0 {
            return Err(openunpack_application::ApplicationError::InvalidInput(
                "密码导入列表包含无效记录".into(),
            ));
        }
        coord
            .update_visible_state(|| {
                coord
                    .vault()?
                    .apply_import(&preview.token, &text, "txt", "")
            })
            .map(|result| result.added)
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn upsert_entry(
        &self,
        input: PasswordInput,
    ) -> Result<PasswordView, openunpack_application::ApplicationError> {
        let coord = self.0.clone();
        coord
            .update_visible_state(|| coord.vault()?.upsert(input))
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn groups(&self) -> Result<Vec<String>, openunpack_application::ApplicationError> {
        self.0
            .vault()
            .map_err(openunpack_application::ApplicationError::VaultError)?
            .groups()
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn bulk_action(
        &self,
        ids: &[String],
        action: &str,
        groups: &[String],
    ) -> Result<(), openunpack_application::ApplicationError> {
        let coord = self.0.clone();
        let ids_vec = ids.to_vec();
        let act = action.to_string();
        let grps = groups.to_vec();
        coord
            .update_visible_state(|| coord.vault()?.bulk_action(&ids_vec, &act, &grps))
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn preview_import(
        &self,
        text: &str,
        format: &str,
        group: &str,
    ) -> Result<ImportPreview, openunpack_application::ApplicationError> {
        self.0
            .vault()
            .map_err(openunpack_application::ApplicationError::VaultError)?
            .preview_import(text, format, group)
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn apply_import(
        &self,
        token: &str,
        text: &str,
        format: &str,
        group: &str,
    ) -> Result<ImportResult, openunpack_application::ApplicationError> {
        let coord = self.0.clone();
        let tok = token.to_string();
        let txt = text.to_string();
        let fmt = format.to_string();
        let grp = group.to_string();
        coord
            .update_visible_state(|| coord.vault()?.apply_import(&tok, &txt, &fmt, &grp))
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn export_backup(
        &self,
        path: &Path,
        passphrase: &str,
    ) -> Result<(), openunpack_application::ApplicationError> {
        let path_buf = path.to_path_buf();
        let pass = passphrase.to_string();
        self.0
            .vault()
            .map_err(openunpack_application::ApplicationError::VaultError)?
            .export_backup(&path_buf, &pass)
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn preview_restore_backup(
        &self,
        path: &Path,
        passphrase: &str,
    ) -> Result<ImportPreview, openunpack_application::ApplicationError> {
        let path_buf = path.to_path_buf();
        let pass = passphrase.to_string();
        self.0
            .vault()
            .map_err(openunpack_application::ApplicationError::VaultError)?
            .preview_restore_backup(&path_buf, &pass)
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn confirm_restore_backup(
        &self,
        path: &Path,
        passphrase: &str,
        token: &str,
    ) -> Result<ImportResult, openunpack_application::ApplicationError> {
        let coord = self.0.clone();
        let path_buf = path.to_path_buf();
        let pass = passphrase.to_string();
        let tok = token.to_string();
        coord
            .update_visible_state(|| {
                coord
                    .vault()?
                    .confirm_restore_backup(&path_buf, &pass, &tok)
            })
            .map_err(openunpack_application::ApplicationError::VaultError)
    }

    fn reveal_entry(&self, id: &str) -> Result<String, openunpack_application::ApplicationError> {
        self.0
            .vault()
            .map_err(openunpack_application::ApplicationError::VaultError)?
            .reveal(id)
            .map_err(openunpack_application::ApplicationError::VaultError)
    }
}

fn path_contains_or_equal(base: &Path, target: &Path) -> bool {
    let mut b_iter = base.components();
    let mut t_iter = target.components();
    loop {
        match (b_iter.next(), t_iter.next()) {
            (None, _) => return true,
            (Some(b), Some(t)) if b == t => continue,
            _ => return false,
        }
    }
}

fn verify_no_reparse_ancestors(path: &Path) -> Result<(), String> {
    let mut current = Some(path);
    while let Some(segment) = current {
        if segment.as_os_str().is_empty() {
            break;
        }
        if segment.exists() {
            let meta =
                fs::symlink_metadata(segment).map_err(|_| "Native operation failed".to_string())?;
            if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(format!(
                    "Path or ancestor is a symlink/reparse point: {}",
                    segment.display()
                ));
            }
        }
        current = segment.parent();
    }
    Ok(())
}

fn sanitize_path_string(raw: &str) -> Result<PathBuf, String> {
    if raw.contains('\0') {
        return Err("Path contains invalid NUL bytes".to_string());
    }
    let p = PathBuf::from(raw);
    if p.as_os_str().is_empty() {
        return Err("Path is empty".to_string());
    }
    for comp in p.components() {
        if matches!(comp, Component::ParentDir) {
            return Err("Parent directory navigation (..) is not permitted".to_string());
        }
    }
    Ok(p)
}

fn canonicalize_existing(raw: &str) -> Result<PathBuf, String> {
    let path = sanitize_path_string(raw)?;
    if !path.exists() {
        return Err(format!("Path does not exist: {}", path.display()));
    }
    verify_no_reparse_ancestors(&path)?;
    let canonical = path
        .canonicalize()
        .map_err(|_| "Native operation failed".to_string())?;
    verify_no_reparse_ancestors(&canonical)?;
    Ok(canonical)
}

fn canonicalize_for_save(raw: &str) -> Result<PathBuf, String> {
    let path = sanitize_path_string(raw)?;
    if path.exists() {
        return canonicalize_existing(raw);
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Target file must have a parent directory".to_string())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "Target path must have a valid file name".to_string())?;
    if !parent.exists() {
        return Err(format!(
            "Parent directory does not exist: {}",
            parent.display()
        ));
    }
    verify_no_reparse_ancestors(parent)?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| "Native operation failed".to_string())?;
    verify_no_reparse_ancestors(&canonical_parent)?;
    Ok(canonical_parent.join(file_name))
}

pub fn is_trusted_origin_url(url_str: &str) -> bool {
    let Ok(url) = tauri::Url::parse(url_str) else {
        return false;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let host = url.host_str().unwrap_or_default();
    let scheme = url.scheme();
    let port = url.port();
    if scheme == "tauri" && host == "localhost" && port.is_none() {
        return true;
    }
    if (scheme == "http" || scheme == "https") && host == "tauri.localhost" && port.is_none() {
        return true;
    }
    if cfg!(debug_assertions) && scheme == "http" && host == "127.0.0.1" && port == Some(1420) {
        return true;
    }
    false
}

pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", stripped))
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

pub fn is_reserved_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.trim().to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

pub fn sanitize_explorer_target(path: &Path) -> Result<PathBuf, String> {
    let clean = strip_verbatim_prefix(path);
    let str_val = clean.to_string_lossy();
    let trimmed = str_val.trim();
    if trimmed.is_empty() {
        return Err("Target path is empty".to_string());
    }
    if trimmed.starts_with('-') || trimmed.starts_with('/') {
        return Err("Path starts with illegal command switch character".to_string());
    }
    if str_val.chars().any(|c| c.is_control()) {
        return Err("Path contains control characters".to_string());
    }
    for comp in clean.components() {
        if let std::path::Component::Normal(os_str) = comp
            && let Some(name) = os_str.to_str()
            && is_reserved_windows_device_name(name)
        {
            return Err(format!(
                "Path contains reserved Windows device name: {name}"
            ));
        }
    }
    if !clean.is_absolute() {
        return Err("Target path must be an absolute path".to_string());
    }
    if !clean.is_dir() {
        return Err("Target path is not an existing directory".to_string());
    }
    Ok(clean)
}

fn verify_frontend_window(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Untrusted window caller".to_string());
    }
    let current_url = window
        .url()
        .map_err(|_| "Native operation failed".to_string())?;
    if !is_trusted_origin_url(current_url.as_str()) {
        return Err(format!("Untrusted origin: {current_url}"));
    }
    Ok(())
}

fn canonicalize_worker_pair(exe: &Path, dll: &Path) -> Option<(PathBuf, PathBuf)> {
    if !exe.is_file() || !dll.is_file() {
        return None;
    }
    verify_no_reparse_ancestors(exe).ok()?;
    verify_no_reparse_ancestors(dll).ok()?;
    let canon_exe = exe.canonicalize().ok()?;
    let canon_dll = dll.canonicalize().ok()?;
    verify_no_reparse_ancestors(&canon_exe).ok()?;
    verify_no_reparse_ancestors(&canon_dll).ok()?;
    let clean_exe = strip_verbatim_prefix(&canon_exe);
    let clean_dll = strip_verbatim_prefix(&canon_dll);
    if !clean_exe.is_absolute() || !clean_dll.is_absolute() {
        return None;
    }
    Some((clean_exe, clean_dll))
}

fn resolve_worker_paths_standalone() -> Result<(PathBuf, PathBuf), String> {
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        let pair = (
            dir.join("worker/OpenUnpack.Worker.exe"),
            dir.join("worker/7z.dll"),
        );
        if let Some(valid_pair) = canonicalize_worker_pair(&pair.0, &pair.1) {
            return Ok(valid_pair);
        }
        let pair2 = (dir.join("OpenUnpack.Worker.exe"), dir.join("7z.dll"));
        if let Some(valid_pair) = canonicalize_worker_pair(&pair2.0, &pair2.1) {
            return Ok(valid_pair);
        }
    }
    #[cfg(debug_assertions)]
    {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dev_root = manifest_dir.join("../../artifacts/worker");
        let dev_exe = dev_root.join("OpenUnpack.Worker.exe");
        let dev_dll = dev_root.join("7z.dll");
        if let Some(pair) = canonicalize_worker_pair(&dev_exe, &dev_dll) {
            return Ok(pair);
        }

        let source_exe = manifest_dir
            .join("../../src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe");
        let source_dll = manifest_dir.join("../../tools/7zip/7z.dll");
        if let Some(pair) = canonicalize_worker_pair(&source_exe, &source_dll) {
            return Ok(pair);
        }
    }
    let default_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    Ok((
        default_dir.join("worker/OpenUnpack.Worker.exe"),
        default_dir.join("worker/7z.dll"),
    ))
}

fn resolve_worker_paths(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), String> {
    if let Ok(resource) = app.path().resource_dir() {
        let packaged = (
            resource.join("worker/OpenUnpack.Worker.exe"),
            resource.join("worker/7z.dll"),
        );
        if let Some(pair) = canonicalize_worker_pair(&packaged.0, &packaged.1) {
            return Ok(pair);
        }
    }
    resolve_worker_paths_standalone()
}

fn resolve_data_directory() -> Result<PathBuf, String> {
    if let Ok(custom) = std::env::var("OPENUNPACK_DATA_DIR") {
        let p = PathBuf::from(custom.trim());
        if !p.as_os_str().is_empty() {
            fs::create_dir_all(&p).map_err(|_| "Native operation failed".to_string())?;
            return Ok(p);
        }
    }
    #[cfg(windows)]
    let path = {
        let local_app_data = std::env::var("LOCALAPPDATA")
            .map_err(|_| "LOCALAPPDATA environment variable is missing".to_string())?;
        PathBuf::from(local_app_data).join("OpenUnpack")
    };
    #[cfg(not(windows))]
    let path = {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".local/share/openunpack")
        } else {
            PathBuf::from("/data/config")
        }
    };
    fs::create_dir_all(&path).map_err(|_| "Native operation failed".to_string())?;
    Ok(path)
}

fn acquire_instance_lock(data_dir: &Path) -> Result<File, String> {
    let lock_path = data_dir.join(".instance.lock");
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        opts.share_mode(0);
    }
    opts.open(&lock_path)
        .map_err(|_| "Another instance is running or the data directory is unavailable".to_string())
}

pub(crate) async fn core_snapshot(state: &AppState) -> Result<AppSnapshot, String> {
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.snapshot())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_queue_snapshot(state: &AppState) -> Result<AppSnapshot, String> {
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.queue_snapshot())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_snapshot_update(
    state: &AppState,
    since_revision: Option<String>,
) -> Result<SnapshotUpdate, String> {
    let coord = state.coordinator.clone();
    let since = since_revision.and_then(|s| s.trim().parse::<u64>().ok());
    spawn_blocking(move || coord.snapshot_update(since))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_history_page(
    state: &AppState,
    query: HistoryPageQuery,
) -> Result<JobHistoryPage, String> {
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.history_page(query))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_job_details(
    state: &AppState,
    job_id: &str,
) -> Result<Option<JobView>, String> {
    let coord = state.coordinator.clone();
    let jid = job_id.to_string();
    spawn_blocking(move || coord.job_details(&jid))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn snapshot(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    verify_frontend_window(&window)?;
    core_snapshot(&state).await
}

#[tauri::command]
async fn queue_snapshot(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    verify_frontend_window(&window)?;
    core_queue_snapshot(&state).await
}

#[tauri::command]
async fn snapshot_update(
    window: WebviewWindow,
    state: State<'_, AppState>,
    since_revision: Option<String>,
) -> Result<SnapshotUpdate, String> {
    verify_frontend_window(&window)?;
    core_snapshot_update(&state, since_revision).await
}

#[tauri::command]
async fn history_page(
    window: WebviewWindow,
    state: State<'_, AppState>,
    query: HistoryPageQuery,
) -> Result<JobHistoryPage, String> {
    verify_frontend_window(&window)?;
    core_history_page(&state, query).await
}

#[tauri::command]
async fn job_details(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<Option<JobView>, String> {
    verify_frontend_window(&window)?;
    core_job_details(&state, &job_id).await
}

async fn scan_authorized(
    state: &AppState,
    paths: &[String],
    cancel: Arc<AtomicBool>,
) -> Result<ScanResult, String> {
    let paths = paths.to_vec();
    let access = state.access.clone();
    let checked_paths = spawn_blocking(move || -> Result<Vec<String>, String> {
        let guard = access
            .lock()
            .map_err(|_| "Access lock poisoned".to_string())?
            .clone();
        let mut canonical_list = Vec::with_capacity(paths.len());
        for raw in &paths {
            let canon = canonicalize_existing(raw)?;
            if !guard.is_readable(&canon) && !guard.native_drop_paths.contains(raw) {
                return Err(format!("Access denied to scan source: {}", canon.display()));
            }
            canonical_list.push(canon.to_string_lossy().into_owned());
        }
        Ok(canonical_list)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())??;
    let coord = state.coordinator.clone();
    let access = state.access.clone();
    spawn_blocking(move || {
        let result = coord.scan_with_cancel(&checked_paths, &cancel)?;
        // Sibling volumes are authorized only when discovered by the native scanner.
        let mut volumes = Vec::new();
        for source in &result.sources {
            for volume in &source.volumes {
                volumes.push(canonicalize_existing(volume)?);
            }
        }
        let mut roots = Vec::new();
        for path in &checked_paths {
            let canonical = PathBuf::from(path);
            let is_directory = fs::metadata(&canonical)
                .map_err(|_| "Source metadata unavailable".to_string())?
                .is_dir();
            roots.push((canonical, is_directory));
        }
        let mut guard = access
            .lock()
            .map_err(|_| "Access lock poisoned".to_string())?;
        for (canonical, is_directory) in roots {
            guard.grant_read(canonical, is_directory);
        }
        for volume in volumes {
            guard.grant_read(volume, false);
        }
        Ok(result)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_scan_sources(
    state: &AppState,
    paths: Vec<String>,
    request_id: Option<String>,
) -> Result<ScanResult, String> {
    let scan_id = request_id.unwrap_or_else(|| {
        format!(
            "scan-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    });
    if scan_id.len() > 128 {
        return Err("扫描请求编号过长。".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut scans = state.scans.lock().map_err(|_| "扫描状态不可用。")?;
        if scans.len() >= 2 || scans.contains_key(&scan_id) {
            return Err("已有扫描请求，请等待或取消。".into());
        }
        scans.insert(scan_id.clone(), cancel.clone());
    }
    let result = scan_authorized(state, &paths, cancel).await;
    if let Ok(mut scans) = state.scans.lock() {
        scans.remove(&scan_id);
    }
    result
}

pub(crate) async fn core_cancel_scan(state: &AppState, request_id: &str) -> Result<(), String> {
    if let Some(cancel) = state
        .scans
        .lock()
        .map_err(|_| "扫描状态不可用。")?
        .get(request_id)
    {
        cancel.store(true, Ordering::Release);
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ExclusionMatch {
    pub path: String,
    pub matched: bool,
}

pub(crate) async fn core_preview_exclusions(
    state: &AppState,
    paths: Vec<String>,
    patterns: Vec<String>,
    scope: Option<String>,
    request_id: Option<String>,
) -> Result<Vec<ExclusionMatch>, String> {
    validate_exclusion_preview(&paths, &patterns, scope.as_deref())?;
    if scope.as_deref() == Some("output") {
        let matches = match request_id {
            Some(id) => {
                state
                    .coordinator
                    .preview_output_rules_with_id(&paths, &patterns, id)
                    .await
            }
            None => {
                state
                    .coordinator
                    .preview_output_rules(&paths, &patterns)
                    .await
            }
        }
        .map_err(|error| match error.as_str() {
            "WorkerUnavailable" => "作业进程缺失，无法试算输出规则。".to_string(),
            "WorkerResourceProbeUnavailable" => {
                "无法读取系统可用内存，请检查系统后重试。".to_string()
            }
            "RulePreviewLimit" => "试算内容过大，请减少路径数量或长度。".to_string(),
            "RulePreviewTimeout" => "输出规则试算超时，请减少试算内容后重试。".to_string(),
            _ => "输出规则试算未完成，请重试或检查程序文件。".to_string(),
        })?;
        return Ok(paths
            .into_iter()
            .zip(matches)
            .map(|(path, matched)| ExclusionMatch { path, matched })
            .collect());
    }
    preview_exclusion_matches(paths, patterns, scope.as_deref())
}

pub(crate) async fn core_cancel_preview_exclusions(
    state: &AppState,
    request_id: &str,
) -> Result<(), String> {
    state
        .coordinator
        .cancel_preview_output_rules_request(request_id)
}

#[tauri::command]
async fn scan_sources(
    window: WebviewWindow,
    state: State<'_, AppState>,
    paths: Vec<String>,
    request_id: Option<String>,
) -> Result<ScanResult, String> {
    verify_frontend_window(&window)?;
    core_scan_sources(&state, paths, request_id).await
}

#[tauri::command]
async fn cancel_scan(
    window: WebviewWindow,
    state: State<'_, AppState>,
    request_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_cancel_scan(&state, &request_id).await
}

#[tauri::command]
async fn preview_exclusions(
    window: WebviewWindow,
    state: State<'_, AppState>,
    paths: Vec<String>,
    patterns: Vec<String>,
    scope: Option<String>,
    request_id: Option<String>,
) -> Result<Vec<ExclusionMatch>, String> {
    verify_frontend_window(&window)?;
    core_preview_exclusions(&state, paths, patterns, scope, request_id).await
}

#[tauri::command]
async fn cancel_preview_exclusions(
    window: WebviewWindow,
    state: State<'_, AppState>,
    request_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_cancel_preview_exclusions(&state, &request_id).await
}

fn validate_exclusion_preview(
    paths: &[String],
    patterns: &[String],
    scope: Option<&str>,
) -> Result<(), String> {
    use openunpack_domain::archive::exclusions::is_exclusion_pattern_within_limit;
    if paths.len() > 1000
        || patterns.len() > 100
        || paths.iter().any(|path| path.len() > 32768)
        || patterns
            .iter()
            .any(|pattern| !is_exclusion_pattern_within_limit(pattern))
    {
        return Err("匹配预览内容超出限制：最多 100 条规则，单条不超过 512 UTF-8 字节。".into());
    }
    if !matches!(scope.unwrap_or("input"), "input" | "output") {
        return Err("不支持的规则试算范围。".into());
    }
    Ok(())
}

fn preview_exclusion_matches(
    paths: Vec<String>,
    patterns: Vec<String>,
    scope: Option<&str>,
) -> Result<Vec<ExclusionMatch>, String> {
    validate_exclusion_preview(&paths, &patterns, scope)?;
    if scope == Some("output") {
        return Err("输出规则试算需要作业进程。".into());
    }
    let matches = paths
        .iter()
        .map(|path| openunpack_domain::archive::exclusions::matches_exclusion(path, &patterns))
        .collect::<Vec<_>>();
    Ok(paths
        .into_iter()
        .zip(matches)
        .map(|(path, matched)| ExclusionMatch { path, matched })
        .collect())
}

#[cfg(test)]
mod exclusion_preview_tests {
    use super::{preview_exclusion_matches, validate_exclusion_preview};

    #[test]
    fn preview_rule_limit_accepts_257_to_512_utf8_bytes() {
        for pattern in [
            "a".repeat(257),
            "a".repeat(512),
            format!("{}aa", "中".repeat(170)),
        ] {
            let result =
                preview_exclusion_matches(vec![pattern.clone()], vec![pattern], None).unwrap();
            assert!(result[0].matched);
        }
        for pattern in ["a".repeat(513), "中".repeat(171)] {
            assert!(preview_exclusion_matches(vec![], vec![pattern], None).is_err());
        }
    }

    #[test]
    fn preview_scope_defaults_to_legacy_input_and_routes_output_to_worker() {
        let child = "temp/deep/file.bin".to_string();
        let rule = vec!["temp".into()];
        assert!(
            !preview_exclusion_matches(vec![child.clone()], rule.clone(), None).unwrap()[0].matched
        );
        assert!(
            validate_exclusion_preview(&["temp/".into(), child.clone()], &rule, Some("output"))
                .is_ok()
        );
        assert!(preview_exclusion_matches(vec![child], rule, Some("output")).is_err());
        assert!(preview_exclusion_matches(vec![], vec![], Some("unknown")).is_err());
    }
}

pub(crate) async fn core_retry_node(
    state: &AppState,
    job_id: String,
    node_id: String,
) -> Result<(), String> {
    let coord = state.coordinator.clone();
    let c = coord.clone();
    spawn_blocking(move || c.retry_node(&job_id, &node_id))
        .await
        .map_err(|_| "节点重试未完成。")??;
    coord.start()
}

pub(crate) async fn core_bulk_password_action(
    state: &AppState,
    ids: Vec<String>,
    action: String,
    groups: Option<Vec<String>>,
) -> Result<(), String> {
    if !matches!(action.as_str(), "enable" | "disable" | "delete" | "group") {
        return Err("不支持的批量操作类型。".into());
    }
    let vault = state.vault_use_cases.clone();
    spawn_blocking(move || {
        vault
            .bulk_action(&ids, &action, &groups.unwrap_or_default())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "批量操作未完成。")?
}

pub(crate) async fn core_copy_password(
    state: &AppState,
    id: &str,
    owner: usize,
) -> Result<(), String> {
    let vault = state.vault_use_cases.clone();
    let id_str = id.to_string();
    let sequence = spawn_blocking(move || {
        let text =
            zeroize::Zeroizing::new(vault.reveal_password(&id_str).map_err(|e| e.to_string())?);
        clipboard::copy(owner, &text)
    })
    .await
    .map_err(|_| "复制密码未完成。")??;
    *state
        .clipboard_sequence
        .lock()
        .map_err(|_| "剪贴板状态不可用。")? = Some(sequence);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let _ = spawn_blocking(move || clipboard::clear_if_unchanged(sequence)).await;
    });
    Ok(())
}

pub(crate) async fn core_add_jobs(
    state: &AppState,
    sources: Vec<SourceGroup>,
    options: Settings,
) -> Result<Vec<JobView>, String> {
    let access = state.access.clone();
    let (v_sources, v_options) =
        spawn_blocking(move || -> Result<(Vec<SourceGroup>, Settings), String> {
            let guard = access
                .lock()
                .map_err(|_| "Access lock poisoned".to_string())?
                .clone();
            for sg in &sources {
                let src_canon = canonicalize_existing(&sg.path)?;
                if !guard.is_readable(&src_canon) {
                    return Err(format!(
                        "Source path is not granted for read: {}",
                        src_canon.display()
                    ));
                }
                for vol in &sg.volumes {
                    let vol_canon = canonicalize_existing(vol)?;
                    if !guard.is_readable(&vol_canon) {
                        return Err(format!(
                            "Volume path is not granted for read: {}",
                            vol_canon.display()
                        ));
                    }
                }
            }
            if !options.output_root.trim().is_empty() {
                let out_canon = canonicalize_existing(&options.output_root)?;
                if !guard.is_writable_dir(&out_canon) {
                    return Err(format!(
                        "Output directory not granted for write: {}",
                        out_canon.display()
                    ));
                }
            } else {
                for sg in &sources {
                    let src_path = Path::new(&sg.path);
                    let parent = src_path.parent().ok_or_else(|| {
                        format!("Invalid source path without parent: {}", sg.path)
                    })?;
                    let parent_canon = canonicalize_existing(&parent.to_string_lossy())?;
                    if !guard.is_writable_dir(&parent_canon) {
                        return Err(format!(
                            "In-place extraction destination directory not granted for write: {}",
                            parent_canon.display()
                        ));
                    }
                }
            }
            if !options.cache_root.trim().is_empty() {
                let cache_canon = canonicalize_existing(&options.cache_root)?;
                if !guard.is_writable_dir(&cache_canon) {
                    return Err(format!(
                        "Cache directory not granted for write: {}",
                        cache_canon.display()
                    ));
                }
            }
            Ok((sources, options))
        })
        .await
        .map_err(|_| "Background operation failed".to_string())??;
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.add_jobs(v_sources, v_options))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) fn core_start_jobs(state: &AppState) -> Result<(), String> {
    state.coordinator.start()
}

#[tauri::command]
async fn retry_node(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
    node_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_retry_node(&state, job_id, node_id).await
}

#[tauri::command]
async fn bulk_password_action(
    window: WebviewWindow,
    state: State<'_, AppState>,
    ids: Vec<String>,
    action: String,
    groups: Option<Vec<String>>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_bulk_password_action(&state, ids, action, groups).await
}

#[tauri::command]
async fn copy_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let owner = window.hwnd().map_err(|_| "窗口句柄不可用。")?.0 as usize;
    core_copy_password(&state, &id, owner).await
}

#[tauri::command]
async fn add_jobs(
    window: WebviewWindow,
    state: State<'_, AppState>,
    sources: Vec<SourceGroup>,
    options: Settings,
) -> Result<Vec<JobView>, String> {
    verify_frontend_window(&window)?;
    core_add_jobs(&state, sources, options).await
}

#[tauri::command]
async fn start_jobs(window: WebviewWindow, state: State<'_, AppState>) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_start_jobs(&state)
}

pub(crate) async fn core_pause_jobs(state: &AppState) -> Result<(), String> {
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.pause())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_cancel_jobs(
    state: &AppState,
    job_id: Option<String>,
) -> Result<(), String> {
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.cancel(job_id.as_deref()))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_retry_job(
    state: &AppState,
    job_id: String,
    partial: bool,
    password: Option<String>,
) -> Result<(), String> {
    let coord = state.coordinator.clone();
    let c = coord.clone();
    let jid = job_id.clone();
    spawn_blocking(move || c.retry_with_password(&jid, partial, password))
        .await
        .map_err(|_| "Background operation failed".to_string())??;
    coord.start()
}

pub(crate) async fn core_remove_job(state: &AppState, job_id: String) -> Result<(), String> {
    let coord = state.coordinator.clone();
    coord.delete_task(&job_id).await
}

pub(crate) async fn core_clear_completed(state: &AppState) -> Result<(), String> {
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.clear_completed())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_passwords(state: &AppState) -> Result<Vec<PasswordView>, String> {
    let vault = state.vault_use_cases.clone();
    spawn_blocking(move || vault.list_passwords().map_err(|e| e.to_string()))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_save_password(
    state: &AppState,
    input: PasswordInput,
) -> Result<PasswordView, String> {
    let vault = state.vault_use_cases.clone();
    spawn_blocking(move || vault.upsert_password(input).map_err(|e| e.to_string()))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_delete_password(state: &AppState, id: &str) -> Result<(), String> {
    let vault = state.vault_use_cases.clone();
    let id_str = id.to_string();
    spawn_blocking(move || vault.delete_password(&id_str).map_err(|e| e.to_string()))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_reveal_password(state: &AppState, id: &str) -> Result<String, String> {
    let vault = state.vault_use_cases.clone();
    let id_str = id.to_string();
    spawn_blocking(move || vault.reveal_password(&id_str).map_err(|e| e.to_string()))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_preview_import(
    state: &AppState,
    text: String,
    group: String,
    format: String,
) -> Result<ImportPreview, String> {
    let vault = state.vault_use_cases.clone();
    spawn_blocking(move || {
        vault
            .preview_import(&text, &format, &group)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_confirm_import(
    state: &AppState,
    text: String,
    group: String,
    format: String,
    token: String,
) -> Result<ImportResult, String> {
    let vault = state.vault_use_cases.clone();
    spawn_blocking(move || {
        vault
            .apply_import(&token, &text, &format, &group)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn pause_jobs(window: WebviewWindow, state: State<'_, AppState>) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_pause_jobs(&state).await
}

#[tauri::command]
async fn cancel_jobs(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: Option<String>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_cancel_jobs(&state, job_id).await
}

#[tauri::command]
async fn retry_job(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
    partial: bool,
    password: Option<String>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_retry_job(&state, job_id, partial, password).await
}

#[tauri::command]
async fn remove_job(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_remove_job(&state, job_id).await
}

#[tauri::command]
async fn clear_completed(window: WebviewWindow, state: State<'_, AppState>) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_clear_completed(&state).await
}

#[tauri::command]
async fn passwords(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<PasswordView>, String> {
    verify_frontend_window(&window)?;
    core_passwords(&state).await
}

#[tauri::command]
async fn save_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    input: PasswordInput,
) -> Result<PasswordView, String> {
    verify_frontend_window(&window)?;
    core_save_password(&state, input).await
}

#[tauri::command]
async fn delete_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_delete_password(&state, &id).await
}

#[tauri::command]
async fn reveal_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    verify_frontend_window(&window)?;
    core_reveal_password(&state, &id).await
}

#[tauri::command]
async fn preview_import(
    window: WebviewWindow,
    state: State<'_, AppState>,
    text: String,
    group: String,
    format: String,
) -> Result<ImportPreview, String> {
    verify_frontend_window(&window)?;
    core_preview_import(&state, text, group, format).await
}

#[tauri::command]
async fn confirm_import(
    window: WebviewWindow,
    state: State<'_, AppState>,
    text: String,
    group: String,
    format: String,
    token: String,
) -> Result<ImportResult, String> {
    verify_frontend_window(&window)?;
    core_confirm_import(&state, text, group, format, token).await
}

fn protect_backup_target(coord: &Coordinator, path: &Path) -> Result<(), String> {
    let key = path.to_string_lossy().to_lowercase();
    let data = coord
        .data
        .canonicalize()
        .map_err(|_| "应用数据目录不可检查。")?
        .to_string_lossy()
        .to_lowercase();
    if key == data || key.starts_with(&(data + "\\")) {
        return Err("备份不能覆盖应用数据目录，请选择独立位置。".into());
    }
    let snap = coord.snapshot()?;
    if snap.jobs.iter().any(|j| {
        j.source
            .volumes
            .iter()
            .chain(std::iter::once(&j.source.path))
            .any(|p| p.to_lowercase() == key)
    }) {
        return Err("备份不能覆盖已登记的源压缩包或分卷。".into());
    }
    Ok(())
}

pub(crate) async fn core_export_backup(
    state: &AppState,
    path: String,
    passphrase: String,
) -> Result<(), String> {
    let access = state.access.clone();
    let validated = spawn_blocking(move || -> Result<PathBuf, String> {
        let target = canonicalize_for_save(&path)?;
        let guard = access
            .lock()
            .map_err(|_| "Access lock poisoned".to_string())?;
        if !guard.is_backup_write_allowed(&target) {
            return Err(format!(
                "Export path was not authorized via dialog: {}",
                target.display()
            ));
        }
        Ok(target)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())??;
    let coord = state.coordinator.clone();
    let vault = state.vault_use_cases.clone();
    spawn_blocking(move || {
        protect_backup_target(&coord, &validated)?;
        let passphrase = zeroize::Zeroizing::new(passphrase);
        vault
            .export_backup(&validated, &passphrase)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_preview_restore_backup(
    state: &AppState,
    path: String,
    passphrase: String,
) -> Result<ImportPreview, String> {
    let access = state.access.clone();
    let validated = spawn_blocking(move || -> Result<PathBuf, String> {
        let target = canonicalize_existing(&path)?;
        let guard = access
            .lock()
            .map_err(|_| "Access lock poisoned".to_string())?;
        if !guard.is_backup_read_allowed(&target) {
            return Err(format!(
                "Restore path was not authorized via dialog: {}",
                target.display()
            ));
        }
        Ok(target)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())??;
    let vault = state.vault_use_cases.clone();
    spawn_blocking(move || {
        let phrase = zeroize::Zeroizing::new(passphrase);
        vault
            .preview_restore_backup(&validated, &phrase)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_confirm_restore_backup(
    state: &AppState,
    path: String,
    passphrase: String,
    token: String,
) -> Result<ImportResult, String> {
    let access = state.access.clone();
    let vault = state.vault_use_cases.clone();
    spawn_blocking(move || {
        let validated = canonicalize_existing(&path)?;
        if !access
            .lock()
            .map_err(|_| "路径授权不可用。")?
            .is_backup_read_allowed(&validated)
        {
            return Err("请使用备份文件选择器授权此路径。".into());
        }
        let phrase = zeroize::Zeroizing::new(passphrase);
        vault
            .confirm_restore_backup(&validated, &phrase, &token)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "备份恢复未完成。")?
}

pub(crate) async fn core_save_settings(
    state: &AppState,
    settings: Settings,
    window: Option<&WebviewWindow>,
) -> Result<Settings, String> {
    let access = state.access.clone();
    let validated = spawn_blocking(move || -> Result<Settings, String> {
        let guard = access
            .lock()
            .map_err(|_| "Access lock poisoned".to_string())?
            .clone();
        if !settings.output_root.trim().is_empty() {
            let out_canon = canonicalize_existing(&settings.output_root)?;
            if !guard.is_writable_dir(&out_canon) {
                return Err(
                    "Output root directory must be picked via native directory dialog".to_string(),
                );
            }
        }
        if !settings.cache_root.trim().is_empty() {
            let cache_canon = canonicalize_existing(&settings.cache_root)?;
            if !guard.is_writable_dir(&cache_canon) {
                return Err(
                    "Cache root directory must be picked via native directory dialog".to_string(),
                );
            }
        }
        Ok(settings)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())??;
    let coord = state.coordinator.clone();
    let saved = spawn_blocking(move || coord.save_settings(validated))
        .await
        .map_err(|_| "Background operation failed".to_string())??;
    state
        .minimize_to_tray
        .store(saved.minimize_to_tray, Ordering::Release);
    if let Some(w) = window {
        let _ = w.set_always_on_top(saved.always_on_top);
    }
    Ok(saved)
}

pub(crate) async fn core_history(state: &AppState) -> Result<Vec<JobView>, String> {
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.history())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_clear_history(state: &AppState) -> Result<(), String> {
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.clear_history())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_recover_job(
    state: &AppState,
    job_id: &str,
    action: &str,
) -> Result<(), String> {
    state.coordinator.recover(job_id, action).await
}

pub(crate) async fn core_inspect_archive(
    state: &AppState,
    path: String,
    password: Option<String>,
    inspect_id: String,
) -> Result<ArchiveInspectionView, String> {
    let canon = canonicalize_existing(&path)?;
    {
        let guard = state
            .access
            .lock()
            .map_err(|_| "Access lock poisoned".to_string())?;
        if !guard.is_readable(&canon)
            && !guard.native_drop_paths.contains(&path)
            && state.coordinator.authorize_open(Path::new(&path)).is_err()
        {
            return Err(format!(
                "Access denied to inspect archive: {}",
                canon.display()
            ));
        }
    }
    let checked_path = canon.to_string_lossy().into_owned();
    let coord = state.coordinator.clone();
    coord
        .inspect_archive_with_id(&checked_path, password, inspect_id)
        .await
        .map_err(|error| {
            if error == "WorkerResourceProbeUnavailable" {
                "无法读取系统可用内存，请检查系统后重试。".to_string()
            } else {
                error
            }
        })
}

pub(crate) async fn core_cancel_inspect_archive(
    state: &AppState,
    inspect_id: &str,
) -> Result<(), String> {
    state.coordinator.cancel_inspect_archive_request(inspect_id)
}

pub(crate) async fn core_extract_selective(
    state: &AppState,
    request: SelectiveExtractRequest,
) -> Result<SelectiveExtractResult, String> {
    let canon = canonicalize_existing(&request.archive_path)?;
    {
        let guard = state
            .access
            .lock()
            .map_err(|_| "Access lock poisoned".to_string())?;
        if !guard.is_readable(&canon)
            && !guard.native_drop_paths.contains(&request.archive_path)
            && state
                .coordinator
                .authorize_open(Path::new(&request.archive_path))
                .is_err()
        {
            return Err(format!("Access denied to archive: {}", canon.display()));
        }
        let destination = request
            .destination_dir
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| canon.parent().unwrap_or(Path::new("")).to_owned());
        let dest_canon = canonicalize_existing(&destination.to_string_lossy())?;
        if !guard.is_writable_dir(&dest_canon) {
            return Err("目标目录尚未获准写入，请先选择输出目录。".into());
        }
        let scan =
            openunpack_core::scanner::scan(&[canon.to_string_lossy().into_owned()], false, &[])?;
        for volume in scan.sources.iter().flat_map(|source| &source.volumes) {
            let path = canonicalize_existing(volume)?;
            if !guard.is_readable(&path)
                && !guard.native_drop_paths.contains(volume)
                && state.coordinator.authorize_open(&path).is_err()
            {
                return Err("分卷文件尚未获准读取。".into());
            }
        }
    }
    let mut req = request;
    req.archive_path = canon.to_string_lossy().into_owned();
    state.coordinator.extract_selective(req).await
}

pub(crate) async fn core_open_path(state: &AppState, path: String) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Invalid path parameter: empty path".to_string());
    }
    if path.chars().any(|c| c.is_control()) {
        return Err("Invalid path parameter: contains control characters".to_string());
    }
    if trimmed.starts_with('-') || trimmed.starts_with('/') {
        return Err("Invalid path parameter: starts with command switch".to_string());
    }
    let coord = state.coordinator.clone();
    spawn_blocking(move || -> Result<(), String> {
        coord.authorize_open(Path::new(&path))?;
        let canon = canonicalize_existing(&path)?;
        let target_dir = if canon.is_dir() {
            canon
        } else {
            canon
                .parent()
                .ok_or_else(|| "Parent directory unavailable".to_string())?
                .to_path_buf()
        };
        let safe_target = sanitize_explorer_target(&target_dir)?;
        let system_root = std::env::var_os("SystemRoot")
            .or_else(|| std::env::var_os("windir"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let explorer_path = system_root.join("explorer.exe");
        if !explorer_path.is_file() {
            return Err("System explorer executable not found".to_string());
        }
        Command::new(&explorer_path)
            .current_dir(&system_root)
            .arg(&safe_target)
            .spawn()
            .map_err(|_| "Native operation failed".to_string())?;
        Ok(())
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn export_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    passphrase: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_export_backup(&state, path, passphrase).await
}

#[tauri::command]
async fn preview_restore_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    passphrase: String,
) -> Result<ImportPreview, String> {
    verify_frontend_window(&window)?;
    core_preview_restore_backup(&state, path, passphrase).await
}

#[tauri::command]
async fn confirm_restore_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    passphrase: String,
    token: String,
) -> Result<ImportResult, String> {
    verify_frontend_window(&window)?;
    core_confirm_restore_backup(&state, path, passphrase, token).await
}

#[tauri::command]
async fn save_settings(
    window: WebviewWindow,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    verify_frontend_window(&window)?;
    core_save_settings(&state, settings, Some(&window)).await
}

#[tauri::command]
async fn history(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<JobView>, String> {
    verify_frontend_window(&window)?;
    core_history(&state).await
}

#[tauri::command]
async fn clear_history(window: WebviewWindow, state: State<'_, AppState>) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_clear_history(&state).await
}

#[tauri::command]
async fn recover_job(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
    action: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_recover_job(&state, &job_id, &action).await
}

#[tauri::command]
async fn inspect_archive(
    window: Window,
    state: State<'_, AppState>,
    path: String,
    password: Option<String>,
    inspect_id: String,
) -> Result<ArchiveInspectionView, String> {
    verify_frontend_window(&window)?;
    core_inspect_archive(&state, path, password, inspect_id).await
}

#[tauri::command]
async fn cancel_inspect_archive(
    window: Window,
    state: State<'_, AppState>,
    inspect_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_cancel_inspect_archive(&state, &inspect_id).await
}

#[tauri::command]
async fn extract_selective(
    window: Window,
    state: State<'_, AppState>,
    request: SelectiveExtractRequest,
) -> Result<SelectiveExtractResult, String> {
    verify_frontend_window(&window)?;
    core_extract_selective(&state, request).await
}

#[tauri::command]
async fn open_path(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_open_path(&state, path).await
}

pub(crate) async fn core_pick_files(state: &AppState, folder: bool) -> Result<Vec<String>, String> {
    let access = state.access.clone();
    spawn_blocking(move || -> Result<Vec<String>, String> {
        let mut results = Vec::new();
        if folder {
            if let Some(folder_path) = FileDialog::new().pick_folder()
                && let Ok(canon) = canonicalize_existing(&folder_path.to_string_lossy())
            {
                let mut guard = access
                    .lock()
                    .map_err(|_| "Access lock poisoned".to_string())?;
                guard.grant_read(canon.clone(), true);
                guard.grant_write_dir(canon.clone());
                results.push(canon.to_string_lossy().into_owned());
            }
        } else if let Some(files) = FileDialog::new().pick_files() {
            let mut approved = Vec::new();
            for file_path in files {
                if let Ok(canon) = canonicalize_existing(&file_path.to_string_lossy()) {
                    approved.push(canon);
                }
            }
            let mut guard = access
                .lock()
                .map_err(|_| "Access lock poisoned".to_string())?;
            for canon in approved {
                if let Some(parent) = canon.parent() {
                    guard.grant_write_dir(parent.to_path_buf());
                }
                guard.grant_read(canon.clone(), false);
                results.push(canon.to_string_lossy().into_owned());
            }
        }
        Ok(results)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_pick_directory(state: &AppState) -> Result<Option<String>, String> {
    let access = state.access.clone();
    spawn_blocking(move || -> Result<Option<String>, String> {
        if let Some(dir) = FileDialog::new().pick_folder() {
            let canon = canonicalize_existing(&dir.to_string_lossy())?;
            let mut guard = access
                .lock()
                .map_err(|_| "Access lock poisoned".to_string())?;
            guard.grant_write_dir(canon.clone());
            guard.grant_read(canon.clone(), true);
            return Ok(Some(canon.to_string_lossy().into_owned()));
        }
        Ok(None)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_pick_save_backup(state: &AppState) -> Result<Option<String>, String> {
    let access = state.access.clone();
    spawn_blocking(move || -> Result<Option<String>, String> {
        if let Some(path) = FileDialog::new()
            .add_filter("OpenUnpack Backup", &["opbak", "bin", "json"])
            .save_file()
        {
            let target = canonicalize_for_save(&path.to_string_lossy())?;
            let mut guard = access
                .lock()
                .map_err(|_| "Access lock poisoned".to_string())?;
            guard.grant_backup_write(target.clone());
            return Ok(Some(target.to_string_lossy().into_owned()));
        }
        Ok(None)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_pick_open_backup(state: &AppState) -> Result<Option<String>, String> {
    let access = state.access.clone();
    spawn_blocking(move || -> Result<Option<String>, String> {
        if let Some(path) = FileDialog::new()
            .add_filter("OpenUnpack Backup", &["opbak", "bin", "json"])
            .pick_file()
        {
            let canon = canonicalize_existing(&path.to_string_lossy())?;
            let mut guard = access
                .lock()
                .map_err(|_| "Access lock poisoned".to_string())?;
            guard.grant_backup_read(canon.clone());
            return Ok(Some(canon.to_string_lossy().into_owned()));
        }
        Ok(None)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_read_import_file(
    state: &AppState,
    path: String,
    encoding: String,
) -> Result<String, String> {
    let access = state.access.clone();
    spawn_blocking(move || -> Result<String, String> {
        let canon = canonicalize_existing(&path)?;
        {
            let guard = access
                .lock()
                .map_err(|_| "Access lock poisoned".to_string())?;
            if !guard.is_readable(&canon) {
                return Err(format!(
                    "File is not authorized for read: {}",
                    canon.display()
                ));
            }
        }
        let file = File::open(&canon).map_err(|_| "Native operation failed".to_string())?;
        let meta = file
            .metadata()
            .map_err(|_| "Native operation failed".to_string())?;
        if meta.len() > MAX_IMPORT_BYTES {
            return Err("File size exceeds maximum permitted 8 MiB limit".to_string());
        }
        let mut bytes = Vec::new();
        file.take(MAX_IMPORT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Native operation failed".to_string())?;
        if bytes.len() as u64 > MAX_IMPORT_BYTES {
            return Err("Import file exceeded its permitted limit".to_string());
        }
        decode_import_bytes(&bytes, &encoding)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

fn authorize_toolbox_options(
    access: &Mutex<NativeAccess>,
    mut options: ToolboxOptions,
) -> Result<ToolboxOptions, String> {
    let src_canon = canonicalize_existing(&options.source_directory)?;
    let dst_canon = match options.target_directory.as_deref() {
        Some(t) if !t.trim().is_empty() => Some(canonicalize_existing(t)?),
        _ => None,
    };

    let guard = access
        .lock()
        .map_err(|_| "Access lock poisoned".to_string())?;
    if !guard.is_readable(&src_canon) || !guard.is_writable_dir(&src_canon) {
        return Err(format!(
            "权限被拒绝：请先通过文件夹选择器授权源目录读写访问 ({})",
            src_canon.display()
        ));
    }
    if let Some(ref dst) = dst_canon
        && !guard.is_writable_dir(dst)
    {
        return Err(format!(
            "权限被拒绝：请先通过文件夹选择器授权目标目录写入访问 ({})",
            dst.display()
        ));
    }
    options.source_directory = src_canon.to_string_lossy().into_owned();
    options.target_directory = dst_canon.map(|d| d.to_string_lossy().into_owned());
    Ok(options)
}

pub(crate) async fn core_preview_toolbox_operation(
    state: &AppState,
    options: ToolboxOptions,
) -> Result<ToolboxPreviewResult, String> {
    let access = state.access.clone();
    spawn_blocking(move || {
        let checked = authorize_toolbox_options(&access, options)?;
        openunpack_core::toolbox::preview_toolbox(&checked)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

pub(crate) async fn core_execute_toolbox_operation(
    state: &AppState,
    request: ToolboxExecuteRequest,
) -> Result<ToolboxExecuteResult, String> {
    let access = state.access.clone();
    spawn_blocking(move || {
        let checked_options = authorize_toolbox_options(&access, request.options)?;
        let mut checked_approved = None;
        if let Some(paths) = request.approved_paths {
            let mut list = Vec::with_capacity(paths.len());
            for p in paths {
                let canon = canonicalize_existing(&p)?;
                list.push(canon.to_string_lossy().into_owned());
            }
            checked_approved = Some(list);
        }
        openunpack_core::toolbox::execute_toolbox(&ToolboxExecuteRequest {
            options: checked_options,
            approved_paths: checked_approved,
            token: request.token,
        })
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn pick_files(
    window: WebviewWindow,
    state: State<'_, AppState>,
    folder: bool,
) -> Result<Vec<String>, String> {
    verify_frontend_window(&window)?;
    core_pick_files(&state, folder).await
}

#[tauri::command]
async fn pick_directory(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    verify_frontend_window(&window)?;
    core_pick_directory(&state).await
}

#[tauri::command]
async fn pick_save_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    verify_frontend_window(&window)?;
    core_pick_save_backup(&state).await
}

#[tauri::command]
async fn pick_open_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    verify_frontend_window(&window)?;
    core_pick_open_backup(&state).await
}

#[tauri::command]
async fn read_import_file(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    encoding: String,
) -> Result<String, String> {
    verify_frontend_window(&window)?;
    core_read_import_file(&state, path, encoding).await
}

#[tauri::command]
async fn preview_toolbox_operation(
    window: Window,
    state: State<'_, AppState>,
    options: ToolboxOptions,
) -> Result<ToolboxPreviewResult, String> {
    verify_frontend_window(&window)?;
    core_preview_toolbox_operation(&state, options).await
}

#[tauri::command]
async fn execute_toolbox_operation(
    window: Window,
    state: State<'_, AppState>,
    request: ToolboxExecuteRequest,
) -> Result<ToolboxExecuteResult, String> {
    verify_frontend_window(&window)?;
    core_execute_toolbox_operation(&state, request).await
}

#[tauri::command]
async fn execute_post_action(
    window: Window,
    state: State<'_, AppState>,
    action: String,
    target_path: Option<String>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let trimmed = action.trim();
    match trimmed {
        "openOutput" => {
            if let Some(path) = target_path
                && !path.trim().is_empty()
            {
                open_path(window, state, path).await?;
            }
            Ok(())
        }
        "sleep" | "shutdown" => {
            // 安全前置守卫：校验 Coordinator 状态与配置匹配
            let snapshot = state
                .coordinator
                .snapshot()
                .map_err(|e| format!("无法获取运行状态快照: {e}"))?;

            if snapshot.running {
                return Err("解压队列仍在运行中，已拦截关机/休眠操作。".into());
            }

            let has_active_jobs = snapshot.jobs.iter().any(|j| {
                !matches!(
                    j.status.as_str(),
                    "completed" | "salvaged" | "partial" | "failed" | "cancelled" | "skipped"
                )
            });
            if has_active_jobs {
                return Err("队列中仍有未完成的任务，已拦截关机/休眠操作。".into());
            }

            if snapshot.settings.post_extraction_action != trimmed {
                return Err(format!(
                    "当前设置的后置操作为 '{}'，拒绝执行未经配置的 '{}' 操作。",
                    snapshot.settings.post_extraction_action, trimmed
                ));
            }

            if trimmed == "sleep" {
                spawn_blocking(|| -> Result<(), String> {
                    let system_root = std::env::var_os("SystemRoot")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
                    let rundll = system_root.join(r"System32\rundll32.exe");
                    let _ = Command::new(rundll)
                        .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                        .status();
                    Ok(())
                })
                .await
                .map_err(|e| format!("执行休眠失败: {e}"))?
            } else {
                spawn_blocking(|| -> Result<(), String> {
                    let system_root = std::env::var_os("SystemRoot")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
                    let shutdown = system_root.join(r"System32\shutdown.exe");
                    let _ = Command::new(shutdown).args(["/s", "/t", "0"]).status();
                    Ok(())
                })
                .await
                .map_err(|e| format!("执行关机失败: {e}"))?
            }
        }
        "none" => Ok(()),
        _ => Err(format!("未知的操作类型: {}", action)),
    }
}

#[tauri::command]
async fn get_shell_context_menu_status(window: Window) -> Result<bool, String> {
    verify_frontend_window(&window)?;
    spawn_blocking(|| {
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let reg = system_root.join(r"System32\reg.exe");
        let output = Command::new(reg)
            .args(["query", r"HKCU\Software\Classes\*\shell\OpenUnpack"])
            .output()
            .map_err(|e| format!("查询注册表失败: {e}"))?;
        Ok(output.status.success())
    })
    .await
    .map_err(|e| format!("后台操作失败: {e}"))?
}

#[tauri::command]
async fn set_shell_context_menu_status(window: Window, enabled: bool) -> Result<bool, String> {
    verify_frontend_window(&window)?;
    spawn_blocking(move || {
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let reg = system_root.join(r"System32\reg.exe");
        if enabled {
            let current_exe =
                std::env::current_exe().map_err(|e| format!("无法定位当前程序路径: {e}"))?;
            let exe_str = current_exe.to_string_lossy();
            let key = r"HKCU\Software\Classes\*\shell\OpenUnpack";
            let cmd_key = r"HKCU\Software\Classes\*\shell\OpenUnpack\command";

            let status = Command::new(&reg)
                .args(["add", key, "/ve", "/d", "使用 OpenUnpack 解压", "/f"])
                .status()
                .map_err(|e| format!("写入注册表主项失败: {e}"))?;
            if !status.success() {
                return Err("注册表添加主项失败".into());
            }

            let _ = Command::new(&reg)
                .args([
                    "add",
                    key,
                    "/v",
                    "Icon",
                    "/d",
                    &format!("\"{}\"", exe_str),
                    "/f",
                ])
                .status();

            let cmd_val = format!("\"{}\" \"%1\"", exe_str);
            let status_cmd = Command::new(&reg)
                .args(["add", cmd_key, "/ve", "/d", &cmd_val, "/f"])
                .status()
                .map_err(|e| format!("写入注册表命令项失败: {e}"))?;
            if !status_cmd.success() {
                return Err("注册表添加命令项失败".into());
            }
            Ok(true)
        } else {
            let key = r"HKCU\Software\Classes\*\shell\OpenUnpack";
            let _ = Command::new(&reg).args(["delete", key, "/f"]).status();
            Ok(false)
        }
    })
    .await
    .map_err(|e| format!("后台操作失败: {e}"))?
}

fn decode_import_bytes(bytes: &[u8], encoding: &str) -> Result<String, String> {
    match encoding.trim().to_ascii_uppercase().as_str() {
        "GB18030" | "GBK" | "CP936" => {
            let decoder = if encoding.trim().eq_ignore_ascii_case("gb18030") {
                encoding_rs::GB18030
            } else {
                encoding_rs::GBK
            };
            decoder
                .decode_without_bom_handling_and_without_replacement(bytes)
                .map(|s| s.into_owned())
                .ok_or_else(|| "导入文件包含无效的中文编码字节。".into())
        }
        "UTF-8" | "UTF8" => {
            std::str::from_utf8(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes))
                .map(str::to_owned)
                .map_err(|_| "导入文件包含无效的 UTF-8 字节。".into())
        }
        "UTF-16LE" => {
            let content = bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes);
            if !content.len().is_multiple_of(2) || content.starts_with(&[0xFE, 0xFF]) {
                return Err("导入文件不是有效的 UTF-16LE 文本。".into());
            }
            let units: Vec<u16> = content
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            String::from_utf16(&units).map_err(|_| "导入文件包含无效的 UTF-16 字符。".into())
        }
        _ => Err("不支持所选导入编码。".into()),
    }
}

struct NativeUi {
    language: Mutex<&'static str>,
    show: tauri::menu::MenuItem<tauri::Wry>,
    exit: tauri::menu::MenuItem<tauri::Wry>,
}

fn native_text(language: &str) -> [&'static str; 6] {
    match language {
        "en-US" => [
            "Open OpenUnpack",
            "Exit",
            "Exit OpenUnpack",
            "Extraction tasks are running. Choose Yes to cancel tasks and exit, or No to keep running.",
            "Exit timed out",
            "The extraction process has not fully stopped. Temporary directories have been preserved. The window will remain open; check the task status.",
        ],
        "ja-JP" => [
            "OpenUnpack を開く",
            "終了",
            "OpenUnpack を終了",
            "解凍タスクが実行中です。「はい」でタスクをキャンセルして終了し、「いいえ」で実行を続けます。",
            "終了待機がタイムアウトしました",
            "バックグラウンドの解凍プロセスがまだ終了していません。一時フォルダーは保持されています。ウィンドウは開いたままです。タスクの状態を確認してください。",
        ],
        _ => [
            "打开 OpenUnpack",
            "退出",
            "退出 OpenUnpack",
            "当前有解压任务正在进行。选择“是”取消任务并退出；选择“否”继续运行。",
            "退出等待超时",
            "后台解压进程尚未完全退出。已保留临时目录，窗口继续打开，请检查任务状态。",
        ],
    }
}

#[tauri::command]
fn set_ui_language(
    window: WebviewWindow,
    state: State<'_, NativeUi>,
    language: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let language = match language.as_str() {
        "zh-CN" => "zh-CN",
        "en-US" => "en-US",
        "ja-JP" => "ja-JP",
        _ => return Err("Unsupported UI language".into()),
    };
    let mut current = state
        .language
        .lock()
        .map_err(|_| "UI language state unavailable")?;
    let text = native_text(language);
    state.show.set_text(text[0]).map_err(|e| e.to_string())?;
    state.exit.set_text(text[1]).map_err(|e| e.to_string())?;
    *current = language;
    Ok(())
}

fn current_native_text(app: &tauri::AppHandle) -> [&'static str; 6] {
    let language = app
        .try_state::<NativeUi>()
        .and_then(|s| s.language.lock().ok().map(|v| *v))
        .unwrap_or("zh-CN");
    native_text(language)
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
fn request_exit(app: tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        app.exit(0);
        return;
    };
    if state.exiting.swap(true, Ordering::AcqRel) {
        return;
    }
    let coord = state.coordinator.clone();
    tauri::async_runtime::spawn(async move {
        let c = coord.clone();
        let snap = spawn_blocking(move || c.snapshot()).await;
        let active = match snap {
            Ok(Ok(s)) => s.running,
            _ => true,
        };
        if active {
            let text = current_native_text(&app);
            let choice = spawn_blocking(move || {
                MessageDialog::new()
                    .set_title(text[2])
                    .set_description(text[3])
                    .set_level(MessageLevel::Info)
                    .set_buttons(MessageButtons::YesNo)
                    .show()
            })
            .await;
            if !matches!(choice, Ok(MessageDialogResult::Yes)) {
                if let Some(s) = app.try_state::<AppState>() {
                    s.exiting.store(false, Ordering::Release);
                }
                return;
            }
            let c = coord.clone();
            let _ = spawn_blocking(move || c.cancel(None)).await;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
            loop {
                let c = coord.clone();
                let status = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    spawn_blocking(move || c.snapshot()),
                )
                .await;
                if matches!(status,Ok(Ok(Ok(s))) if !s.running&& !s.jobs.iter().any(|j|j.status=="stopping"))
                {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    if let Some(s) = app.try_state::<AppState>() {
                        s.exiting.store(false, Ordering::Release);
                    }
                    let text = current_native_text(&app);
                    let _ = spawn_blocking(move || {
                        MessageDialog::new()
                            .set_title(text[4])
                            .set_description(text[5])
                            .set_level(MessageLevel::Warning)
                            .show()
                    })
                    .await;
                    show_main(&app);
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
        }
        let sequence = app
            .try_state::<AppState>()
            .and_then(|s| s.clipboard_sequence.lock().ok().and_then(|v| *v));
        if let Some(sequence) = sequence {
            let _ = spawn_blocking(move || clipboard::clear_if_unchanged(sequence)).await;
        }
        app.exit(0);
    });
}
fn install_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };
    let show = MenuItem::with_id(app, "show", "打开 OpenUnpack", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &exit])?;
    let mut tray = TrayIconBuilder::with_id("openunpack-tray")
        .tooltip("OpenUnpack")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "exit" => request_exit(app.clone()),
            _ => (),
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } | TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(NativeUi {
        language: Mutex::new("zh-CN"),
        show,
        exit,
    });
    Ok(())
}
pub fn run() {
    Builder::default()
        .setup(|app| {
            let data_dir = match resolve_data_directory() {
                Ok(dir) => dir,
                Err(err) => {
                    MessageDialog::new()
                        .set_title("OpenUnpack Initialization Error")
                        .set_description(format!("Failed to locate data directory: {err}"))
                        .set_level(MessageLevel::Error)
                        .show();
                    return Err(err.into());
                }
            };
            let lock_file = match acquire_instance_lock(&data_dir) {
                Ok(file) => file,
                Err(err) => {
                    MessageDialog::new()
                        .set_title("OpenUnpack Instance Error")
                        .set_description(format!("Another instance running or locked: {err}"))
                        .set_level(MessageLevel::Error)
                        .show();
                    return Err(err.into());
                }
            };
            let (worker_exe, worker_dll) = match resolve_worker_paths(app.handle()) {
                Ok(pair) => pair,
                Err(err) => {
                    MessageDialog::new()
                        .set_title("OpenUnpack Worker Error")
                        .set_description(format!("Worker binaries missing: {err}"))
                        .set_level(MessageLevel::Error)
                        .show();
                    return Err(err.into());
                }
            };
            let coordinator = match Coordinator::open(data_dir, worker_exe, worker_dll) {
                Ok(coord) => coord,
                Err(err) => {
                    MessageDialog::new()
                        .set_title("OpenUnpack Vault/Database Error")
                        .set_description(format!("Failed to open OpenUnpack Coordinator: {err}"))
                        .set_level(MessageLevel::Error)
                        .show();
                    return Err(err.into());
                }
            };

            let mut native_access = NativeAccess::default();
            if let Ok(snap) = coordinator.snapshot() {
                if !snap.settings.output_root.trim().is_empty()
                    && let Ok(p) = canonicalize_existing(&snap.settings.output_root)
                {
                    native_access.grant_write_dir(p);
                }
                if !snap.settings.cache_root.trim().is_empty()
                    && let Ok(p) = canonicalize_existing(&snap.settings.cache_root)
                {
                    native_access.grant_write_dir(p);
                }
                for j in &snap.jobs {
                    if let Ok(p) = canonicalize_existing(&j.source.path) {
                        native_access.grant_read(p, false);
                    }
                    for v in &j.source.volumes {
                        if let Ok(p) = canonicalize_existing(v) {
                            native_access.grant_read(p, false);
                        }
                    }
                }
            }

            let access_state = Arc::new(Mutex::new(native_access));
            let vault_store = Arc::new(CoordinatorVaultStore(coordinator.clone()));
            let vault_use_cases = Arc::new(openunpack_application::VaultUseCases::new(vault_store));
            let sys_env: Arc<dyn openunpack_application::SystemEnvironment> =
                Arc::new(openunpack_infrastructure::WindowsSystemEnvironment::new());

            app.manage(AppState {
                coordinator: coordinator.clone(),
                vault_use_cases,
                sys_env,
                access: access_state,
                _lock_file: lock_file,
                scans: Mutex::new(HashMap::new()),
                clipboard_sequence: Mutex::new(None),
                exiting: AtomicBool::new(false),
                minimize_to_tray: AtomicBool::new(
                    coordinator
                        .snapshot()
                        .map(|s| s.settings.minimize_to_tray)
                        .unwrap_or(false),
                ),
            });
            let api_manager = local_api::ApiManager::open(&coordinator.data);
            app.manage(api_manager.clone());
            let api_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                api_manager.restore(api_app).await;
            });
            install_tray(app)?;
            if let (Some(w), Ok(s)) = (app.get_webview_window("main"), coordinator.snapshot()) {
                w.set_always_on_top(s.settings.always_on_top)?;
            }

            let app_handle = app.handle().clone();
            let mut sub = coordinator.subscribe();
            tauri::async_runtime::spawn(async move {
                let mut known_finished = HashSet::new();
                if let Some(s) = app_handle.try_state::<AppState>()
                    && let Ok(snapshot) = s.coordinator.snapshot()
                {
                    for j in snapshot.jobs {
                        if j.finished_at.is_some() {
                            known_finished.insert(j.id);
                        }
                    }
                }
                while let Ok(()) | Err(RecvError::Lagged(_)) = sub.recv().await {
                    if let Some(s) = app_handle.try_state::<AppState>() {
                        let c = s.coordinator.clone();
                        let (session_id, revision) = c.revision_info();
                        let invalidated = openunpack_protocol::StateInvalidated {
                            session_id,
                            revision: revision.to_string(),
                        };
                        let _ = app_handle.emit_to("main", "state-changed", invalidated);
                        let beep = c.newly_finished_jobs(&mut known_finished);
                        if beep && c.completion_sound() {
                            let _ = spawn_blocking(|| unsafe {
                                windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(0x40)
                            })
                            .await;
                        }
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let app = window.app_handle().clone();
                    let to_tray = window
                        .try_state::<AppState>()
                        .is_some_and(|s| s.minimize_to_tray.load(Ordering::Acquire));
                    if to_tray {
                        let _ = window.hide();
                    } else {
                        request_exit(app);
                    }
                }
                if matches!(event, WindowEvent::Resized(_))
                    && window
                        .try_state::<AppState>()
                        .is_some_and(|s| s.minimize_to_tray.load(Ordering::Acquire))
                    && window.is_minimized().unwrap_or(false)
                {
                    let _ = window.hide();
                }
                if let WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event
                    && let Some(state) = window.try_state::<AppState>()
                    && let Ok(mut guard) = state.access.lock()
                {
                    for path in paths {
                        if let Some(raw) = path.to_str() {
                            guard.native_drop_paths.insert(raw.to_string());
                            if let Ok(canon) = canonicalize_existing(raw)
                                && let Some(parent) = canon.parent()
                            {
                                guard.grant_write_dir(parent.to_path_buf());
                            }
                        }
                    }
                }
            }
        })
        .invoke_handler(generate_handler![
            set_ui_language,
            local_api::api_status,
            local_api::api_configure,
            local_api::api_create_client,
            local_api::api_revoke_client,
            local_api::api_open_docs,
            snapshot,
            queue_snapshot,
            snapshot_update,
            history_page,
            job_details,
            scan_sources,
            add_jobs,
            start_jobs,
            pause_jobs,
            cancel_jobs,
            retry_job,
            remove_job,
            clear_completed,
            passwords,
            save_password,
            delete_password,
            reveal_password,
            preview_import,
            confirm_import,
            export_backup,
            preview_restore_backup,
            confirm_restore_backup,
            copy_password,
            bulk_password_action,
            cancel_scan,
            retry_node,
            preview_exclusions,
            cancel_preview_exclusions,
            save_settings,
            history,
            clear_history,
            recover_job,
            inspect_archive,
            cancel_inspect_archive,
            extract_selective,
            open_path,
            pick_files,
            pick_directory,
            pick_save_backup,
            pick_open_backup,
            read_import_file,
            preview_toolbox_operation,
            execute_toolbox_operation,
            execute_post_action,
            get_shell_context_menu_status,
            set_shell_context_menu_status,
        ])
        .run(generate_context!())
        .expect("error while running OpenUnpack desktop application");
}

fn collect_flag_values(args: &[String], flag: &str) -> Vec<String> {
    args.iter()
        .enumerate()
        .filter(|(_, arg)| arg.as_str() == flag)
        .filter_map(|(index, _)| args.get(index + 1))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn canonical_api_roots(raw: &[String], kind: &str) -> Vec<String> {
    raw.iter()
        .map(|value| {
            let canonical = canonicalize_existing(value).unwrap_or_else(|error| {
                eprintln!("[Error] --{kind}-root '{value}' is not usable: {error}");
                std::process::exit(1);
            });
            if !canonical.is_dir() {
                eprintln!("[Error] --{kind}-root '{value}' must be an existing directory.");
                std::process::exit(1);
            }
            canonical.to_string_lossy().into_owned()
        })
        .collect()
}

fn mask_token(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 10 {
        return "***".to_string();
    }
    let head: String = chars[..6].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

pub fn run_headless() {
    println!("==================================================");
    println!(" OpenUnpack Daemon (Headless Mode)");
    println!("==================================================");
    let args: Vec<String> = std::env::args().collect();

    // Check for --data-dir override
    if let Some(pos) = args.iter().position(|a| a == "--data-dir")
        && let Some(val) = args.get(pos + 1)
    {
        unsafe {
            std::env::set_var("OPENUNPACK_DATA_DIR", val);
        }
    }
    // Check for --port override
    let custom_port: Option<u16> = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|pos| args.get(pos + 1))
        .and_then(|val| val.parse().ok());

    // Check for --trust-all (headless only). This is process state, never an
    // inherited environment variable, so a parent process cannot disable path
    // authorization just by launching the daemon.
    if args.iter().any(|a| a == "--trust-all") {
        openunpack_local_api::set_trusted_all_paths(true);
        eprintln!(
            "[Warning] --trust-all enabled: local API path authorization is bypassed for this process."
        );
    }
    // Check for --on-complete or --on-extract-complete
    let on_complete_script: Option<String> = args
        .iter()
        .position(|a| a == "--on-complete" || a == "--on-extract-complete")
        .and_then(|pos| args.get(pos + 1))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    // Check for --watch-dir or --watch
    let watch_dirs: Vec<PathBuf> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--watch-dir" || *a == "--watch")
        .filter_map(|(i, _)| args.get(i + 1).map(PathBuf::from))
        .collect();

    // Credentials and path grants are opt-in: a plain headless boot must neither
    // print a full token into container logs nor force operators to choose
    // between "no file access" and "--trust-all".
    let init_token = args.iter().any(|a| a == "--init-token");
    let read_roots = canonical_api_roots(&collect_flag_values(&args, "--read-root"), "read");
    let write_roots = canonical_api_roots(&collect_flag_values(&args, "--write-root"), "write");

    let data_dir = match resolve_data_directory() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("[Error] Failed to resolve data directory: {err}");
            std::process::exit(1);
        }
    };
    println!("Data directory: {}", data_dir.display());

    let lock_file = match acquire_instance_lock(&data_dir) {
        Ok(file) => file,
        Err(err) => {
            eprintln!(
                "[Error] Another instance is already running or data directory is locked: {err}"
            );
            std::process::exit(1);
        }
    };

    let (worker_exe, worker_dll) = match resolve_worker_paths_standalone() {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("[Error] Worker engine binaries missing: {err}");
            std::process::exit(1);
        }
    };
    println!("Worker engine:  {}", worker_exe.display());

    let coordinator = match Coordinator::open(data_dir.clone(), worker_exe, worker_dll) {
        Ok(coord) => coord,
        Err(err) => {
            eprintln!("[Error] Failed to initialize Coordinator: {err}");
            std::process::exit(1);
        }
    };

    // `--on-complete` used to be written to an environment variable that nothing
    // read, so headless automation silently had no hook. Persist it into settings
    // instead, which is what the scheduler actually consults.
    if let Some(script) = &on_complete_script {
        let path = std::path::Path::new(script);
        if !path.is_absolute() || !path.is_file() {
            eprintln!(
                "[Error] --on-complete must point to an existing absolute script path: {script}"
            );
            std::process::exit(1);
        }
        match coordinator.snapshot() {
            Ok(snapshot) => {
                let mut settings = snapshot.settings.clone();
                settings.on_complete = Some(script.clone());
                if let Err(err) = coordinator.save_settings(settings) {
                    eprintln!("[Error] Failed to persist --on-complete hook: {err}");
                    std::process::exit(1);
                }
                println!("On-complete hook: {script}");
            }
            Err(err) => {
                eprintln!("[Error] Failed to read settings for --on-complete: {err}");
                std::process::exit(1);
            }
        }
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to initialize Tokio runtime");

    rt.block_on(async move {
        let mut native_access = NativeAccess::default();
        if let Ok(snap) = coordinator.snapshot() {
            if !snap.settings.output_root.trim().is_empty()
                && let Ok(p) = canonicalize_existing(&snap.settings.output_root)
            {
                native_access.grant_write_dir(p);
            }
            if !snap.settings.cache_root.trim().is_empty()
                && let Ok(p) = canonicalize_existing(&snap.settings.cache_root)
            {
                native_access.grant_write_dir(p);
            }
            for j in &snap.jobs {
                if let Ok(p) = canonicalize_existing(&j.source.path) {
                    native_access.grant_read(p, false);
                }
                for v in &j.source.volumes {
                    if let Ok(p) = canonicalize_existing(v) {
                        native_access.grant_read(p, false);
                    }
                }
            }
        }

        let access_state = Arc::new(Mutex::new(native_access));
        let vault_store = Arc::new(CoordinatorVaultStore(coordinator.clone()));
        let vault_use_cases = Arc::new(openunpack_application::VaultUseCases::new(vault_store));
        let sys_env: Arc<dyn openunpack_application::SystemEnvironment> =
            Arc::new(openunpack_infrastructure::WindowsSystemEnvironment::new());

        let app_state = Arc::new(AppState {
            coordinator: coordinator.clone(),
            vault_use_cases,
            sys_env,
            access: access_state,
            _lock_file: lock_file,
            scans: Mutex::new(HashMap::new()),
            clipboard_sequence: Mutex::new(None),
            exiting: AtomicBool::new(false),
            minimize_to_tray: AtomicBool::new(false),
        });

        // Register any CLI watch directories
        for w in &watch_dirs {
            let p_str = w.to_string_lossy().to_string();
            if let Ok(canon) = canonicalize_existing(&p_str) {
                let canon_str = canon.to_string_lossy().to_string();
                println!("Registering watch directory: {}", canon_str);
                if let Ok(mut lock) = app_state.access.lock() {
                    lock.grant_read(canon.clone(), true);
                    lock.grant_write_dir(canon);
                }
                if let Ok(snap) = coordinator.snapshot() {
                    let mut s = snap.settings.clone();
                    if !s.watched_folders.contains(&canon_str) {
                        s.watched_folders.push(canon_str);
                        let _ = coordinator.save_settings(s);
                    }
                }
            } else {
                eprintln!(
                    "[Warning] Watch directory does not exist or inaccessible: {}",
                    p_str
                );
            }
        }

        // Initialize background folder watcher
        coordinator.init_folder_watcher();

        // Start Local API server
        let api_manager = local_api::ApiManager::open(&coordinator.data);
        if let Ok(store) = api_manager.store() {
            let mut settings = store.settings().unwrap_or_default();
            settings.enabled = true;
            if let Some(port) = custom_port {
                settings.port = port;
            }
            let _ = store.save_settings(settings.clone());
            let _ = api_manager.restore_standalone(app_state.clone()).await;

            // Ensure at least one client exists for local operation. The default
            // grant is deliberately least-privilege: vault and settings writes
            // must be requested explicitly, and the token expires.
            let clients = store.clients().unwrap_or_default();
            if clients.is_empty() || init_token {
                let scopes = [
                    openunpack_local_api::Scope::JobsRead,
                    openunpack_local_api::Scope::JobsWrite,
                    openunpack_local_api::Scope::FilesRead,
                    openunpack_local_api::Scope::FilesWrite,
                    openunpack_local_api::Scope::SettingsRead,
                ]
                .into_iter()
                .collect();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let name = if clients.is_empty() {
                    "default-cli".to_string()
                } else {
                    format!("cli-{now}")
                };
                let default_client = openunpack_local_api::ApiClientInput {
                    name,
                    scopes,
                    read_roots: read_roots.clone(),
                    write_roots: write_roots.clone(),
                    expires_at: Some((now + 30 * 24 * 3600).to_string()),
                };
                match store.create_client(default_client) {
                    Ok(created) => {
                        println!("--------------------------------------------------");
                        println!(" Created API Client: {}", created.client.name);
                        if init_token {
                            println!(" Token: {}", created.token);
                            println!(" Store it now; the full value is never shown again.");
                        } else {
                            println!(
                                " Token: {} (pass --init-token to print a new full token)",
                                mask_token(&created.token)
                            );
                        }
                        if let Some(expires) = &created.client.expires_at {
                            println!(" Expires at (unix seconds): {expires}");
                        }
                    }
                    Err(error) => {
                        eprintln!("[Warning] Failed to create the default API client: {error}");
                    }
                }
            }
        }

        println!("--------------------------------------------------");
        if let Ok(status) = api_manager.status().await
            && let Some(url) = status.base_url
        {
            println!(" Local API listening at: {}", url);
        }
        println!(" OpenUnpack daemon is active. Press Ctrl+C to stop.");
        println!("==================================================");

        let _ = tokio::signal::ctrl_c().await;
        println!("\nReceived shutdown signal, stopping OpenUnpack daemon...");
        let _ = coordinator.cancel(None);
        let _ = coordinator.pause();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use openunpack_protocol::{ConflictStrategy, ToolboxKind};

    #[test]
    fn import_encodings_preserve_unicode_and_reject_invalid_bytes() {
        let text = "  中文𠀀口令  \r\n";
        let (gb, _, errors) = encoding_rs::GB18030.encode(text);
        assert!(!errors);
        assert_eq!(decode_import_bytes(&gb, "gb18030").unwrap(), text);
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_import_bytes(&utf16, "utf-16le").unwrap(), text);
        assert_eq!(decode_import_bytes(&utf16[2..], "UTF-16LE").unwrap(), text);
        assert_eq!(decode_import_bytes(text.as_bytes(), "utf-8").unwrap(), text);
        assert!(decode_import_bytes(&[0x00, 0xD8], "utf-16le").is_err());
        assert!(decode_import_bytes(&[0x01], "utf-16le").is_err());
        assert!(decode_import_bytes(&[0xFE, 0xFF, 0x00, 0x61], "utf-16le").is_err());
        assert!(decode_import_bytes(&[0xFF], "utf-8").is_err());
        assert!(decode_import_bytes(&[0x81], "gb18030").is_err());
    }

    #[test]
    fn test_origin_verification() {
        assert!(is_trusted_origin_url("tauri://localhost"));
        assert!(is_trusted_origin_url("tauri://localhost/index.html"));
        assert!(is_trusted_origin_url("http://tauri.localhost"));
        assert!(is_trusted_origin_url("https://tauri.localhost"));
        assert!(!is_trusted_origin_url("http://attacker.com"));
        assert!(!is_trusted_origin_url("https://attacker.localhost"));
        assert!(!is_trusted_origin_url("http://user:pass@tauri.localhost"));
        assert!(!is_trusted_origin_url("http://tauri.localhost:8080"));
        assert!(!is_trusted_origin_url("file:///C:/malicious.html"));
    }

    #[test]
    fn test_path_containment_helper() {
        let base = PathBuf::from("C:\\Users\\Public\\OpenUnpack");
        let child = PathBuf::from("C:\\Users\\Public\\OpenUnpack\\sub\\file.txt");
        let outside = PathBuf::from("C:\\Users\\Public\\Other\\file.txt");
        let prefix_spoof = PathBuf::from("C:\\Users\\Public\\OpenUnpackSecret\\file.txt");

        assert!(path_contains_or_equal(&base, &base));
        assert!(path_contains_or_equal(&base, &child));
        assert!(!path_contains_or_equal(&base, &outside));
        assert!(!path_contains_or_equal(&base, &prefix_spoof));
    }

    #[test]
    fn toolbox_native_access_guard_enforces_directory_authorization() {
        let temp = tempfile::tempdir().unwrap();
        let canon = canonicalize_existing(&temp.path().to_string_lossy()).unwrap();
        let unauth_temp = tempfile::tempdir().unwrap();
        let unauth_canon = canonicalize_existing(&unauth_temp.path().to_string_lossy()).unwrap();

        let access = Mutex::new(NativeAccess::default());

        let unauth_opts = ToolboxOptions {
            kind: ToolboxKind::NormalizeExtensions,
            source_directory: unauth_canon.to_string_lossy().into_owned(),
            target_directory: None,
            recursive: true,
            remove_empty_folders: false,
            conflict_strategy: ConflictStrategy::Rename,
            rename_rule: None,
        };

        // Unauthorized access must be rejected
        assert!(authorize_toolbox_options(&access, unauth_opts).is_err());

        // Grant access
        {
            let mut guard = access.lock().unwrap();
            guard.grant_read(canon.clone(), true);
            guard.grant_write_dir(canon.clone());
        }

        let auth_opts = ToolboxOptions {
            kind: ToolboxKind::NormalizeExtensions,
            source_directory: canon.to_string_lossy().into_owned(),
            target_directory: None,
            recursive: true,
            remove_empty_folders: false,
            conflict_strategy: ConflictStrategy::Rename,
            rename_rule: None,
        };

        // Authorized access must succeed
        let authorized = authorize_toolbox_options(&access, auth_opts).unwrap();
        assert_eq!(authorized.source_directory, canon.to_string_lossy());
    }

    #[test]
    fn strip_verbatim_prefix_cleans_extended_paths() {
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\C:\Users\im\Desktop")),
            PathBuf::from(r"C:\Users\im\Desktop")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\UNC\server\share\sub")),
            PathBuf::from(r"\\server\share\sub")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"C:\Users\im\Desktop")),
            PathBuf::from(r"C:\Users\im\Desktop")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\server\share\sub")),
            PathBuf::from(r"\\server\share\sub")
        );
    }

    #[test]
    fn sanitize_explorer_target_rejects_switches_and_reserved_devices() {
        assert!(sanitize_explorer_target(Path::new("-select,C:\\Windows")).is_err());
        assert!(sanitize_explorer_target(Path::new("/e,C:\\Windows")).is_err());
        assert!(sanitize_explorer_target(Path::new("C:\\Windows\n")).is_err());
        assert!(sanitize_explorer_target(Path::new(r"C:\CON")).is_err());
        assert!(sanitize_explorer_target(Path::new(r"C:\Windows\AUX\sub")).is_err());
        assert!(sanitize_explorer_target(Path::new(r"relative\path")).is_err());

        let temp = tempfile::tempdir().unwrap();
        let canon = temp.path().canonicalize().unwrap();
        let safe = sanitize_explorer_target(&canon).unwrap();
        assert!(!safe.to_string_lossy().starts_with(r"\\?\"));
        assert!(safe.is_dir());
    }

    #[test]
    fn inplace_extraction_requires_parent_write_permission() {
        let temp = tempfile::tempdir().unwrap();
        let test_file = temp.path().join("archive.zip");
        fs::write(&test_file, b"test content").unwrap();
        let file_canon = canonicalize_existing(&test_file.to_string_lossy()).unwrap();
        let parent_canon = canonicalize_existing(&temp.path().to_string_lossy()).unwrap();

        let access = Arc::new(Mutex::new(NativeAccess::new()));

        // Case 1: only read granted on the file itself, no write granted on parent
        {
            let mut guard = access.lock().unwrap();
            guard.grant_read(file_canon.clone(), false);
        }

        let guard = access.lock().unwrap().clone();
        assert!(guard.is_readable(&file_canon));
        assert!(!guard.is_writable_dir(&parent_canon));

        // Case 2: grant write to parent
        {
            let mut guard = access.lock().unwrap();
            guard.grant_write_dir(parent_canon.clone());
        }
        let guard = access.lock().unwrap().clone();
        assert!(guard.is_writable_dir(&parent_canon));
    }
}

#[cfg(test)]
mod headless_args_tests {
    use super::{collect_flag_values, mask_token};

    #[test]
    fn collects_repeated_flag_values_and_skips_blanks() {
        let args: Vec<String> = [
            "--read-root",
            "D://in",
            "--read-root",
            "   ",
            "--write-root",
            "D://out",
        ]
        .iter()
        .map(|value| value.to_string())
        .collect();
        assert_eq!(collect_flag_values(&args, "--read-root"), vec!["D://in"]);
        assert_eq!(collect_flag_values(&args, "--write-root"), vec!["D://out"]);
        assert!(collect_flag_values(&args, "--missing").is_empty());
    }

    #[test]
    fn masks_tokens_without_revealing_the_body() {
        let token =
            "oup_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let masked = mask_token(token);
        assert!(masked.starts_with("oup_01"));
        assert!(masked.ends_with(&token[token.len() - 4..]));
        assert!(!masked.contains(&token[10..50]));
        assert_eq!(mask_token("short"), "***");
    }
}
