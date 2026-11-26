//! 系统文件选择器、文本编码解码与工具箱批量操作命令。

use crate::*;

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

pub(crate) fn authorize_toolbox_options(
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

pub(crate) async fn core_fix_archive_extensions(
    state: &AppState,
    paths: Vec<String>,
) -> Result<Vec<ArchiveExtensionFixItem>, String> {
    let access = state.access.clone();
    spawn_blocking(move || {
        let mut checked_paths = Vec::with_capacity(paths.len());
        for p in paths {
            let canon = canonicalize_existing(&p)?;
            checked_paths.push(canon.to_string_lossy().into_owned());
        }
        {
            let mut guard = access
                .lock()
                .map_err(|_| "Access lock poisoned".to_string())?;
            for p in &checked_paths {
                let path = Path::new(p);
                if path.is_file() {
                    if let Some(parent) = path.parent() {
                        guard.grant_write_dir(parent.to_path_buf());
                        guard.grant_read(parent.to_path_buf(), true);
                    }
                    guard.grant_read(path.to_path_buf(), false);
                } else if path.is_dir() {
                    guard.grant_write_dir(path.to_path_buf());
                    guard.grant_read(path.to_path_buf(), true);
                }
            }
        }
        openunpack_core::toolbox::fix_archive_extensions(&checked_paths)
    })
    .await
    .map_err(|_| "Background operation failed".to_string())?
}

#[tauri::command]
pub(crate) async fn pick_files(
    window: WebviewWindow,
    state: State<'_, AppState>,
    folder: bool,
) -> Result<Vec<String>, String> {
    verify_frontend_window(&window)?;
    core_pick_files(&state, folder).await
}

#[tauri::command]
pub(crate) async fn pick_directory(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    verify_frontend_window(&window)?;
    core_pick_directory(&state).await
}

#[tauri::command]
pub(crate) async fn pick_save_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    verify_frontend_window(&window)?;
    core_pick_save_backup(&state).await
}

#[tauri::command]
pub(crate) async fn pick_open_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    verify_frontend_window(&window)?;
    core_pick_open_backup(&state).await
}

#[tauri::command]
pub(crate) async fn read_import_file(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    encoding: String,
) -> Result<String, String> {
    verify_frontend_window(&window)?;
    core_read_import_file(&state, path, encoding).await
}

#[tauri::command]
pub(crate) async fn preview_toolbox_operation(
    window: Window,
    state: State<'_, AppState>,
    options: ToolboxOptions,
) -> Result<ToolboxPreviewResult, String> {
    verify_frontend_window(&window)?;
    core_preview_toolbox_operation(&state, options).await
}

#[tauri::command]
pub(crate) async fn execute_toolbox_operation(
    window: Window,
    state: State<'_, AppState>,
    request: ToolboxExecuteRequest,
) -> Result<ToolboxExecuteResult, String> {
    verify_frontend_window(&window)?;
    core_execute_toolbox_operation(&state, request).await
}

#[tauri::command]
pub(crate) async fn fix_archive_extensions(
    window: Window,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<ArchiveExtensionFixItem>, String> {
    verify_frontend_window(&window)?;
    core_fix_archive_extensions(&state, paths).await
}

pub(crate) fn decode_import_bytes(bytes: &[u8], encoding: &str) -> Result<String, String> {
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
