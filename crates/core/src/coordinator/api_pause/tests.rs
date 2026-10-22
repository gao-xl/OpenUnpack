use super::*;

struct PausePolicy(Option<String>);
impl JobExecutionPolicy for PausePolicy {
    fn authorize(&self, job: &JobView) -> Result<(), String> {
        if self.0.as_ref().is_none_or(|id| id == &job.id) {
            Ok(())
        } else {
            Err("synthetic control denied".into())
        }
    }
    fn authorize_path(&self, _: &Path, _: bool) -> Result<(), String> {
        Ok(())
    }
}
fn all() -> Arc<dyn JobExecutionPolicy> {
    Arc::new(PausePolicy(None))
}
fn fixture() -> (tempfile::TempDir, Arc<Coordinator>, Vec<JobView>) {
    let directory = tempfile::tempdir().unwrap();
    let worker = directory.path().join("worker.exe");
    let engine = directory.path().join("engine.dll");
    // Current-thread tests never yield after start, so the scheduled driver is
    // dropped with the runtime. These identity files are never executed.
    fs::write(&worker, b"not executed").unwrap();
    fs::write(&engine, b"not loaded").unwrap();
    let core = Coordinator::open(directory.path().join("data"), worker, engine).unwrap();
    let paths: Vec<_> = ["a.zip", "b.zip"]
        .into_iter()
        .map(|name| {
            let path = directory.path().join(name);
            fs::write(&path, b"synthetic metadata-only archive").unwrap();
            paths::path_string(&path)
        })
        .collect();
    let sources = scanner::scan(&paths, false, &[]).unwrap().sources;
    let jobs = core.add_jobs(sources, Settings::default()).unwrap();
    (directory, core, jobs)
}

