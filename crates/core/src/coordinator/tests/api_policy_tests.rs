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
