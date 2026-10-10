mod clipboard;
use openunpack_core::{
    coordinator::Coordinator,
    types::{
        AppSnapshot, ImportPreview, ImportResult, JobView, PasswordInput, PasswordView, ScanResult,
        Settings, SourceGroup,
    },
};
use rfd::{FileDialog, MessageDialog, MessageLevel,MessageButtons,MessageDialogResult};
use std::{
    collections::{HashMap,HashSet},
    fs::{self, File, OpenOptions},
    io::Read,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex,atomic::{AtomicBool,Ordering}},
};
use tauri::{
    Builder, Emitter, Manager, State, WebviewWindow, WindowEvent, async_runtime::spawn_blocking,
    generate_context, generate_handler,
};
use tokio::sync::broadcast::error::RecvError;

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
    pub access: Arc<Mutex<NativeAccess>>,
    pub _lock_file: File,
    scans:Mutex<HashMap<String,Arc<AtomicBool>>>,
    clipboard_sequence:Mutex<Option<u32>>,
    exiting:AtomicBool,
    minimize_to_tray:AtomicBool,
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

fn resolve_worker_paths(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let resource = app
        .path()
        .resource_dir()
        .map_err(|_| "Resource directory unavailable".to_string())?
        .join("worker");
    let packaged = (
        resource.join("OpenUnpack.Worker.exe"),
        resource.join("7z.dll"),
    );
    if packaged.0.is_file() && packaged.1.is_file() {
        return Ok(packaged);
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p|p.parent().map(Path::to_path_buf)) {
        let pair = (dir.join("worker/OpenUnpack.Worker.exe"),dir.join("worker/7z.dll"));
        if pair.0.is_file() && pair.1.is_file() { return Ok(pair); }
    }
    // Only development builds may use files from the source checkout. A release
    // must report its own missing engine instead of silently depending on it.
    #[cfg(debug_assertions)]
    {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dev_root = manifest_dir.join("../../artifacts/worker");
    let dev_exe = dev_root.join("OpenUnpack.Worker.exe");
    let dev_dll = dev_root.join("7z.dll");
    if dev_exe.is_file() && dev_dll.is_file() {
        return Ok((dev_exe, dev_dll));
    }

    let source_exe = manifest_dir
        .join("../../src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe");
    let source_dll = manifest_dir.join("../../tools/7zip/7z.dll");
    if source_exe.is_file() && source_dll.is_file() {
        return Ok((source_exe, source_dll));
    }
    }
    // The shell and vault remain usable; Coordinator reports engineAvailable=false.
    Ok(packaged)
}

fn resolve_data_directory() -> Result<PathBuf, String> {
    if let Ok(custom) = std::env::var("OPENUNPACK_DATA_DIR") {
        let p = PathBuf::from(custom.trim());
        if !p.as_os_str().is_empty() {
            fs::create_dir_all(&p).map_err(|_| "Native operation failed".to_string())?;
            return Ok(p);
        }
    }
    let local_app_data = std::env::var("LOCALAPPDATA")
        .map_err(|_| "LOCALAPPDATA environment variable is missing".to_string())?;
    let path = PathBuf::from(local_app_data).join("OpenUnpack");
    fs::create_dir_all(&path).map_err(|_| "Native operation failed".to_string())?;
    Ok(path)
}

fn acquire_instance_lock(data_dir: &Path) -> Result<File, String> {
    let lock_path = data_dir.join(".instance.lock");
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(&lock_path)
        .map_err(|_| "Another instance is running or the data directory is unavailable".to_string())
}

