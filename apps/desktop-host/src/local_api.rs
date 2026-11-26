use super::*;
use openunpack_infrastructure::paths::{
    path_eq_case_insensitive, path_starts_with_case_insensitive,
};
use openunpack_local_api::store::ScopeInfo;
use openunpack_local_api::{
    ApiBackend, ApiClient, ApiClientInput, ApiCommand, ApiError, ApiSettings, ApiStatus, ApiStore,
    BackendFuture, CreatedClient, LocalApiServer, Scope,
};
use serde_json::Value;

pub struct ApiManager {
    store: Result<Arc<ApiStore>, String>,
    server: tokio::sync::Mutex<Option<LocalApiServer>>,
    error: Mutex<Option<String>>,
}
impl ApiManager {
    pub fn open(data: &Path) -> Arc<Self> {
        let store = ApiStore::open(&data.join("local-api.json")).map(Arc::new);
        Arc::new(Self {
            store,
            server: tokio::sync::Mutex::new(None),
            error: Mutex::new(None),
        })
    }
    pub fn store(&self) -> Result<Arc<ApiStore>, String> {
        self.store.clone()
    }
    pub async fn status(&self) -> Result<ApiStatus, String> {
        let (settings, clients, error) = match &self.store {
            Ok(store) => (
                store.settings()?,
                store.clients()?,
                self.error.lock().map_err(|_| "API 状态不可用。")?.clone(),
            ),
            Err(error) => (ApiSettings::default(), vec![], Some(error.clone())),
        };
        let server = self.server.lock().await;
        let running = server.as_ref().is_some_and(LocalApiServer::is_running);
        let base = running.then(|| {
            format!(
                "http://127.0.0.1:{}",
                server.as_ref().expect("running server").port
            )
        });
        Ok(ApiStatus {
            settings,
            running,
            base_url: base.as_ref().map(|v| format!("{v}/api/v1")),
            docs_url: base.map(|v| format!("{v}/docs")),
            error,
            clients,
            permissions: ScopeInfo::all(),
        })
    }
    pub async fn configure(
        &self,
        app: tauri::AppHandle,
        settings: ApiSettings,
    ) -> Result<ApiStatus, String> {
        self.configure_internal(Some(app), None, settings).await
    }
    pub async fn configure_standalone(
        &self,
        state: Arc<AppState>,
        settings: ApiSettings,
    ) -> Result<ApiStatus, String> {
        self.configure_internal(None, Some(state), settings).await
    }
    async fn configure_internal(
        &self,
        app: Option<tauri::AppHandle>,
        standalone_state: Option<Arc<AppState>>,
        settings: ApiSettings,
    ) -> Result<ApiStatus, String> {
        let store = self.store()?;
        store.save_settings(settings.clone())?;
        let mut server = self.server.lock().await;
        if let Some(previous) = server.take() {
            previous.shutdown().await;
        }
        *self.error.lock().map_err(|_| "API 状态不可用。")? = None;
        if settings.enabled {
            let backend = Arc::new(DesktopBackend {
                app,
                standalone_state,
                store: store.clone(),
            });
            match LocalApiServer::start(store, backend).await {
                Ok(started) => *server = Some(started),
                Err(error) => {
                    *self.error.lock().map_err(|_| "API 状态不可用。")? = Some(error.clone());
                    return Err(error);
                }
            }
        }
        drop(server);
        self.status().await
    }
    pub async fn restore(&self, app: tauri::AppHandle) {
        if let Ok(store) = self.store()
            && let Ok(settings) = store.settings()
            && settings.enabled
        {
            let _ = self.configure(app, settings).await;
        }
    }
    pub async fn restore_standalone(&self, state: Arc<AppState>) {
        if let Ok(store) = self.store() {
            let mut settings = store.settings().unwrap_or_default();
            if !settings.enabled {
                settings.enabled = true;
                let _ = store.save_settings(settings.clone());
            }
            let _ = self.configure_standalone(state, settings).await;
        }
    }
}

#[tauri::command]
pub async fn api_status(
    window: WebviewWindow,
    manager: State<'_, Arc<ApiManager>>,
) -> Result<ApiStatus, String> {
    verify_frontend_window(&window)?;
    manager.status().await
}
#[tauri::command]
pub async fn api_configure(
    window: WebviewWindow,
    app: tauri::AppHandle,
    manager: State<'_, Arc<ApiManager>>,
    settings: ApiSettings,
) -> Result<ApiStatus, String> {
    verify_frontend_window(&window)?;
    manager.configure(app, settings).await
}
#[tauri::command]
pub async fn api_create_client(
    window: WebviewWindow,
    state: State<'_, AppState>,
    manager: State<'_, Arc<ApiManager>>,
    mut input: ApiClientInput,
) -> Result<CreatedClient, String> {
    verify_frontend_window(&window)?;
    if !openunpack_local_api::is_trusted_all_paths() {
        let access = state.access.lock().map_err(|_| "目录授权不可用。")?;
        let data = state
            .coordinator
            .data
            .canonicalize()
            .map_err(|_| "应用数据目录不可用。")?;
        for (paths, write) in [
            (&mut input.read_roots, false),
            (&mut input.write_roots, true),
        ] {
            for path in paths {
                let canonical = canonicalize_existing(path)?;
                if !canonical.is_dir()
                    || canonical.starts_with(&data)
                    || (write && !access.is_writable_dir(&canonical))
                    || (!write
                        && !access.is_readable(&canonical)
                        && !access.is_writable_dir(&canonical))
                {
                    return Err("请通过本机目录选择器批准 API 使用的目录。".into());
                }
                *path = canonical.to_string_lossy().into_owned();
            }
        }
    }
    manager.store()?.create_client(input)
}
#[tauri::command]
pub async fn api_revoke_client(
    window: WebviewWindow,
    manager: State<'_, Arc<ApiManager>>,
    id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    manager.store()?.revoke_client(&id)
}
#[tauri::command]
pub async fn api_open_docs(
    window: WebviewWindow,
    manager: State<'_, Arc<ApiManager>>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let url = manager
        .status()
        .await?
        .docs_url
        .ok_or("请先启用本机 API。")?;
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .ok_or("系统目录不可用。")?;
    Command::new(root.join("explorer.exe"))
        .current_dir(&root)
        .arg(url)
        .spawn()
        .map_err(|_| "无法打开本机 API 文档。")?;
    Ok(())
}

