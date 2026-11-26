use super::*;

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
