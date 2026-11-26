use super::*;
fn client(id: char) -> ApiClient {
    ApiClient {
        id: id.to_string().repeat(32),
        name: "synthetic application".into(),
        scopes: Scope::ALL.into_iter().collect(),
        read_roots: vec![],
        write_roots: vec![],
        created_at: "0".into(),
        expires_at: None,
        last_used_at: None,
    }
}
fn job() -> JobView {
    serde_json::from_value(serde_json::json!({
        "id":"synthetic-job", "source":{"path":"C:\\approved\\sample.zip","displayName":"sample","volumes":["C:\\approved\\sample.zip"],"size":1,"missingVolume":false},
        "options":Settings::default(), "status":"queued", "stage":"queued", "progress":0.0,"message":"","attempts":0,"outputPath":null,"verificationScope":"","createdAt":"","finishedAt":null,"workspace":null,"nodes":[],"stageTimes":{}
    })).unwrap()
}
#[test]
fn job_execution_requires_file_scopes_and_cleanup_permission() {
    let mut application = client('a');
    let mut options = Settings::default();
    application.scopes.remove(&Scope::FilesDelete);
    assert!(require_execution_scopes(&application, &options, true).is_ok());
    for cleanup in ["delete", "recycle"] {
        options.source_cleanup = cleanup.into();
        assert!(require_execution_scopes(&application, &options, true).is_err());
    }
    options.source_cleanup = "keep".into();
    for scope in [Scope::FilesRead, Scope::FilesWrite] {
        let mut restricted = application.clone();
        restricted.scopes.remove(&scope);
        assert!(require_execution_scopes(&restricted, &options, true).is_err());
    }
}
#[test]
fn retry_projection_checks_current_settings_and_retained_paths() {
    let application = client('a');
    let mut original = job();
    original.status = "failed".into();
    original.options.output_root = "C:\\old-output".into();
    original.workspace = Some("C:\\old-cache\\owned-work".into());
    let settings = Settings {
        output_root: "C:\\new-output".into(),
        cache_root: "C:\\new-cache".into(),
        ..Settings::default()
    };
    let snapshot = AppSnapshot {
        jobs: vec![original.clone()],
        passwords: vec![],
        settings,
        running: false,
        paused: false,
        vault_error: None,
        data_directory: String::new(),
        engine_available: true,
        recoverable_count: 0,
        cache_used_bytes: 0,
        session_id: String::new(),
        revision: 1,
    };
    let requests = execution_requests(
        &application,
        &ExecutionAccess::Retry {
            job_id: original.id.clone(),
            preserve_options: false,
        },
        &snapshot,
    )
    .unwrap_or_else(|_| panic!("authorized retry projection should succeed"));
    for path in [
        "C:\\new-output",
        "C:\\new-cache",
        "C:\\old-cache\\owned-work",
    ] {
        assert!(requests.iter().any(|request| request.path == path));
    }
    let mut restricted = application;
    restricted.scopes.remove(&Scope::FilesDelete);
    let mut snapshot = snapshot;
    snapshot.settings.source_cleanup = "delete".into();
    assert!(
        execution_requests(
            &restricted,
            &ExecutionAccess::Retry {
                job_id: original.id,
                preserve_options: false,
            },
            &snapshot
        )
        .is_err()
    );
}
fn execution_snapshot(jobs: Vec<JobView>, settings: Settings) -> AppSnapshot {
    AppSnapshot {
        jobs,
        passwords: vec![],
        settings,
        running: false,
        paused: false,
        vault_error: None,
        data_directory: String::new(),
        engine_available: true,
        recoverable_count: 0,
        cache_used_bytes: 0,
        session_id: String::new(),
        revision: 1,
    }
}
#[test]
fn targeted_execution_projection_never_authorizes_unrelated_gui_queue() {
    let mut application = client('a');
    application.scopes.remove(&Scope::FilesDelete);
    let target = job();
    let mut outside = target.clone();
    outside.id = "unrelated-gui-job".into();
    outside.source.path = "C:\\outside\\gui.zip".into();
    outside.source.volumes = vec![outside.source.path.clone()];
    outside.options.source_cleanup = "delete".into();
    let snapshot = execution_snapshot(vec![target.clone(), outside], Settings::default());
    assert!(execution_requests(&application, &ExecutionAccess::Queue, &snapshot).is_err());
    for access in [
        ExecutionAccess::Selected {
            job_ids: vec![target.id.clone()],
        },
        ExecutionAccess::Add {
            sources: vec![target.source.clone()],
            options: Box::new(target.options.clone()),
        },
        ExecutionAccess::Retry {
            job_id: target.id.clone(),
            preserve_options: true,
        },
        ExecutionAccess::Retry {
            job_id: target.id.clone(),
            preserve_options: false,
        },
        ExecutionAccess::Recover {
            job_id: target.id.clone(),
            requeue: true,
            cleanup: false,
        },
    ] {
        let requests = execution_requests(&application, &access, &snapshot)
            .unwrap_or_else(|_| panic!("unrelated GUI job leaked into target authorization"));
        assert!(
            requests
                .iter()
                .all(|request| !request.path.contains("outside"))
        );
        assert!(
            requests
                .iter()
                .any(|request| request.path == target.source.path)
        );
    }
    assert!(
        execution_requests(
            &application,
            &ExecutionAccess::Selected {
                job_ids: vec!["missing-job".into()],
            },
            &snapshot
        )
        .is_err()
    );
}
#[test]
fn preserving_retry_projection_uses_original_options_without_global_settings_access() {
    let mut application = client('a');
    application.scopes.remove(&Scope::FilesDelete);
    let mut original = job();
    original.options.output_root = "C:\\original-output".into();
    original.options.cache_root = "C:\\original-cache".into();
    let settings = Settings {
        output_root: "C:\\gui-output".into(),
        source_cleanup: "delete".into(),
        ..Settings::default()
    };
    let snapshot = execution_snapshot(vec![original.clone()], settings);
    let requests = execution_requests(
        &application,
        &ExecutionAccess::Retry {
            job_id: original.id.clone(),
            preserve_options: true,
        },
        &snapshot,
    )
    .unwrap_or_else(|_| panic!("frozen retry should not require GUI cleanup rights"));
    assert!(
        requests
            .iter()
            .any(|request| request.path == "C:\\original-output")
    );
    assert!(
        requests
            .iter()
            .any(|request| request.path == "C:\\original-cache")
    );
    assert!(
        requests
            .iter()
            .all(|request| request.path != "C:\\gui-output")
    );
    assert!(
        execution_requests(
            &application,
            &ExecutionAccess::Retry {
                job_id: original.id,
                preserve_options: false,
            },
            &snapshot
        )
        .is_err()
    );
    let access = execution_access(&ApiCommand::RetryNode {
        job_id: "synthetic-job".into(),
        node_id: "inner".into(),
        preserve_options: true,
    });
    let requests = execution_requests(&application, &access, &snapshot)
        .unwrap_or_else(|_| panic!("preserving node retry must use frozen task options"));
    assert!(
        requests
            .iter()
            .any(|request| request.path == "C:\\original-output")
    );
    assert!(
        requests
            .iter()
            .all(|request| request.path != "C:\\gui-output")
    );
}
#[test]
fn directory_grants_are_per_application_and_exclude_data_and_ancestors() {
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("data");
    let approved = directory.path().join("approved");
    let outside = directory.path().join("outside");
    for path in [&data, &approved, &outside] {
        fs::create_dir(path).unwrap();
    }
    let readable = approved.join("input.zip");
    fs::write(&readable, b"synthetic").unwrap();
    let mut application = client('a');
    application
        .read_roots
        .push(approved.to_string_lossy().into_owned());
    let data = data.canonicalize().unwrap();
    assert!(
        check_client_path(
            &data,
            &application,
            &readable.to_string_lossy(),
            false,
            false,
            false
        )
        .is_ok()
    );
    assert!(
        check_client_path(
            &data,
            &application,
            &outside.to_string_lossy(),
            false,
            false,
            false
        )
        .is_err()
    );
    assert!(
        check_client_path(
            &data,
            &application,
            &approved.to_string_lossy(),
            true,
            false,
            false
        )
        .is_err()
    );
    application
        .write_roots
        .push(directory.path().to_string_lossy().into_owned());
    assert!(
        check_client_path(
            &data,
            &application,
            &data.to_string_lossy(),
            false,
            false,
            false
        )
        .is_err()
    );
    assert!(
        check_client_path(
            &data,
            &application,
            &directory.path().to_string_lossy(),
            false,
            false,
            true
        )
        .is_err()
    );
    assert!(
        check_client_path(
            &data,
            &application,
            &directory.path().to_string_lossy(),
            true,
            false,
            false
        )
        .is_err()
    );
    assert!(
        check_client_path(
            &data,
            &client('b'),
            &readable.to_string_lossy(),
            false,
            false,
            false
        )
        .is_err()
    );
}
#[test]
fn cancellation_ids_cannot_address_another_client_or_escape_the_prefix() {
    let left = owned_request(&client('a'), "same-request".into())
        .unwrap_or_else(|_| panic!("valid request ID"));
    let right = owned_request(&client('b'), "same-request".into())
        .unwrap_or_else(|_| panic!("valid request ID"));
    assert_ne!(left, right);
    for id in ["", "../other", "a/b", "a b", "🔑"] {
        assert!(owned_request(&client('a'), id.into()).is_err());
    }
    assert!(owned_request(&client('a'), "x".repeat(65)).is_err());
}
#[test]
fn job_metadata_redaction_removes_nested_vault_and_settings_fields() {
    let directory = tempfile::tempdir().unwrap();
    let approved = directory.path().join("approved");
    let output = directory.path().join("output");
    fs::create_dir(&approved).unwrap();
    fs::create_dir(&output).unwrap();
    let archive = approved.join("approved.zip");
    fs::write(&archive, b"synthetic").unwrap();
    let archive = archive.to_string_lossy().into_owned();
    let output = output.to_string_lossy().into_owned();
    let original = serde_json::json!({ "jobs": [{ "source": { "path": archive }, "options": { "passwordGroup": "sensitive-group", "outputRoot": output }, "nodes": [{"passwordLabel":"sensitive-label","children":[{"passwordLabel":"nested-label"}]}] }] });
    let mut application = client('a');
    application
        .read_roots
        .push(approved.to_string_lossy().into_owned());
    application.write_roots.push(output.clone());
    let mut full = original.clone();
    redact_job_metadata(&mut full, &application);
    assert_eq!(full, original);
    application.scopes.remove(&Scope::VaultRead);
    let mut no_vault = original.clone();
    redact_job_metadata(&mut no_vault, &application);
    assert_eq!(
        no_vault["jobs"][0]["options"].get("passwordGroup"),
        Some(&serde_json::json!(""))
    );
    assert_eq!(
        no_vault["jobs"][0]["nodes"][0].get("passwordLabel"),
        Some(&Value::Null)
    );
    assert_eq!(
        no_vault["jobs"][0]["nodes"][0]["children"][0].get("passwordLabel"),
        Some(&Value::Null)
    );
    assert_eq!(no_vault["jobs"][0]["options"]["outputRoot"], output);
    application.scopes.remove(&Scope::SettingsRead);
    let mut only_jobs = original;
    redact_job_metadata(&mut only_jobs, &application);
    assert_eq!(only_jobs["jobs"][0]["options"]["outputRoot"], "");
    assert_eq!(only_jobs["jobs"][0]["source"]["path"], archive);

    // SEC-05: verify that job paths outside approved roots are redacted
    let mut unauthorized = serde_json::json!({
        "jobs": [{
            "source": { "path": "C:\\secret\\payroll.zip", "volumes": ["C:\\secret\\payroll.zip"] },
            "outputPath": "C:\\secret\\out",
            "nodes": [{"path": "salary.xlsx"}]
        }]
    });
    redact_job_metadata(&mut unauthorized, &application);
    assert_eq!(unauthorized["jobs"][0]["source"]["path"], "[REDACTED]");
    assert_eq!(
        unauthorized["jobs"][0]["source"]["volumes"][0],
        "[REDACTED]"
    );
    assert!(unauthorized["jobs"][0]["outputPath"].is_null());
    assert_eq!(
        unauthorized["jobs"][0]["nodes"].as_array().unwrap().len(),
        0
    );
}
#[test]
fn bound_job_policy_rechecks_revocation_without_granting_data_access() {
    use openunpack_core::coordinator::JobExecutionPolicy;
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("data");
    let approved = directory.path().join("approved");
    fs::create_dir(&data).unwrap();
    fs::create_dir(&approved).unwrap();
    let archive = approved.join("sample.zip");
    fs::write(&archive, b"synthetic archive").unwrap();
    let store = Arc::new(ApiStore::open(&data.join("api.json")).unwrap());
    store
        .save_settings(ApiSettings {
            enabled: true,
            ..ApiSettings::default()
        })
        .unwrap();
    let created = store
        .create_client(ApiClientInput {
            name: "synthetic client".into(),
            scopes: Scope::ALL.into_iter().collect(),
            read_roots: vec![approved.to_string_lossy().into_owned()],
            write_roots: vec![approved.to_string_lossy().into_owned()],
            expires_at: None,
        })
        .unwrap();
    let policy = ClientJobPolicy {
        client: created.client.clone(),
        store: store.clone(),
        data: data.clone(),
        cleanup_only: false,
    };
    let mut planned = job();
    planned.source.path = archive.to_string_lossy().into_owned();
    planned.source.volumes = vec![planned.source.path.clone()];
    assert!(policy.authorize(&planned).is_ok());
    assert!(policy.authorize_path(&data, false).is_err());
    store.revoke_client(&created.client.id).unwrap();
    assert!(policy.authorize(&planned).is_err());
    assert!(policy.authorize_path(&archive, false).is_err());
}
#[test]
fn check_client_path_rejects_ntfs_ads_and_supports_case_insensitive_matching() {
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("data");
    let approved = directory.path().join("approved");
    fs::create_dir(&data).unwrap();
    fs::create_dir(&approved).unwrap();
    let file = approved.join("target.zip");
    fs::write(&file, b"test").unwrap();
    let mut application = client('a');
    application
        .read_roots
        .push(approved.to_string_lossy().into_owned());
    let data = data.canonicalize().unwrap();

    assert!(
        check_client_path(
            &data,
            &application,
            &file.to_string_lossy(),
            false,
            false,
            false
        )
        .is_ok()
    );

    let ads_path = format!("{}:hidden_stream", file.to_string_lossy());
    assert!(check_client_path(&data, &application, &ads_path, false, false, false).is_err());
    let ads_data = format!("{}::$DATA", file.to_string_lossy());
    assert!(check_client_path(&data, &application, &ads_data, false, false, false).is_err());

    // Scanner results are canonical Windows paths, including the verbatim prefix.
    let canonical = file.canonicalize().unwrap().to_string_lossy().into_owned();
    assert!(check_client_path(&data, &application, &canonical, false, false, false).is_ok());
    for suffix in [":hidden_stream", "::$DATA"] {
        let canonical_ads = format!("{canonical}{suffix}");
        assert!(
            check_client_path(&data, &application, &canonical_ads, false, false, false).is_err()
        );
    }

    let uppercase = file.to_string_lossy().to_ascii_uppercase();
    assert!(check_client_path(&data, &application, &uppercase, false, false, false).is_ok());
}

