use super::*;

struct TestExecutionPolicy {
    max_volumes: usize,
    output_root: Option<String>,
    seen: Mutex<Vec<usize>>,
}
impl JobExecutionPolicy for TestExecutionPolicy {
    fn authorize(&self, job: &JobView) -> Result<(), String> {
        self.seen.lock().unwrap().push(job.source.volumes.len());
        if job.source.volumes.len() > self.max_volumes
            || self
                .output_root
                .as_ref()
                .is_some_and(|root| root != &job.options.output_root)
        {
            return Err("synthetic authorization denied".into());
        }
        Ok(())
    }
    fn authorize_path(&self, _: &Path, _: bool) -> Result<(), String> {
        Ok(())
    }
}
fn execution_policy(max_volumes: usize) -> Arc<TestExecutionPolicy> {
    Arc::new(TestExecutionPolicy {
        max_volumes,
        output_root: None,
        seen: Mutex::new(vec![]),
    })
}
#[test]
fn api_job_admission_binds_policy_before_queue_visibility_and_rejects_atomically() {
    let (directory, core, _) = deletion_core("queued");
    let archive = directory.path().join("new.zip");
    fs::write(&archive, b"synthetic zip").unwrap();
    let source = SourceGroup {
        path: paths::path_string(&archive),
        display_name: "new".into(),
        volumes: vec![],
        size: 0,
        missing_volume: false,
    };
    let before = core.repository.jobs().unwrap().len();
    assert!(
        core.add_jobs_authorized(
            vec![source.clone()],
            Settings::default(),
            execution_policy(0)
        )
        .is_err()
    );
    assert_eq!(core.repository.jobs().unwrap().len(), before);
    assert_eq!(core.jobs.lock().unwrap().len(), before);
    let added = core
        .add_jobs_authorized(vec![source], Settings::default(), execution_policy(1))
        .unwrap();
    assert_eq!(added.len(), 1);
    assert!(core.execution_policy(&added[0].id).unwrap().is_some());
    assert_eq!(core.get_job(&added[0].id).unwrap().status, "queued");
}
#[test]
fn api_multi_source_authorization_rejection_admits_no_part_of_the_batch() {
    let directory = tempfile::tempdir().unwrap();
    let core = Coordinator::open(
        directory.path().join("data"),
        directory.path().join("missing-worker.exe"),
        directory.path().join("missing-7z.dll"),
    )
    .unwrap();
    let first = directory.path().join("a-first.zip");
    let second = directory.path().join("z-second.zip.001");
    let last = directory.path().join("z-second.zip.002");
    for path in [&first, &second, &last] {
        fs::write(path, b"synthetic archive member").unwrap();
    }
    let sources: Vec<_> = [&first, &second]
        .into_iter()
        .map(|path| SourceGroup {
            path: paths::path_string(path),
            display_name: "synthetic".into(),
            volumes: vec![],
            size: 0,
            missing_volume: false,
        })
        .collect();
    let policy = execution_policy(1);
    assert!(
        core.add_jobs_authorized(sources.clone(), Settings::default(), policy.clone())
            .is_err()
    );
    assert_eq!(
        *policy.seen.lock().unwrap(),
        vec![1, 2],
        "the second real source must exercise the denied-volume branch"
    );
    assert!(core.repository.jobs().unwrap().is_empty());
    assert!(core.jobs.lock().unwrap().is_empty());
    assert!(core.execution_policies.lock().unwrap().is_empty());
    assert!(!core.running.load(Ordering::Acquire));
    let added = core
        .add_jobs_authorized(sources.clone(), Settings::default(), execution_policy(2))
        .unwrap();
    assert_eq!(added.len(), 2);
    assert!(
        core.add_jobs_authorized(sources, Settings::default(), execution_policy(2))
            .unwrap()
            .is_empty(),
        "active duplicate sources retain their prior skip behavior"
    );
    assert_eq!(core.repository.jobs().unwrap().len(), 2);
}
#[test]
fn api_retry_authorizes_current_settings_before_any_mutation() {
    let (_directory, core, job) = deletion_core("failed");
    core.settings.lock().unwrap().output_root = "current-output".into();
    let policy = Arc::new(TestExecutionPolicy {
        max_volumes: 1,
        output_root: Some("old-output".into()),
        seen: Mutex::new(vec![]),
    });
    let before = serde_json::to_value(core.get_job(&job.id).unwrap()).unwrap();
    assert!(
        core.retry_authorized(&job.id, false, Some("synthetic-password".into()), policy)
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(core.get_job(&job.id).unwrap()).unwrap(),
        before
    );
    assert!(core.ephemeral_passwords.lock().unwrap().is_empty());
    assert!(core.execution_policy(&job.id).unwrap().is_none());
    core.retry_authorized(&job.id, false, None, execution_policy(1))
        .unwrap();
    let retry = core.get_job(&job.id).unwrap();
    assert_eq!(retry.options.output_root, "current-output");
    assert_eq!(retry.status, "queued");
    assert!(core.execution_policy(&job.id).unwrap().is_some());
}
#[tokio::test(flavor = "current_thread")]
async fn api_start_denial_leaves_all_existing_queue_state_and_policies_unchanged() {
    let (_directory, core, first) = deletion_core("queued");
    fs::write(&core.worker.executable, b"not executed by this test").unwrap();
    fs::write(&core.worker.engine, b"synthetic engine identity only").unwrap();
    let mut second = first.clone();
    second.id = "unapproved-job".into();
    second.source.volumes.push("unapproved-volume".into());
    core.repository.save_job(&second).unwrap();
    core.jobs.lock().unwrap().push(second.clone());
    assert!(core.start_authorized(execution_policy(1)).is_err());
    assert!(!core.running.load(Ordering::Acquire));
    assert!(core.scheduler_scope.lock().unwrap().is_none());
    assert!(core.execution_policies.lock().unwrap().is_empty());
    assert_eq!(core.get_job(&first.id).unwrap().status, "queued");
    assert_eq!(core.get_job(&second.id).unwrap().status, "queued");
}
#[tokio::test(flavor = "current_thread")]
async fn api_start_freezes_ids_and_a_trusted_gui_start_restores_regular_scheduling() {
    let (_directory, core, first) = deletion_core("queued");
    fs::write(&core.worker.executable, b"not executed by this test").unwrap();
    fs::write(&core.worker.engine, b"synthetic engine identity only").unwrap();
    core.start_authorized(execution_policy(1)).unwrap();
    let mut later = first.clone();
    later.id = "later-gui-job".into();
    core.repository.save_job(&later).unwrap();
    core.jobs.lock().unwrap().push(later.clone());
    let scope = core.scheduler_scope.lock().unwrap().clone().unwrap();
    assert!(scope.contains(&first.id));
    assert!(!scope.contains(&later.id));
    core.start().unwrap();
    assert!(core.scheduler_scope.lock().unwrap().is_none());
    assert!(core.execution_policies.lock().unwrap().is_empty());
    core.stop_batch.store(true, Ordering::Release);
}
#[tokio::test(flavor = "current_thread")]
async fn api_start_cannot_unpause_a_running_gui_queue() {
    let (_directory, core, _) = deletion_core("queued");
    fs::write(&core.worker.executable, b"not executed by this test").unwrap();
    fs::write(&core.worker.engine, b"synthetic engine identity only").unwrap();
    core.running.store(true, Ordering::Release);
    core.paused.store(true, Ordering::Release);
    assert!(core.start_authorized(execution_policy(1)).is_err());
    assert!(core.paused.load(Ordering::Acquire));
    assert!(core.scheduler_scope.lock().unwrap().is_none());
}
#[tokio::test(flavor = "current_thread")]
async fn api_selected_start_rejects_invalid_or_denied_selection_before_mutation() {
    let (_directory, core, first) = deletion_core("queued");
    fs::write(&core.worker.executable, b"not executed by this test").unwrap();
    fs::write(&core.worker.engine, b"synthetic engine identity only").unwrap();
    let mut denied = first.clone();
    denied.id = "unapproved-volume-job".into();
    denied.source.volumes.push("unapproved-volume".into());
    core.jobs.lock().unwrap().push(denied.clone());
    core.put(&denied).unwrap();
    let mut completed = first.clone();
    completed.id = "completed-job".into();
    completed.status = "completed".into();
    core.jobs.lock().unwrap().push(completed.clone());
    core.put(&completed).unwrap();
    let mut running = first.clone();
    running.id = "already-running-job".into();
    running.status = "running".into();
    core.jobs.lock().unwrap().push(running.clone());
    core.put(&running).unwrap();
    for ids in [
        vec![],
        vec![String::new()],
        vec![" ".into()],
        vec![first.id.clone(), first.id.clone()],
        vec![first.id.clone(), "missing-job".into()],
        vec![first.id.clone(), completed.id.clone()],
        vec![first.id.clone(), running.id.clone()],
        vec![first.id.clone(), denied.id.clone()],
        (0..1001).map(|index| format!("job-{index}")).collect(),
    ] {
        let before = serde_json::to_value(core.repository.jobs().unwrap()).unwrap();
        assert!(
            core.start_selected_authorized(&ids, execution_policy(1))
                .is_err()
        );
        assert!(!core.running.load(Ordering::Acquire));
        assert!(core.scheduler_scope.lock().unwrap().is_none());
        assert!(core.execution_policies.lock().unwrap().is_empty());
        assert_eq!(
            serde_json::to_value(core.repository.jobs().unwrap()).unwrap(),
            before
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn api_selected_start_and_resource_requeue_leave_unselected_gui_jobs_untouched() {
    let (_directory, core, first) = deletion_core("queued");
    fs::write(&core.worker.executable, b"not executed by this test").unwrap();
    fs::write(&core.worker.engine, b"synthetic engine identity only").unwrap();
    let mut outside = first.clone();
    outside.id = "outside-gui-job".into();
    outside.status = "waitingResource".into();
    outside.stage = "extracting".into();
    outside.message = "untouched GUI resource wait".into();
    outside
        .source
        .volumes
        .push("outside-unapproved-volume".into());
    core.jobs.lock().unwrap().push(outside.clone());
    core.put(&outside).unwrap();
    let mut selected_waiter = first.clone();
    selected_waiter.id = "selected-waiter".into();
    selected_waiter.status = "waitingResource".into();
    core.jobs.lock().unwrap().push(selected_waiter.clone());
    core.put(&selected_waiter).unwrap();
    let before = serde_json::to_value(&outside).unwrap();
    core.start_selected_authorized(
        &[first.id.clone(), selected_waiter.id.clone()],
        execution_policy(1),
    )
    .unwrap();
    assert_eq!(core.get_job(&selected_waiter.id).unwrap().status, "queued");
    assert_eq!(
        serde_json::to_value(core.get_job(&outside.id).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        core.scheduler_scope.lock().unwrap().as_ref().unwrap().len(),
        2
    );
    assert!(core.execution_policy(&outside.id).unwrap().is_none());
    assert!(!core.requeue_resource_waiters().unwrap());
    assert_eq!(
        serde_json::to_value(core.get_job(&outside.id).unwrap()).unwrap(),
        before
    );
    assert_ne!(core.claim_next_job().unwrap().unwrap().0.id, outside.id);
    core.stop_batch.store(true, Ordering::Release);
}
#[test]
fn api_add_and_retry_reject_active_gui_before_admitting_or_changing_jobs() {
    let (directory, core, job) = deletion_core("failed");
    fs::write(&core.worker.executable, b"not executed by this test").unwrap();
    fs::write(&core.worker.engine, b"synthetic engine identity only").unwrap();
    core.running.store(true, Ordering::Release);
    assert!(
        core.start_selected_authorized(std::slice::from_ref(&job.id), execution_policy(1))
            .is_err()
    );
    let archive = directory.path().join("api-new.zip");
    fs::write(&archive, b"synthetic archive").unwrap();
    let source = SourceGroup {
        path: paths::path_string(&archive),
        display_name: "api-new".into(),
        volumes: vec![],
        size: 0,
        missing_volume: false,
    };
    assert!(
        core.add_jobs_authorized(vec![source], Settings::default(), execution_policy(1))
            .is_err()
    );
    assert!(
        core.retry_preserving_options_authorized(
            &job.id,
            false,
            Some("synthetic-secret".into()),
            execution_policy(1),
        )
        .is_err()
    );
    assert_eq!(core.jobs.lock().unwrap().len(), 1);
    assert_eq!(core.get_job(&job.id).unwrap().status, "failed");
    assert!(core.execution_policies.lock().unwrap().is_empty());
    assert!(core.ephemeral_passwords.lock().unwrap().is_empty());
    assert!(core.scheduler_scope.lock().unwrap().is_none());
}
#[tokio::test(flavor = "current_thread")]
async fn api_operations_cannot_resume_or_admit_into_an_idle_paused_queue() {
    let (directory, core, job) = deletion_core("failed");
    fs::write(&core.worker.executable, b"not executed by this test").unwrap();
    fs::write(&core.worker.engine, b"synthetic engine identity only").unwrap();
    core.paused.store(true, Ordering::Release);
    assert!(
        core.start_selected_authorized(std::slice::from_ref(&job.id), execution_policy(1))
            .unwrap_err()
            .contains("暂停")
    );
    assert!(
        core.retry_preserving_options_authorized(&job.id, false, None, execution_policy(1))
            .unwrap_err()
            .contains("暂停")
    );
    let archive = directory.path().join("api-new.zip");
    fs::write(&archive, b"synthetic archive").unwrap();
    let source = SourceGroup {
        path: paths::path_string(&archive),
        display_name: "api-new".into(),
        volumes: vec![],
        size: 0,
        missing_volume: false,
    };
    assert!(
        core.add_jobs_authorized(vec![source], Settings::default(), execution_policy(1))
            .unwrap_err()
            .contains("暂停")
    );
    assert!(core.paused.load(Ordering::Acquire));
    assert!(!core.running.load(Ordering::Acquire));
    assert_eq!(core.get_job(&job.id).unwrap().status, "failed");
    assert_eq!(core.jobs.lock().unwrap().len(), 1);
    assert!(core.execution_policies.lock().unwrap().is_empty());
    core.start().unwrap();
    assert!(
        !core.paused.load(Ordering::Acquire),
        "local window keeps explicit resume authority"
    );
    core.stop_batch.store(true, Ordering::Release);
}
#[test]
fn api_preserving_retry_uses_frozen_settings_and_cancel_finishes_resource_waiter() {
    let (_directory, core, mut job) = deletion_core("needsPassword");
    job.options.output_root = "original-output".into();
    job.options.password_group = "original-group".into();
    core.put(&job).unwrap();
    core.settings.lock().unwrap().output_root = "gui-output".into();
    let policy = Arc::new(TestExecutionPolicy {
        max_volumes: 1,
        output_root: Some("original-output".into()),
        seen: Mutex::new(vec![]),
    });
    core.retry_preserving_options_authorized(
        &job.id,
        false,
        Some("synthetic-secret".into()),
        policy,
    )
    .unwrap();
    let mut retry = core.get_job(&job.id).unwrap();
    assert_eq!(
        serde_json::to_value(&retry.options).unwrap(),
        serde_json::to_value(&job.options).unwrap()
    );
    assert_eq!(core.settings.lock().unwrap().output_root, "gui-output");
    assert_eq!(core.ephemeral_passwords.lock().unwrap()[&job.id].len(), 1);
    retry.status = "waitingResource".into();
    core.put(&retry).unwrap();
    core.cancel(Some(&job.id)).unwrap();
    let cancelled = core.get_job(&job.id).unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert!(cancelled.finished_at.is_some());
    assert_eq!(core.repository.jobs().unwrap()[0].status, "cancelled");
}
#[test]
fn api_preserving_node_retry_keeps_frozen_options_and_selects_only_that_node() {
    let (_directory, core, mut job) = deletion_core("needsPassword");
    job.options.output_root = "original-output".into();
    job.options.password_group = "original-group".into();
    let mut inner = job.nodes[0].clone();
    inner.id = "failed-inner".into();
    inner.depth = 1;
    inner.status = "needsPassword".into();
    job.nodes.push(inner.clone());
    core.put(&job).unwrap();
    core.settings.lock().unwrap().output_root = "gui-output".into();
    let policy = Arc::new(TestExecutionPolicy {
        max_volumes: 1,
        output_root: Some("original-output".into()),
        seen: Mutex::new(vec![]),
    });
    core.retry_node_preserving_options_authorized(&job.id, &inner.id, policy)
        .unwrap();
    let retry = core.get_job(&job.id).unwrap();
    assert_eq!(
        serde_json::to_value(&retry.options).unwrap(),
        serde_json::to_value(&job.options).unwrap()
    );
    assert_eq!(
        core.node_retry_requests.lock().unwrap()[&job.id],
        HashSet::from([inner.id])
    );
    assert_eq!(core.settings.lock().unwrap().output_root, "gui-output");
}
#[tokio::test(flavor = "current_thread")]
async fn api_scope_expansion_and_idle_handoff_cannot_lose_a_new_start() {
    let (_directory, core, first) = deletion_core("queued");
    fs::write(&core.worker.executable, b"not executed by this test").unwrap();
    fs::write(&core.worker.engine, b"synthetic engine identity only").unwrap();
    core.running.store(true, Ordering::Release);
    *core.scheduler_scope.lock().unwrap() = Some(HashSet::new());
    let observed = core.revision.load(Ordering::Acquire);
    let mut old = BatchRunGuard {
        core: core.clone(),
        completed: false,
    };
    core.start_selected_authorized(std::slice::from_ref(&first.id), execution_policy(1))
        .unwrap();
    assert!(!core.finish_idle_batch(observed, &mut old));
    assert!(core.running.load(Ordering::Acquire));
    let mut later = first.clone();
    later.id = "new-selected-job".into();
    core.jobs.lock().unwrap().push(later.clone());
    core.put(&later).unwrap();
    core.start_selected_authorized(std::slice::from_ref(&later.id), execution_policy(1))
        .unwrap();
    assert_eq!(
        core.scheduler_scope.lock().unwrap().as_ref().unwrap().len(),
        2
    );
    // An out-of-scope queued GUI item must not keep a paused API run alive.
    *core.scheduler_scope.lock().unwrap() = Some(HashSet::new());
    core.paused.store(true, Ordering::Release);
    assert!(core.finish_idle_batch(core.revision.load(Ordering::Acquire), &mut old));
    assert!(!core.running.load(Ordering::Acquire));
    core.paused.store(false, Ordering::Release);
    core.start_selected_authorized(std::slice::from_ref(&later.id), execution_policy(1))
        .unwrap();
    drop(old);
    assert!(
        core.running.load(Ordering::Acquire),
        "old completed guard clobbered the new driver"
    );
    assert!(core.claim_next_job().unwrap().is_some());
    core.stop_batch.store(true, Ordering::Release);
}
#[tokio::test]
async fn api_run_job_rechecks_actual_sibling_volumes_before_workspace_writes() {
    let (directory, core, mut job) = deletion_core("queued");
    let first = directory.path().join("sample.zip.001");
    let second = directory.path().join("sample.zip.002");
    fs::write(&first, b"first synthetic volume").unwrap();
    fs::write(&second, b"second synthetic volume").unwrap();
    job.source.path = paths::path_string(&first.canonicalize().unwrap());
    job.source.volumes = vec![job.source.path.clone()];
    let output = directory.path().join("output");
    fs::create_dir(&output).unwrap();
    job.options.output_root = paths::path_string(&output);
    let policy = execution_policy(1);
    core.execution_policies
        .lock()
        .unwrap()
        .insert(job.id.clone(), policy.clone());
    let result = core
        .run_job(&mut job, Arc::new(AtomicBool::new(false)), false, None)
        .await;
    assert_eq!(result.unwrap_err(), "ApiAuthorizationDenied");
    assert!(policy.seen.lock().unwrap().contains(&2));
    assert!(job.workspace.is_none());
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    assert_eq!(fs::read(&first).unwrap(), b"first synthetic volume");
    assert_eq!(fs::read(&second).unwrap(), b"second synthetic volume");
}
#[tokio::test]
async fn api_inspection_rechecks_discovered_volumes_before_worker_or_scratch_writes() {
    struct ReadOne(PathBuf);
    impl JobExecutionPolicy for ReadOne {
        fn authorize(&self, _: &JobView) -> Result<(), String> {
            Ok(())
        }
        fn authorize_path(&self, path: &Path, write: bool) -> Result<(), String> {
            if !write && path == self.0.as_path() {
                Ok(())
            } else {
                Err("synthetic volume not authorized".into())
            }
        }
    }
    let (directory, core, _) = deletion_core("queued");
    fs::write(&core.worker.executable, b"not executed by this test").unwrap();
    fs::write(&core.worker.engine, b"synthetic engine identity only").unwrap();
    let first = directory.path().join("inspect.zip.001");
    let second = directory.path().join("inspect.zip.002");
    fs::write(&first, b"synthetic first").unwrap();
    fs::write(&second, b"synthetic second").unwrap();
    let policy = Arc::new(ReadOne(first.canonicalize().unwrap()));
    let result = core
        .inspect_archive_authorized(
            &paths::path_string(&first),
            None,
            "api-test-inspection".into(),
            policy,
        )
        .await;
    assert_eq!(result.unwrap_err(), "synthetic volume not authorized");
    assert!(!fs::read_dir(&core.data).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("inspect-")
    }));
    assert_eq!(fs::read(&first).unwrap(), b"synthetic first");
    assert_eq!(fs::read(&second).unwrap(), b"synthetic second");
}

#[test]
fn cache_policy_defaults_are_compatible_and_invalid_policy_is_rejected() {
    let settings: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.cache_policy, "auto");
    assert!(validate_settings(&settings).is_ok());
    let mut forced = settings.clone();
    forced.cache_policy = "always".into();
    assert!(validate_settings(&forced).is_ok());
    assert!(!extraction_compatible(&settings, &forced));
    forced.cache_policy = "unrecognised".into();
    assert!(validate_settings(&forced).is_err());
}

#[test]
fn resumed_storage_uses_directory_identity_and_rejects_unregistered_parent() {
    let (directory, _core, job) = telemetry_core();
    let settings = Settings {
        cache_enabled: true,
        cache_root: paths::path_string(&directory.path().join("cache")),
        output_root: paths::path_string(&directory.path().join("old-output")),
        ..Settings::default()
    };
    let cache = Path::new(&settings.cache_root).join("OpenUnpack-cache");
    let direct = Path::new(&settings.output_root).join(".openunpack-work");
    let foreign = directory.path().join("foreign");
    for parent in [&cache, &direct, &foreign] {
        fs::create_dir_all(parent).unwrap();
    }
    paths::mark_owned(&cache.join("job"), &job.id, "").unwrap();
    let _held_workspace = lock_private_workspace(&cache.join("job")).unwrap();
    let target = directory.path().join("new-output");
    let cached_work = PathBuf::from(paths::path_string(&cache.join("job")).to_uppercase());
    let (route, _) =
        resumed_storage_route(&settings, &job.source, &cached_work, target.clone()).unwrap();
    assert!(matches!(route, ExtractionRoute::SsdCache { target_dir, .. } if target_dir == target));
    let (route, parent) =
        resumed_storage_route(&settings, &job.source, &direct.join("job"), target.clone()).unwrap();
    assert!(matches!(route, ExtractionRoute::Direct { target_dir, .. } if target_dir == target));
    assert_eq!(parent, direct);
    assert!(resumed_storage_route(&settings, &job.source, &foreign.join("job"), target).is_err());
}

#[test]
fn automatic_route_does_not_treat_nested_preference_as_detected_contents() {
    let (directory, core, mut job) = telemetry_core();
    let cache = directory.path().join("cache");
    fs::create_dir(&cache).unwrap();
    job.source.path = paths::path_string(&directory.path().join("archive.zip"));
    job.source.size = 1024 * 1024;
    let mut settings = Settings {
        cache_enabled: true,
        cache_root: paths::path_string(&cache),
        output_root: paths::path_string(directory.path()),
        min_free_bytes: 0,
        expand_nested: true,
        ..Settings::default()
    };
    assert!(matches!(
        core.decide_route(&settings, &job.source).unwrap(),
        ExtractionRoute::Direct { .. }
    ));
    settings.cache_policy = "always".into();
    assert!(matches!(
        core.decide_route(&settings, &job.source).unwrap(),
        ExtractionRoute::SsdCache { .. }
    ));
}

#[test]
fn load_settings_accept_five_active_jobs_and_reject_six() {
    let mut settings = Settings::default();
    assert_eq!(settings.concurrency, 1);
    settings.concurrency = 5;
    assert!(validate_settings(&settings).is_ok());
    settings.concurrency = 6;
    assert!(validate_settings(&settings).is_err());
}

#[test]
fn load_auto_threads_share_logical_cores_across_configured_jobs() {
    let logical_cores = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    for concurrency in [1, 2, 3, 5] {
        let settings = Settings {
            concurrency,
            ..Settings::default()
        };
        let expected = (logical_cores / concurrency as usize).clamp(1, 64) as u32;
        for operation in ["extract", "inspect", "probe", "copy", "readback"] {
            let request = request(operation, Path::new(r"C:\synthetic-output"), &settings);
            assert_eq!(
                request.engine_threads, expected,
                "{operation}/{concurrency}"
            );
        }
    }
    let settings = Settings {
        concurrency: 5,
        engine_threads: 7,
        ..Settings::default()
    };
    assert_eq!(
        request("extract", Path::new(r"C:\synthetic-output"), &settings).engine_threads,
        7,
        "an explicit thread count must not be divided"
    );
}

#[test]
fn load_auto_threads_keep_a_nonzero_bounded_share_on_small_machines() {
    for (cores, concurrency, expected) in [
        (0, 5, 1),
        (1, 5, 1),
        (2, 5, 1),
        (6, 1, 6),
        (6, 2, 3),
        (6, 3, 2),
        (6, 5, 1),
        (16, 5, 3),
        (192, 1, 64),
    ] {
        assert_eq!(per_task_engine_threads(cores, concurrency), expected);
    }
}

#[test]
fn test_hdd_seek_storm_auto_threads_throttling() {
    // When is_hdd is true and threads are 0 (auto), it must clamp to 2 threads
    assert_eq!(resolve_engine_threads_with_topology(0, 1, true), 2);
    assert_eq!(resolve_engine_threads_with_topology(0, 8, true), 2);
    // Explicit user choice must be respected even on HDD
    assert_eq!(resolve_engine_threads_with_topology(4, 1, true), 4);
    assert_eq!(resolve_engine_threads_with_topology(1, 1, true), 1);
    // On SSD (is_hdd = false), normal core-sharing applies
    let auto_ssd = resolve_engine_threads_with_topology(0, 1, false);
    assert!(auto_ssd >= 1);
}

#[test]
fn test_deduplicate_hardlinks_setting_and_candidate_dispatch() {
    use crate::dedup::{self, FileLinker};
    use std::sync::Mutex;

    // 1. Settings default must have deduplicate_hardlinks enabled
    let settings = Settings::default();
    assert!(settings.deduplicate_hardlinks);

    // 2. Test candidate dispatch with CopyFile records
    struct TestLinker(Mutex<Vec<(String, String)>>);
    impl FileLinker for TestLinker {
        fn replace_with_hardlink(&self, primary: &Path, duplicate: &Path) -> std::io::Result<bool> {
            self.0.lock().unwrap().push((
                primary.to_string_lossy().replace('\\', "/"),
                duplicate.to_string_lossy().replace('\\', "/"),
            ));
            Ok(true)
        }
    }

    let linker = TestLinker(Mutex::new(Vec::new()));
    let root = Path::new("C:/dest/folder");
    let sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    let copy_files = vec![
        CopyFile {
            source_path: "C:/cache/dup1.dll".into(),
            relative_path: "bin/dup1.dll".into(),
            size: 1048576,
            size_known: true,
            sha256: sha.into(),
            is_directory: false,
            is_corrupted: false,
        },
        CopyFile {
            source_path: "C:/cache/dup2.dll".into(),
            relative_path: "plugin/dup2.dll".into(),
            size: 1048576,
            size_known: true,
            sha256: sha.into(),
            is_directory: false,
            is_corrupted: false,
        },
        CopyFile {
            source_path: "C:/cache/unique.dll".into(),
            relative_path: "bin/unique.dll".into(),
            size: 2097152,
            size_known: true,
            sha256: "f".repeat(64),
            is_directory: false,
            is_corrupted: false,
        },
    ];

    let report =
        dedup::deduplicate_extracted_files_with_linker(root, &copy_files, &linker).unwrap();
    assert_eq!(report.scanned_files, 3);
    assert_eq!(report.duplicate_files, 1);
    assert_eq!(report.hardlinked_files, 1);
    assert_eq!(report.saved_bytes, 1048576);

    let links = linker.0.lock().unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].0, "C:/dest/folder/bin/dup1.dll");
    assert_eq!(links[0].1, "C:/dest/folder/plugin/dup2.dll");
}