struct DesktopBackend {
    app: Option<tauri::AppHandle>,
    standalone_state: Option<Arc<AppState>>,
    store: Arc<ApiStore>,
}
fn value<T: serde::Serialize>(result: Result<T, String>) -> Result<Value, ApiError> {
    serde_json::to_value(result.map_err(|error| ApiError::operation(&error))?)
        .map_err(|_| ApiError::internal())
}
fn is_client_path_authorized(path_str: &str, client: &ApiClient) -> bool {
    if !client.scopes.contains(&Scope::FilesRead)
        || path_str.is_empty()
        || path_str == "[REDACTED]"
        || !Path::new(path_str).is_absolute()
    {
        return false;
    }
    let Ok(target) = canonicalize_existing(path_str).or_else(|_| canonicalize_for_save(path_str))
    else {
        return false;
    };
    if openunpack_local_api::is_trusted_all_paths() {
        return true;
    }
    client
        .read_roots
        .iter()
        .chain(&client.write_roots)
        .any(|root| {
            canonicalize_existing(root)
                .or_else(|_| canonicalize_for_save(root))
                .is_ok_and(|root| {
                    path_eq_case_insensitive(&target, &root)
                        || root.is_dir() && path_starts_with_case_insensitive(&target, &root)
                })
        })
}

fn redact_job_metadata(output: &mut Value, client: &ApiClient) {
    match output {
        Value::Object(fields) => {
            if !client.scopes.contains(&Scope::SettingsRead) && fields.contains_key("options") {
                fields.insert("options".into(), serde_json::json!(Settings::default()));
            }
            if !client.scopes.contains(&Scope::VaultRead) {
                if fields.contains_key("passwordLabel") {
                    fields.insert("passwordLabel".into(), Value::Null);
                }
                if fields.contains_key("passwordGroup") {
                    fields.insert("passwordGroup".into(), Value::String(String::new()));
                }
            }
            // Local hook commands and the application database location are
            // never part of a client's file grants.
            if fields.contains_key("onComplete") {
                fields.insert("onComplete".into(), Value::Null);
            }
            if fields.contains_key("dataDirectory") {
                fields.insert("dataDirectory".into(), Value::String("[REDACTED]".into()));
            }
            if let Some(path) = fields
                .get("source")
                .and_then(|source| source.get("path"))
                .and_then(Value::as_str)
                && !is_client_path_authorized(path, client)
                && let Some(nodes) = fields.get_mut("nodes").and_then(Value::as_array_mut)
            {
                nodes.clear();
            }
            // Every location has its own boundary. An approved source says
            // nothing about its sibling volumes, output or cache directories.
            for key in [
                "path",
                "sourcePath",
                "archivePath",
                "currentFile",
                "outputPath",
                "output_path",
                "workspace",
                "outputRoot",
                "cacheRoot",
                "dataDirectory",
            ] {
                if let Some(path) = fields.get(key).and_then(Value::as_str)
                    && !path.is_empty()
                    && !is_client_path_authorized(path, client)
                {
                    fields.insert(
                        key.into(),
                        if matches!(
                            key,
                            "outputPath" | "output_path" | "workspace" | "currentFile"
                        ) {
                            Value::Null
                        } else {
                            Value::String("[REDACTED]".into())
                        },
                    );
                }
            }
            for key in ["volumes", "watchedFolders"] {
                if let Some(paths) = fields.get_mut(key).and_then(Value::as_array_mut) {
                    for path in paths {
                        if path
                            .as_str()
                            .is_some_and(|path| !is_client_path_authorized(path, client))
                        {
                            *path = Value::String("[REDACTED]".into());
                        }
                    }
                }
            }
            for value in fields.values_mut() {
                redact_job_metadata(value, client);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_job_metadata(value, client);
            }
        }
        _ => {}
    }
}
fn snapshot_value(
    result: Result<AppSnapshot, String>,
    client: &ApiClient,
) -> Result<Value, ApiError> {
    let mut output = value(result)?;
    let fields = output.as_object_mut().ok_or_else(ApiError::internal)?;
    if !client.scopes.contains(&Scope::VaultRead) {
        fields.insert("passwords".into(), serde_json::json!([]));
        fields.insert("vaultError".into(), Value::Null);
    }
    if !client.scopes.contains(&Scope::SettingsRead) {
        fields.insert("settings".into(), serde_json::json!(Settings::default()));
    }
    redact_job_metadata(&mut output, client);
    Ok(output)
}
fn project_progress_paths(
    mut update: openunpack_protocol::JobProgressUpdate,
    client: &ApiClient,
) -> openunpack_protocol::JobProgressUpdate {
    if update
        .current_file
        .as_ref()
        .is_some_and(|path| !is_client_path_authorized(path, client))
    {
        update.current_file = None;
    }
    update
}
fn project_need_input_paths(
    mut event: openunpack_protocol::NeedPasswordEvent,
    client: &ApiClient,
) -> openunpack_protocol::NeedPasswordEvent {
    if !is_client_path_authorized(&event.archive_path, client) {
        event.archive_path.clear();
    }
    event
}
fn job_value<T: serde::Serialize>(
    result: Result<T, String>,
    client: &ApiClient,
) -> Result<Value, ApiError> {
    let mut output = value(result)?;
    redact_job_metadata(&mut output, client);
    Ok(output)
}
fn owned_request(client: &ApiClient, id: String) -> Result<String, ApiError> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || matches!(v, b'-' | b'_'))
    {
        return Err(ApiError::new(
            400,
            "invalid_request",
            "请求编号必须是 1 至 64 位字母、数字、横线或下划线。",
        ));
    }
    Ok(format!("api-{}-{id}", client.id))
}

struct ArchiveSubmission {
    path: String,
    output_dir: Option<String>,
    password: Option<zeroize::Zeroizing<String>>,
    delete_source: Option<bool>,
    auto_start: bool,
}

