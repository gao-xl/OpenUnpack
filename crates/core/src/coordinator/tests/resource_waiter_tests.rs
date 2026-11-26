use super::*;

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

#[test]
fn transfer_progress_emits_are_throttled_to_60ms_unless_boundary() {
    use std::time::Duration;
    let (_directory, core, job) = telemetry_core();
    let started = Instant::now();
    let owner = core
        .begin_transfer(&job, &telemetry_request(&job), started)
        .unwrap();
    let mut rx = core.subscribe_progress();

    // 1st tick at t=0: initial observation must emit immediately
    core.observe_transfer(
        &job.id,
        &owner.identity,
        &telemetry_event(&owner.identity, 100),
        (0, Some(1000)),
        None,
        started,
    );
    let first = rx
        .try_recv()
        .expect("first observation must be emitted immediately");
    assert_eq!(first.completed_bytes, 100);

    // 2nd tick at t=20ms (< 60ms): throttled, must NOT emit
    core.observe_transfer(
        &job.id,
        &owner.identity,
        &telemetry_event(&owner.identity, 200),
        (0, Some(1000)),
        None,
        started + Duration::from_millis(20),
    );
    assert!(
        rx.try_recv().is_err(),
        "progress update within 60ms must be throttled"
    );

    // 3rd tick at t=40ms (< 60ms): still throttled, must NOT emit
    core.observe_transfer(
        &job.id,
        &owner.identity,
        &telemetry_event(&owner.identity, 300),
        (0, Some(1000)),
        None,
        started + Duration::from_millis(40),
    );
    assert!(
        rx.try_recv().is_err(),
        "progress update at 40ms must be throttled"
    );

    // 4th tick at t=65ms (>= 60ms): must emit!
    core.observe_transfer(
        &job.id,
        &owner.identity,
        &telemetry_event(&owner.identity, 400),
        (0, Some(1000)),
        None,
        started + Duration::from_millis(65),
    );
    let second = rx.try_recv().expect("progress update after 60ms must emit");
    assert_eq!(second.completed_bytes, 400);

    // 5th tick at t=70ms with 100% completion (boundary): must emit immediately despite <60ms interval!
    core.observe_transfer(
        &job.id,
        &owner.identity,
        &telemetry_event(&owner.identity, 1000),
        (0, Some(1000)),
        None,
        started + Duration::from_millis(70),
    );
    let boundary = rx
        .try_recv()
        .expect("boundary completion (100%) must bypass throttle");
    assert_eq!(boundary.completed_bytes, 1000);
    assert_eq!(boundary.progress, 1.0);
}