#[test]
fn load_claim_counts_each_job_once_when_source_output_and_cache_share_an_ssd() {
    fn topology(_: &Path, _: &Path) -> crate::route::DiskTopology {
        crate::route::DiskTopology {
            source_is_hdd: false,
            target_is_hdd: false,
            is_same_physical_disk: true,
            source_device_number: Some(7),
            target_device_number: Some(7),
        }
    }
    let (_directory, core, mut active) = telemetry_core();
    *core.topology_probe.lock().unwrap() = Some(topology);
    core.settings.lock().unwrap().concurrency = 5;
    active.source.path = r"A:\archives\active.zip".into();
    active.options.output_root = r"D:\output".into();
    active.options.cache_enabled = true;
    active.options.cache_root = r"S:\cache".into();
    active.workspace = None;
    active.stage = "checking".into();
    core.put(&active).unwrap();
    for index in 1..4 {
        let mut peer = active.clone();
        peer.id = format!("ssd-peer-{index}");
        core.jobs.lock().unwrap().push(peer.clone());
        core.repository.save_job(&peer).unwrap();
    }
    let mut fifth = active;
    fifth.id = "ssd-fifth".into();
    fifth.status = "queued".into();
    core.jobs.lock().unwrap().push(fifth.clone());
    core.repository.save_job(&fifth).unwrap();
    assert_eq!(core.claim_next_job().unwrap().unwrap().0.id, fifth.id);
}