fn add_archive_authorized(
    core: &Arc<Coordinator>,
    client: &ApiClient,
    policy: Arc<dyn openunpack_core::coordinator::JobExecutionPolicy>,
    submission: ArchiveSubmission,
) -> Result<Vec<JobView>, ApiError> {
    let data = core.data.canonicalize().map_err(|_| ApiError::internal())?;
    let path = check_client_path(&data, client, &submission.path, false, false, false)?;
    if !path.is_file() {
        return Err(ApiError::operation("源路径必须是受支持的压缩包文件。"));
    }
    let mut options = core
        .snapshot()
        .map_err(|error| ApiError::operation(&error))?
        .settings;
    if let Some(output) = submission.output_dir.filter(|path| !path.trim().is_empty()) {
        options.output_root = canonicalize_existing(&output)
            .or_else(|_| canonicalize_for_save(&output))
            .map_err(|error| ApiError::operation(&error))?
            .to_string_lossy()
            .into_owned();
    }
    if let Some(delete_source) = submission.delete_source {
        options.source_cleanup = if delete_source { "recycle" } else { "keep" }.into();
    }
    let scan = openunpack_core::scanner::scan(
        &[path.to_string_lossy().into_owned()],
        false,
        &options.input_exclusions,
    )
    .map_err(|error| ApiError::operation(&error))?;
    if scan.sources.is_empty() {
        return Err(ApiError::operation("未发现支持的压缩包文件。"));
    }
    // Use the same complete plan as AddJobs, including inherited cleanup,
    // output/cache settings and independently rediscovered sibling volumes.
    // No native-window grant is installed by this API convenience endpoint.
    preflight_grants(
        &data,
        core,
        client,
        vec![],
        ExecutionAccess::Add {
            sources: scan.sources.clone(),
            options: Box::new(options.clone()),
        },
    )?;
    let added = core
        .add_jobs_with_password_authorized(
            scan.sources,
            options,
            submission.password,
            policy.clone(),
        )
        .map_err(|error| ApiError::operation(&error))?;
    if submission.auto_start && !added.is_empty() {
        let ids = added.iter().map(|job| job.id.clone()).collect::<Vec<_>>();
        core.start_selected_authorized(&ids, policy)
            .map_err(|error| ApiError::operation(&error))?;
    }
    Ok(added)
}

fn submit_job_input_authorized(
    core: &Arc<Coordinator>,
    client: &ApiClient,
    policy: Arc<dyn openunpack_core::coordinator::JobExecutionPolicy>,
    job_id: &str,
    input: zeroize::Zeroizing<String>,
) -> Result<bool, ApiError> {
    core.job_details(job_id)
        .map_err(|error| ApiError::operation(&error))?
        .ok_or_else(|| ApiError::new(404, "not_found", "任务不存在。"))?;
    preflight_grants(
        &core.data,
        core,
        client,
        vec![],
        ExecutionAccess::Selected {
            job_ids: vec![job_id.to_owned()],
        },
    )?;
    let handled = core
        .submit_input_authorized(job_id, input.to_string(), policy.clone())
        .map_err(|error| ApiError::operation(&error))?;
    if !handled {
        core.retry_preserving_options_authorized(
            job_id,
            false,
            Some(input.to_string()),
            policy.clone(),
        )
        .map_err(|error| ApiError::operation(&error))?;
        core.start_selected_authorized(std::slice::from_ref(&job_id.to_owned()), policy)
            .map_err(|error| ApiError::operation(&error))?;
    }
    Ok(handled)
}