#[tokio::test(flavor = "current_thread")]
async fn api_cannot_take_over_a_native_pause_or_active_native_batch() {
    let (_directory, core, jobs) = fixture();
    core.pause().unwrap();
    assert!(core.pause_authorized(all()).is_err());
    assert!(
        core.start_selected_resuming_authorized(&[jobs[0].id.clone()], all())
            .is_err()
    );
    assert!(core.paused.load(Ordering::Acquire));
    assert!(!core.api_paused.load(Ordering::Acquire));
    core.start().unwrap();
    assert!(core.pause_authorized(all()).is_err());
    assert!(!core.paused.load(Ordering::Acquire));
    assert!(core.scheduler_scope.lock().unwrap().is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn api_pause_requires_authority_over_every_existing_api_batch_member() {
    let (_directory, core, jobs) = fixture();
    let ids: Vec<_> = jobs.iter().map(|job| job.id.clone()).collect();
    core.start_selected_authorized(&ids, all()).unwrap();
    let only_first = Arc::new(PausePolicy(Some(jobs[0].id.clone())));
    assert!(core.pause_authorized(only_first).is_err());
    assert!(!core.paused.load(Ordering::Acquire));
    assert!(!core.api_paused.load(Ordering::Acquire));
    core.pause_authorized(all()).unwrap();
    assert!(core.paused.load(Ordering::Acquire));
    assert!(core.api_paused.load(Ordering::Acquire));
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_api_resume_replaces_scope_and_never_claims_unselected_waiters() {
    let (_directory, core, jobs) = fixture();
    let ids: Vec<_> = jobs.iter().map(|job| job.id.clone()).collect();
    core.start_selected_authorized(&ids, all()).unwrap();
    core.pause_authorized(all()).unwrap();
    assert!(core.start_authorized(all()).is_err());
    assert!(
        core.start_selected_authorized(&[jobs[0].id.clone()], all())
            .is_err()
    );
    assert!(
        core.add_jobs_authorized(vec![jobs[0].source.clone()], Settings::default(), all())
            .is_err()
    );
    for job in &jobs {
        let mut waiting = job.clone();
        waiting.status = "waitingResource".into();
        core.put(&waiting).unwrap();
    }
    core.start_selected_resuming_authorized(&[jobs[0].id.clone()], all())
        .unwrap();
    assert!(!core.paused.load(Ordering::Acquire));
    assert!(!core.api_paused.load(Ordering::Acquire));
    assert_eq!(
        core.scheduler_scope.lock().unwrap().as_ref().unwrap(),
        &HashSet::from([jobs[0].id.clone()])
    );
    assert_eq!(core.get_job(&jobs[1].id).unwrap().status, "waitingResource");
    let (claimed, _, _, _) = core.claim_next_job().unwrap().unwrap();
    assert_eq!(claimed.id, jobs[0].id);
    assert!(core.claim_next_job().unwrap().is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_api_resume_rechecks_selected_and_already_active_jobs_before_mutation() {
    let (_directory, core, jobs) = fixture();
    let ids: Vec<_> = jobs.iter().map(|job| job.id.clone()).collect();
    core.start_selected_authorized(&ids, all()).unwrap();
    core.pause_authorized(all()).unwrap();
    let before = core.scheduler_scope.lock().unwrap().clone();
    assert!(
        core.start_selected_resuming_authorized(
            &[jobs[1].id.clone()],
            Arc::new(PausePolicy(Some(jobs[0].id.clone())))
        )
        .is_err()
    );
    let mut active = jobs[1].clone();
    active.status = "running".into();
    core.put(&active).unwrap();
    core.active_cancel
        .lock()
        .unwrap()
        .insert(active.id.clone(), Arc::new(AtomicBool::new(false)));
    assert!(
        core.start_selected_resuming_authorized(
            &[jobs[0].id.clone()],
            Arc::new(PausePolicy(Some(jobs[0].id.clone())))
        )
        .is_err()
    );
    assert!(core.paused.load(Ordering::Acquire));
    assert!(core.api_paused.load(Ordering::Acquire));
    assert_eq!(*core.scheduler_scope.lock().unwrap(), before);
}

#[tokio::test(flavor = "current_thread")]
async fn local_controls_override_api_pause_origin_and_global_cancel_clears_it() {
    let (_directory, core, jobs) = fixture();
    core.pause_authorized(all()).unwrap();
    core.pause().unwrap();
    assert!(!core.api_paused.load(Ordering::Acquire));
    assert!(
        core.start_selected_resuming_authorized(&[jobs[0].id.clone()], all())
            .is_err()
    );
    core.start().unwrap();
    assert!(!core.api_paused.load(Ordering::Acquire));
    assert!(!core.paused.load(Ordering::Acquire));
    let (_directory_two, idle, _) = fixture();
    idle.pause_authorized(all()).unwrap();
    idle.cancel(None).unwrap();
    assert!(!idle.paused.load(Ordering::Acquire));
    assert!(!idle.api_paused.load(Ordering::Acquire));
}

#[tokio::test(flavor = "current_thread")]
async fn a_running_api_owner_is_an_explicit_resume_anchor_and_empty_queue_cannot_pause() {
    let (_directory, core, jobs) = fixture();
    core.start_selected_authorized(&[jobs[0].id.clone()], all())
        .unwrap();
    let mut running = jobs[0].clone();
    running.status = "running".into();
    core.put(&running).unwrap();
    core.active_cancel
        .lock()
        .unwrap()
        .insert(running.id.clone(), Arc::new(AtomicBool::new(false)));
    assert!(
        core.start_selected_authorized(&[running.id.clone()], all())
            .is_err()
    );
    core.pause_authorized(all()).unwrap();
    assert!(core.paused.load(Ordering::Acquire));
    core.start_selected_resuming_authorized(&[running.id.clone()], all())
        .unwrap();
    assert!(!core.paused.load(Ordering::Acquire));
    assert!(!core.api_paused.load(Ordering::Acquire));
    assert_eq!(
        core.scheduler_scope.lock().unwrap().as_ref().unwrap(),
        &HashSet::from([running.id.clone()])
    );
    assert_eq!(core.get_job(&jobs[1].id).unwrap().status, "queued");
    let (_empty_directory, empty, empty_jobs) = fixture();
    for job in empty_jobs {
        empty.remove(&job.id).unwrap();
    }
    assert!(empty.pause_authorized(all()).is_err());
    assert!(!empty.paused.load(Ordering::Acquire));
    assert!(!empty.api_paused.load(Ordering::Acquire));
}

#[tokio::test(flavor = "current_thread")]
async fn removing_or_cancelling_last_resume_target_clears_only_api_pause() {
    let (_directory, core, jobs) = fixture();
    core.pause_authorized(all()).unwrap();
    core.cancel(Some(&jobs[0].id)).unwrap();
    assert!(core.api_paused.load(Ordering::Acquire));
    core.remove(&jobs[1].id).unwrap();
    assert!(!core.paused.load(Ordering::Acquire));
    assert!(!core.api_paused.load(Ordering::Acquire));
    let (_native_directory, native, native_jobs) = fixture();
    native.pause().unwrap();
    for job in native_jobs {
        native.cancel(Some(&job.id)).unwrap();
    }
    assert!(native.paused.load(Ordering::Acquire));
    assert!(!native.api_paused.load(Ordering::Acquire));
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_last_queued_job_never_wakes_a_different_active_owner() {
    let (_directory, core, jobs) = fixture();
    let ids: Vec<_> = jobs.iter().map(|job| job.id.clone()).collect();
    core.start_selected_authorized(&ids, all()).unwrap();
    let mut running = jobs[0].clone();
    running.status = "running".into();
    core.put(&running).unwrap();
    core.active_cancel
        .lock()
        .unwrap()
        .insert(running.id.clone(), Arc::new(AtomicBool::new(false)));
    core.pause_authorized(all()).unwrap();
    // A client authorized only for B can cancel B, but that ordinary action
    // must not clear the global pause and wake active owner A.
    core.cancel(Some(&jobs[1].id)).unwrap();
    assert!(core.paused.load(Ordering::Acquire));
    assert!(core.api_paused.load(Ordering::Acquire));
    assert!(
        core.start_selected_resuming_authorized(
            &[running.id.clone()],
            Arc::new(PausePolicy(Some(jobs[1].id.clone())))
        )
        .is_err()
    );
    assert!(core.paused.load(Ordering::Acquire));
    core.start_selected_resuming_authorized(&[running.id.clone()], all())
        .unwrap();
    assert!(!core.paused.load(Ordering::Acquire));
    assert_eq!(
        core.scheduler_scope.lock().unwrap().as_ref().unwrap(),
        &HashSet::from([running.id.clone()])
    );
}