#[test]
fn load_pause_waits_for_in_progress_claim_before_returning() {
    let (_directory, core, _) = telemetry_core();
    let claim_gate = core.mutations.lock().unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let pausing = core.clone();
    let thread = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        finished_tx.send(pausing.pause()).unwrap();
    });
    started_rx.recv().unwrap();
    let early = finished_rx.recv_timeout(std::time::Duration::from_millis(200));
    drop(claim_gate);
    let returned_while_claim_was_open = early.is_ok();
    let result = match early {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => finished_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap(),
        Err(error) => panic!("pause thread disconnected: {error}"),
    };
    thread.join().unwrap();
    result.unwrap();
    assert!(core.paused.load(Ordering::Acquire));
    assert!(core.claim_next_job().unwrap().is_none());
    assert!(
        !returned_while_claim_was_open,
        "pause must share the claim gate so no older claim can publish after pause returns"
    );
}

fn assert_load_shared_cache_is_serialized(topology: TopologyProbe) {
    for stage in ["checking", "extracting", "copying"] {
        let (_directory, core, mut active) = telemetry_core();
        *core.topology_probe.lock().unwrap() = Some(topology);
        core.settings.lock().unwrap().concurrency = 3;
        active.source.path = r"A:\archives\active.zip".into();
        active.workspace = (stage != "checking").then(|| r"H:\cache\active".into());
        active.options.output_root = r"D:\output".into();
        active.options.cache_enabled = true;
        active.options.cache_root = r"H:\cache".into();
        active.stage = stage.into();
        core.put(&active).unwrap();
        let mut blocked = telemetry_job("cache-blocked");
        blocked.status = "queued".into();
        blocked.source.path = r"B:\archives\blocked.zip".into();
        blocked.options.output_root = r"E:\output".into();
        blocked.options.cache_enabled = true;
        blocked.options.cache_root = r"H:\cache".into();
        core.jobs.lock().unwrap().push(blocked.clone());
        core.repository.save_job(&blocked).unwrap();
        assert!(
            core.claim_next_job().unwrap().is_none(),
            "{stage}: candidate must respect the shared serialized cache endpoint"
        );
        let mut independent = telemetry_job("cache-independent");
        independent.status = "queued".into();
        independent.source.path = r"F:\archives\independent.zip".into();
        independent.options.output_root = r"G:\output".into();
        independent.options.cache_enabled = true;
        independent.options.cache_root = r"S:\cache".into();
        core.jobs.lock().unwrap().push(independent.clone());
        core.repository.save_job(&independent).unwrap();
        assert_eq!(core.claim_next_job().unwrap().unwrap().0.id, independent.id);
    }
}

fn load_cache_topology(
    source: &Path,
    target: &Path,
    unknown_cache: bool,
) -> crate::route::DiskTopology {
    let source_drive = paths::drive_letter_of(source).unwrap();
    let target_drive = paths::drive_letter_of(target).unwrap();
    let device = |drive: char| (!(unknown_cache && drive == 'H')).then_some(drive as u32);
    crate::route::DiskTopology {
        source_is_hdd: source_drive == 'H',
        target_is_hdd: target_drive == 'H',
        is_same_physical_disk: source_drive == target_drive,
        source_device_number: device(source_drive),
        target_device_number: device(target_drive),
    }
}

#[test]
fn load_claim_serializes_shared_hdd_cache_before_workspace_exists() {
    fn topology(source: &Path, target: &Path) -> crate::route::DiskTopology {
        load_cache_topology(source, target, false)
    }
    assert_load_shared_cache_is_serialized(topology);
}

#[test]
fn load_claim_serializes_shared_unknown_cache_before_workspace_exists() {
    fn topology(source: &Path, target: &Path) -> crate::route::DiskTopology {
        load_cache_topology(source, target, true)
    }
    assert_load_shared_cache_is_serialized(topology);
}

#[tokio::test]
async fn workspace_identity_is_pinned_between_separate_worker_requests() {
    let fixture = tempfile::tempdir().unwrap();
    let parent = fixture.path().join("shared-parent");
    fs::create_dir(&parent).unwrap();
    let work = parent.join("owned-work");
    paths::mark_owned(&work, "job", "manifest").unwrap();
    let lease = lock_private_workspace(&work).unwrap();
    assert!(fs::rename(&parent, fixture.path().join("moved-parent")).is_err());
    tokio::task::yield_now().await;
    assert!(fs::rename(&work, parent.join("replaced-work")).is_err());
    drop(lease);
    fs::rename(&parent, fixture.path().join("moved-parent")).unwrap();
}

#[test]
fn cached_writeback_is_contention_after_phase_reservation_releases() {
    let (fixture, core, mut publisher) = telemetry_core();
    let cache = fixture.path().join("cache");
    fs::create_dir(&cache).unwrap();
    let work = cache.join("publisher-work");
    paths::mark_owned(&work, &publisher.id, "synthetic").unwrap();
    fs::write(work.join("retained.bin"), vec![0; 6 * 1024 * 1024]).unwrap();
    let settings = Settings {
        cache_enabled: true,
        cache_root: paths::path_string(&cache),
        cache_quota_bytes: 8 * 1024 * 1024,
        max_expanded_bytes: 64 * 1024 * 1024,
        min_free_bytes: 0,
        ..Settings::default()
    };
    let route = ExtractionRoute::SsdCache {
        cache_workspace: cache.join("work"),
        target_dir: fixture.path().join("output"),
        score: 50,
        reason: "synthetic".into(),
    };
    publisher.workspace = Some(paths::path_string(&work));
    publisher.options = settings.clone();
    publisher.stage = "copying".into();
    core.put(&publisher).unwrap();
    core.active_cancel
        .lock()
        .unwrap()
        .insert(publisher.id.clone(), Arc::new(AtomicBool::new(false)));
    // No phase reservation remains, but the running task will remove its
    // verified cache files after delivery. This is temporary occupancy.
    assert_eq!(core.cache_budget.reserved_bytes(), 0);
    assert_eq!(
        core.claim_budget_for_job(
            "waiting-job",
            &settings,
            &route,
            &cache,
            Some(4 * 1024 * 1024)
        )
        .err()
        .unwrap(),
        "CacheQuotaContention"
    );
    // Retained data with no live owner is a permanent capacity limitation.
    core.active_cancel.lock().unwrap().remove(&publisher.id);
    assert_eq!(
        core.claim_budget_for_job(
            "waiting-job",
            &settings,
            &route,
            &cache,
            Some(4 * 1024 * 1024)
        )
        .err()
        .unwrap(),
        "ResourceLimit"
    );
    // An individually oversized request still fails while another owner runs.
    core.active_cancel
        .lock()
        .unwrap()
        .insert(publisher.id.clone(), Arc::new(AtomicBool::new(false)));
    assert_eq!(
        core.claim_budget_for_job(
            "waiting-job",
            &settings,
            &route,
            &cache,
            Some(9 * 1024 * 1024)
        )
        .err()
        .unwrap(),
        "ResourceLimit"
    );
}

#[tokio::test]
async fn s06_readback_accepts_windows_parent_directory_case_aliases() {
    use sha2::{Digest, Sha256};
    let fixture = tempfile::tempdir().unwrap();
    let target = fixture.path().join("target");
    paths::mark_owned(&target, "publication", "manifest").unwrap();
    fs::create_dir(target.join("Folder")).unwrap();
    fs::write(target.join("Folder/a.txt"), b"case alias").unwrap();
    let files = vec![
        CopyFile {
            relative_path: "Folder".into(),
            is_directory: true,
            ..CopyFile::default()
        },
        CopyFile {
            relative_path: "folder/a.txt".into(),
            size: 10,
            sha256: hex::encode(Sha256::digest(b"case alias")),
            ..CopyFile::default()
        },
    ];
    lock_validated_publication(
        &target,
        "publication",
        "manifest",
        &files,
        true,
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
}

#[test]
fn s06_source_cleanup_rejects_replacements_before_deleting_any_volume() {
    let fixture = tempfile::tempdir().unwrap();
    let one = fixture.path().join("one.zip");
    let two = fixture.path().join("two.zip");
    fs::write(&one, b"first volume").unwrap();
    fs::write(&two, b"second volume").unwrap();
    let source = SourceGroup {
        path: paths::path_string(&one),
        display_name: "split.zip".into(),
        volumes: vec![paths::path_string(&one), paths::path_string(&two)],
        size: 25,
        missing_volume: false,
    };
    let expected = source_witnesses(&source).unwrap();
    fs::rename(&two, fixture.path().join("original-two.zip")).unwrap();
    fs::write(&two, b"unrelated replacement").unwrap();
    assert!(lock_cleanup_sources(&source, &expected, false).is_err());
    assert_eq!(fs::read(&one).unwrap(), b"first volume");
    assert_eq!(fs::read(&two).unwrap(), b"unrelated replacement");
}

#[test]
fn s06_source_cleanup_holds_verified_objects_until_same_handle_deletion() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("source.zip");
    fs::write(&path, b"original bytes").unwrap();
    let source = SourceGroup {
        path: paths::path_string(&path),
        display_name: "source.zip".into(),
        volumes: vec![paths::path_string(&path)],
        size: 14,
        missing_volume: false,
    };
    let expected = source_witnesses(&source).unwrap();
    let entries = lock_cleanup_sources(&source, &expected, false).unwrap();
    assert!(fs::rename(&path, fixture.path().join("replaced.zip")).is_err());
    assert!(fs::write(&path, b"modified").is_err());
    entries[0].cleanup().unwrap();
    drop(entries);
    assert!(!path.exists());
}

#[tokio::test]
async fn s06_publication_readback_keeps_identity_and_detects_hash_mismatch() {
    use sha2::{Digest, Sha256};
    let fixture = tempfile::tempdir().unwrap();
    let target = fixture.path().join("target");
    paths::mark_owned(&target, "publication", "manifest").unwrap();
    fs::write(target.join("data.txt"), b"published bytes").unwrap();
    let files = vec![CopyFile {
        relative_path: "data.txt".into(),
        size: 15,
        sha256: hex::encode(Sha256::digest(b"published bytes")),
        ..CopyFile::default()
    }];
    let cancel = Arc::new(AtomicBool::new(false));
    let tree = lock_validated_publication(
        &target,
        "publication",
        "manifest",
        &files,
        true,
        cancel.clone(),
    )
    .await
    .unwrap();
    assert!(fs::rename(&target, fixture.path().join("replacement")).is_err());
    assert!(fs::write(target.join("data.txt"), b"attacker").is_err());
    tree.verify_owned("publication", Some("manifest")).unwrap();
    let mut invalid = files.clone();
    invalid[0].sha256 = "0".repeat(64);
    assert_eq!(
        validate_locked_target(&tree, &target, &invalid, true, &cancel).unwrap_err(),
        "HashMismatch"
    );
    cancel.store(true, Ordering::Release);
    assert_eq!(
        validate_locked_target(&tree, &target, &files, true, &cancel).unwrap_err(),
        "Cancelled"
    );
}
fn telemetry_job(id: &str) -> JobView {
    JobView {
        id: id.into(),
        source: SourceGroup {
            path: "synthetic.zip".into(),
            display_name: "synthetic.zip".into(),
            volumes: vec![],
            size: 1000,
            missing_volume: false,
        },
        options: Settings::default(),
        status: "running".into(),
        stage: "extracting".into(),
        progress: 0.,
        message: String::new(),
        attempts: 2,
        output_path: None,
        verification_scope: String::new(),
        created_at: now(),
        finished_at: None,
        workspace: None,
        stage_times: BTreeMap::new(),
        transfer_progress: None,
        nodes: vec![NodeView {
            id: "root-node".into(),
            parent_id: None,
            name: "root".into(),
            depth: 0,
            status: "running".into(),
            stage: "extracting".into(),
            progress: 0.,
            attempts: 2,
            message: String::new(),
            password_label: None,
        }],
    }
}
fn telemetry_core() -> (tempfile::TempDir, Arc<Coordinator>, JobView) {
    let directory = tempfile::tempdir().unwrap();
    let core = Coordinator::open(
        directory.path().join("data"),
        directory.path().join("missing-worker.exe"),
        directory.path().join("missing-7z.dll"),
    )
    .unwrap();
    let job = telemetry_job("job-one");
    core.update_visible_state(|| {
        core.jobs.lock().unwrap().push(job.clone());
        Ok(())
    })
    .unwrap();
    core.put(&job).unwrap();
    (directory, core, job)
}
fn telemetry_request(job: &JobView) -> WorkerRequest {
    let mut req = request("extract", Path::new("synthetic-output"), &job.options);
    req.generation = Some(job.attempts.max(1));
    req
}
fn telemetry_event(identity: &TransferIdentity, completed: u64) -> WorkerEvent {
    let mut event = WorkerEvent::default();
    event.protocol_version = identity.protocol_version;
    event.request_id = identity.request_id.clone();
    event.operation_id = Some(identity.operation_id.clone());
    event.generation = Some(identity.generation);
    event.kind = "progress".into();
    event.completed_bytes = completed;
    event.total_bytes = 3000;
    event
}