impl ApiBackend for DesktopBackend {
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<openunpack_protocol::StateInvalidated> {
        // Convert invalidations, without forwarding password metadata or settings.
        let (sender, receiver) = tokio::sync::broadcast::channel(32);
        let coordinator = if let Some(ref app) = self.app {
            app.state::<AppState>().coordinator.clone()
        } else if let Some(ref st) = self.standalone_state {
            st.coordinator.clone()
        } else {
            return receiver;
        };
        // Register before returning the ready stream. Delaying subscription until
        // the bridge task runs could lose the first concurrent state change.
        let mut source = coordinator.subscribe();
        let coord_clone = coordinator.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    _ = sender.closed() => break,
                    event = source.recv() => match event {
                        Ok(()) | Err(RecvError::Lagged(_)) => {
                            let (session_id, revision) = coord_clone.revision_info();
                            if sender.send(openunpack_protocol::StateInvalidated { session_id, revision: revision.to_string() }).is_err() { break; }
                        },
                        Err(RecvError::Closed) => break,
                    }
                }
            }
        });
        receiver
    }
    fn subscribe_progress(
        &self,
    ) -> tokio::sync::broadcast::Receiver<openunpack_protocol::JobProgressUpdate> {
        let coordinator = if let Some(ref app) = self.app {
            app.state::<AppState>().coordinator.clone()
        } else if let Some(ref st) = self.standalone_state {
            st.coordinator.clone()
        } else {
            let (_s, r) = tokio::sync::broadcast::channel(16);
            return r;
        };
        coordinator.subscribe_progress()
    }
    fn subscribe_need_input(
        &self,
    ) -> tokio::sync::broadcast::Receiver<openunpack_protocol::NeedPasswordEvent> {
        let coordinator = if let Some(ref app) = self.app {
            app.state::<AppState>().coordinator.clone()
        } else if let Some(ref st) = self.standalone_state {
            st.coordinator.clone()
        } else {
            let (_s, r) = tokio::sync::broadcast::channel(16);
            return r;
        };
        coordinator.subscribe_need_input()
    }
    fn project_progress(
        &self,
        update: openunpack_protocol::JobProgressUpdate,
        client: &ApiClient,
    ) -> Option<openunpack_protocol::JobProgressUpdate> {
        (client.scopes.contains(&Scope::JobsRead) && self.store.active_client(&client.id))
            .then(|| project_progress_paths(update, client))
    }
    fn project_need_input(
        &self,
        event: openunpack_protocol::NeedPasswordEvent,
        client: &ApiClient,
    ) -> Option<openunpack_protocol::NeedPasswordEvent> {
        (client.scopes.contains(&Scope::JobsRead) && self.store.active_client(&client.id))
            .then(|| project_need_input_paths(event, client))
    }
    fn execute(&self, command: ApiCommand, client: ApiClient) -> BackendFuture<'_> {
        Box::pin(async move {
            let (state_from_app, window_opt) = if let Some(ref app) = self.app {
                (
                    Some(app.state::<AppState>()),
                    app.get_webview_window("main"),
                )
            } else {
                (None, None)
            };
            let state: &AppState = match (&state_from_app, &self.standalone_state) {
                (Some(s), _) => s.inner(),
                (_, Some(s)) => s.as_ref(),
                _ => return Err(ApiError::internal()),
            };

            let requests = file_requests(&command)?;
            let execution = execution_access(&command);
            let check_data = state.coordinator.data.clone();
            let check_access = state.access.clone();
            let check_coord = state.coordinator.clone();
            let check_client = client.clone();
            spawn_blocking(move || {
                preflight(
                    &check_data,
                    &check_access,
                    &check_coord,
                    &check_client,
                    requests,
                    execution,
                )
            })
            .await
            .map_err(|_| ApiError::internal())??;

            let policy: Arc<dyn openunpack_core::coordinator::JobExecutionPolicy> = Arc::new(
                ClientJobPolicy {
                    client: client.clone(),
                    store: self.store.clone(),
                    data: state.coordinator.data.clone(),
                    cleanup_only: matches!(&command, ApiCommand::RecoverJob { action, .. } if action == "cleanup"),
                },
            );
            use ApiCommand::*;
            match command {
                Snapshot => snapshot_value(super::core_snapshot(state).await, &client),
                QueueSnapshot => snapshot_value(super::core_queue_snapshot(state).await, &client),
                ReadSettings => job_value(
                    super::core_snapshot(state).await.map(|view| view.settings),
                    &client,
                ),
                History => job_value(super::core_history(state).await, &client),
                HistoryPage { query } => {
                    job_value(super::core_history_page(state, query).await, &client)
                }
                JobDetails { job_id } => {
                    job_value(super::core_job_details(state, &job_id).await, &client)
                }
                ScanSources { paths, request_id } => {
                    let result = super::core_scan_sources(
                        state,
                        paths,
                        request_id.map(|v| owned_request(&client, v)).transpose()?,
                    )
                    .await
                    .map_err(|error| ApiError::operation(&error))?;
                    let data = state.coordinator.data.clone();
                    let application = client.clone();
                    let result = spawn_blocking(move || {
                        let data = data.canonicalize().map_err(|_| ApiError::internal())?;
                        for source in &result.sources {
                            for path in std::iter::once(&source.path).chain(source.volumes.iter()) {
                                check_client_path(&data, &application, path, false, false, false)?;
                            }
                        }
                        Ok::<_, ApiError>(result)
                    })
                    .await
                    .map_err(|_| ApiError::internal())??;
                    value(Ok(result))
                }
                CancelScan { request_id } => value(
                    super::core_cancel_scan(state, &owned_request(&client, request_id)?).await,
                ),
                PreviewExclusions {
                    paths,
                    patterns,
                    scope,
                    request_id,
                } => value(
                    super::core_preview_exclusions(
                        state,
                        paths,
                        patterns,
                        Some(scope),
                        request_id.map(|v| owned_request(&client, v)).transpose()?,
                    )
                    .await,
                ),
                CancelPreviewExclusions { request_id } => value(
                    super::core_cancel_preview_exclusions(
                        state,
                        &owned_request(&client, request_id)?,
                    )
                    .await,
                ),
                AddJobs { sources, options } => {
                    let core = state.coordinator.clone();
                    job_value(
                        spawn_blocking(move || core.add_jobs_authorized(sources, options, policy))
                            .await
                            .map_err(|_| ApiError::internal())?,
                        &client,
                    )
                }
                StartJobs => {
                    let core = state.coordinator.clone();
                    value(
                        spawn_blocking(move || core.start_authorized(policy))
                            .await
                            .map_err(|_| ApiError::internal())?,
                    )
                }
                StartSelectedJobs { job_ids } => {
                    let core = state.coordinator.clone();
                    let can_resume = client.scopes.contains(&Scope::SettingsWrite);
                    value(
                        spawn_blocking(move || {
                            if can_resume {
                                core.start_selected_resuming_authorized(&job_ids, policy)
                            } else {
                                core.start_selected_authorized(&job_ids, policy)
                            }
                        })
                        .await
                        .map_err(|_| ApiError::internal())?,
                    )
                }
                PauseJobs => {
                    // SEC-01: An API client cannot arbitrarily pause the whole engine
                    // unless it possesses system management scope (SettingsWrite).
                    if !client.scopes.contains(&Scope::SettingsWrite) {
                        return Err(ApiError::permission_denied(
                            "暂停系统全局解压引擎需要 settings.write 权限。",
                        ));
                    }
                    if !self.store.active_client(&client.id) {
                        return Err(ApiError::denied());
                    }
                    let core = state.coordinator.clone();
                    value(
                        spawn_blocking(move || core.pause_authorized(policy))
                            .await
                            .map_err(|_| ApiError::internal())?,
                    )
                }
                CancelJobs { job_id } => {
                    // SEC-01: Cross-tenant / BOLA protection on job cancellation
                    if let Some(ref id) = job_id {
                        let job = state
                            .coordinator
                            .job_details(id)
                            .map_err(|e| ApiError::operation(&e))?
                            .ok_or_else(|| ApiError::new(404, "not_found", "任务不存在。"))?;
                        policy.authorize(&job).map_err(|e| {
                            ApiError::permission_denied(&format!(
                                "无权取消未授权或属于其他租户的任务: {e}"
                            ))
                        })?;
                        value(super::core_cancel_jobs(state, Some(id.clone())).await)
                    } else {
                        // Cancelling without a specific job_id cancels only jobs authorized for this client
                        let core = state.coordinator.clone();
                        let policy_clone = policy.clone();
                        spawn_blocking(move || {
                            let snap = core.snapshot().map_err(|e| ApiError::operation(&e))?;
                            for job in &snap.jobs {
                                if policy_clone.authorize(job).is_ok()
                                    && !matches!(
                                        job.status.as_str(),
                                        "completed" | "cancelled" | "failed" | "skipped"
                                    )
                                {
                                    let _ = core.cancel(Some(&job.id));
                                }
                            }
                            Ok::<(), ApiError>(())
                        })
                        .await
                        .map_err(|_| ApiError::internal())??;
                        value(Ok(()))
                    }
                }
                RetryJob {
                    job_id,
                    partial,
                    password,
                    preserve_options,
                } => {
                    let core = state.coordinator.clone();
                    value(
                        spawn_blocking(move || {
                            if preserve_options {
                                core.retry_preserving_options_authorized(
                                    &job_id,
                                    partial,
                                    password,
                                    policy.clone(),
                                )?;
                            } else {
                                core.retry_authorized(&job_id, partial, password, policy.clone())?;
                            }
                            core.start_selected_authorized(std::slice::from_ref(&job_id), policy)
                        })
                        .await
                        .map_err(|_| ApiError::internal())?,
                    )
                }
                RetryNode {
                    job_id,
                    node_id,
                    preserve_options,
                } => {
                    let core = state.coordinator.clone();
                    value(
                        spawn_blocking(move || {
                            if preserve_options {
                                core.retry_node_preserving_options_authorized(
                                    &job_id,
                                    &node_id,
                                    policy.clone(),
                                )?;
                            } else {
                                core.retry_node_authorized(&job_id, &node_id, policy.clone())?;
                            }
                            core.start_selected_authorized(std::slice::from_ref(&job_id), policy)
                        })
                        .await
                        .map_err(|_| ApiError::internal())?,
                    )
                }
                RemoveJob { job_id } => {
                    // SEC-01: BOLA protection on job removal
                    let job = state
                        .coordinator
                        .job_details(&job_id)
                        .map_err(|e| ApiError::operation(&e))?
                        .ok_or_else(|| ApiError::new(404, "not_found", "任务不存在。"))?;
                    policy.authorize(&job).map_err(|e| {
                        ApiError::permission_denied(&format!(
                            "无权删除未授权或属于其他租户的任务: {e}"
                        ))
                    })?;
                    value(super::core_remove_job(state, job_id).await)
                }
                ClearCompleted => {
                    // SEC-01: Only clear completed jobs authorized for this client
                    let core = state.coordinator.clone();
                    let policy_clone = policy.clone();
                    spawn_blocking(move || {
                        let snap = core.snapshot().map_err(|e| ApiError::operation(&e))?;
                        let eligible_ids: Vec<_> = snap
                            .jobs
                            .iter()
                            .filter(|j| {
                                matches!(
                                    j.status.as_str(),
                                    "completed" | "partial" | "salvaged" | "skipped"
                                ) && j.workspace.is_none()
                                    && policy_clone.authorize(j).is_ok()
                            })
                            .map(|j| j.id.clone())
                            .collect();
                        for id in eligible_ids {
                            let _ = core.remove(&id);
                        }
                        Ok::<(), ApiError>(())
                    })
                    .await
                    .map_err(|_| ApiError::internal())??;
                    value(Ok(()))
                }
                ClearHistory => {
                    // SEC-01: Global history clear is restricted to administrators
                    if !client.scopes.contains(&Scope::SettingsWrite) {
                        return Err(ApiError::permission_denied(
                            "清空全局任务历史记录需要 settings.write 权限。",
                        ));
                    }
                    value(super::core_clear_history(state).await)
                }
                RecoverJob { job_id, action } => value(
                    state
                        .coordinator
                        .recover_authorized(&job_id, &action, policy)
                        .await,
                ),
                Passwords => value(super::core_passwords(state).await),
                SavePassword { input } => value(super::core_save_password(state, input).await),
                DeletePassword { id } => value(super::core_delete_password(state, &id).await),
                RevealPassword { id } => value(super::core_reveal_password(state, &id).await),
                CopyPassword { id } => {
                    let owner = window_opt
                        .as_ref()
                        .and_then(|w| w.hwnd().ok())
                        .map(|h| h.0 as usize)
                        .unwrap_or(0);
                    value(super::core_copy_password(state, &id, owner).await)
                }
                BulkPasswordAction {
                    ids,
                    action,
                    groups,
                } => {
                    value(super::core_bulk_password_action(state, ids, action, Some(groups)).await)
                }
                PreviewImport {
                    text,
                    group,
                    format,
                } => value(super::core_preview_import(state, text, group, format).await),
                ConfirmImport {
                    text,
                    group,
                    format,
                    token,
                } => value(super::core_confirm_import(state, text, group, format, token).await),
                ExportBackup { path, passphrase } => {
                    value(super::core_export_backup(state, path, passphrase).await)
                }
                PreviewRestoreBackup { path, passphrase } => {
                    value(super::core_preview_restore_backup(state, path, passphrase).await)
                }
                ConfirmRestoreBackup {
                    path,
                    passphrase,
                    token,
                } => {
                    value(super::core_confirm_restore_backup(state, path, passphrase, token).await)
                }
                SaveSettings { mut settings } => {
                    // SEC-03: Settings Injection Guard
                    // API clients are strictly forbidden from modifying system-privileged fields:
                    // post_extraction_action (shutdown/sleep/hibernate injection), watched_folders,
                    // and on_complete (arbitrary command execution on job terminal state).
                    let current_settings = state
                        .coordinator
                        .snapshot()
                        .map(|s| s.settings)
                        .unwrap_or_default();
                    settings.post_extraction_action = current_settings.post_extraction_action;
                    settings.watched_folders = current_settings.watched_folders;
                    settings.on_complete = current_settings.on_complete;

                    // Output root and cache root boundary validation
                    if !openunpack_local_api::is_trusted_all_paths() {
                        if !settings.output_root.trim().is_empty()
                            && settings.output_root != current_settings.output_root
                        {
                            let out_path = Path::new(&settings.output_root);
                            let authorized = client.write_roots.iter().any(|root| {
                                let root_path = Path::new(root);
                                path_eq_case_insensitive(out_path, root_path)
                                    || path_starts_with_case_insensitive(out_path, root_path)
                            });
                            if !authorized {
                                return Err(ApiError::permission_denied(
                                    "输出根目录超出当前 API 客户端授权写入范围。",
                                ));
                            }
                        }
                        if !settings.cache_root.trim().is_empty()
                            && settings.cache_root != current_settings.cache_root
                        {
                            let cache_path = Path::new(&settings.cache_root);
                            let authorized = client.write_roots.iter().any(|root| {
                                let root_path = Path::new(root);
                                path_eq_case_insensitive(cache_path, root_path)
                                    || path_starts_with_case_insensitive(cache_path, root_path)
                            });
                            if !authorized {
                                return Err(ApiError::permission_denied(
                                    "缓存根目录超出当前 API 客户端授权写入范围。",
                                ));
                            }
                        }
                    }

                    job_value(
                        super::core_save_settings(state, settings, window_opt.as_ref()).await,
                        &client,
                    )
                }
                InspectArchive {
                    path,
                    password,
                    inspect_id,
                } => value(
                    state
                        .coordinator
                        .inspect_archive_authorized(
                            &path,
                            password,
                            owned_request(&client, inspect_id)?,
                            policy,
                        )
                        .await,
                ),
                CancelInspectArchive { inspect_id } => value(
                    super::core_cancel_inspect_archive(state, &owned_request(&client, inspect_id)?)
                        .await,
                ),
                ExtractSelective { request } => {
                    policy
                        .authorize_path(Path::new(&request.archive_path), false)
                        .map_err(|_| ApiError::denied())?;
                    let output = request
                        .destination_dir
                        .as_deref()
                        .filter(|path| !path.trim().is_empty())
                        .map(PathBuf::from)
                        .unwrap_or_else(|| {
                            Path::new(&request.archive_path)
                                .parent()
                                .unwrap_or(Path::new(""))
                                .to_owned()
                        });
                    policy
                        .authorize_path(&output, true)
                        .map_err(|_| ApiError::denied())?;
                    value(
                        state
                            .coordinator
                            .extract_selective_authorized(request, policy)
                            .await,
                    )
                }
                OpenPath { path } => value(super::core_open_path(state, path).await),
                PickFiles { folder } => {
                    // SEC-02: Native pickers return user-selected paths without mutating persistent client roots.
                    let paths = super::core_pick_files(state, folder)
                        .await
                        .map_err(|e| ApiError::operation(&e))?;
                    value(Ok(paths))
                }
                PickDirectory => {
                    let path = super::core_pick_directory(state)
                        .await
                        .map_err(|e| ApiError::operation(&e))?;
                    value(Ok(path))
                }
                PickSaveBackup => {
                    let path = super::core_pick_save_backup(state)
                        .await
                        .map_err(|e| ApiError::operation(&e))?;
                    value(Ok(path))
                }
                PickOpenBackup => {
                    let path = super::core_pick_open_backup(state)
                        .await
                        .map_err(|e| ApiError::operation(&e))?;
                    value(Ok(path))
                }
                ReadImportFile { path, encoding } => {
                    value(super::core_read_import_file(state, path, encoding).await)
                }
                PreviewToolboxOperation { options } => {
                    value(super::core_preview_toolbox_operation(state, options).await)
                }
                ExecuteToolboxOperation { request } => {
                    value(super::core_execute_toolbox_operation(state, request).await)
                }
                FixArchiveExtensions { paths } => {
                    value(super::core_fix_archive_extensions(state, paths).await)
                }
                AddArchive {
                    path,
                    output_dir,
                    password,
                    delete_source,
                    auto_start,
                } => {
                    let submission = ArchiveSubmission {
                        path,
                        output_dir,
                        password: password.map(zeroize::Zeroizing::new),
                        delete_source,
                        auto_start,
                    };
                    let core = state.coordinator.clone();
                    let application = client.clone();
                    let added = spawn_blocking(move || {
                        add_archive_authorized(&core, &application, policy, submission)
                    })
                    .await
                    .map_err(|_| ApiError::internal())??;
                    job_value(Ok(added), &client)
                }
                SubmitInput {
                    job_id,
                    input_type,
                    value: input_value,
                } => {
                    if input_type != "password" {
                        return Err(ApiError::new(
                            400,
                            "invalid_request",
                            "仅支持 password 类型的任务输入。",
                        ));
                    }
                    let input = zeroize::Zeroizing::new(input_value);
                    let core = state.coordinator.clone();
                    let application = client.clone();
                    let target = job_id.clone();
                    let handled = spawn_blocking(move || {
                        submit_job_input_authorized(&core, &application, policy, &target, input)
                    })
                    .await
                    .map_err(|_| ApiError::internal())??;
                    value(Ok(serde_json::json!({
                        "success": true,
                        "jobId": job_id,
                        "inputType": input_type,
                        "resumedInFlight": handled,
                    })))
                }
            }
        })
    }
}