struct BackendRig {
    backend: DesktopBackend,
    client: ApiClient,
    approved: PathBuf,
    outside: PathBuf,
    _directory: tempfile::TempDir,
}
impl BackendRig {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join("data");
        let approved = directory.path().join("approved");
        let outside = directory.path().join("outside");
        for path in [&data, &approved, &outside] {
            fs::create_dir(path).unwrap();
        }
        let core = Coordinator::open(
            data.clone(),
            directory.path().join("missing-worker.exe"),
            directory.path().join("missing-engine.dll"),
        )
        .unwrap();
        core.save_settings(Settings {
            output_root: approved.to_string_lossy().into_owned(),
            cache_root: approved.to_string_lossy().into_owned(),
            ..Settings::default()
        })
        .unwrap();
        let store = Arc::new(ApiStore::open(&data.join("api.json")).unwrap());
        store
            .save_settings(ApiSettings {
                enabled: true,
                ..ApiSettings::default()
            })
            .unwrap();
        let scopes = Scope::ALL
            .into_iter()
            .filter(|scope| {
                !matches!(
                    scope,
                    Scope::FilesDelete | Scope::VaultRead | Scope::VaultWrite | Scope::VaultReveal
                )
            })
            .collect();
        let client = store
            .create_client(ApiClientInput {
                name: "isolated host fixture".into(),
                scopes,
                read_roots: vec![approved.to_string_lossy().into_owned()],
                write_roots: vec![approved.to_string_lossy().into_owned()],
                expires_at: None,
            })
            .unwrap()
            .client;
        let state = Arc::new(AppState {
            vault_use_cases: Arc::new(openunpack_application::VaultUseCases::new(Arc::new(
                CoordinatorVaultStore(core.clone()),
            ))),
            sys_env: Arc::new(openunpack_infrastructure::WindowsSystemEnvironment::new()),
            coordinator: core,
            access: Arc::new(Mutex::new(NativeAccess::default())),
            _lock_file: File::create(data.join("fixture.lock")).unwrap(),
            scans: Mutex::new(HashMap::new()),
            clipboard_sequence: Mutex::new(None),
            exiting: AtomicBool::new(false),
            minimize_to_tray: AtomicBool::new(false),
        });
        Self {
            backend: DesktopBackend {
                app: None,
                standalone_state: Some(state),
                store,
            },
            client,
            approved,
            outside,
            _directory: directory,
        }
    }
    fn state(&self) -> &AppState {
        self.backend.standalone_state.as_ref().unwrap()
    }
    fn archive(&self, name: &str) -> String {
        let path = self.approved.join(name);
        fs::write(&path, b"synthetic metadata-only archive fixture").unwrap();
        path.to_string_lossy().into_owned()
    }
    fn command(&self, path: String) -> ApiCommand {
        ApiCommand::AddArchive {
            path,
            output_dir: None,
            password: Some("synthetic-one-time-password".into()),
            delete_source: None,
            auto_start: false,
        }
    }
    fn assert_native_access_empty(&self) {
        let access = self.state().access.lock().unwrap();
        assert!(access.readable_entries.is_empty());
        assert!(access.readable_directories.is_empty());
        assert!(access.writable_directories.is_empty());
        assert!(access.backup_read_files.is_empty());
        assert!(access.backup_write_files.is_empty());
    }
}

