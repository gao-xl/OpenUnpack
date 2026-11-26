//! 应用设置、历史记录、压缩包内容检视与按需解压命令。

use crate::*;

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
    core_open_path_scoped(state, path, false).await
}

pub(crate) async fn core_open_path_scoped(
    state: &AppState,
    path: String,
    allow_native_access: bool,
) -> Result<(), String> {
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
    let access = state.access.clone();
    spawn_blocking(move || -> Result<(), String> {
        let canon = canonicalize_existing(&path)?;
        let allowed_by_coordinator = coord.authorize_open(Path::new(&path)).is_ok();
        let allowed_by_native = allow_native_access
            && !allowed_by_coordinator
            && access
                .lock()
                .is_ok_and(|guard| guard.is_readable(&canon) || guard.is_writable_dir(&canon));
        if !allowed_by_coordinator && !allowed_by_native {
            coord.authorize_open(Path::new(&path))?;
        }
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
pub(crate) async fn save_settings(
    window: WebviewWindow,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    verify_frontend_window(&window)?;
    core_save_settings(&state, settings, Some(&window)).await
}

#[tauri::command]
pub(crate) async fn history(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<JobView>, String> {
    verify_frontend_window(&window)?;
    core_history(&state).await
}

#[tauri::command]
pub(crate) async fn clear_history(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_clear_history(&state).await
}

#[tauri::command]
pub(crate) async fn recover_job(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
    action: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_recover_job(&state, &job_id, &action).await
}

#[tauri::command]
pub(crate) async fn inspect_archive(
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
pub(crate) async fn cancel_inspect_archive(
    window: Window,
    state: State<'_, AppState>,
    inspect_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_cancel_inspect_archive(&state, &inspect_id).await
}

#[tauri::command]
pub(crate) async fn extract_selective(
    window: Window,
    state: State<'_, AppState>,
    request: SelectiveExtractRequest,
) -> Result<SelectiveExtractResult, String> {
    verify_frontend_window(&window)?;
    core_extract_selective(&state, request).await
}

#[tauri::command]
pub(crate) async fn open_path(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_open_path_scoped(&state, path, true).await
}

#[tauri::command]
pub(crate) async fn submit_native_input(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
    input_type: String,
    value: String,
) -> Result<bool, String> {
    verify_frontend_window(&window)?;
    if input_type != "password" {
        return Err("Unsupported input type".to_string());
    }
    if value.is_empty() || value.len() > 4096 {
        return Err("Password input out of bounds".to_string());
    }
    state.coordinator.submit_input(&job_id, value)
}