enum Grant {
    Read(PathBuf),
    Write(PathBuf),
    BackupRead(PathBuf),
    BackupWrite(PathBuf),
}
#[derive(Clone, Copy)]
enum GrantKind {
    Read,
    WriteDirectory,
    CheckWrite,
    BackupRead,
    BackupWrite,
}
struct FileRequest {
    path: String,
    write: bool,
    recursive: bool,
    kind: GrantKind,
}

// Only legacy start_jobs selects the whole current queue. Admission and retry
// authorize their own targets; preserving retries use the task's frozen options.
enum ExecutionAccess {
    None,
    Queue,
    Selected {
        job_ids: Vec<String>,
    },
    Add {
        sources: Vec<SourceGroup>,
        options: Box<Settings>,
    },
    Retry {
        job_id: String,
        preserve_options: bool,
    },
    Recover {
        job_id: String,
        requeue: bool,
        cleanup: bool,
    },
}

fn execution_access(command: &ApiCommand) -> ExecutionAccess {
    match command {
        ApiCommand::StartJobs => ExecutionAccess::Queue,
        ApiCommand::StartSelectedJobs { job_ids } => ExecutionAccess::Selected {
            job_ids: job_ids.clone(),
        },
        ApiCommand::AddJobs { sources, options } => ExecutionAccess::Add {
            sources: sources.clone(),
            options: Box::new(options.clone()),
        },
        ApiCommand::RetryJob {
            job_id,
            preserve_options,
            ..
        }
        | ApiCommand::RetryNode {
            job_id,
            preserve_options,
            ..
        } => ExecutionAccess::Retry {
            job_id: job_id.clone(),
            preserve_options: *preserve_options,
        },
        ApiCommand::RecoverJob { job_id, action } => ExecutionAccess::Recover {
            job_id: job_id.clone(),
            requeue: matches!(action.as_str(), "retry" | "writeback"),
            cleanup: action == "cleanup",
        },
        _ => ExecutionAccess::None,
    }
}