#[tokio::test]
async fn host_add_archive_checks_inherited_output_cache_and_cleanup_before_admission() {
    let rig = BackendRig::new();
    let archive = rig.archive("inherited.zip");
    let original = rig.state().coordinator.snapshot().unwrap().settings;
    for field in ["output", "cache", "cleanup"] {
        let mut settings = original.clone();
        match field {
            "output" => settings.output_root = rig.outside.to_string_lossy().into_owned(),
            "cache" => settings.cache_root = rig.outside.to_string_lossy().into_owned(),
            _ => settings.source_cleanup = "delete".into(),
        }
        rig.state().coordinator.save_settings(settings).unwrap();
        assert!(
            rig.backend
                .execute(rig.command(archive.clone()), rig.client.clone())
                .await
                .is_err()
        );
        assert!(rig.state().coordinator.snapshot().unwrap().jobs.is_empty());
        assert!(
            rig.state()
                .coordinator
                .vault()
                .unwrap()
                .views()
                .unwrap()
                .is_empty()
        );
        rig.assert_native_access_empty();
    }
    assert!(Path::new(&archive).exists());
}

#[tokio::test]
async fn host_add_archive_uses_temporary_password_and_false_cleanup_means_keep() {
    let rig = BackendRig::new();
    let archive = rig.archive("ephemeral.zip");
    let mut settings = rig.state().coordinator.snapshot().unwrap().settings;
    settings.source_cleanup = "delete".into();
    rig.state().coordinator.save_settings(settings).unwrap();
    let mut command = rig.command(archive.clone());
    if let ApiCommand::AddArchive { delete_source, .. } = &mut command {
        *delete_source = Some(false);
    }
    let added = rig
        .backend
        .execute(command, rig.client.clone())
        .await
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(added[0]["options"]["sourceCleanup"], "keep");
    let snapshot = rig.state().coordinator.snapshot().unwrap();
    assert_eq!(snapshot.jobs.len(), 1);
    assert_eq!(snapshot.jobs[0].status, "queued");
    assert_eq!(snapshot.settings.source_cleanup, "delete");
    assert!(
        rig.state()
            .coordinator
            .vault()
            .unwrap()
            .views()
            .unwrap()
            .is_empty()
    );
    rig.assert_native_access_empty();
    assert!(Path::new(&archive).exists());
}

