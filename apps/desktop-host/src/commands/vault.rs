//! 密码库增删查改、导入及加密备份恢复命令。

use crate::*;

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
    spawn_blocking(move || {
        let secret = vault.reveal_password(&id_str).map_err(|e| e.to_string())?;
        Ok((*secret).clone())
    })
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
pub(crate) async fn passwords(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<PasswordView>, String> {
    verify_frontend_window(&window)?;
    core_passwords(&state).await
}

#[tauri::command]
pub(crate) async fn save_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    input: PasswordInput,
) -> Result<PasswordView, String> {
    verify_frontend_window(&window)?;
    core_save_password(&state, input).await
}

#[tauri::command]
pub(crate) async fn delete_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_delete_password(&state, &id).await
}

#[tauri::command]
pub(crate) async fn reveal_password(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    verify_frontend_window(&window)?;
    core_reveal_password(&state, &id).await
}

#[tauri::command]
pub(crate) async fn preview_import(
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
pub(crate) async fn confirm_import(
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

pub(crate) fn protect_backup_target(coord: &Coordinator, path: &Path) -> Result<(), String> {
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

#[tauri::command]
pub(crate) async fn export_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    passphrase: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    core_export_backup(&state, path, passphrase).await
}

#[tauri::command]
pub(crate) async fn preview_restore_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    passphrase: String,
) -> Result<ImportPreview, String> {
    verify_frontend_window(&window)?;
    core_preview_restore_backup(&state, path, passphrase).await
}

#[tauri::command]
pub(crate) async fn confirm_restore_backup(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
    passphrase: String,
    token: String,
) -> Result<ImportResult, String> {
    verify_frontend_window(&window)?;
    core_confirm_restore_backup(&state, path, passphrase, token).await
}