fn require_execution_scopes(
    client: &ApiClient,
    options: &Settings,
    may_clean_source: bool,
) -> Result<(), ApiError> {
    if !client.scopes.contains(&Scope::FilesRead)
        || !client.scopes.contains(&Scope::FilesWrite)
        || may_clean_source
            && options.source_cleanup != "keep"
            && !client.scopes.contains(&Scope::FilesDelete)
    {
        return Err(ApiError::denied());
    }
    Ok(())
}

fn source_execution_requests(
    source: &SourceGroup,
    options: &Settings,
) -> Result<Vec<FileRequest>, ApiError> {
    let mut requests = Vec::new();
    for path in std::iter::once(&source.path).chain(source.volumes.iter()) {
        requests.push(FileRequest {
            path: path.clone(),
            write: options.source_cleanup != "keep",
            recursive: false,
            kind: GrantKind::Read,
        });
    }
    let output = if options.output_root.trim().is_empty() {
        Path::new(&source.path)
            .parent()
            .ok_or_else(ApiError::denied)?
            .to_string_lossy()
            .into_owned()
    } else {
        options.output_root.clone()
    };
    requests.push(FileRequest {
        path: output,
        write: true,
        recursive: false,
        kind: GrantKind::WriteDirectory,
    });
    if !options.cache_root.trim().is_empty() {
        requests.push(FileRequest {
            path: options.cache_root.clone(),
            write: true,
            recursive: false,
            kind: GrantKind::WriteDirectory,
        });
    }
    Ok(requests)
}

fn retained_job_requests(job: &JobView) -> Vec<FileRequest> {
    [&job.workspace, &job.output_path]
        .into_iter()
        .filter_map(|path| path.as_ref())
        .map(|path| FileRequest {
            path: path.clone(),
            write: true,
            recursive: false,
            kind: GrantKind::CheckWrite,
        })
        .collect()
}

