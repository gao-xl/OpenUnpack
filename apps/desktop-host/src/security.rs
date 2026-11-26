//! 桌面宿主路径安全校验、重解析点检查、工作进程解析与实例锁管理。

use super::*;

pub type Window = WebviewWindow;

pub(super) const MAX_IMPORT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Default)]
pub struct NativeAccess {
    pub(crate) readable_entries: HashSet<PathBuf>,
    pub(crate) readable_directories: HashSet<PathBuf>,
    pub(crate) writable_directories: HashSet<PathBuf>,
    pub(crate) backup_read_files: HashSet<PathBuf>,
    pub(crate) backup_write_files: HashSet<PathBuf>,
    pub(crate) native_drop_paths: HashSet<String>,
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
                .any(|b| path_starts_with_case_insensitive(target, b))
    }
    pub fn is_writable_dir(&self, target: &Path) -> bool {
        self.writable_directories
            .iter()
            .any(|b| path_starts_with_case_insensitive(target, b))
    }
    pub fn is_backup_read_allowed(&self, target: &Path) -> bool {
        self.backup_read_files.contains(target)
    }
    pub fn is_backup_write_allowed(&self, target: &Path) -> bool {
        self.backup_write_files.contains(target)
    }
}

pub struct PendingOpenPaths(pub Mutex<Vec<String>>);

pub(super) fn authorize_open_paths(
    access: &Mutex<NativeAccess>,
    raw_paths: &[String],
) -> Vec<String> {
    let mut authorized = Vec::new();
    let Ok(mut guard) = access.lock() else {
        return authorized;
    };
    for raw in raw_paths {
        let Ok(canon) = canonicalize_existing(raw) else {
            tracing::warn!(path = %raw, "cli_or_wake_path_rejected");
            continue;
        };
        let display = strip_verbatim_prefix(&canon).to_string_lossy().into_owned();
        guard.native_drop_paths.insert(raw.clone());
        guard.native_drop_paths.insert(display.clone());
        guard.grant_read(canon.clone(), canon.is_dir());
        if canon.is_dir() {
            guard.grant_write_dir(canon.clone());
        } else if let Some(parent) = canon.parent() {
            guard.grant_write_dir(parent.to_path_buf());
        }
        authorized.push(display);
    }
    authorized
}

#[tauri::command]
pub(crate) fn take_pending_open_paths(
    window: WebviewWindow,
    pending: State<'_, PendingOpenPaths>,
) -> Result<Vec<String>, String> {
    verify_frontend_window(&window)?;
    let mut guard = pending
        .0
        .lock()
        .map_err(|_| "打开路径队列状态不可用。".to_string())?;
    Ok(std::mem::take(&mut *guard))
}

#[cfg(windows)]
pub(super) const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
#[cfg(windows)]
pub(super) const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
#[cfg(windows)]
pub(super) const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
#[cfg(windows)]
pub(super) const IO_REPARSE_TAG_LX_SYMLINK: u32 = 0xA000_001D;

/// Permits cloud file placeholders (OneDrive / ProjFS / Dedup) on Windows while
/// blocking symlinks, NTFS junctions, and arbitrary custom reparse points.
#[cfg(windows)]
pub(super) fn is_allowed_cloud_reparse_tag(tag: u32) -> bool {
    (tag & 0xFFFF_0FFF) == 0x9000_001A || matches!(tag, 0x8000_0013 | 0x8000_0021 | 0x9000_001C)
}

#[cfg(windows)]
pub(super) fn query_reparse_tag(path: &Path) -> Option<u32> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{FindClose, FindFirstFileW, WIN32_FIND_DATAW};

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
    if wide.len() <= 1 {
        return None;
    }
    // Trim trailing directory separator so FindFirstFileW queries the directory entry itself.
    while wide.len() > 2 && matches!(wide[wide.len() - 2], 47 | 92) && wide[wide.len() - 3] != 58 {
        let nul = wide.pop().unwrap_or(0);
        wide.pop();
        wide.push(nul);
    }
    let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    let handle = unsafe { FindFirstFileW(wide.as_ptr(), &mut data) };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    unsafe { FindClose(handle) };
    if (data.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        Some(data.dwReserved0)
    } else {
        None
    }
}