#[tokio::test]
async fn host_add_archive_checks_discovered_sibling_volumes_independently() {
    let rig = BackendRig::new();
    let first = rig.archive("split.7z.001");
    let second = rig.archive("split.7z.002");
    let mut restricted = rig.client.clone();
    restricted.read_roots = vec![first.clone()];
    restricted.write_roots.clear();
    let output = rig.outside.join("output");
    fs::create_dir(&output).unwrap();
    restricted
        .write_roots
        .push(output.to_string_lossy().into_owned());
    let mut settings = rig.state().coordinator.snapshot().unwrap().settings;
    settings.output_root = output.to_string_lossy().into_owned();
    settings.cache_root.clear();
    rig.state().coordinator.save_settings(settings).unwrap();
    assert!(
        rig.backend
            .execute(rig.command(first.clone()), restricted)
            .await
            .is_err()
    );
    assert!(rig.state().coordinator.snapshot().unwrap().jobs.is_empty());
    assert!(Path::new(&first).exists() && Path::new(&second).exists());
    rig.assert_native_access_empty();
}

#[tokio::test]
async fn host_add_archive_reports_start_failure_without_starting_other_jobs() {
    let rig = BackendRig::new();
    let outside = rig.outside.join("gui.zip");
    fs::write(&outside, b"synthetic").unwrap();
    let sources =
        openunpack_core::scanner::scan(&[outside.to_string_lossy().into_owned()], false, &[])
            .unwrap()
            .sources;
    rig.state()
        .coordinator
        .add_jobs(sources, Settings::default())
        .unwrap();
    let mut command = rig.command(rig.archive("selected.zip"));
    if let ApiCommand::AddArchive { auto_start, .. } = &mut command {
        *auto_start = true;
    }
    assert!(
        rig.backend
            .execute(command, rig.client.clone())
            .await
            .is_err()
    );
    let snapshot = rig.state().coordinator.snapshot().unwrap();
    assert!(!snapshot.running);
    assert_eq!(snapshot.jobs.len(), 2);
    assert!(
        snapshot
            .jobs
            .iter()
            .all(|job| job.status == "queued" && job.attempts == 0)
    );
    rig.assert_native_access_empty();
}