fn deletion_core(status: &str) -> (tempfile::TempDir, Arc<Coordinator>, JobView) {
    let (directory, core, mut job) = telemetry_core();
    let source = directory.path().join("original.zip");
    fs::write(&source, b"synthetic original archive").unwrap();
    job.source.path = paths::path_string(&source);
    job.source.volumes = vec![job.source.path.clone()];
    job.status = status.into();
    core.put(&job).unwrap();
    (directory, core, job)
}

#[test]
fn queue_snapshot_bounds_plain_history_and_preserves_all_attention_tasks() {
    let (_directory, core, original) = deletion_core("completed");
    let mut jobs = Vec::new();
    for index in 0..100 {
        let mut job = original.clone();
        job.id = format!("history-{index:03}");
        job.created_at = "2026-10-15T00:00:00Z".into();
        jobs.push(job);
    }
    for (id, status, workspace) in [
        ("live-running", "running", None),
        ("live-queued", "queued", None),
        ("live-paused", "paused", None),
        ("live-waiting", "waitingResource", None),
        ("live-password", "needsPassword", None),
        ("old-interrupted", "interrupted", None),
        ("old-publish", "publishPending", None),
        ("old-cleanup", "cleanupPending", None),
        ("old-failure", "failed", Some("retained-workspace")),
        ("old-cancelled", "cancelled", Some("retained-workspace")),
    ] {
        let mut job = original.clone();
        job.id = id.into();
        job.status = status.into();
        job.workspace = workspace.map(String::from);
        job.created_at = "2000-01-01".into();
        jobs.push(job);
    }
    *core.jobs.lock().unwrap() = jobs;
    let full = core.snapshot().unwrap();
    let queue = core.queue_snapshot().unwrap();
    assert_eq!(full.jobs.len(), 110);
    assert_eq!(queue.jobs.len(), 30);
    assert_eq!(queue.recoverable_count, full.recoverable_count);
    assert_eq!(queue.recoverable_count, 3);
    assert!(!queue.jobs.iter().any(|job| job.id == "history-079"));
    assert!(queue.jobs.iter().any(|job| job.id == "history-080"));
    assert!(queue.jobs.iter().any(|job| job.id == "old-cleanup"));
    assert!(queue.jobs.iter().any(|job| job.id == "old-failure"));
    assert!(queue.jobs.iter().any(|job| job.id == "old-cancelled"));
    assert_eq!(
        queue
            .jobs
            .iter()
            .find(|job| job.id == "live-running")
            .unwrap()
            .nodes,
        original.nodes
    );
    assert_eq!(
        core.job_details("history-000").unwrap().unwrap().nodes,
        original.nodes
    );
    assert!(core.job_details("missing").unwrap().is_none());
    assert_eq!(core.history().unwrap().len(), 107);
}

#[test]
fn history_page_is_stable_bounded_filtered_and_checks_hidden_workspace() {
    let (_directory, core, original) = deletion_core("completed");
    let mut jobs = Vec::new();
    for index in 0..24 {
        let mut job = original.clone();
        job.id = format!("row-{index:03}");
        job.created_at = "2026-10-15T00:00:00Z".into();
        job.source.display_name = format!("Archive-{index:03}.ZIP");
        jobs.push(job);
    }
    jobs[0].workspace = Some("hidden-preserved-workspace".into());
    jobs[0].status = "failed".into();
    *core.jobs.lock().unwrap() = jobs;
    core.settings.lock().unwrap().history_limit = 20;
    let query = HistoryPageQuery {
        offset: 1000,
        limit: 10,
        status: "all".into(),
        search: String::new(),
    };
    let page = core.history_page(query.clone()).unwrap();
    assert_eq!(page.offset, 10);
    assert_eq!(page.items.len(), 10);
    assert_eq!(page.total, 20);
    assert_eq!(page.completed, 20);
    assert_eq!(page.items[0].id, "row-013");
    assert!(
        !page.clear_allowed,
        "a protected record outside the display limit must block clear"
    );
    let json = serde_json::to_value(&page).unwrap();
    assert!(json["items"][0].get("nodes").is_none());
    assert!(json["items"][0].get("options").is_none());
    assert!(json["revision"].is_string());
    let mut search = query.clone();
    search.search = "archive-023.zip".into();
    let found = core.history_page(search).unwrap();
    assert_eq!(found.filtered_total, 1);
    assert_eq!(found.offset, 0);
    assert_eq!(found.items[0].id, "row-023");
    for limit in [0, 101] {
        let mut invalid = query.clone();
        invalid.limit = limit;
        assert!(core.history_page(invalid).is_err());
    }
    let mut invalid = query;
    invalid.search = "a".repeat(4097);
    assert!(core.history_page(invalid).is_err());
}

#[test]
fn queue_snapshot_projection_keeps_persisted_history_and_restart_recovery() {
    let (directory, core, original) = deletion_core("completed");
    let mut jobs = Vec::new();
    for index in 0..25 {
        let mut job = original.clone();
        job.id = format!("durable-{index:03}");
        job.status = if index == 0 { "running" } else { "completed" }.into();
        core.repository.save_job(&job).unwrap();
        jobs.push(job);
    }
    core.repository.remove_job(&original.id).unwrap();
    *core.jobs.lock().unwrap() = jobs;
    assert_eq!(core.queue_snapshot().unwrap().jobs.len(), 21);
    assert_eq!(core.repository.jobs().unwrap().len(), 25);
    let data = core.data.clone();
    drop(core);
    let reopened = Coordinator::open(
        data,
        directory.path().join("missing-worker.exe"),
        directory.path().join("missing-engine.dll"),
    )
    .unwrap();
    assert_eq!(reopened.snapshot().unwrap().jobs.len(), 25);
    assert_eq!(
        reopened.job_details("durable-000").unwrap().unwrap().status,
        "interrupted"
    );
    assert!(
        reopened
            .queue_snapshot()
            .unwrap()
            .jobs
            .iter()
            .any(|job| job.id == "durable-000")
    );
}

#[tokio::test]
async fn delete_queued_is_durable_idempotent_and_keeps_source_and_vault() {
    let (directory, core, job) = deletion_core("queued");
    let password = core
        .vault()
        .unwrap()
        .upsert(PasswordInput {
            id: None,
            name: "synthetic saved entry".into(),
            secret: Some("fixture-only".into()),
            groups: vec![],
            notes: String::new(),
            enabled: true,
        })
        .unwrap();
    let vault_before = fs::read(core.data.join("passwords.vault")).unwrap();
    let revision = core.snapshot().unwrap().revision;
    core.delete_task(&job.id).await.unwrap();
    core.delete_task(&job.id).await.unwrap();
    assert!(core.snapshot().unwrap().jobs.is_empty());
    assert!(core.history().unwrap().is_empty());
    assert!(core.snapshot().unwrap().revision > revision);
    assert_eq!(
        fs::read(&job.source.path).unwrap(),
        b"synthetic original archive"
    );
    assert_eq!(
        fs::read(core.data.join("passwords.vault")).unwrap(),
        vault_before
    );
    assert_eq!(core.vault().unwrap().views().unwrap()[0].id, password.id);
    let data = core.data.clone();
    drop(core);
    let reopened = Coordinator::open(
        data,
        directory.path().join("missing-worker.exe"),
        directory.path().join("missing-7z.dll"),
    )
    .unwrap();
    assert!(reopened.snapshot().unwrap().jobs.is_empty());
}

#[tokio::test]
async fn delete_cleans_owned_failed_cache_and_keeps_committed_outputs() {
    for status in [
        "failed",
        "needsPassword",
        "cleanupPending",
        "completed",
        "partial",
    ] {
        let (directory, core, mut job) = deletion_core(status);
        let work = directory.path().join("owned-cache");
        paths::mark_owned(&work, &job.id, "").unwrap();
        fs::write(work.join("temporary.txt"), b"synthetic temporary content").unwrap();
        job.workspace = Some(paths::path_string(&work));
        let output = directory.path().join("published-output");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("keep.txt"), b"synthetic published content").unwrap();
        job.output_path = Some(paths::path_string(&output));
        core.put(&job).unwrap();
        core.delete_task(&job.id).await.unwrap();
        assert!(!work.exists(), "{status}");
        assert_eq!(
            fs::read(output.join("keep.txt")).unwrap(),
            b"synthetic published content"
        );
        assert_eq!(
            fs::read(&job.source.path).unwrap(),
            b"synthetic original archive"
        );
        assert!(core.repository.jobs().unwrap().is_empty());
    }
}

#[tokio::test]
async fn delete_rejects_locked_or_unowned_cache_and_remains_retryable() {
    use std::os::windows::fs::OpenOptionsExt;
    let (directory, core, mut job) = deletion_core("failed");
    let work = directory.path().join("owned-cache");
    paths::mark_owned(&work, &job.id, "").unwrap();
    let retained = work.join("locked.bin");
    fs::write(&retained, b"synthetic locked cache").unwrap();
    job.workspace = Some(paths::path_string(&work));
    core.put(&job).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&retained)
        .unwrap();
    assert!(core.delete_task(&job.id).await.is_err());
    assert!(core.get_job(&job.id).is_ok());
    assert!(work.join(paths::OWNER_FILE).exists());
    assert!(!core.deleting_jobs.lock().unwrap().contains(&job.id));
    drop(lock);
    let original_marker = fs::read(work.join(paths::OWNER_FILE)).unwrap();
    fs::write(
        work.join(paths::OWNER_FILE),
        br#"{"application":"OpenUnpack/1","id":"another-task","manifest":""}"#,
    )
    .unwrap();
    assert!(core.delete_task(&job.id).await.is_err());
    assert!(retained.exists());
    fs::write(work.join(paths::OWNER_FILE), original_marker).unwrap();
    core.delete_task(&job.id).await.unwrap();
    assert!(!work.exists());
    assert!(core.repository.jobs().unwrap().is_empty());
}

#[tokio::test]
async fn delete_storage_failure_after_cache_cleanup_retains_recovery_record() {
    let (directory, core, mut job) = deletion_core("failed");
    let work = directory.path().join("owned-cache");
    paths::mark_owned(&work, &job.id, "").unwrap();
    fs::write(work.join("file.bin"), b"synthetic cached content").unwrap();
    job.workspace = Some(paths::path_string(&work));
    core.put(&job).unwrap();
    let connection = rusqlite::Connection::open(core.data.join("jobs.sqlite")).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_cleanup_save BEFORE UPDATE ON jobs BEGIN SELECT RAISE(FAIL, 'synthetic storage failure'); END;").unwrap();
    assert_eq!(core.delete_task(&job.id).await.unwrap_err(), "StorageError");
    assert!(!work.exists());
    assert_eq!(core.repository.jobs().unwrap()[0].workspace, job.workspace);
    assert!(core.get_job(&job.id).is_ok());
    connection
        .execute_batch("DROP TRIGGER fail_cleanup_save;")
        .unwrap();
    core.delete_task(&job.id).await.unwrap();
    assert!(core.repository.jobs().unwrap().is_empty());
}

#[tokio::test]
async fn delete_record_failure_after_cleanup_retains_partial_or_salvaged_quality() {
    for (scope, node_status, expected_status) in [
        ("archiveCrcAndStreamSha256", "needsPassword", "partial"),
        ("salvagedWithCrcWarning", "completed", "salvaged"),
    ] {
        let (directory, core, mut job) = deletion_core("cleanupPending");
        let work = directory.path().join("owned-cache");
        paths::mark_owned(&work, &job.id, "").unwrap();
        job.workspace = Some(paths::path_string(&work));
        let output = directory.path().join("published-partial");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("keep.txt"), b"synthetic partial result").unwrap();
        job.output_path = Some(paths::path_string(&output));
        job.nodes[0].status = node_status.into();
        job.verification_scope = scope.into();
        core.put(&job).unwrap();
        let connection = rusqlite::Connection::open(core.data.join("jobs.sqlite")).unwrap();
        connection.execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON jobs BEGIN SELECT RAISE(FAIL, 'synthetic delete failure'); END;").unwrap();
        assert_eq!(core.delete_task(&job.id).await.unwrap_err(), "StorageError");
        let retained = core.get_job(&job.id).unwrap();
        assert_eq!(retained.status, expected_status);
        assert!(retained.workspace.is_none());
        assert_eq!(core.repository.jobs().unwrap()[0].status, expected_status);
        assert_eq!(
            fs::read(output.join("keep.txt")).unwrap(),
            b"synthetic partial result"
        );
        connection
            .execute_batch("DROP TRIGGER fail_delete;")
            .unwrap();
        core.delete_task(&job.id).await.unwrap();
        assert!(core.repository.jobs().unwrap().is_empty());
        assert!(output.join("keep.txt").exists());
    }
}