#[tauri::command]
async fn snapshot(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.snapshot())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn scan_sources(
    window: WebviewWindow,
    state: State<'_, AppState>,
    paths: Vec<String>,
    request_id:Option<String>,
) -> Result<ScanResult, String> {
    verify_frontend_window(&window)?;
    let scan_id=request_id.unwrap_or_else(||format!("scan-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    if scan_id.len()>128{return Err("扫描请求编号过长。".into());}
    let cancel=Arc::new(AtomicBool::new(false));
    {let mut scans=state.scans.lock().map_err(|_|"扫描状态不可用。")?;if scans.len()>=2||scans.contains_key(&scan_id){return Err("已有扫描请求，请等待或取消。".into());}scans.insert(scan_id.clone(),cancel.clone());}
    let result=scan_authorized(&state,&paths,cancel).await;
    if let Ok(mut scans)=state.scans.lock(){scans.remove(&scan_id);}
    result
}
async fn scan_authorized(state:&AppState,paths:&[String],cancel:Arc<AtomicBool>)->Result<ScanResult,String>{
    let paths=paths.to_vec();
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
        let result = coord.scan_with_cancel(&checked_paths,&cancel)?;
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

#[tauri::command]
async fn cancel_scan(window:WebviewWindow,state:State<'_,AppState>,request_id:String)->Result<(),String>{verify_frontend_window(&window)?;if let Some(cancel)=state.scans.lock().map_err(|_|"扫描状态不可用。")?.get(&request_id){cancel.store(true,Ordering::Release);}Ok(())}

#[derive(serde::Serialize)]struct ExclusionMatch {path:String,matched:bool}
#[tauri::command]
async fn preview_exclusions(window:WebviewWindow,paths:Vec<String>,patterns:Vec<String>)->Result<Vec<ExclusionMatch>,String>{verify_frontend_window(&window)?;if paths.len()>1000||patterns.len()>100||paths.iter().any(|p|p.len()>32768)||patterns.iter().any(|p|p.len()>256){return Err("匹配预览内容超出限制。".into());}Ok(paths.into_iter().map(|path|{let matched=openunpack_core::scanner::matches_exclusion(&path,&patterns);ExclusionMatch{path,matched}}).collect())}

#[tauri::command]
async fn retry_node(window:WebviewWindow,state:State<'_,AppState>,job_id:String,node_id:String)->Result<(),String>{verify_frontend_window(&window)?;let coord=state.coordinator.clone();let c=coord.clone();spawn_blocking(move||c.retry_node(&job_id,&node_id)).await.map_err(|_|"节点重试未完成。")??;coord.start()}

#[tauri::command]
async fn bulk_password_action(window:WebviewWindow,state:State<'_,AppState>,ids:Vec<String>,action:String,groups:Option<Vec<String>>)->Result<(),String>{verify_frontend_window(&window)?;let coord=state.coordinator.clone();spawn_blocking(move||{coord.vault()?.bulk_action(&ids,&action,&groups.unwrap_or_default())?;coord.changed();Ok(())}).await.map_err(|_|"批量操作未完成。")?}

#[tauri::command]
async fn copy_password(window:WebviewWindow,state:State<'_,AppState>,id:String)->Result<(),String>{
    verify_frontend_window(&window)?;
    let owner=window.hwnd().map_err(|_|"窗口句柄不可用。")?.0 as usize;let coord=state.coordinator.clone();
    let sequence=spawn_blocking(move||{let text=zeroize::Zeroizing::new(coord.vault()?.reveal(&id)?);clipboard::copy(owner,&text)}).await.map_err(|_|"复制密码未完成。")??;
    *state.clipboard_sequence.lock().map_err(|_|"剪贴板状态不可用。")?=Some(sequence);
    tauri::async_runtime::spawn(async move{tokio::time::sleep(std::time::Duration::from_secs(30)).await;let _=spawn_blocking(move||clipboard::clear_if_unchanged(sequence)).await;});Ok(())
}

#[tauri::command]
async fn add_jobs(
    window: WebviewWindow,
    state: State<'_, AppState>,
    sources: Vec<SourceGroup>,
    options: Settings,
) -> Result<Vec<JobView>, String> {
    verify_frontend_window(&window)?;
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

#[tauri::command]
async fn start_jobs(window: WebviewWindow, state: State<'_, AppState>) -> Result<(), String> {
    verify_frontend_window(&window)?;
    state.coordinator.start()
}

#[tauri::command]
async fn pause_jobs(window: WebviewWindow, state: State<'_, AppState>) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.pause())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn cancel_jobs(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: Option<String>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.cancel(job_id.as_deref()))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn retry_job(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
    partial: bool,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    let c = coord.clone();
    let jid = job_id.clone();
    spawn_blocking(move || c.retry(&jid, partial))
        .await
        .map_err(|_| "Background operation failed".to_string())??;
    coord.start()
}

#[tauri::command]
async fn remove_job(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.remove(&job_id))
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn clear_completed(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.clear_completed())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn passwords(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<PasswordView>, String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.vault()?.views())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn save_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    input: PasswordInput,
) -> Result<PasswordView, String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || {
        let result = coord.vault()?.upsert(input)?;
        coord.changed();
        Ok(result)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn delete_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || {
        coord.vault()?.delete(&id)?;
        coord.changed();
        Ok(())
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn reveal_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.vault()?.reveal(&id))
        .await
        .map_err(|_| "Background operation failed".to_string())?
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
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.vault()?.preview_import(&text, &format, &group))
        .await
        .map_err(|_| "Background operation failed".to_string())?
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
    let coord = state.coordinator.clone();
    spawn_blocking(move || {
        let result = coord
            .vault()?
            .apply_import(&token, &text, &format, &group)?;
        coord.changed();
        Ok(result)
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
    spawn_blocking(move || {protect_backup_target(&coord,&validated)?;let passphrase=zeroize::Zeroizing::new(passphrase);coord.vault()?.export_backup(&validated, &passphrase)})
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn preview_restore_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    passphrase: String,
) -> Result<ImportPreview, String> {
    verify_frontend_window(&window)?;
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
    let coord = state.coordinator.clone();
    spawn_blocking(move || {let phrase=zeroize::Zeroizing::new(passphrase);coord.vault()?.preview_restore_backup(&validated,&phrase)})
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

fn protect_backup_target(coord:&Coordinator,path:&Path)->Result<(),String>{
    let key=path.to_string_lossy().to_lowercase();let data=coord.data.canonicalize().map_err(|_|"应用数据目录不可检查。")?.to_string_lossy().to_lowercase();
    if key==data||key.starts_with(&(data+"\\")){return Err("备份不能覆盖应用数据目录，请选择独立位置。".into());}
    let snap=coord.snapshot()?;
    if snap.jobs.iter().any(|j|j.source.volumes.iter().chain(std::iter::once(&j.source.path)).any(|p|p.to_lowercase()==key)){return Err("备份不能覆盖已登记的源压缩包或分卷。".into());}Ok(())
}
#[tauri::command]
async fn confirm_restore_backup(window:WebviewWindow,state:State<'_,AppState>,path:String,passphrase:String,token:String)->Result<ImportResult,String>{
    verify_frontend_window(&window)?;let access=state.access.clone();let coord=state.coordinator.clone();
    spawn_blocking(move||{let validated=canonicalize_existing(&path)?;if !access.lock().map_err(|_|"路径授权不可用。")?.is_backup_read_allowed(&validated){return Err("请使用备份文件选择器授权此路径。".into());}let phrase=zeroize::Zeroizing::new(passphrase);let result=coord.vault()?.confirm_restore_backup(&validated,&phrase,&token)?;coord.changed();Ok(result)}).await.map_err(|_|"备份恢复未完成。")?
}
#[tauri::command]
async fn save_settings(
    window: WebviewWindow,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    verify_frontend_window(&window)?;
    let access = state.access.clone();
    let validated = spawn_blocking(move || -> Result<Settings, String> {
        let guard = access
            .lock()
            .map_err(|_| "Access lock poisoned".to_string())?.clone();
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
    state.minimize_to_tray.store(saved.minimize_to_tray,Ordering::Release);
    let _ = window.set_always_on_top(saved.always_on_top);
    Ok(saved)
}

#[tauri::command]
async fn history(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<JobView>, String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.history())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn clear_history(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let coord = state.coordinator.clone();
    spawn_blocking(move || coord.clear_history())
        .await
        .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn recover_job(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
    action: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    state.coordinator.recover(&job_id, &action).await
}

#[tauri::command]
async fn open_path(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
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
        Command::new("explorer.exe")
            .arg(&target_dir)
            .spawn()
            .map_err(|_| "Native operation failed".to_string())?;
        Ok(())
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
    let access = state.access.clone();
    spawn_blocking(move || -> Result<Vec<String>, String> {
        let mut results = Vec::new();
        if folder {
            if let Some(folder_path) = FileDialog::new().pick_folder() {
                if let Ok(canon) = canonicalize_existing(&folder_path.to_string_lossy()) {
                    let mut guard = access
                        .lock()
                        .map_err(|_| "Access lock poisoned".to_string())?;
                    guard.grant_read(canon.clone(), true);
                    guard.grant_write_dir(canon.clone());
                    results.push(canon.to_string_lossy().into_owned());
                }
            }
        } else if let Some(files) = FileDialog::new().pick_files() {
            let mut approved = Vec::new();
            for file_path in files {
                if let Ok(canon) = canonicalize_existing(&file_path.to_string_lossy()) { approved.push(canon); }
            }
            let mut guard = access
                .lock()
                .map_err(|_| "Access lock poisoned".to_string())?;
            for canon in approved {
                guard.grant_read(canon.clone(), false);
                results.push(canon.to_string_lossy().into_owned());
            }
        }
        Ok(results)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
async fn pick_directory(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    verify_frontend_window(&window)?;
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

#[tauri::command]
async fn pick_save_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    verify_frontend_window(&window)?;
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

#[tauri::command]
async fn pick_open_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    verify_frontend_window(&window)?;
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

#[tauri::command]
async fn read_import_file(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    encoding: String,
) -> Result<String, String> {
    verify_frontend_window(&window)?;
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
        let meta = fs::metadata(&canon).map_err(|_| "Native operation failed".to_string())?;
        if meta.len() > MAX_IMPORT_BYTES {
            return Err("File size exceeds maximum permitted 8 MiB limit".to_string());
        }
        let mut bytes = Vec::new();
        File::open(&canon)
            .map_err(|_| "Native operation failed".to_string())?
            .take(MAX_IMPORT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Native operation failed".to_string())?;
        if bytes.len() as u64 > MAX_IMPORT_BYTES {
            return Err("Import file exceeded its permitted limit".to_string());
        }
        let norm_enc = encoding.trim().to_uppercase();
        if norm_enc == "GBK" || norm_enc == "CP936" {
            let decoded = encoding_rs::GBK
                .decode_without_bom_handling_and_without_replacement(&bytes)
                .ok_or_else(|| "Invalid GBK import encoding".to_string())?;
            Ok(decoded.into_owned())
        } else if norm_enc == "UTF-8" || norm_enc == "UTF8" {
            let content = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
                &bytes[3..]
            } else {
                &bytes[..]
            };
            std::str::from_utf8(content)
                .map(|s| s.to_string())
                .map_err(|_| {
                    "Failed to decode UTF-8 import file: invalid byte sequence".to_string()
                })
        } else {
            Err("Unsupported import encoding".to_string())
        }
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

fn show_main(app:&tauri::AppHandle){if let Some(w)=app.get_webview_window("main"){let _=w.show();let _=w.unminimize();let _=w.set_focus();}}
fn request_exit(app:tauri::AppHandle){
    let Some(state)=app.try_state::<AppState>() else {app.exit(0);return;};
    if state.exiting.swap(true,Ordering::AcqRel){return;}
    let coord=state.coordinator.clone();drop(state);
    tauri::async_runtime::spawn(async move {
        let c=coord.clone();let snap=spawn_blocking(move||c.snapshot()).await;
        let active=match snap {Ok(Ok(s))=>s.running,_=>true};
        if active {
            let choice=spawn_blocking(||MessageDialog::new().set_title("退出解压工坊").set_description("正在处理任务。选择“是”会取消任务，等待作业进程退出后关闭；选择“否”继续运行。").set_level(MessageLevel::Info).set_buttons(MessageButtons::YesNo).show()).await;
            if !matches!(choice,Ok(MessageDialogResult::Yes)){if let Some(s)=app.try_state::<AppState>(){s.exiting.store(false,Ordering::Release);}return;}
            let c=coord.clone();let _=spawn_blocking(move||c.cancel(None)).await;
            let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(45);
            loop {
                let c=coord.clone();let status=tokio::time::timeout(std::time::Duration::from_secs(2),spawn_blocking(move||c.snapshot())).await;
                if matches!(status,Ok(Ok(Ok(s))) if !s.running&& !s.jobs.iter().any(|j|j.status=="stopping")){break;}
                if tokio::time::Instant::now()>=deadline{
                    if let Some(s)=app.try_state::<AppState>(){s.exiting.store(false,Ordering::Release);}
                    let _=spawn_blocking(||MessageDialog::new().set_title("退出等待未完成").set_description("尚未确认全部作业退出。工作目录已保留，窗口继续打开，可检查任务状态。").set_level(MessageLevel::Warning).show()).await;show_main(&app);return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
        }
        let sequence=app.try_state::<AppState>().and_then(|s|s.clipboard_sequence.lock().ok().and_then(|v|*v));
        if let Some(sequence)=sequence{let _=spawn_blocking(move||clipboard::clear_if_unchanged(sequence)).await;}
        app.exit(0);
    });
}
fn install_tray(app:&tauri::App)->Result<(),Box<dyn std::error::Error>>{
    use tauri::{menu::{Menu,MenuItem},tray::{TrayIconBuilder,TrayIconEvent,MouseButton,MouseButtonState}};
    let show=MenuItem::with_id(app,"show","打开解压工坊",true,None::<&str>)?;
    let exit=MenuItem::with_id(app,"exit","退出",true,None::<&str>)?;
    let menu=Menu::with_items(app,&[&show,&exit])?;
    let mut tray=TrayIconBuilder::with_id("openunpack-tray").tooltip("解压工坊 OpenUnpack").menu(&menu).show_menu_on_left_click(false).on_menu_event(|app,event|match event.id.as_ref(){"show"=>show_main(app),"exit"=>request_exit(app.clone()),_=>()}).on_tray_icon_event(|tray,event|{if matches!(event,TrayIconEvent::DoubleClick{button:MouseButton::Left,..}|TrayIconEvent::Click{button:MouseButton::Left,button_state:MouseButtonState::Up,..}){show_main(tray.app_handle());}});
    if let Some(icon)=app.default_window_icon(){tray=tray.icon(icon.clone());}
    tray.build(app)?;Ok(())
}
pub fn run() {
    Builder::default()
        .setup(|app| {
            let data_dir = match resolve_data_directory() {
                Ok(dir) => dir,
                Err(err) => {
                    MessageDialog::new()
                        .set_title("OpenUnpack Initialization Error")
                        .set_description(&format!("Failed to locate data directory: {err}"))
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
                        .set_description(&format!("Another instance running or locked: {err}"))
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
                        .set_description(&format!("Worker binaries missing: {err}"))
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
                        .set_description(&format!("Failed to open OpenUnpack Coordinator: {err}"))
                        .set_level(MessageLevel::Error)
                        .show();
                    return Err(err.into());
                }
            };

            let mut native_access = NativeAccess::default();
            if let Ok(snap) = coordinator.snapshot() {
                if !snap.settings.output_root.trim().is_empty() {
                    if let Ok(p) = canonicalize_existing(&snap.settings.output_root) {
                        native_access.grant_write_dir(p);
                    }
                }
                if !snap.settings.cache_root.trim().is_empty() {
                    if let Ok(p) = canonicalize_existing(&snap.settings.cache_root) {
                        native_access.grant_write_dir(p);
                    }
                }
            }

            let access_state = Arc::new(Mutex::new(native_access));
            app.manage(AppState {
                coordinator: coordinator.clone(),
                access: access_state,
                _lock_file: lock_file,
                scans:Mutex::new(HashMap::new()),clipboard_sequence:Mutex::new(None),exiting:AtomicBool::new(false),
                minimize_to_tray:AtomicBool::new(coordinator.snapshot().map(|s|s.settings.minimize_to_tray).unwrap_or(false)),
            });
            install_tray(app)?;
            if let (Some(w),Ok(s))=(app.get_webview_window("main"),coordinator.snapshot()){w.set_always_on_top(s.settings.always_on_top)?;}

            let app_handle = app.handle().clone();
            let mut sub = coordinator.subscribe();
            tauri::async_runtime::spawn(async move {
                let mut known_finished=HashSet::new();
                if let Some(s)=app_handle.try_state::<AppState>(){if let Ok(snapshot)=s.coordinator.snapshot(){for j in snapshot.jobs{if j.finished_at.is_some(){known_finished.insert(j.id);}}}}
                loop {
                    match sub.recv().await {
                        Ok(()) | Err(RecvError::Lagged(_)) => {
                            let _ = app_handle.emit_to("main", "state-changed", ());
                            if let Some(s)=app_handle.try_state::<AppState>(){
                                let c=s.coordinator.clone();drop(s);
                                if let Ok(Ok(snapshot))=spawn_blocking(move||c.snapshot()).await {
                                    let mut beep=false;for j in &snapshot.jobs {if matches!(j.status.as_str(),"completed"|"partial"|"cleanupPending")&&known_finished.insert(j.id.clone()){beep=true;}}
                                    if beep&&snapshot.settings.completion_sound{let _=spawn_blocking(||unsafe{windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(0x40)}).await;}
                                }
                            }
                        }
                        Err(RecvError::Closed) => break,
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested{api,..}=event {
                    api.prevent_close();
                    let app=window.app_handle().clone();
                    let to_tray=window.try_state::<AppState>().is_some_and(|s|s.minimize_to_tray.load(Ordering::Acquire));
                    if to_tray {let _=window.hide();} else {request_exit(app);}
                }
                if matches!(event,WindowEvent::Resized(_))
                    && window.try_state::<AppState>().is_some_and(|s|s.minimize_to_tray.load(Ordering::Acquire))
                    && window.is_minimized().unwrap_or(false) {
                    let _=window.hide();
                }
                if let WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                    if let Some(state) = window.try_state::<AppState>() {
                        if let Ok(mut guard) = state.access.lock() {
                            for path in paths {
                                if let Some(raw) = path.to_str() {
                                    guard.native_drop_paths.insert(raw.to_string());
                                }
                            }
                        }
                    }
                }
            }
        })
        .invoke_handler(generate_handler![
            snapshot,
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
            preview_restore_backup,confirm_restore_backup,
            copy_password,bulk_password_action,cancel_scan,retry_node,preview_exclusions,
            save_settings,
            history,
            clear_history,
            recover_job,
            open_path,
            pick_files,
            pick_directory,
            pick_save_backup,
            pick_open_backup,
            read_import_file,
        ])
        .run(generate_context!())
        .expect("error while running OpenUnpack desktop application");
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
