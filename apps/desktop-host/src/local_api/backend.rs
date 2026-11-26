//! 本机 API 面向桌面/独立运行态的命令执行后端实现。

use super::*;

pub(super) struct DesktopBackend {
    pub(super) app: Option<tauri::AppHandle>,
    pub(super) standalone_state: Option<Arc<AppState>>,
    pub(super) store: Arc<ApiStore>,
}

pub(super) struct ArchiveSubmission {
    pub(super) path: String,
    pub(super) output_dir: Option<String>,
    pub(super) password: Option<zeroize::Zeroizing<String>>,
    pub(super) delete_source: Option<bool>,
    pub(super) auto_start: bool,
}

pub(super) fn add_archive_authorized(
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

pub(super) fn submit_job_input_authorized(
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
