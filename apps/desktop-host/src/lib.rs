//! OpenUnpack Desktop Host - Tauri 桌面宿主与本地运行入口。
//!
//! 负责桌面窗口与托盘生命周期、单实例命名管道唤醒、原生路径访问授权、
//! Tauri IPC 命令暴露以及 `--headless` 守护进程引导。

mod clipboard;
pub(crate) mod commands;
mod headless;
mod local_api;
mod logging;
mod security;
mod single_instance;
mod system_actions;
mod tray;

use headless::open_coordinator_on_runtime;
pub use headless::run_headless;
#[cfg(test)]
use headless::{collect_flag_values, mask_token};
use openunpack_core::coordinator::Coordinator;
use openunpack_infrastructure::paths::path_starts_with_case_insensitive;
pub use openunpack_infrastructure::paths::{
    is_reserved_windows_device_name, strip_verbatim_prefix,
};
use openunpack_protocol::{
    AppSnapshot, ArchiveExtensionFixItem, ArchiveInspectionView, HistoryPageQuery, ImportPreview,
    ImportResult, JobHistoryPage, JobView, PasswordInput, PasswordView, ScanResult,
    SelectiveExtractRequest, SelectiveExtractResult, Settings, SnapshotUpdate, SourceGroup,
    SuccessEvent, ToolboxExecuteRequest, ToolboxExecuteResult, ToolboxOptions,
    ToolboxPreviewResult,
};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
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

use commands::{archive::*, queue::*, toolbox::*, vault::*};
use security::*;
use tray::*;

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

    fn reveal_entry(
        &self,
        id: &str,
    ) -> Result<zeroize::Zeroizing<String>, openunpack_application::ApplicationError> {
        self.0
            .vault()
            .map_err(openunpack_application::ApplicationError::VaultError)?
            .reveal(id)
            .map_err(openunpack_application::ApplicationError::VaultError)
    }
}