#[tokio::test]
async fn delete_keeps_unresolved_publication_and_unconfirmed_worker_records() {
    let (directory, core, mut job) = deletion_core("failed");
    let work = directory.path().join("owned-cache");
    paths::mark_owned(&work, &job.id, "").unwrap();
    job.workspace = Some(paths::path_string(&work));
    core.put(&job).unwrap();
    let publication = Publication {
        id: "publication-one".into(),
        job_id: job.id.clone(),
        temporary_path: paths::path_string(&directory.path().join("publish-temp")),
        final_path: paths::path_string(&directory.path().join("publish-final")),
        manifest_hash: "a".repeat(64),
        state: "ready".into(),
        files: vec![],
        verification: "synthetic fixture".into(),
    };
    core.repository.save_publication(&publication).unwrap();
    assert!(
        core.delete_task(&job.id)
            .await
            .unwrap_err()
            .contains("发布结果尚未确认")
    );
    assert!(work.exists());
    assert_eq!(core.repository.publications().unwrap().len(), 1);
    let mut abandoned = publication;
    abandoned.state = "abandoned".into();
    core.repository.save_publication(&abandoned).unwrap();
    fs::write(work.join(UNCONFIRMED_FILE), br#"{"processes":[]}"#).unwrap();
    assert!(
        core.delete_task(&job.id)
            .await
            .unwrap_err()
            .contains("退出尚未确认")
    );
    assert!(
        core.repository
            .jobs()
            .unwrap()
            .iter()
            .any(|saved| saved.id == job.id)
    );
    fs::remove_file(work.join(UNCONFIRMED_FILE)).unwrap();
    job.status = "stopping".into();
    core.put(&job).unwrap();
    assert!(
        core.delete_task(&job.id)
            .await
            .unwrap_err()
            .contains("退出尚未确认")
    );
    job.status = "failed".into();
    core.put(&job).unwrap();
    core.delete_task(&job.id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_waits_complete_claim_owner_and_late_put_cannot_revive_it() {
    let (_directory, core, queued) = deletion_core("queued");
    let (mut claimed, cancel, _, _) = core.claim_next_job().unwrap().unwrap();
    assert_eq!(claimed.id, queued.id);
    let deleting = core.clone();
    let id = claimed.id.clone();
    let operation = tokio::spawn(async move { deleting.delete_task(&id).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !cancel.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!operation.is_finished());
    let snapshot = core.snapshot().unwrap();
    assert_eq!(snapshot.jobs[0].status, "running");
    // The final writer remains owned even after its terminal state is visible.
    claimed.status = "cancelled".into();
    core.put(&claimed).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(!operation.is_finished());
    core.active_cancel.lock().unwrap().remove(&claimed.id);
    core.changed();
    tokio::time::timeout(std::time::Duration::from_secs(3), operation)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(core.put(&claimed).is_err());
    assert!(
        core.begin_transfer(&claimed, &telemetry_request(&claimed), Instant::now())
            .is_err()
    );
    assert!(core.transfers.lock().unwrap().is_empty());
    assert!(core.repository.jobs().unwrap().is_empty());
    assert!(core.snapshot().unwrap().jobs.is_empty());
}

#[test]
fn delete_claim_uses_id_after_removing_prior_queue_rows() {
    let (_directory, core, first) = deletion_core("queued");
    for id in ["second", "third"] {
        let mut job = first.clone();
        job.id = id.into();
        core.jobs.lock().unwrap().push(job.clone());
        core.put(&job).unwrap();
    }
    core.remove(&first.id).unwrap();
    let (claimed, _, _, _) = core.claim_next_job().unwrap().unwrap();
    assert_eq!(claimed.id, "second");
    assert_eq!(core.get_job("second").unwrap().status, "running");
    assert_eq!(core.get_job("third").unwrap().status, "queued");
    assert!(
        !core
            .repository
            .jobs()
            .unwrap()
            .iter()
            .any(|job| job.id == first.id)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_guard_keeps_detached_cleanup_owned_until_blocking_work_finishes() {
    let (_directory, core, job) = deletion_core("failed");
    let intent = core.begin_delete(&job.id).unwrap().unwrap();
    let blocking_intent = intent.clone();
    let operation_core = core.clone();
    let id = job.id.clone();
    let (entered_send, entered_recv) = std::sync::mpsc::channel();
    let (release_send, release_recv) = std::sync::mpsc::channel();
    let blocking = tokio::task::spawn_blocking(move || {
        let _intent = blocking_intent;
        entered_send.send(()).unwrap();
        release_recv.recv().unwrap();
        operation_core.finish_delete(&id).unwrap()
    });
    entered_recv.recv().unwrap();
    drop(intent); // Simulate the awaiting caller being dropped/aborted.
    assert!(core.retry(&job.id, false).unwrap_err().contains("正在删除"));
    assert!(
        core.recover(&job.id, "cleanup")
            .await
            .unwrap_err()
            .contains("正在删除")
    );
    assert!(
        core.snapshot()
            .unwrap()
            .jobs
            .iter()
            .any(|saved| saved.id == job.id)
    );
    release_send.send(()).unwrap();
    assert!(blocking.await.unwrap());
    assert!(!core.deleting_jobs.lock().unwrap().contains(&job.id));
    assert!(core.repository.jobs().unwrap().is_empty());
}

#[tokio::test]
async fn delete_rejects_full_recovery_owner_and_recovery_releases_owner_on_error() {
    let (_directory, core, job) = deletion_core("failed");
    core.recovering_jobs.lock().unwrap().insert(job.id.clone());
    let owner = RecoveryOwner {
        core: core.clone(),
        job_id: job.id.clone(),
        completed: true,
    };
    assert!(
        core.delete_task(&job.id)
            .await
            .unwrap_err()
            .contains("恢复操作完成")
    );
    assert!(
        core.recover(&job.id, "cleanup")
            .await
            .unwrap_err()
            .contains("恢复操作完成")
    );
    drop(owner);
    assert!(core.recover(&job.id, "invalid-operation").await.is_err());
    assert!(!core.recovering_jobs.lock().unwrap().contains(&job.id));
    assert!(!core.snapshot().unwrap().running);
    core.delete_task(&job.id).await.unwrap();
}

#[tokio::test]
async fn delete_cannot_enter_recovery_cancelled_before_exit_confirmation() {
    let (_directory, core, job) = deletion_core("failed");
    core.recovering_jobs.lock().unwrap().insert(job.id.clone());
    core.running.store(true, Ordering::Release);
    drop(RecoveryOwner {
        core: core.clone(),
        job_id: job.id.clone(),
        completed: false,
    });
    assert!(core.recovering_jobs.lock().unwrap().contains(&job.id));
    assert_eq!(core.get_job(&job.id).unwrap().status, "stopping");
    assert_eq!(core.repository.jobs().unwrap()[0].status, "stopping");
    assert!(core.delete_task(&job.id).await.is_err());
    assert!(core.recover(&job.id, "cleanup").await.is_err());
    assert!(core.snapshot().unwrap().running);
}

#[test]
fn transfer_snapshot_is_versioned_transient_and_keeps_active_speed_while_paused() {
    use std::time::Duration;
    let (_directory, core, job) = telemetry_core();
    let started = Instant::now();
    let owner = core
        .begin_transfer(&job, &telemetry_request(&job), started)
        .unwrap();
    core.observe_transfer(
        &job.id,
        &owner.identity,
        &telemetry_event(&owner.identity, 0),
        (1000, Some(5000)),
        None,
        started,
    );
    let revision = core.snapshot().unwrap().revision;
    core.pause().unwrap();
    core.observe_transfer(
        &job.id,
        &owner.identity,
        &telemetry_event(&owner.identity, 500),
        (1000, Some(5000)),
        None,
        started + Duration::from_millis(500),
    );
    let snapshot = core.snapshot().unwrap();
    assert!(snapshot.paused && snapshot.revision > revision);
    let observation = snapshot.jobs[0].transfer_progress.as_ref().unwrap();
    assert_eq!(observation.completed_bytes, 1500);
    assert_eq!(observation.total_bytes, 5000);
    assert_eq!(observation.bytes_per_second, Some(1000.0));
    assert_eq!(observation.remaining_seconds, Some(3.5));
    assert!(chrono::DateTime::parse_from_rfc3339(&observation.updated_at).is_ok());
    let same = core.snapshot().unwrap();
    assert_eq!(snapshot.revision, same.revision);
    assert_eq!(
        snapshot.jobs[0].transfer_progress,
        same.jobs[0].transfer_progress
    );
    assert!(
        core.repository.jobs().unwrap()[0]
            .transfer_progress
            .is_none()
    );
    drop(owner);
    let finished = core.snapshot().unwrap();
    assert!(finished.revision > snapshot.revision);
    assert!(finished.jobs[0].transfer_progress.is_none());
}

#[test]
fn transfer_rejects_old_event_identity_and_old_owner_cannot_clear_new_request() {
    let (_directory, core, job) = telemetry_core();
    let started = Instant::now();
    let old = core
        .begin_transfer(&job, &telemetry_request(&job), started)
        .unwrap();
    let event = telemetry_event(&old.identity, 100);
    for wrong in 0..5 {
        let mut invalid = event.clone();
        match wrong {
            0 => invalid.request_id = "old-request".into(),
            1 => invalid.operation_id = Some("old-operation".into()),
            2 => invalid.generation = Some(1),
            3 => invalid.kind = "heartbeat".into(),
            _ => invalid.protocol_version = 1,
        }
        let revision = core.snapshot().unwrap().revision;
        core.observe_transfer(&job.id, &old.identity, &invalid, (0, None), None, started);
        assert_eq!(revision, core.snapshot().unwrap().revision);
    }
    let new = core
        .begin_transfer(&job, &telemetry_request(&job), started)
        .unwrap();
    let revision = core.snapshot().unwrap().revision;
    core.observe_transfer(&job.id, &old.identity, &event, (0, None), None, started);
    drop(old);
    assert_eq!(revision, core.snapshot().unwrap().revision);
    assert_eq!(
        core.transfers.lock().unwrap()[&job.id].identity,
        new.identity
    );
    core.observe_transfer(
        &job.id,
        &new.identity,
        &telemetry_event(&new.identity, 200),
        (0, None),
        None,
        started,
    );
    assert_eq!(
        core.snapshot().unwrap().jobs[0]
            .transfer_progress
            .as_ref()
            .unwrap()
            .completed_bytes,
        200
    );
}

#[test]
fn transfer_status_phase_generation_and_node_transitions_clear_observations() {
    let (_directory, core, job) = telemetry_core();
    for transition in [
        "stage",
        "generation",
        "node",
        "completed",
        "failed",
        "cancelled",
        "needsPassword",
        "waitingResource",
        "publishPending",
        "stopping",
        "queued",
    ] {
        core.put(&job).unwrap();
        let started = Instant::now();
        let owner = core
            .begin_transfer(&job, &telemetry_request(&job), started)
            .unwrap();
        core.observe_transfer(
            &job.id,
            &owner.identity,
            &telemetry_event(&owner.identity, 100),
            (0, None),
            None,
            started,
        );
        let mut next = job.clone();
        match transition {
            "stage" => next.stage = "nested".into(),
            "generation" => next.attempts += 1,
            "node" => next.nodes[0].id = "different-inner-node".into(),
            "publishPending" => {
                next.status = transition.into();
                next.stage = "planning".into();
            }
            status => next.status = status.into(),
        }
        core.put(&next).unwrap();
        assert!(
            core.snapshot().unwrap().jobs[0].transfer_progress.is_none(),
            "{transition}"
        );
        let revision = core.snapshot().unwrap().revision;
        core.observe_transfer(
            &job.id,
            &owner.identity,
            &telemetry_event(&owner.identity, 200),
            (0, None),
            None,
            started,
        );
        assert_eq!(
            revision,
            core.snapshot().unwrap().revision,
            "late sample after {transition}"
        );
    }
}

#[test]
fn cache_progress_sums_independent_requests_and_ignores_stale_identity() {
    let (_directory, core, first) = telemetry_core();
    let second = telemetry_job("cache-job-two");
    core.update_visible_state(|| {
        core.jobs.lock().unwrap().push(second.clone());
        Ok(())
    })
    .unwrap();
    let started = Instant::now();
    let first_owner = core
        .begin_transfer(&first, &telemetry_request(&first), started)
        .unwrap();
    let second_owner = core
        .begin_transfer(&second, &telemetry_request(&second), started)
        .unwrap();
    for (job, owner, base, bytes) in [
        (&first, &first_owner, 1000, 100),
        (&second, &second_owner, 2000, 50),
        (&first, &first_owner, 1000, 120),
    ] {
        core.observe_transfer(
            &job.id,
            &owner.identity,
            &telemetry_event(&owner.identity, bytes),
            (0, None),
            Some(base),
            started,
        );
    }
    assert_eq!(core.snapshot().unwrap().cache_used_bytes, 3170);
    let mut stale = telemetry_event(&first_owner.identity, 9999);
    stale.operation_id = Some("retired-cache-operation".into());
    core.observe_transfer(
        &first.id,
        &first_owner.identity,
        &stale,
        (0, None),
        Some(1000),
        started,
    );
    assert_eq!(core.snapshot().unwrap().cache_used_bytes, 3170);
    drop(second_owner);
    assert_eq!(core.snapshot().unwrap().cache_used_bytes, 3170);
    let next_owner = core
        .begin_transfer(&first, &telemetry_request(&first), started)
        .unwrap();
    drop(first_owner);
    core.observe_transfer(
        &first.id,
        &next_owner.identity,
        &telemetry_event(&next_owner.identity, 10),
        (0, None),
        Some(1120),
        started,
    );
    assert_eq!(core.snapshot().unwrap().cache_used_bytes, 3180);
}

#[test]
fn cache_reconcile_does_not_overwrite_a_request_started_during_scan() {
    let (directory, core, mut job) = telemetry_core();
    let workspace = directory.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    job.workspace = Some(paths::path_string(&workspace));
    core.put(&job).unwrap();
    let started = Instant::now();
    core.reconcile_usage_with(Some(&job.id), |_| {
        let owner = core
            .begin_transfer(&job, &telemetry_request(&job), started)
            .unwrap();
        core.observe_transfer(
            &job.id,
            &owner.identity,
            &telemetry_event(&owner.identity, 70),
            (0, None),
            Some(100),
            started,
        );
        drop(owner);
        Ok(10)
    })
    .unwrap();
    assert_eq!(
        core.snapshot().unwrap().cache_used_bytes,
        170,
        "a stale scan must not replace newer request progress, even after it exits"
    );
    core.reconcile_usage_with(Some(&job.id), |_| Ok(180))
        .unwrap();
    assert_eq!(core.snapshot().unwrap().cache_used_bytes, 180);
    core.reconcile_usage_with(Some(&job.id), |_| {
        let owner = core
            .begin_transfer(&job, &telemetry_request(&job), started)
            .unwrap();
        drop(owner);
        Err("IoError".into())
    })
    .unwrap();
    assert_eq!(
        core.snapshot().unwrap().cache_used_bytes,
        180,
        "a scan invalidated by a writer must not fail an unrelated completed phase"
    );
    assert_eq!(
        core.reconcile_usage_with(Some(&job.id), |_| Err("UnsafePath".into()))
            .unwrap_err(),
        "UnsafePath",
        "current idle workspace errors remain visible"
    );
}

#[test]
fn cache_reconcile_keeps_aggregate_consistent_when_current_scan_fails() {
    let (directory, core, mut first) = telemetry_core();
    first.workspace = Some(paths::path_string(&directory.path().join("first")));
    core.put(&first).unwrap();
    let mut second = telemetry_job("cache-scan-failure-peer");
    second.workspace = Some(paths::path_string(&directory.path().join("second")));
    core.jobs.lock().unwrap().push(second.clone());
    core.put(&second).unwrap();
    core.reconcile_usage_with(None, |_| Ok(100)).unwrap();
    assert_eq!(core.snapshot().unwrap().cache_used_bytes, 200);
    // Deletion removes the job row before requesting a global refresh.
    core.update_visible_state(|| {
        core.jobs.lock().unwrap().retain(|job| job.id != first.id);
        Ok(())
    })
    .unwrap();
    let revision = core.snapshot().unwrap().revision;
    assert_eq!(
        core.reconcile_usage_with(None, |_| Err("UnsafePath".into()))
            .unwrap_err(),
        "UnsafePath"
    );
    assert_eq!(core.cache_usage_by_job.lock().unwrap().len(), 1);
    assert_eq!(
        core.snapshot().unwrap().cache_used_bytes,
        100,
        "a retained scan error must not leave removed workspaces in the aggregate"
    );
    assert_ne!(
        core.snapshot().unwrap().revision,
        revision,
        "the changed aggregate must be observable to snapshot subscribers on error too"
    );
}

#[test]
fn cache_reconcile_scans_selected_idle_workspace_and_retains_active_peer() {
    let (directory, core, mut first) = telemetry_core();
    first.workspace = Some(paths::path_string(&directory.path().join("first")));
    core.put(&first).unwrap();
    let mut second = telemetry_job("second-cache-job");
    second.workspace = Some(paths::path_string(&directory.path().join("second")));
    core.jobs.lock().unwrap().push(second.clone());
    core.put(&second).unwrap();
    let started = Instant::now();
    let owner = core
        .begin_transfer(&second, &telemetry_request(&second), started)
        .unwrap();
    core.observe_transfer(
        &second.id,
        &owner.identity,
        &telemetry_event(&owner.identity, 50),
        (0, None),
        Some(200),
        started,
    );
    let mut scanned = Vec::new();
    core.reconcile_usage_with(Some(&first.id), |path| {
        scanned.push(paths::path_string(path));
        Ok(120)
    })
    .unwrap();
    assert_eq!(scanned, vec![first.workspace.clone().unwrap()]);
    assert_eq!(core.snapshot().unwrap().cache_used_bytes, 370);
    drop(owner);
    second.workspace = None;
    core.put(&second).unwrap();
    assert_eq!(core.snapshot().unwrap().cache_used_bytes, 120);
    core.reconcile_usage_with(None, |_| Ok(125)).unwrap();
    assert_eq!(core.snapshot().unwrap().cache_used_bytes, 125);
}

#[test]
fn transfer_jobs_are_independent_and_probe_has_no_measured_metrics() {
    let (_directory, core, first) = telemetry_core();
    let mut second = telemetry_job("job-two");
    second.stage = "nested".into();
    core.update_visible_state(|| {
        core.jobs.lock().unwrap().push(second.clone());
        Ok(())
    })
    .unwrap();
    let started = Instant::now();
    let first_owner = core
        .begin_transfer(&first, &telemetry_request(&first), started)
        .unwrap();
    let second_owner = core
        .begin_transfer(&second, &telemetry_request(&second), started)
        .unwrap();
    for (job, owner, bytes) in [(&first, &first_owner, 100), (&second, &second_owner, 200)] {
        core.observe_transfer(
            &job.id,
            &owner.identity,
            &telemetry_event(&owner.identity, bytes),
            (0, None),
            None,
            started,
        );
    }
    drop(first_owner);
    let snapshot = core.snapshot().unwrap();
    assert!(snapshot.jobs[0].transfer_progress.is_none());
    assert_eq!(
        snapshot.jobs[1]
            .transfer_progress
            .as_ref()
            .unwrap()
            .completed_bytes,
        200
    );
    drop(second_owner);
    let mut probe = telemetry_request(&first);
    probe.operation = "probe".into();
    let owner = core.begin_transfer(&first, &probe, started).unwrap();
    core.observe_transfer(
        &first.id,
        &owner.identity,
        &telemetry_event(&owner.identity, 100),
        (0, None),
        None,
        started,
    );
    assert!(core.snapshot().unwrap().jobs[0].transfer_progress.is_none());
}

#[tokio::test]
async fn transfer_request_failure_and_unwind_leave_no_runtime_observations() {
    let (_directory, core, mut job) = telemetry_core();
    let req = telemetry_request(&job);
    assert!(
        core.request_once(&mut job, req, Arc::new(AtomicBool::new(false)), 0, None)
            .await
            .is_err()
    );
    assert!(core.transfers.lock().unwrap().is_empty());
    assert!(core.snapshot().unwrap().jobs[0].transfer_progress.is_none());
    let owner = core
        .begin_transfer(&job, &telemetry_request(&job), Instant::now())
        .unwrap();
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _owner = owner;
        panic!("synthetic operation unwind");
    }));
    assert!(unwind.is_err());
    assert!(core.transfers.lock().unwrap().is_empty());
}

#[test]
fn transfer_restart_drops_legacy_observations_for_every_loaded_status() {
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("data");
    fs::create_dir_all(&data).unwrap();
    let repository = Repository::open(&data.join("jobs.sqlite")).unwrap();
    for status in ["running", "queued", "completed", "needsPassword"] {
        let mut job = telemetry_job(status);
        job.status = status.into();
        job.transfer_progress = Some(TransferProgress {
            completed_bytes: 100,
            total_bytes: 1000,
            bytes_per_second: Some(100.),
            remaining_seconds: Some(9.),
            updated_at: now(),
        });
        repository.save_job(&job).unwrap();
    }
    drop(repository);
    let core = Coordinator::open(
        data,
        directory.path().join("missing-worker.exe"),
        directory.path().join("missing-7z.dll"),
    )
    .unwrap();
    assert!(
        core.snapshot()
            .unwrap()
            .jobs
            .iter()
            .all(|job| job.transfer_progress.is_none())
    );
}

#[test]
fn snapshot_waits_for_visible_update_and_its_revision() {
    use std::sync::mpsc;
    use std::time::Duration;
    let directory = tempfile::tempdir().unwrap();
    let core = Coordinator::open(
        directory.path().join("data"),
        directory.path().join("worker.exe"),
        directory.path().join("7z.dll"),
    )
    .unwrap();
    let prior = core.snapshot().unwrap().revision;
    let writer = core.clone();
    let (halfway_send, halfway_recv) = mpsc::channel();
    let (release_send, release_recv) = mpsc::channel();
    let update = std::thread::spawn(move || {
        writer
            .update_visible_state(|| {
                writer.paused.store(true, Ordering::Release);
                halfway_send.send(()).unwrap();
                release_recv.recv().unwrap();
                writer.settings.lock().unwrap().password_group = "one visible revision".into();
                Ok(())
            })
            .unwrap()
    });
    halfway_recv.recv().unwrap();
    let reader = core.clone();
    let (snapshot_send, snapshot_recv) = mpsc::channel();
    let read = std::thread::spawn(move || snapshot_send.send(reader.snapshot().unwrap()).unwrap());
    let blocked = snapshot_recv.recv_timeout(Duration::from_millis(50));
    release_send.send(()).unwrap();
    update.join().unwrap();
    read.join().unwrap();
    assert!(
        matches!(blocked, Err(mpsc::RecvTimeoutError::Timeout)),
        "a snapshot cannot publish the half-written visible state"
    );
    let snapshot = snapshot_recv.recv().unwrap();
    assert!(snapshot.paused);
    assert_eq!(snapshot.settings.password_group, "one visible revision");
    assert_eq!(snapshot.revision, prior + 1);
}
#[test]
fn test_strip_single_root_wrapper() {
    let f = |p: &str, d: bool| CopyFile {
        size_known: true,
        source_path: String::new(),
        relative_path: p.into(),
        size: 10,
        sha256: "a".repeat(64),
        is_directory: d,
        is_corrupted: false,
    };
    // Case 1: Redundant single wrapper folder "content/..."
    let mut files1 = vec![
        f("content", true),
        f("content/a.txt", false),
        f("content/sub/b.txt", false),
    ];
    strip_single_root_wrapper(&mut files1);
    assert_eq!(files1.len(), 2);
    assert_eq!(files1[0].relative_path, "a.txt");
    assert_eq!(files1[1].relative_path, "sub\\b.txt");

    // Case 2: Root already has multiple files/folders, should NOT unwrap
    let mut files2 = vec![f("file1.txt", false), f("folder/file2.txt", false)];
    strip_single_root_wrapper(&mut files2);
    assert_eq!(files2.len(), 2);
    assert_eq!(files2[0].relative_path, "file1.txt");

    // Case 3: Multiple folders at root
    let mut files3 = vec![f("folderA/file1.txt", false), f("folderB/file2.txt", false)];
    strip_single_root_wrapper(&mut files3);
    assert_eq!(files3.len(), 2);
    assert_eq!(files3[0].relative_path, "folderA/file1.txt");

    // Case 4: OS noise (.DS_Store, __MACOSX) does not block unwrapping payload
    let mut files4 = vec![
        f(".DS_Store", false),
        f("__MACOSX", true),
        f("__MACOSX/._payload", false),
        f("payload", true),
        f("payload/file1.txt", false),
        f("payload/sub/file2.txt", false),
    ];
    strip_single_root_wrapper(&mut files4);
    assert_eq!(files4.len(), 2);
    assert_eq!(files4[0].relative_path, "file1.txt");
    assert_eq!(files4[1].relative_path, "sub\\file2.txt");

    // Case 5: Windows case variations between container and children
    let mut files5 = vec![f("MyContainer", true), f("mycontainer/document.pdf", false)];
    strip_single_root_wrapper(&mut files5);
    assert_eq!(files5.len(), 1);
    assert_eq!(files5[0].relative_path, "document.pdf");

    // Case 6: Multi-level redundant nesting (nested wrappers)
    let mut files6 = vec![
        f("layer1", true),
        f("layer1/layer2", true),
        f("layer1/layer2/final.dat", false),
    ];
    strip_single_root_wrapper(&mut files6);
    assert_eq!(files6.len(), 1);
    assert_eq!(files6[0].relative_path, "final.dat");
}

#[test]
fn manifest_rejects_alias_conflicts() {
    let f = |p: &str, d: bool| CopyFile {
        size_known: true,
        source_path: String::new(),
        relative_path: p.into(),
        size: 0,
        sha256: if d { String::new() } else { "a".repeat(64) },
        is_directory: d,
        is_corrupted: false,
    };
    assert!(validate_manifest(&[f("a.txt", false), f("A.txt", false)]).is_err());
    assert!(validate_manifest(&[f("a", false), f("a/b", false)]).is_err());
    assert!(validate_manifest(&[f("中文", true), f("中文/b", false)]).is_ok());
}
#[test]
fn settings_reject_wrong_policy_and_empty_cache() {
    let mut s = Settings::default();
    validate_settings(&s).unwrap();
    s.cache_enabled = true;
    assert!(validate_settings(&s).is_err());
    s.cache_enabled = false;
    s.conflict_policy = "unknown-policy".into();
    assert!(validate_settings(&s).is_err());
    for policy in [
        "rename",
        "skip",
        "timestamp",
        "keepNewer",
        "keepOlder",
        "keepLarger",
        "overwrite",
    ] {
        s.conflict_policy = policy.into();
        assert!(validate_settings(&s).is_ok(), "supported policy {policy}");
    }
    s.conflict_policy = "rename".into();
    s.concurrency = 0;
    assert!(validate_settings(&s).is_err());
    s.concurrency = MAX_CONCURRENT_JOBS + 1;
    assert!(validate_settings(&s).is_err());
    s.concurrency = MAX_CONCURRENT_JOBS;
    assert!(validate_settings(&s).is_ok());
}
#[test]
fn transfer_batches_preserve_global_budget_and_fit_real_wire_limit() {
    let mut input = request("copy", Path::new(r"C:\destination"), &Settings::default());
    input.files = (0..20_000)
        .map(|index| CopyFile {
            size_known: true,
            source_path: format!(r"C:\source\{}\{index:05}.bin", "folder".repeat(35)),
            relative_path: format!(r"{}\{index:05}.bin", "folder".repeat(35)),
            size: 4,
            sha256: "a".repeat(64),
            is_directory: false,
            is_corrupted: false,
        })
        .collect();
    input.max_total_bytes = 80_000;
    input.max_written_bytes = 80_000;
    assert!(serde_json::to_vec(&input).unwrap().len() > MAX_MESSAGE_BYTES);
    let expected = input.files.clone();
    let batches = transfer_batches(input.clone()).unwrap();
    assert!(batches.len() > 1);
    let mut assigned = 0;
    let mut copied = Vec::new();
    let mut identities = HashSet::new();
    for mut batch in batches {
        assert!(serde_json::to_vec(&batch).unwrap().len() <= MAX_MESSAGE_BYTES - 64 * 1024);
        assert_eq!(batch.max_written_bytes, 80_000 - assigned);
        assert!(identities.insert((
            std::mem::take(&mut batch.request_id),
            std::mem::take(&mut batch.operation_id),
        )));
        assigned += batch.files.iter().map(|file| file.size).sum::<u64>();
        copied.extend(std::mem::take(&mut batch.files));
    }
    assert_eq!(copied, expected);
    assert_eq!(assigned, 80_000);
    input.max_written_bytes -= 1;
    assert_eq!(
        transfer_batches(input.clone()).unwrap_err(),
        "ResourceLimit"
    );
    input.operation = "readback".into();
    input.max_written_bytes = 0; // Reading never consumes a write quota.
    assert!(transfer_batches(input).is_ok());
}
#[test]
fn transfer_batches_preflight_conflicts_across_the_entire_list() {
    let mut input = request("copy", Path::new(r"C:\destination"), &Settings::default());
    input.files = vec![
        CopyFile {
            size_known: true,
            source_path: r"C:\source\a".into(),
            relative_path: "a".into(),
            size: 1,
            sha256: "b".repeat(64),
            is_directory: false,
            is_corrupted: false,
        },
        CopyFile {
            size_known: true,
            source_path: r"C:\source\b".into(),
            relative_path: "a/b".into(),
            size: 1,
            sha256: "c".repeat(64),
            is_directory: false,
            is_corrupted: false,
        },
    ];
    assert_eq!(transfer_batches(input.clone()).unwrap_err(), "UnsafePath");
    input.files[1].relative_path = "A".into();
    assert_eq!(transfer_batches(input).unwrap_err(), "UnsafePath");
}

#[tokio::test]
async fn inspect_archive_checks_worker_and_path() {
    let directory = tempfile::tempdir().unwrap();
    let core = Coordinator::open(
        directory.path().join("data"),
        directory.path().join("missing-worker.exe"),
        directory.path().join("missing-7z.dll"),
    )
    .unwrap();
    let res = core.inspect_archive("C:\\nonexistent.zip", None).await;
    assert_eq!(res.unwrap_err(), "WorkerUnavailable");

    let fake_worker = directory.path().join("fake-worker.exe");
    let fake_dll = directory.path().join("fake-7z.dll");
    fs::write(&fake_worker, b"fake").unwrap();
    fs::write(&fake_dll, b"fake").unwrap();
    let core2 = Coordinator::open(directory.path().join("data2"), fake_worker, fake_dll).unwrap();
    let res2 = core2
        .inspect_archive("C:\\nonexistent-file-openunpack.zip", None)
        .await;
    assert!(res2.is_err());
}

#[test]
fn derivation_tree_closure_rejects_pending_failed_and_orphan_nodes() {
    let make_node = |id: &str, depth: u32, parent: Option<&str>| NodeWork {
        id: id.into(),
        group: SourceGroup {
            path: format!("{id}.zip"),
            display_name: format!("{id}.zip"),
            volumes: vec![format!("{id}.zip")],
            size: 100,
            missing_volume: false,
        },
        prefix: PathBuf::new(),
        depth,
        parent: parent.map(String::from),
    };
    let make_view = |id: &str, depth: u32, parent: Option<&str>, status: &str| NodeView {
        id: id.into(),
        parent_id: parent.map(String::from),
        name: id.into(),
        depth,
        status: status.into(),
        stage: "finished".into(),
        progress: 1.0,
        attempts: 1,
        message: String::new(),
        password_label: None,
    };
    let root_node = make_node("root", 0, None);
    let child_node = make_node("child", 1, Some("root"));
    let leaf_node = make_node("leaf", 2, Some("child"));

    let state = SavedRun {
        version: 2,
        job_id: "job-1".into(),
        source: root_node.group.clone(),
        witnesses: vec![],
        options: Settings::default(),
        input_files: vec![],
        input_retired: false,
        fingerprint: None,
        pending: VecDeque::new(),
        failed: vec![],
        sealed: vec![
            SealedNode {
                node: root_node.clone(),
                directory: PathBuf::from("d0"),
                files: vec![],
                verification_scope: String::new(),
            },
            SealedNode {
                node: child_node.clone(),
                directory: PathBuf::from("d1"),
                files: vec![],
                verification_scope: String::new(),
            },
            SealedNode {
                node: leaf_node.clone(),
                directory: PathBuf::from("d2"),
                files: vec![],
                verification_scope: String::new(),
            },
        ],
        final_files: vec![],
        events: vec![],
        expanded: 0,
        entries: 0,
        views: vec![
            make_view("root", 0, None, "completed"),
            make_view("child", 1, Some("root"), "completed"),
            make_view("leaf", 2, Some("child"), "completed"),
        ],
        retired_files: HashSet::new(),
    };

    // 1. Valid 3-level nested derivation tree -> closed
    assert!(is_derivation_tree_closed(&state));

    // 2. Pending child node -> not closed
    let mut s_pending = state.clone();
    s_pending
        .pending
        .push_back(make_node("extra", 2, Some("child")));
    assert!(!is_derivation_tree_closed(&s_pending));

    // 3. Failed node -> not closed
    let mut s_failed = state.clone();
    s_failed
        .failed
        .push((make_node("bad", 2, Some("child")), "Corrupt".into()));
    assert!(!is_derivation_tree_closed(&s_failed));

    // 4. Non-completed view -> not closed
    let mut s_view = state.clone();
    s_view.views[2].status = "needsPassword".into();
    assert!(!is_derivation_tree_closed(&s_view));

    // 5. Orphan parent -> not closed
    let mut s_orphan = state.clone();
    s_orphan.sealed[2].node.parent = Some("nonexistent-parent".into());
    assert!(!is_derivation_tree_closed(&s_orphan));
}

#[test]
fn claim_budget_allows_multiple_small_jobs_without_hoarding() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join("cache");
    fs::create_dir_all(&cache_dir).unwrap();

    let core = Coordinator::open(
        temp.path().join("data"),
        temp.path().join("worker.exe"),
        temp.path().join("7z.dll"),
    )
    .unwrap();

    let settings = Settings {
        cache_enabled: true,
        cache_root: cache_dir.to_string_lossy().into_owned(),
        cache_quota_bytes: 48 * 1024 * 1024,
        max_expanded_bytes: 100 * 1024 * 1024 * 1024,
        ..Settings::default()
    };

    let route = ExtractionRoute::SsdCache {
        cache_workspace: cache_dir.join("work"),
        target_dir: temp.path().join("out"),
        score: 50,
        reason: "test".into(),
    };

    // Job 1: small 2 MB archive, estimates 6 MB
    let claim1 = core
        .claim_budget(&settings, &route, &cache_dir, Some(6 * 1024 * 1024))
        .unwrap();
    assert_eq!(claim1.budget(), 6 * 1024 * 1024);

    // Job 2: small 3 MB archive, estimates 9 MB -> should also successfully claim from the remaining quota
    let claim2 = core
        .claim_budget(&settings, &route, &cache_dir, Some(9 * 1024 * 1024))
        .unwrap();
    assert_eq!(claim2.budget(), 9 * 1024 * 1024);

    // Total reserved is 15 MB, 33 MB remains in quota
    let claim3 = core
        .claim_budget(&settings, &route, &cache_dir, Some(33 * 1024 * 1024))
        .unwrap();
    assert_eq!(claim3.budget(), 33 * 1024 * 1024);

    // Now quota is completely exhausted (6 + 9 + 33 = 48 MB of 48 MB)
    let claim_fail = core.claim_budget(&settings, &route, &cache_dir, Some(10 * 1024 * 1024));
    assert!(claim_fail.is_err());

    // Drop claim1 (6 MB freed)
    drop(claim1);

    // Now 6 MB available, 6 MB request should succeed
    let claim4 = core
        .claim_budget(&settings, &route, &cache_dir, Some(6 * 1024 * 1024))
        .unwrap();
    assert_eq!(claim4.budget(), 6 * 1024 * 1024);
}
#[test]
fn cache_budget_distinguishes_permanent_limits_from_live_contention() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    fs::create_dir(&cache).unwrap();
    let core = Coordinator::open(
        temp.path().join("data"),
        temp.path().join("worker"),
        temp.path().join("engine"),
    )
    .unwrap();
    let settings = Settings {
        cache_root: paths::path_string(&cache),
        cache_quota_bytes: 8 * 1024 * 1024,
        max_expanded_bytes: 64 * 1024 * 1024,
        min_free_bytes: 0,
        ..Settings::default()
    };
    let route = ExtractionRoute::SsdCache {
        cache_workspace: cache.join("work"),
        target_dir: temp.path().join("output"),
        score: 50,
        reason: "synthetic".into(),
    };
    assert_eq!(
        core.claim_budget(&settings, &route, &cache, Some(9 * 1024 * 1024))
            .err()
            .unwrap(),
        "ResourceLimit"
    );
    let claim = core
        .claim_budget(&settings, &route, &cache, Some(6 * 1024 * 1024))
        .unwrap();
    assert_eq!(
        core.claim_budget(&settings, &route, &cache, Some(4 * 1024 * 1024))
            .err()
            .unwrap(),
        "CacheQuotaContention"
    );
    drop(claim);
    fs::write(cache.join("retained.bin"), vec![0u8; 6 * 1024 * 1024]).unwrap();
    assert_eq!(
        core.claim_budget(&settings, &route, &cache, Some(4 * 1024 * 1024))
            .err()
            .unwrap(),
        "ResourceLimit"
    );
    let unknown = core.claim_budget(&settings, &route, &cache, None).unwrap();
    assert_eq!(unknown.budget(), 2 * 1024 * 1024);
    drop(unknown);
    let retained = core
        .claim_budget_for_job(
            "unconfirmed-job",
            &settings,
            &route,
            &cache,
            Some(1024 * 1024),
        )
        .unwrap();
    core.in_flight_requests
        .lock()
        .unwrap()
        .insert("unconfirmed-job".into());
    drop(retained);
    assert_eq!(core.cache_budget.reserved_bytes(), 1024 * 1024);
    assert!(core.stop_batch.load(Ordering::Acquire));
}

#[test]
fn budget_claim_keeps_worker_ownership_after_coordinator_drop() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    fs::create_dir(&cache).unwrap();
    let core = Coordinator::open(
        temp.path().join("data"),
        temp.path().join("worker"),
        temp.path().join("engine"),
    )
    .unwrap();
    let settings = Settings {
        cache_root: paths::path_string(&cache),
        cache_quota_bytes: 8 * 1024 * 1024,
        min_free_bytes: 0,
        ..Settings::default()
    };
    let route = ExtractionRoute::SsdCache {
        cache_workspace: cache.join("work"),
        target_dir: temp.path().join("output"),
        score: 50,
        reason: "synthetic".into(),
    };
    let claim = core
        .claim_budget_for_job("unconfirmed-job", &settings, &route, &cache, Some(1024))
        .unwrap();
    let stop_batch = core.stop_batch.clone();
    core.in_flight_requests
        .lock()
        .unwrap()
        .insert("unconfirmed-job".into());
    // Claim disposal must still consult the real request registration even if
    // no coordinator reference remains to upgrade during future cancellation.
    let observer = Arc::downgrade(&core);
    drop(core);
    assert!(observer.upgrade().is_none());
    drop(claim);
    assert!(stop_batch.load(Ordering::Acquire));
}

