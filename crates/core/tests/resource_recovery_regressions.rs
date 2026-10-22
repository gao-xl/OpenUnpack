//! Regression tests for the 2026-10-12 comprehensive review findings R1–R4.
//!
//! R1: a failed job-store write during scheduling must leave the job queued and
//!     retryable, never a phantom running task that freezes the queue.
//! R2: a blocked workspace cleanup must keep the workspace association so the
//!     directory stays reachable for retry and explicit cleanup.
//! R3: concurrent jobs sharing one cache root must reserve from the shared quota
//!     instead of each claiming the whole remaining quota.
//! R4: a Direct fallback (cache drive unavailable) must not be charged the SSD
//!     cache quota of a drive it never writes to.
#![cfg(windows)]

use openunpack_core::{
    coordinator::Coordinator,
    paths,
    types::{JobView, Settings},
};
use std::{
    fs,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::Builder;

struct Rig {
    core: Arc<Coordinator>,
    root: PathBuf,
    _temp: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let worker = std::env::var_os("OPENUNPACK_TEST_WORKER")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                workspace
                    .join("src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe")
            });
        let engine = workspace.join("tools/7zip/7z.dll");
        assert!(
            worker.is_file() && engine.is_file(),
            "build the .NET Worker and prepare 7-Zip before running these regressions"
        );
        let temp = Builder::new()
            .prefix("openunpack-review-regression-")
            .tempdir()
            .unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("output")).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        let core = Coordinator::open(root.join("data"), worker, engine).unwrap();
        Self {
            core,
            root,
            _temp: temp,
        }
    }

    fn settings(&self) -> Settings {
        Settings {
            output_root: self.root.join("output").to_string_lossy().into_owned(),
            cache_root: self.root.join("cache").to_string_lossy().into_owned(),
            min_free_bytes: 0,
            max_expanded_bytes: 64 * 1024 * 1024,
            cache_quota_bytes: 48 * 1024 * 1024,
            cache_enabled: true,
            cache_policy: "always".into(),
            cache_input: false,
            concurrency: 2,
            ..Settings::default()
        }
    }

    fn archive(&self, name: &str, size: usize) -> PathBuf {
        let input = self.root.join(format!("payload-{name}"));
        fs::create_dir_all(&input).unwrap();
        fs::write(input.join("data.bin"), vec![0x51; size]).unwrap();
        let target = self.root.join(format!("{name}.zip"));
        let seven_zip = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/7zip/7z.exe");
        assert!(
            Command::new(seven_zip)
                .current_dir(&input)
                .args(["a", "-tzip", "-mx=0", "-y", "-bso0", "-bsp0", "-bse0"])
                .arg(&target)
                .arg(".")
                .status()
                .unwrap()
                .success(),
            "fixture archive creation failed"
        );
        target
    }

    fn add(&self, path: &Path, settings: &Settings) -> String {
        self.core.save_settings(settings.clone()).unwrap();
        let scan = self
            .core
            .scan(&[path.to_string_lossy().into_owned()])
            .unwrap();
        self.core
            .add_jobs(scan.sources, settings.clone())
            .unwrap()
            .remove(0)
            .id
    }

    async fn finish(&self) -> Vec<JobView> {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let snapshot = self.core.snapshot().unwrap();
            if !snapshot.running {
                return snapshot.jobs;
            }
            assert!(
                Instant::now() < deadline,
                "bounded regression fixture timed out"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scheduler_write_failure_keeps_job_queued_and_retryable() {
    let rig = Rig::new();
    let mut settings = rig.settings();
    settings.cache_enabled = false;
    settings.concurrency = 1;
    let archive = rig.archive("locked-db", 32);
    let id = rig.add(&archive, &settings);

    // Hold the SQLite write lock longer than the busy timeout so the first claim fails.
    let connection = rusqlite::Connection::open(rig.root.join("data/jobs.sqlite")).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE;").unwrap();
    rig.core.start().unwrap();
    let jobs = rig.finish().await;
    let job = jobs.into_iter().find(|job| job.id == id).unwrap();
    let snapshot = rig.core.snapshot().unwrap();
    connection.execute_batch("ROLLBACK;").unwrap();

    assert!(!snapshot.running, "the batch must end after a failed claim");
    assert_eq!(
        job.status, "queued",
        "a failed job-store write must leave the job queued, not running"
    );
    assert!(
        job.message.contains("写入失败"),
        "the storage error must be reported on the queued job, got: {}",
        job.message
    );

    // Once storage recovers, starting again must run the same job to completion.
    rig.core.start().unwrap();
    let jobs = rig.finish().await;
    let job = jobs.into_iter().find(|job| job.id == id).unwrap();
    assert_eq!(
        job.status, "completed",
        "job must run after storage recovers: {}",
        job.message
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blocked_cleanup_keeps_workspace_association() {
    let rig = Rig::new();
    let mut settings = rig.settings();
    settings.cache_enabled = false;
    settings.concurrency = 1;
    settings.max_expanded_bytes = 1024 * 1024;
    let archive = rig.archive("retry", 2 * 1024 * 1024);
    let id = rig.add(&archive, &settings);
    rig.core.start().unwrap();
    let first = rig
        .finish()
        .await
        .into_iter()
        .find(|job| job.id == id)
        .unwrap();
    let old = PathBuf::from(
        first
            .workspace
            .as_ref()
            .expect("first attempt keeps a workspace"),
    );

    // Block recursive removal with a handle that denies delete sharing.
    let blocked = old.join("held-review-fixture.bin");
    fs::write(&blocked, b"synthetic file held without delete sharing").unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&blocked)
        .unwrap();

    settings.max_expanded_bytes = 16 * 1024 * 1024;
    settings.output_exclusions = vec!["never-match-review.txt".into()];
    rig.core.save_settings(settings).unwrap();
    rig.core.retry(&id, false).unwrap();
    rig.core.start().unwrap();
    let second = rig
        .finish()
        .await
        .into_iter()
        .find(|job| job.id == id)
        .unwrap();

    assert!(old.exists(), "the blocked directory must remain on disk");
    assert_eq!(
        second.status, "failed",
        "blocked cleanup must surface as failure"
    );
    assert!(
        second.workspace.is_some(),
        "a partial cleanup must keep the workspace association for retry/cleanup"
    );
    assert!(
        second.message.contains("清理失败") || second.message.contains("清理"),
        "cleanup failure must be reported, got: {}",
        second.message
    );

    // Removing the blocker lets a later retry finish the job.
    drop(held);
    rig.core.retry(&id, false).unwrap();
    rig.core.start().unwrap();
    let third = rig
        .finish()
        .await
        .into_iter()
        .find(|job| job.id == id)
        .unwrap();
    assert_eq!(
        third.status,
        "completed",
        "retry after cleanup recovers: stage={} message={} nodes={:?}",
        third.stage,
        third.message,
        third
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.status.clone(), n.message.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        third.workspace.is_none(),
        "successful runs release their workspace"
    );
    assert!(!old.exists(), "cleanup removes the stale generation");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_cache_quota_is_reserved_across_concurrent_jobs() {
    let rig = Rig::new();
    let settings = rig.settings();
    let a = rig.archive("quota-a", 32 * 1024 * 1024);
    let b = rig.archive("quota-b", 32 * 1024 * 1024);
    let id_a = rig.add(&a, &settings);
    let id_b = rig.add(&b, &settings);
    rig.core.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut peak = 0u64;
    let jobs = loop {
        let snapshot = rig.core.snapshot().unwrap();
        peak = peak.max(paths::tree_size(&rig.root.join("cache")).unwrap_or(0));
        if !snapshot.running {
            break snapshot.jobs;
        }
        assert!(
            Instant::now() < deadline,
            "bounded regression fixture timed out"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    };
    assert!(
        peak <= settings.cache_quota_bytes,
        "cache peak {peak} exceeded the shared quota {}",
        settings.cache_quota_bytes
    );
    let status = |id: &str| jobs.iter().find(|job| job.id == id).unwrap().status.clone();
    let completed = [status(&id_a), status(&id_b)]
        .iter()
        .filter(|s| s.as_str() == "completed")
        .count();
    assert_eq!(
        completed, 2,
        "with a 48 MiB quota and two 32 MiB jobs, both complete safely in serial without exceeding the quota"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_fallback_is_not_charged_the_cache_quota() {
    let rig = Rig::new();
    let mut settings = rig.settings();
    settings.concurrency = 1;
    let unused = (b'D'..=b'Z')
        .rev()
        .find(|c| !PathBuf::from(format!("{}:\\", *c as char)).exists())
        .expect("a disconnected drive letter for the fixture");
    settings.cache_root = format!("{}:\\OpenUnpackDisconnectedReviewCache", unused as char);
    settings.cache_quota_bytes = 1024 * 1024;
    settings.max_expanded_bytes = 16 * 1024 * 1024;
    settings.expand_nested = false;
    let archive = rig.archive("direct-fallback", 2 * 1024 * 1024);
    let id = rig.add(&archive, &settings);
    rig.core.start().unwrap();
    let first = rig
        .finish()
        .await
        .into_iter()
        .find(|job| job.id == id)
        .unwrap();

    assert_eq!(
        first.status, "completed",
        "Direct fallback must honour only its own volume, not the disconnected cache quota: {}",
        first.message
    );
    let published = first.output_path.expect("completed runs publish a result");
    assert!(
        PathBuf::from(&published).starts_with(rig.root.join("output")),
        "the fallback must publish to the output volume, got {published}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_small_jobs_fit_quota_and_complete_together() {
    let rig = Rig::new();
    let mut settings = rig.settings();
    settings.concurrency = 2;
    settings.cache_quota_bytes = 48 * 1024 * 1024;
    let a = rig.archive("small-a", 2 * 1024 * 1024);
    let b = rig.archive("small-b", 2 * 1024 * 1024);
    let id_a = rig.add(&a, &settings);
    let id_b = rig.add(&b, &settings);
    rig.core.start().unwrap();
    let jobs = rig.finish().await;
    let status = |id: &str| jobs.iter().find(|job| job.id == id).unwrap().status.clone();
    assert_eq!(status(&id_a), "completed", "small job A must complete");
    assert_eq!(status(&id_b), "completed", "small job B must complete");
}