pub(super) fn verify_no_reparse_ancestors(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    use std::os::windows::fs::MetadataExt;

    let mut current = Some(path);
    while let Some(segment) = current {
        if segment.as_os_str().is_empty() {
            break;
        }
        if segment.exists() {
            let meta =
                fs::symlink_metadata(segment).map_err(|_| "Native operation failed".to_string())?;
            #[cfg(windows)]
            {
                let has_reparse_attr = (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
                if meta.file_type().is_symlink() || has_reparse_attr {
                    let allowed = matches!(
                        query_reparse_tag(segment),
                        Some(tag)
                            if !matches!(
                                tag,
                                IO_REPARSE_TAG_SYMLINK
                                    | IO_REPARSE_TAG_MOUNT_POINT
                                    | IO_REPARSE_TAG_LX_SYMLINK
                            ) && is_allowed_cloud_reparse_tag(tag)
                    );
                    if !allowed {
                        return Err(format!(
                            "Path or ancestor is a symlink/reparse point: {}",
                            segment.display()
                        ));
                    }
                }
            }
            #[cfg(not(windows))]
            if meta.file_type().is_symlink() {
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

pub(super) fn sanitize_path_string(raw: &str) -> Result<PathBuf, String> {
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

pub(super) fn canonicalize_existing(raw: &str) -> Result<PathBuf, String> {
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

pub(super) fn canonicalize_for_save(raw: &str) -> Result<PathBuf, String> {
    let path = sanitize_path_string(raw)?;
    if path.exists() {
        return canonicalize_existing(raw);
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| "Target path must have a valid file name".to_string())?;

    let mut current = path.parent();
    let mut uncreated_segments = Vec::new();

    let existing_ancestor = loop {
        match current {
            Some(dir) => {
                if dir.as_os_str().is_empty() {
                    return Err(format!(
                        "No existing ancestor directory found for path: {}",
                        path.display()
                    ));
                }
                if dir.exists() {
                    break dir;
                }
                if let Some(name) = dir.file_name() {
                    uncreated_segments.push(name);
                }
                current = dir.parent();
            }
            None => {
                return Err(format!(
                    "No existing ancestor directory found for path: {}",
                    path.display()
                ));
            }
        }
    };

    verify_no_reparse_ancestors(existing_ancestor)?;
    let canonical_ancestor = existing_ancestor
        .canonicalize()
        .map_err(|_| "Native operation failed".to_string())?;
    verify_no_reparse_ancestors(&canonical_ancestor)?;

    let mut result = canonical_ancestor;
    for seg in uncreated_segments.into_iter().rev() {
        result.push(seg);
    }
    result.push(file_name);
    Ok(result)
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

pub(super) fn verify_frontend_window(window: &WebviewWindow) -> Result<(), String> {
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

pub(super) fn resolve_worker_paths_standalone() -> Result<(PathBuf, PathBuf), String> {
    // Single-stack native Rust microkernel
    Ok((
        PathBuf::from(openunpack_protocol::worker::INPROCESS_WORKER_MARKER),
        PathBuf::from(openunpack_protocol::worker::INPROCESS_ENGINE_MARKER),
    ))
}

pub(super) fn resolve_worker_paths(_app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), String> {
    resolve_worker_paths_standalone()
}

pub fn resolve_data_directory() -> Result<PathBuf, String> {
    if let Ok(custom) = std::env::var("OPENUNPACK_DATA_DIR") {
        let p = PathBuf::from(custom.trim());
        if !p.as_os_str().is_empty() {
            fs::create_dir_all(&p).map_err(|_| "Native operation failed".to_string())?;
            return Ok(p);
        }
    }
    // Check if running in portable mode (e.g. "data" directory adjacent to executable)
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        let portable_data = exe_dir.join("data");
        if portable_data.is_dir() {
            return Ok(portable_data);
        }
    }
    #[cfg(windows)]
    let path = {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA")
            && !local_app_data.trim().is_empty()
        {
            PathBuf::from(local_app_data).join("OpenUnpack")
        } else if let Ok(user_profile) = std::env::var("USERPROFILE")
            && !user_profile.trim().is_empty()
        {
            PathBuf::from(user_profile)
                .join("AppData")
                .join("Local")
                .join("OpenUnpack")
        } else {
            return Err(
                "LOCALAPPDATA and USERPROFILE environment variables are missing".to_string(),
            );
        }
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

pub fn acquire_instance_lock(data_dir: &Path) -> Result<File, String> {
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

pub fn setup_process_environment(data_dir: &Path) {
    if std::env::var("OPENUNPACK_DATA_DIR").is_err() {
        unsafe {
            std::env::set_var("OPENUNPACK_DATA_DIR", data_dir);
        }
    }
    if std::env::var("WEBVIEW2_USER_DATA_FOLDER").is_err() {
        let wv2_dir = data_dir.join("webview2");
        let _ = fs::create_dir_all(&wv2_dir);
        unsafe {
            std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &wv2_dir);
        }
    }
    // Clean up stale WebView2 locks when this process is the sole instance owner
    let default_lock = data_dir
        .join("webview2")
        .join("EBWebView")
        .join("Default")
        .join("LOCK");
    if default_lock.exists() {
        let _ = fs::remove_file(default_lock);
    }
}