#[test]
fn budget_manager_reports_pause_and_coordinator_applies_it() {
    let temp = tempfile::tempdir().unwrap();
    let core = Coordinator::open(
        temp.path().join("data"),
        temp.path().join("worker"),
        temp.path().join("engine"),
    )
    .unwrap();
    let settings = Settings {
        min_free_bytes: u64::MAX,
        ..Settings::default()
    };
    let route = ExtractionRoute::Direct {
        target_dir: temp.path().to_path_buf(),
        reason: "synthetic".into(),
    };
    let error = core
        .cache_budget
        .claim(&settings, &route, temp.path(), Some(1024))
        .err()
        .unwrap();
    assert_eq!(error.code, "SharedResourceUnavailable");
    assert!(error.pause_batch);
    assert!(!core.paused.load(Ordering::Acquire));
    assert!(!core.stop_batch.load(Ordering::Acquire));
    assert_eq!(
        core.claim_budget(&settings, &route, temp.path(), Some(1024))
            .err()
            .unwrap(),
        "SharedResourceUnavailable"
    );
    assert!(core.paused.load(Ordering::Acquire));
    assert!(core.stop_batch.load(Ordering::Acquire));
}

#[test]
fn resource_waiter_requeue_is_durable_and_keeps_waiting_on_storage_error() {
    let (directory, core, mut job) = telemetry_core();
    job.status = "waitingResource".into();
    job.finished_at = Some(now());
    core.put(&job).unwrap();
    let connection = rusqlite::Connection::open(directory.path().join("data/jobs.sqlite")).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_waiter BEFORE UPDATE ON jobs WHEN NEW.status = 'queued' BEGIN SELECT RAISE(FAIL, 'synthetic storage failure'); END;").unwrap();
    assert_eq!(core.requeue_resource_waiters().unwrap_err(), "StorageError");
    assert_eq!(core.get_job(&job.id).unwrap().status, "waitingResource");
    assert_eq!(core.repository.jobs().unwrap()[0].status, "waitingResource");
    connection
        .execute_batch("DROP TRIGGER reject_waiter;")
        .unwrap();
    assert!(core.requeue_resource_waiters().unwrap());
    let durable = core.repository.jobs().unwrap().pop().unwrap();
    assert_eq!(durable.status, "queued");
    assert!(durable.finished_at.is_none());
}