pub fn run() {
    if !single_instance::is_webview2_installed() {
        MessageDialog::new()
            .set_title("解压工坊 OpenUnpack - 缺少运行环境")
            .set_description(
                "未检测到 Microsoft Edge WebView2 运行时。\n\n请重新运行安装包修复安装，或安装 Microsoft Edge WebView2 Runtime 后再启动本程序。",
            )
            .set_level(MessageLevel::Error)
            .show();
        return;
    }
    let initial_open_paths = single_instance::collect_cli_open_paths();
    let data_dir = match resolve_data_directory() {
        Ok(dir) => dir,
        Err(err) => {
            MessageDialog::new()
                .set_title("OpenUnpack Initialization Error")
                .set_description(format!("Failed to locate data directory: {err}"))
                .set_level(MessageLevel::Error)
                .show();
            return;
        }
    };
    // Install logging before the instance lock: a second launch that loses the
    // race is exactly the case where a silent exit used to be unexplainable.
    if let Some(log_path) = logging::init(&data_dir, false) {
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            data_dir = %data_dir.display(),
            log = %log_path.display(),
            "openunpack starting (desktop)"
        );
    }
    let lock_file = match acquire_instance_lock(&data_dir) {
        Ok(file) => file,
        Err(_err) => {
            tracing::warn!("another instance already owns the data directory");
            if single_instance::try_notify_running_instance(&data_dir, &initial_open_paths) {
                return;
            }
            #[cfg(windows)]
            {
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    FindWindowW, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow,
                };
                let title: Vec<u16> = "解压工坊 OpenUnpack\0".encode_utf16().collect();
                let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
                if !hwnd.is_null() {
                    unsafe {
                        ShowWindow(hwnd, SW_SHOW);
                        ShowWindow(hwnd, SW_RESTORE);
                        SetForegroundWindow(hwnd);
                    }
                    return;
                }
            }
            MessageDialog::new()
                .set_title("解压工坊 OpenUnpack")
                .set_description("解压工坊已在运行中，请检查系统任务栏或托盘图标。")
                .set_level(MessageLevel::Info)
                .show();
            return;
        }
    };

    setup_process_environment(&data_dir);

    let run_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Builder::default()
            .setup(move |app| {
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
                let coordinator = match open_coordinator_on_runtime(
                    data_dir.clone(),
                    worker_exe,
                    worker_dll,
                    tauri::async_runtime::handle().inner(),
                ) {
                    Ok(coord) => coord,
                    Err(err) => {
                        MessageDialog::new()
                            .set_title("OpenUnpack Vault/Database Error")
                            .set_description(format!(
                                "Failed to open OpenUnpack Coordinator: {err}"
                            ))
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
                let authorized_cli_paths = authorize_open_paths(&access_state, &initial_open_paths);
                app.manage(PendingOpenPaths(Mutex::new(authorized_cli_paths)));

                let vault_store = Arc::new(CoordinatorVaultStore(coordinator.clone()));
                let vault_use_cases =
                    Arc::new(openunpack_application::VaultUseCases::new(vault_store));
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

                match single_instance::prepare_wake_token(&data_dir) {
                    Ok(wake_token) => {
                        let wake_app = app.handle().clone();
                        single_instance::spawn_wake_server(
                            &data_dir,
                            wake_token,
                            Arc::new(move |raw_paths| {
                                show_main(&wake_app);
                                if raw_paths.is_empty() {
                                    return;
                                }
                                if let Some(s) = wake_app.try_state::<AppState>() {
                                    let authorized = authorize_open_paths(&s.access, &raw_paths);
                                    if authorized.is_empty() {
                                        return;
                                    }
                                    if let Some(pending) = wake_app.try_state::<PendingOpenPaths>()
                                        && let Ok(mut guard) = pending.0.lock()
                                    {
                                        guard.extend(authorized.clone());
                                    }
                                    let _ =
                                        wake_app.emit_to("main", "external-open-paths", authorized);
                                }
                            }),
                        );
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "wake_token_setup_failed");
                    }
                }
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
                let mut progress_sub = coordinator.subscribe_progress();
                let mut need_input_sub = coordinator.subscribe_need_input();
                let app_progress_handle = app.handle().clone();
                let app_need_input_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match progress_sub.recv().await {
                            Ok(update) => {
                                let _ = app_progress_handle.emit_to("main", "job-progress", update);
                            }
                            Err(RecvError::Lagged(_)) => continue,
                            Err(RecvError::Closed) => break,
                        }
                    }
                });
                tauri::async_runtime::spawn(async move {
                    loop {
                        match need_input_sub.recv().await {
                            Ok(event) => {
                                let _ =
                                    app_need_input_handle.emit_to("main", "need-password", event);
                            }
                            Err(RecvError::Lagged(_)) => continue,
                            Err(RecvError::Closed) => break,
                        }
                    }
                });
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
                                    windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(
                                        0x40,
                                    )
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
                tray::set_ui_language,
                local_api::api_status,
                local_api::api_configure,
                local_api::api_create_client,
                local_api::api_revoke_client,
                local_api::api_open_docs,
                commands::queue::snapshot,
                commands::queue::queue_snapshot,
                commands::queue::snapshot_update,
                commands::queue::history_page,
                commands::queue::job_details,
                commands::queue::scan_sources,
                commands::queue::add_jobs,
                commands::queue::start_jobs,
                commands::queue::pause_jobs,
                commands::queue::cancel_jobs,
                commands::queue::retry_job,
                commands::queue::remove_job,
                commands::queue::clear_completed,
                commands::vault::passwords,
                commands::vault::save_password,
                commands::vault::delete_password,
                commands::vault::reveal_password,
                commands::vault::preview_import,
                commands::vault::confirm_import,
                commands::vault::export_backup,
                commands::vault::preview_restore_backup,
                commands::vault::confirm_restore_backup,
                commands::queue::copy_password,
                commands::queue::bulk_password_action,
                commands::queue::cancel_scan,
                commands::queue::retry_node,
                commands::queue::preview_exclusions,
                commands::queue::cancel_preview_exclusions,
                commands::archive::save_settings,
                commands::archive::history,
                commands::archive::clear_history,
                commands::archive::recover_job,
                commands::archive::inspect_archive,
                commands::archive::cancel_inspect_archive,
                commands::archive::extract_selective,
                commands::archive::open_path,
                commands::toolbox::pick_files,
                commands::toolbox::pick_directory,
                commands::toolbox::pick_save_backup,
                commands::toolbox::pick_open_backup,
                commands::toolbox::read_import_file,
                commands::toolbox::preview_toolbox_operation,
                commands::toolbox::execute_toolbox_operation,
                commands::toolbox::fix_archive_extensions,
                system_actions::execute_post_action,
                system_actions::get_shell_context_menu_status,
                system_actions::set_shell_context_menu_status,
                security::take_pending_open_paths,
                commands::archive::submit_native_input,
            ])
            .run(generate_context!())
            .expect("error while running OpenUnpack desktop application");
    }));

    if let Err(payload) = run_res {
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        let user_msg = if msg.contains("0x8000FFFF") || msg.contains("灾难性故障") {
            "检测到当前 Windows 系统处于锁屏状态或无活动图形桌面。\n\n请解锁进入 Windows 桌面后再启动解压工坊。".to_string()
        } else if msg.contains("0x800700AA") || msg.contains("请求的资源在使用中") {
            "检测到 WebView2 渲染组件被残留进程占用。\n\n已为您自动重置环境，请再次双击启动。"
                .to_string()
        } else {
            format!("解压工坊启动异常: {msg}")
        };

        MessageDialog::new()
            .set_title("解压工坊 OpenUnpack - 启动提示")
            .set_description(&user_msg)
            .set_level(MessageLevel::Warning)
            .show();
    }
}

#[cfg(test)]
mod tests;
