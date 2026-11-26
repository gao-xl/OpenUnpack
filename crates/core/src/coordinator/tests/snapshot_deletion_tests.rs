use super::*;

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
    assert!(
        time::OffsetDateTime::parse(
            &observation.updated_at,
            &time::format_description::well_known::Rfc3339,
        )
        .is_ok()
    );
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
