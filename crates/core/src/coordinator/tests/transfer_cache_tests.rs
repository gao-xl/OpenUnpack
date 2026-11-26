use super::*;

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

    // Case 7: Redundant single wrapper with double or mixed leading slashes (ISSUE-H02)
    let mut files7 = vec![
        f("Root", true),
        f("Root//nestedA/file.txt", false),
        f("Root\\/nestedB\\sub.txt", false),
    ];
    strip_single_root_wrapper(&mut files7);
    assert_eq!(files7.len(), 2);
    assert_eq!(files7[0].relative_path, "nestedA\\file.txt");
    assert_eq!(files7[1].relative_path, "nestedB\\sub.txt");
    assert!(!files7[0].relative_path.starts_with('\\'));
    assert!(!files7[1].relative_path.starts_with('\\'));
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