#[tokio::test]
async fn host_submit_input_preserves_original_options_and_propagates_failed_restart() {
    let rig = BackendRig::new();
    let added = rig
        .backend
        .execute(rig.command(rig.archive("input.zip")), rig.client.clone())
        .await
        .unwrap_or_else(|error| panic!("{}", error.message));
    let id = added[0]["id"].as_str().unwrap().to_owned();
    rig.state().coordinator.cancel(Some(&id)).unwrap();
    let original = rig
        .state()
        .coordinator
        .job_details(&id)
        .unwrap()
        .unwrap()
        .options;
    let mut changed = original.clone();
    changed.output_root = rig.outside.to_string_lossy().into_owned();
    changed.source_cleanup = "delete".into();
    rig.state().coordinator.save_settings(changed).unwrap();
    let result = rig
        .backend
        .execute(
            ApiCommand::SubmitInput {
                job_id: id.clone(),
                input_type: "password".into(),
                value: "synthetic-retry-password".into(),
            },
            rig.client.clone(),
        )
        .await;
    assert!(result.is_err(), "missing Worker must remain an error");
    let retried = rig.state().coordinator.job_details(&id).unwrap().unwrap();
    assert_eq!(retried.options, original);
    assert_eq!(retried.status, "queued");
    assert!(!rig.state().coordinator.snapshot().unwrap().running);
    assert!(
        rig.state()
            .coordinator
            .vault()
            .unwrap()
            .views()
            .unwrap()
            .is_empty()
    );
    rig.assert_native_access_empty();
    assert!(
        rig.backend
            .execute(
                ApiCommand::SubmitInput {
                    job_id: "missing-job".into(),
                    input_type: "password".into(),
                    value: "synthetic".into(),
                },
                rig.client.clone()
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn host_snapshot_and_queue_apply_the_same_scope_and_path_projection() {
    let rig = BackendRig::new();
    rig.backend
        .execute(rig.command(rig.archive("snapshot.zip")), rig.client.clone())
        .await
        .unwrap_or_else(|error| panic!("{}", error.message));
    let mut restricted = rig.client.clone();
    for scope in [Scope::FilesRead, Scope::SettingsRead, Scope::VaultRead] {
        restricted.scopes.remove(&scope);
    }
    let snapshot = rig
        .backend
        .execute(ApiCommand::Snapshot, restricted.clone())
        .await
        .unwrap_or_else(|error| panic!("{}", error.message));
    let queue = rig
        .backend
        .execute(ApiCommand::QueueSnapshot, restricted)
        .await
        .unwrap_or_else(|error| panic!("{}", error.message));
    for view in [&snapshot, &queue] {
        assert_eq!(view["settings"]["outputRoot"], "");
        assert_eq!(
            view["settings"].get("passwordGroup"),
            Some(&serde_json::json!(""))
        );
        assert_eq!(view["passwords"], serde_json::json!([]));
        assert!(view["vaultError"].is_null());
        assert_eq!(view["dataDirectory"], "[REDACTED]");
        assert_eq!(view["jobs"][0]["source"]["path"], "[REDACTED]");
        assert_eq!(view["jobs"][0]["source"]["volumes"][0], "[REDACTED]");
        assert_eq!(view["jobs"][0]["options"]["outputRoot"], "");
        assert_eq!(
            view["jobs"][0]["options"].get("passwordGroup"),
            Some(&serde_json::json!(""))
        );
        serde_json::from_value::<AppSnapshot>(view.clone()).unwrap();
    }
    assert_eq!(snapshot["jobs"], queue["jobs"]);
}

#[test]
fn host_path_projection_checks_each_location_canonical_boundary_and_event_scope() {
    let rig = BackendRig::new();
    let archive = rig.archive("approved.zip");
    let secret = rig.outside.join("secret.zip");
    fs::write(&secret, b"synthetic").unwrap();
    let secret = secret.to_string_lossy().into_owned();
    let mut metadata = serde_json::json!({
        "source":{"path":archive,"volumes":[archive,secret]},
        "outputPath":secret,"workspace":secret,
        "options":{"outputRoot":secret,"cacheRoot":secret,"onComplete":"local-command"},
        "dataDirectory":rig.state().coordinator.data,
    });
    redact_job_metadata(&mut metadata, &rig.client);
    assert_eq!(metadata["source"]["path"], archive);
    assert_eq!(metadata["source"]["volumes"][1], "[REDACTED]");
    assert!(metadata["outputPath"].is_null() && metadata["workspace"].is_null());
    assert_eq!(metadata["options"]["outputRoot"], "[REDACTED]");
    assert_eq!(metadata["options"]["cacheRoot"], "[REDACTED]");
    assert!(metadata["options"]["onComplete"].is_null());
    assert_eq!(metadata["dataDirectory"], "[REDACTED]");
    let alias = rig.approved.join("..").join("outside").join("secret.zip");
    assert!(!is_client_path_authorized(
        &alias.to_string_lossy(),
        &rig.client
    ));
    let progress = openunpack_protocol::JobProgressUpdate {
        job_id: "synthetic".into(),
        stage: "extracting".into(),
        progress: 0.5,
        completed_bytes: 1,
        total_bytes: 2,
        bytes_per_second: 1.0,
        remaining_seconds: None,
        current_file: Some(archive.clone()),
    };
    let need = openunpack_protocol::NeedPasswordEvent {
        job_id: "synthetic".into(),
        archive_path: archive.clone(),
        timeout_seconds: 60,
        input_type: "password".into(),
    };
    assert_eq!(
        rig.backend
            .project_progress(progress.clone(), &rig.client)
            .unwrap()
            .current_file,
        Some(archive.clone())
    );
    assert_eq!(
        rig.backend
            .project_need_input(need.clone(), &rig.client)
            .unwrap()
            .archive_path,
        archive
    );
    let mut no_files = rig.client.clone();
    no_files.scopes.remove(&Scope::FilesRead);
    assert!(
        rig.backend
            .project_progress(progress.clone(), &no_files)
            .unwrap()
            .current_file
            .is_none()
    );
    assert!(
        rig.backend
            .project_need_input(need.clone(), &no_files)
            .unwrap()
            .archive_path
            .is_empty()
    );
    rig.backend.store.revoke_client(&rig.client.id).unwrap();
    assert!(
        rig.backend
            .project_progress(progress, &rig.client)
            .is_none()
    );
    assert!(rig.backend.project_need_input(need, &rig.client).is_none());
}

#[tokio::test]
async fn host_pause_requires_settings_write_and_selected_resume_cannot_override_native_pause() {
    let rig = BackendRig::new();
    let added = rig
        .backend
        .execute(rig.command(rig.archive("pause.zip")), rig.client.clone())
        .await
        .unwrap_or_else(|error| panic!("{}", error.message));
    let id = added[0]["id"].as_str().unwrap().to_owned();
    let mut restricted = rig.client.clone();
    restricted.scopes.remove(&Scope::SettingsWrite);
    assert!(
        rig.backend
            .execute(ApiCommand::PauseJobs, restricted.clone())
            .await
            .is_err()
    );
    assert!(!rig.state().coordinator.snapshot().unwrap().paused);
    rig.backend
        .execute(ApiCommand::PauseJobs, rig.client.clone())
        .await
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert!(rig.state().coordinator.snapshot().unwrap().paused);
    // Validation must get past the engine-existence check. These files
    // are never executed because both resume attempts must be rejected.
    fs::write(
        rig._directory.path().join("missing-worker.exe"),
        b"not executed",
    )
    .unwrap();
    fs::write(
        rig._directory.path().join("missing-engine.dll"),
        b"not loaded",
    )
    .unwrap();
    assert!(
        rig.backend
            .execute(
                ApiCommand::StartSelectedJobs {
                    job_ids: vec![id.clone()]
                },
                restricted
            )
            .await
            .is_err()
    );
    assert!(rig.state().coordinator.snapshot().unwrap().paused);
    rig.state().coordinator.pause().unwrap();
    assert!(
        rig.backend
            .execute(
                ApiCommand::StartSelectedJobs { job_ids: vec![id] },
                rig.client.clone()
            )
            .await
            .is_err()
    );
    assert!(rig.state().coordinator.snapshot().unwrap().paused);
    assert!(!rig.state().coordinator.snapshot().unwrap().running);
}