// A pure projection used by authorization tests. Paths from the resulting plan
// are checked against this client's roots before any NativeAccess grant occurs.
fn execution_requests(
    client: &ApiClient,
    access: &ExecutionAccess,
    snapshot: &AppSnapshot,
) -> Result<Vec<FileRequest>, ApiError> {
    let mut requests = Vec::new();
    if matches!(access, ExecutionAccess::Queue) {
        for job in snapshot
            .jobs
            .iter()
            .filter(|job| matches!(job.status.as_str(), "queued" | "waitingResource"))
        {
            require_execution_scopes(client, &job.options, true)?;
            requests.extend(source_execution_requests(&job.source, &job.options)?);
            requests.extend(retained_job_requests(job));
        }
    }
    match access {
        ExecutionAccess::Selected { job_ids } => {
            for id in job_ids {
                let job = snapshot
                    .jobs
                    .iter()
                    .find(|job| &job.id == id)
                    .ok_or_else(ApiError::denied)?;
                require_execution_scopes(client, &job.options, true)?;
                requests.extend(source_execution_requests(&job.source, &job.options)?);
                requests.extend(retained_job_requests(job));
            }
        }
        ExecutionAccess::Add { sources, options } => {
            require_execution_scopes(client, options, true)?;
            for source in sources {
                requests.extend(source_execution_requests(source, options)?);
            }
        }
        ExecutionAccess::Retry {
            job_id,
            preserve_options,
        } => {
            let job = snapshot
                .jobs
                .iter()
                .find(|job| &job.id == job_id)
                .ok_or_else(ApiError::denied)?;
            let options = if *preserve_options {
                &job.options
            } else {
                &snapshot.settings
            };
            require_execution_scopes(client, options, true)?;
            requests.extend(source_execution_requests(&job.source, options)?);
            requests.extend(retained_job_requests(job));
        }
        ExecutionAccess::Recover {
            job_id,
            requeue,
            cleanup,
        } => {
            let job = snapshot
                .jobs
                .iter()
                .find(|job| &job.id == job_id)
                .ok_or_else(ApiError::denied)?;
            require_execution_scopes(client, &job.options, !cleanup)?;
            if !cleanup {
                requests.extend(source_execution_requests(&job.source, &job.options)?);
            }
            requests.extend(retained_job_requests(job));
            if *requeue {
                require_execution_scopes(client, &snapshot.settings, true)?;
                requests.extend(source_execution_requests(&job.source, &snapshot.settings)?);
            }
        }
        ExecutionAccess::None | ExecutionAccess::Queue => {}
    }
    Ok(requests)
}

struct ClientJobPolicy {
    client: ApiClient,
    store: Arc<ApiStore>,
    data: PathBuf,
    cleanup_only: bool,
}
impl openunpack_core::coordinator::JobExecutionPolicy for ClientJobPolicy {
    fn authorize(&self, job: &JobView) -> Result<(), String> {
        if !self.store.active_client(&self.client.id) {
            return Err("应用授权已撤销或本机 API 已停用。".into());
        }
        require_execution_scopes(&self.client, &job.options, !self.cleanup_only)
            .map_err(|e| e.message)?;
        if self.cleanup_only && job.status == "running" {
            return Err("清理授权不能执行解压任务。".into());
        }
        if openunpack_local_api::is_trusted_all_paths() {
            return Ok(());
        }
        let data = self
            .data
            .canonicalize()
            .map_err(|_| "应用数据目录不可用。")?;
        let mut requests = if self.cleanup_only {
            Vec::new()
        } else {
            source_execution_requests(&job.source, &job.options).map_err(|e| e.message)?
        };
        requests.extend(retained_job_requests(job));
        for request in requests {
            check_client_path(
                &data,
                &self.client,
                &request.path,
                request.write,
                matches!(request.kind, GrantKind::CheckWrite),
                request.recursive,
            )
            .map_err(|e| e.message)?;
        }
        Ok(())
    }
    fn authorize_path(&self, path: &Path, write: bool) -> Result<(), String> {
        if !self.store.active_client(&self.client.id)
            || !self.client.scopes.contains(&if write {
                Scope::FilesWrite
            } else {
                Scope::FilesRead
            })
        {
            return Err("此应用没有访问该目录的权限。".into());
        }
        if openunpack_local_api::is_trusted_all_paths() {
            return Ok(());
        }
        let data = self
            .data
            .canonicalize()
            .map_err(|_| "应用数据目录不可用。")?;
        check_client_path(
            &data,
            &self.client,
            &path.to_string_lossy(),
            write,
            write,
            false,
        )
        .map(|_| ())
        .map_err(|e| e.message)
    }
}