#[test]
fn unknown_cache_jobs_have_one_owner_even_with_two_active_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let cache = directory.path().join("cache");
    fs::create_dir(&cache).unwrap();
    let core = Coordinator::open(
        directory.path().join("data"),
        directory.path().join("worker"),
        directory.path().join("engine"),
    )
    .unwrap();
    let settings = Settings {
        cache_root: paths::path_string(&cache),
        cache_quota_bytes: 8 * 1024 * 1024,
        min_free_bytes: 0,
        ..Settings::default()
    };
    let route = ExtractionRoute::SsdCache {
        cache_workspace: cache.join("work"),
        target_dir: directory.path().join("output"),
        score: 50,
        reason: "fixture".into(),
    };
    for id in ["one", "two"] {
        core.active_cancel
            .lock()
            .unwrap()
            .insert(id.into(), Arc::new(AtomicBool::new(false)));
    }
    let first = core
        .claim_budget_for_job("one", &settings, &route, &cache, None)
        .unwrap();
    assert_eq!(
        core.claim_budget_for_job("two", &settings, &route, &cache, None)
            .err()
            .unwrap(),
        "CacheQuotaContention"
    );
    drop(first);
    assert!(
        core.claim_budget_for_job("two", &settings, &route, &cache, None)
            .is_err(),
        "phase completion must keep the job ticket"
    );
    core.release_unknown_budget_owner("one");
    let second = core
        .claim_budget_for_job("two", &settings, &route, &cache, None)
        .unwrap();
    assert!(second.budget() > 0);
    drop(second);
    core.release_unknown_budget_owner("two");
}

#[test]
fn resource_wait_notice_rejects_old_request_with_same_stage_and_generation() {
    let (_directory, core, job) = telemetry_core();
    let old = core
        .begin_transfer(&job, &telemetry_request(&job), Instant::now())
        .unwrap();
    core.set_resource_wait(&job.id, &old.identity, true);
    assert!(
        core.get_job(&job.id)
            .unwrap()
            .message
            .contains("等待可用内存")
    );
    let new = core
        .begin_transfer(&job, &telemetry_request(&job), Instant::now())
        .unwrap();
    core.set_resource_wait(&job.id, &new.identity, true);
    let revision = core.revision_info().1;
    core.set_resource_wait(&job.id, &old.identity, false);
    assert_eq!(core.revision_info().1, revision);
    assert!(
        core.get_job(&job.id)
            .unwrap()
            .message
            .contains("等待可用内存")
    );
    core.set_resource_wait(&job.id, &new.identity, false);
    assert!(core.get_job(&job.id).unwrap().message.is_empty());
    let revision = core.revision_info().1;
    core.set_resource_wait(&job.id, &old.identity, true);
    assert_eq!(core.revision_info().1, revision);
}

