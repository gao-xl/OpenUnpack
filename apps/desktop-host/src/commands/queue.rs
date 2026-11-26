//! 队列快照、源扫描、排除规则预览及任务入队/启停控制命令。

use crate::*;

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
pub(crate) async fn snapshot(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    verify_frontend_window(&window)?;
    core_snapshot(&state).await
}

#[tauri::command]
pub(crate) async fn queue_snapshot(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    verify_frontend_window(&window)?;
    core_queue_snapshot(&state).await
}

#[tauri::command]
pub(crate) async fn snapshot_update(
    window: WebviewWindow,
    state: State<'_, AppState>,
    since_revision: Option<String>,
) -> Result<SnapshotUpdate, String> {
    verify_frontend_window(&window)?;
    core_snapshot_update(&state, since_revision).await
}

#[tauri::command]
pub(crate) async fn history_page(
    window: WebviewWindow,
    state: State<'_, AppState>,
    query: HistoryPageQuery,
) -> Result<JobHistoryPage, String> {
    verify_frontend_window(&window)?;
    core_history_page(&state, query).await
}

#[tauri::command]
pub(crate) async fn job_details(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<Option<JobView>, String> {
    verify_frontend_window(&window)?;
    core_job_details(&state, &job_id).await
}

pub(crate) async fn scan_authorized(
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
pub(crate) async fn scan_sources(
    window: WebviewWindow,
    state: State<'_, AppState>,
    paths: Vec<String>,
    request_id: Option<String>,
) -> Result<ScanResult, String> {
    verify_frontend_window(&window)?;
    core_scan_sources(&state, paths, request_id).await
}

#[tauri::command]
pub(crate) async fn cancel_scan(
    window: WebviewWindow,
    state: State<'_, AppState>,
    request_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_cancel_scan(&state, &request_id).await
}

#[tauri::command]
pub(crate) async fn preview_exclusions(
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
pub(crate) async fn cancel_preview_exclusions(
    window: WebviewWindow,
    state: State<'_, AppState>,
    request_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_cancel_preview_exclusions(&state, &request_id).await
}

pub(crate) fn validate_exclusion_preview(
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

pub(crate) fn preview_exclusion_matches(
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
        let text = vault.reveal_password(&id_str).map_err(|e| e.to_string())?;
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
pub(crate) async fn retry_node(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
    node_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_retry_node(&state, job_id, node_id).await
}

#[tauri::command]
pub(crate) async fn bulk_password_action(
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
pub(crate) async fn copy_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let owner = window.hwnd().map_err(|_| "窗口句柄不可用。")?.0 as usize;
    core_copy_password(&state, &id, owner).await
}

#[tauri::command]
pub(crate) async fn add_jobs(
    window: WebviewWindow,
    state: State<'_, AppState>,
    sources: Vec<SourceGroup>,
    options: Settings,
) -> Result<Vec<JobView>, String> {
    verify_frontend_window(&window)?;
    core_add_jobs(&state, sources, options).await
}

#[tauri::command]
pub(crate) async fn start_jobs(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
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

#[tauri::command]
pub(crate) async fn pause_jobs(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_pause_jobs(&state).await
}

#[tauri::command]
pub(crate) async fn cancel_jobs(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: Option<String>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_cancel_jobs(&state, job_id).await
}

#[tauri::command]
pub(crate) async fn retry_job(
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
pub(crate) async fn remove_job(
    window: WebviewWindow,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_remove_job(&state, job_id).await
}

#[tauri::command]
pub(crate) async fn clear_completed(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_clear_completed(&state).await
}