fn check_client_path(
    data: &Path,
    client: &ApiClient,
    raw: &str,
    write: bool,
    save: bool,
    recursive: bool,
) -> Result<PathBuf, ApiError> {
    if raw.is_empty() || !Path::new(raw).is_absolute() {
        return Err(ApiError::denied());
    }
    let drive_colon = match Path::new(raw).components().next() {
        Some(std::path::Component::Prefix(prefix)) => match prefix.kind() {
            std::path::Prefix::Disk(_) => Some(1),
            std::path::Prefix::VerbatimDisk(_) => Some(5),
            _ => None,
        },
        _ => None,
    };
    let has_illegal_colon = raw
        .char_indices()
        .any(|(i, c)| c == ':' && Some(i) != drive_colon);
    if has_illegal_colon {
        return Err(ApiError::denied());
    }
    let target = if save {
        canonicalize_for_save(raw)
    } else {
        canonicalize_existing(raw)
    }
    .map_err(|_| ApiError::denied())?;
    if openunpack_local_api::is_trusted_all_paths() {
        return Ok(target);
    }
    if path_starts_with_case_insensitive(&target, data)
        || (recursive || write) && path_starts_with_case_insensitive(data, &target)
    {
        return Err(ApiError::denied());
    }
    let allowed = if write {
        client.write_roots.iter().collect::<Vec<_>>()
    } else {
        client
            .read_roots
            .iter()
            .chain(&client.write_roots)
            .collect::<Vec<_>>()
    };
    if !allowed.into_iter().any(|root| {
        canonicalize_existing(root)
            .or_else(|_| canonicalize_for_save(root))
            .is_ok_and(|root| {
                path_eq_case_insensitive(&target, &root)
                    || root.is_dir() && path_starts_with_case_insensitive(&target, &root)
            })
    }) {
        return Err(ApiError::denied());
    }
    Ok(target)
}
fn file_requests(command: &ApiCommand) -> Result<Vec<FileRequest>, ApiError> {
    let mut requests = Vec::new();
    let mut add = |path: &str, kind, write, recursive| {
        requests.push(FileRequest {
            path: path.into(),
            kind,
            write,
            recursive,
        })
    };
    match command {
        ApiCommand::AddJobs { options, .. } | ApiCommand::SaveSettings { settings: options } => {
            for path in [&options.output_root, &options.cache_root] {
                if !path.trim().is_empty() {
                    add(path, GrantKind::WriteDirectory, true, false);
                }
            }
        }
        _ => {}
    }
    match command {
        ApiCommand::ScanSources { paths, .. } => {
            if paths.len() > 1000 {
                return Err(ApiError::new(
                    400,
                    "invalid_request",
                    "一次最多提交 1000 个输入位置。",
                ));
            }
            for path in paths {
                add(path, GrantKind::Read, false, true);
            }
        }
        ApiCommand::AddJobs { sources, options } => {
            if sources.len() > 1000 {
                return Err(ApiError::new(
                    400,
                    "invalid_request",
                    "一次最多添加 1000 个任务。",
                ));
            }
            for source in sources {
                for path in std::iter::once(&source.path).chain(source.volumes.iter()) {
                    add(
                        path,
                        GrantKind::Read,
                        options.source_cleanup != "keep",
                        false,
                    );
                }
                if options.output_root.trim().is_empty() {
                    let parent = Path::new(&source.path)
                        .parent()
                        .ok_or_else(ApiError::denied)?
                        .to_string_lossy();
                    add(&parent, GrantKind::WriteDirectory, true, false);
                }
            }
        }
        ApiCommand::InspectArchive { path, .. }
        | ApiCommand::ReadImportFile { path, .. }
        | ApiCommand::OpenPath { path } => add(path, GrantKind::Read, false, false),
        ApiCommand::ExportBackup { path, .. } => add(path, GrantKind::BackupWrite, true, false),
        ApiCommand::PreviewRestoreBackup { path, .. }
        | ApiCommand::ConfirmRestoreBackup { path, .. } => {
            add(path, GrantKind::BackupRead, false, false)
        }
        ApiCommand::PreviewToolboxOperation { options }
        | ApiCommand::ExecuteToolboxOperation {
            request: ToolboxExecuteRequest { options, .. },
        } => {
            add(
                &options.source_directory,
                GrantKind::Read,
                false,
                options.recursive,
            );
            add(
                &options.source_directory,
                GrantKind::WriteDirectory,
                true,
                options.recursive,
            );
            if let Some(target) = &options.target_directory
                && !target.trim().is_empty()
            {
                add(target, GrantKind::WriteDirectory, true, false);
            }
        }
        // The convenience endpoint first resolves its effective settings and
        // actual volumes, then validates without widening NativeAccess.
        ApiCommand::AddArchive { .. } => {}
        ApiCommand::SubmitInput { .. } => {}
        _ => {}
    }
    if let ApiCommand::ExecuteToolboxOperation { request } = command
        && let Some(paths) = &request.approved_paths
    {
        for path in paths {
            add(path, GrantKind::Read, true, false);
        }
    }
    Ok(requests)
}
fn preflight(
    coord_data: &Path,
    access: &Mutex<NativeAccess>,
    coordinator: &Coordinator,
    client: &ApiClient,
    requests: Vec<FileRequest>,
    execution: ExecutionAccess,
) -> Result<(), ApiError> {
    let grants = preflight_grants(coord_data, coordinator, client, requests, execution)?;
    let mut access_guard = access.lock().map_err(|_| ApiError::internal())?;
    for grant in grants {
        match grant {
            Grant::Read(path) => {
                let dir = path.is_dir();
                access_guard.grant_read(path, dir);
            }
            Grant::Write(path) => access_guard.grant_write_dir(path),
            Grant::BackupRead(path) => access_guard.grant_backup_read(path),
            Grant::BackupWrite(path) => access_guard.grant_backup_write(path),
        }
    }
    Ok(())
}
fn preflight_grants(
    coord_data: &Path,
    coordinator: &Coordinator,
    client: &ApiClient,
    mut requests: Vec<FileRequest>,
    execution: ExecutionAccess,
) -> Result<Vec<Grant>, ApiError> {
    let data = coord_data
        .canonicalize()
        .map_err(|_| ApiError::internal())?;
    let mut grants = Vec::new();
    let check = |raw: &str, write: bool, save: bool, recursive: bool| {
        check_client_path(&data, client, raw, write, save, recursive)
    };
    if !matches!(execution, ExecutionAccess::None) {
        let snapshot = coordinator.snapshot().map_err(|_| ApiError::internal())?;
        requests.extend(execution_requests(client, &execution, &snapshot)?);
    }
    // Core independently discovers real sibling volumes. Authorize its complete
    // current volume set too, rather than trusting a caller's supplied list.
    for request in &requests {
        check(
            &request.path,
            request.write,
            matches!(request.kind, GrantKind::BackupWrite | GrantKind::CheckWrite),
            request.recursive,
        )?;
    }
    let sources: Vec<_> = requests
        .iter()
        .filter(|request| {
            !matches!(execution, ExecutionAccess::None)
                && matches!(request.kind, GrantKind::Read)
                && !request.recursive
        })
        .collect();
    for source in sources {
        let scan = openunpack_core::scanner::scan(std::slice::from_ref(&source.path), false, &[])
            .map_err(|_| ApiError::denied())?;
        for group in scan.sources {
            for path in std::iter::once(&group.path).chain(group.volumes.iter()) {
                check(path, source.write, false, false)?;
            }
        }
    }
    for request in requests {
        let target = check(
            &request.path,
            request.write,
            matches!(request.kind, GrantKind::BackupWrite | GrantKind::CheckWrite),
            request.recursive,
        )?;
        grants.push(match request.kind {
            GrantKind::Read => Grant::Read(target),
            GrantKind::WriteDirectory => {
                if target.is_file() {
                    return Err(ApiError::denied());
                }
                Grant::Write(target)
            }
            GrantKind::BackupRead => Grant::BackupRead(target),
            GrantKind::BackupWrite => Grant::BackupWrite(target),
            GrantKind::CheckWrite => continue,
        });
    }
    Ok(grants)
}

#[cfg(test)]
#[cfg(test)]
mod authorization_tests;

#[cfg(all(test, windows))]
mod native_tests;