#[test]
fn same_id_preview_replacement_keeps_new_cancel_token_after_old_guard_drop() {
    let (_directory, core, _job) = telemetry_core();
    let old = core.begin_preview("same").unwrap();
    let old_registration = PreviewRegistration {
        coordinator: &core,
        request_id: "same".into(),
        cancel: old.clone(),
    };
    let current = core.begin_preview("same").unwrap();
    assert!(old.load(Ordering::Acquire));
    assert!(!current.load(Ordering::Acquire));
    drop(old_registration);
    assert!(
        core.preview_cancel
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(_, token)| Arc::ptr_eq(token, &current))
    );
    core.cancel_preview_output_rules_request("same").unwrap();
    assert!(current.load(Ordering::Acquire));
}

#[test]
fn same_id_inspection_replacement_keeps_new_cancel_token_after_old_guard_drop() {
    let (_directory, core, _job) = telemetry_core();
    let old = core.begin_inspection("same").unwrap();
    let old_registration = InspectionRegistration {
        coordinator: &core,
        request_id: "same".into(),
        cancel: old.clone(),
    };
    let current = core.begin_inspection("same").unwrap();
    assert!(old.load(Ordering::Acquire));
    assert!(!current.load(Ordering::Acquire));
    drop(old_registration);
    assert!(
        core.inspect_cancel
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(_, token)| Arc::ptr_eq(token, &current))
    );
    core.cancel_inspect_archive_request("same").unwrap();
    assert!(current.load(Ordering::Acquire));
}

#[test]
fn cancelled_preview_ids_and_old_registration_cannot_cancel_new_preview() {
    let (_directory, core, _job) = telemetry_core();
    core.cancel_preview_output_rules_request("early").unwrap();
    assert_eq!(core.begin_preview("early").unwrap_err(), "Cancelled");
    let old = core.begin_preview("old").unwrap();
    let current = core.begin_preview("current").unwrap();
    assert!(old.load(Ordering::Acquire));
    core.cancel_preview_output_rules_request("old").unwrap();
    assert!(!current.load(Ordering::Acquire));
    drop(PreviewRegistration {
        coordinator: &core,
        request_id: "old".into(),
        cancel: old.clone(),
    });
    assert!(
        core.preview_cancel
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(id, _)| id == "current")
    );
    core.cancel_preview_output_rules_request("current").unwrap();
    assert!(current.load(Ordering::Acquire));
    assert!(core.begin_preview("invalid/id").is_err());
    for index in 0..300 {
        core.cancel_preview_output_rules_request(&format!("preview-{index}"))
            .unwrap();
    }
    assert_eq!(core.cancelled_previews.lock().unwrap().len(), 128);
}

#[test]
fn cancelled_inspection_ids_cannot_cancel_a_new_request() {
    let (_directory, core, _job) = telemetry_core();
    core.cancel_inspect_archive_request("early").unwrap();
    assert_eq!(core.begin_inspection("early").unwrap_err(), "Cancelled");
    let old = core.begin_inspection("old").unwrap();
    let new = core.begin_inspection("new").unwrap();
    assert!(old.load(Ordering::Acquire));
    core.cancel_inspect_archive_request("old").unwrap();
    assert!(!new.load(Ordering::Acquire));
    let registration = InspectionRegistration {
        coordinator: &core,
        request_id: "old".into(),
        cancel: old.clone(),
    };
    drop(registration);
    core.cancel_inspect_archive_request("new").unwrap();
    assert!(new.load(Ordering::Acquire));
    for index in 0..300 {
        core.cancel_inspect_archive_request(&format!("request-{index}"))
            .unwrap();
    }
    assert_eq!(core.cancelled_inspections.lock().unwrap().len(), 128);
}

#[tokio::test]
async fn join_panic_preserves_files_and_persists_correct_owner_state() {
    for worker_in_flight in [false, true] {
        let (directory, core, mut job) = telemetry_core();
        let source = directory.path().join("original.zip");
        fs::write(&source, b"synthetic original").unwrap();
        let work = directory.path().join("work");
        paths::mark_owned(&work, &job.id, "").unwrap();
        job.source.path = paths::path_string(&source);
        job.workspace = Some(paths::path_string(&work));
        core.put(&job).unwrap();
        core.active_cancel
            .lock()
            .unwrap()
            .insert(job.id.clone(), Arc::new(AtomicBool::new(false)));
        if worker_in_flight {
            core.in_flight_requests
                .lock()
                .unwrap()
                .insert(job.id.clone());
        }
        let mut joins = tokio::task::JoinSet::new();
        let runner = core.clone();
        let job_id = job.id.clone();
        let handle = joins.spawn(async move {
            let _guard = JobRunGuard {
                core: runner,
                job_id,
                finished_normally: false,
            };
            panic!("synthetic job failure");
        });
        let mut owners = HashMap::from([(handle.id(), job.id.clone())]);
        core.finish_join(joins.join_next_with_id().await.unwrap(), &mut owners);
        assert!(owners.is_empty());
        let durable = core.repository.jobs().unwrap().pop().unwrap();
        assert_eq!(
            durable.status,
            if worker_in_flight {
                "stopping"
            } else {
                "interrupted"
            }
        );
        assert_eq!(
            core.active_cancel.lock().unwrap().contains_key(&job.id),
            worker_in_flight
        );
        assert_eq!(core.stop_batch.load(Ordering::Acquire), worker_in_flight);
        assert_eq!(work.join(UNCONFIRMED_FILE).exists(), worker_in_flight);
        assert_eq!(fs::read(&source).unwrap(), b"synthetic original");
        assert!(work.exists());
        if worker_in_flight {
            assert!(core.delete_task(&job.id).await.is_err());
        }
    }
}

#[test]
fn coordinator_claim_includes_cached_jobs_final_hdd_during_extraction_and_writeback() {
    fn topology(source: &Path, target: &Path) -> crate::route::DiskTopology {
        let source_drive = paths::drive_letter_of(source).unwrap();
        let target_drive = paths::drive_letter_of(target).unwrap();
        crate::route::DiskTopology {
            source_is_hdd: source_drive != 'S',
            target_is_hdd: target_drive != 'S',
            is_same_physical_disk: source_drive == target_drive,
            source_device_number: Some(source_drive as u32),
            target_device_number: Some(target_drive as u32),
        }
    }
    for stage in ["extracting", "copying", "verifying", "publishing"] {
        let (_directory, core, mut active) = telemetry_core();
        *core.topology_probe.lock().unwrap() = Some(topology);
        core.settings.lock().unwrap().concurrency = 3;
        active.source.path = r"A:\archives\active.zip".into();
        active.workspace = Some(r"S:\cache\active".into());
        active.options.output_root = r"H:\output".into();
        active.stage = stage.into();
        core.put(&active).unwrap();
        let mut blocked = telemetry_job("blocked");
        blocked.status = "queued".into();
        blocked.source.path = r"B:\archives\blocked.zip".into();
        blocked.options.output_root = r"H:\other".into();
        core.jobs.lock().unwrap().push(blocked.clone());
        core.repository.save_job(&blocked).unwrap();
        assert!(
            core.claim_next_job().unwrap().is_none(),
            "{stage} must occupy final H HDD"
        );
        let mut independent = telemetry_job("independent");
        independent.status = "queued".into();
        independent.source.path = r"D:\archives\independent.zip".into();
        independent.options.output_root = r"E:\output".into();
        core.jobs.lock().unwrap().push(independent.clone());
        core.repository.save_job(&independent).unwrap();
        assert_eq!(core.claim_next_job().unwrap().unwrap().0.id, independent.id);
    }
}
#[test]
fn delivery_scope_combines_real_backends_without_inventing_crc() {
    assert_eq!(
        aggregate_verification_scopes(["frameValidationAndStreamSha256", "targetReadback"]),
        "frameValidationAndStreamSha256+targetReadback"
    );
    assert_eq!(
        aggregate_verification_scopes([
            "archiveCrcAndStreamSha256",
            "frameValidationAndStreamSha256",
            "archiveCrcAndStreamSha256",
            "targetReadback"
        ]),
        "archiveCrcAndStreamSha256+frameValidationAndStreamSha256+targetReadback"
    );
    assert_eq!(aggregate_verification_scopes([""]), "streamSha256");
    let legacy: SealedNode = serde_json::from_value(serde_json::json!({ "node": { "id":"legacy", "group": { "path":"legacy.zip", "displayName":"legacy.zip", "volumes":["legacy.zip"], "size":1, "missingVolume":false }, "prefix":"", "depth":0, "parent":null }, "directory":"cache-node", "files":[] })).unwrap();
    assert!(legacy.verification_scope.is_empty());
}

#[test]
fn test_snapshot_update_protocol_delta_and_apply_to() {
    let (_directory, core, mut job) = telemetry_core();

    // 1. First call with None -> full snapshot
    let first_update = core.snapshot_update(None).unwrap();
    assert!(first_update.full.is_some());
    assert!(first_update.delta.is_none());
    let base_snapshot = first_update.full.unwrap();
    assert_eq!(base_snapshot.jobs.len(), 1);
    assert_eq!(base_snapshot.jobs[0].id, job.id);

    // 2. No changes, calling with current revision -> empty delta
    let noop_update = core.snapshot_update(Some(first_update.revision)).unwrap();
    assert!(noop_update.full.is_none());
    let noop_delta = noop_update.delta.as_ref().unwrap();
    assert!(noop_delta.jobs_changed.is_empty());
    assert!(noop_delta.jobs_removed.is_empty());
    assert!(noop_delta.settings.is_none());
    assert!(noop_delta.state.is_none());
    let applied_noop =
        noop_delta.apply_to(&base_snapshot, noop_update.session_id, noop_update.revision);
    assert_eq!(applied_noop, core.queue_snapshot().unwrap());

    // 3. Mutate job -> delta has changed job, apply_to matches full snapshot
    job.progress = 0.42;
    job.message = "delta update test".into();
    core.put(&job).unwrap();
    let changed_update = core.snapshot_update(Some(noop_update.revision)).unwrap();
    assert!(changed_update.full.is_none());
    let changed_delta = changed_update.delta.as_ref().unwrap();
    assert_eq!(changed_delta.jobs_changed.len(), 1);
    assert_eq!(changed_delta.jobs_changed[0].id, job.id);
    assert_eq!(changed_delta.jobs_changed[0].message, "delta update test");
    let applied_changed = changed_delta.apply_to(
        &applied_noop,
        changed_update.session_id,
        changed_update.revision,
    );
    assert_eq!(applied_changed, core.queue_snapshot().unwrap());

    // 4. Calling snapshot() or queue_snapshot() does not evict or advance delta cache
    let _ = core.snapshot().unwrap();
    let _ = core.queue_snapshot().unwrap();

    // 5. Add a second job -> delta has second job changed
    let mut second = job.clone();
    second.id = "job-two".into();
    core.jobs.lock().unwrap().push(second.clone());
    core.put(&second).unwrap();
    let added_update = core.snapshot_update(Some(changed_update.revision)).unwrap();
    assert!(added_update.full.is_none());
    let added_delta = added_update.delta.as_ref().unwrap();
    assert_eq!(added_delta.jobs_changed.len(), 1);
    assert_eq!(added_delta.jobs_changed[0].id, "job-two");
    let applied_added = added_delta.apply_to(
        &applied_changed,
        added_update.session_id,
        added_update.revision,
    );
    assert_eq!(applied_added, core.queue_snapshot().unwrap());

    // 6. Remove a job -> delta has job in jobs_removed
    job.status = "completed".into();
    core.put(&job).unwrap();
    let before_remove = core.snapshot_update(Some(added_update.revision)).unwrap();
    core.remove(&job.id).unwrap();
    let removed_update = core.snapshot_update(Some(before_remove.revision)).unwrap();
    assert!(removed_update.full.is_none());
    let removed_delta = removed_update.delta.as_ref().unwrap();
    assert_eq!(removed_delta.jobs_removed, vec![job.id.clone()]);
    let applied_before = before_remove.delta.unwrap().apply_to(
        &applied_added,
        before_remove.session_id,
        before_remove.revision,
    );
    let applied_removed = removed_delta.apply_to(
        &applied_before,
        removed_update.session_id,
        removed_update.revision,
    );
    assert_eq!(applied_removed, core.queue_snapshot().unwrap());

    // 7. Settings and state changes
    core.update_visible_state(|| {
        core.paused.store(true, Ordering::Release);
        core.settings.lock().unwrap().completion_sound = true;
        Ok(())
    })
    .unwrap();
    let settings_state_update = core.snapshot_update(Some(removed_update.revision)).unwrap();
    assert!(settings_state_update.full.is_none());
    let ss_delta = settings_state_update.delta.as_ref().unwrap();
    assert!(ss_delta.settings.is_some());
    assert!(ss_delta.settings.as_ref().unwrap().completion_sound);
    assert!(ss_delta.state.is_some());
    assert!(ss_delta.state.as_ref().unwrap().paused);
    let applied_ss = ss_delta.apply_to(
        &applied_removed,
        settings_state_update.session_id,
        settings_state_update.revision,
    );
    assert_eq!(applied_ss, core.queue_snapshot().unwrap());

    // 8. Stale/mismatched since revision -> falls back to full snapshot
    let fallback_update = core.snapshot_update(Some(999999)).unwrap();
    assert!(fallback_update.full.is_some());
    assert!(fallback_update.delta.is_none());
    assert_eq!(
        fallback_update.full.unwrap(),
        core.queue_snapshot().unwrap()
    );
}

fn startable_core(directory: &tempfile::TempDir) -> Arc<Coordinator> {
    let worker = directory.path().join("worker.exe");
    let engine = directory.path().join("7z.dll");
    fs::write(&worker, b"synthetic worker").unwrap();
    fs::write(&engine, b"synthetic engine").unwrap();
    Coordinator::open(directory.path().join("data"), worker, engine).unwrap()
}

#[tokio::test]
async fn queue_start_from_a_watcher_thread_dispatches_to_the_captured_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let core = startable_core(&directory);
    let started = std::thread::spawn({
        let core = core.clone();
        move || core.start()
    })
    .join()
    .expect("starting the queue from a plain thread must not panic");
    assert_eq!(started, Ok(()));
    for _ in 0..100 {
        if !core.running.load(Ordering::Acquire) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        !core.running.load(Ordering::Acquire),
        "an idle batch must release the running latch"
    );
}

#[test]
fn queue_start_without_a_runtime_reports_an_error_instead_of_latching_running() {
    let directory = tempfile::tempdir().unwrap();
    let core = startable_core(&directory);
    assert!(core.start().is_err());
    assert!(!core.running.load(Ordering::Acquire));
}
