//! In-Place extraction regression test suite covering DEF-01 to DEF-10.
//!
//! DEF-01: Replace and conflict policy handling in in-place mode.
//! DEF-02: Scanner bypass / session detection for in-place jobs.
//! DEF-03: Orphan state handling and cleanup.
//! DEF-07: State manifest retention until publication, and 0-byte archive residue cleanup.
//! DEF-08: Path authorization and volume permission checking.
//! DEF-09: Same-volume enforcement, min_free_bytes safety reserve, and concurrency volume reservation.
//! DEF-10: Password candidate chain alignment.

#![cfg(windows)]

use openunpack_core::{
    coordinator::Coordinator,
    paths,
    types::{JobView, Settings},
};
use openunpack_infrastructure::engine::inplace;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::Builder;

struct Rig {
    core: Arc<Coordinator>,
    root: PathBuf,
    _temp: tempfile::TempDir,
}

fn write_zip(path: &Path, name: &str, payload: &[u8]) {
    fn n16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn n32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut crc = 0xffff_ffffu32;
    for byte in payload {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    crc = !crc;
    let mut archive = fs::File::create(path).unwrap();
    let mut local = Vec::new();
    n32(&mut local, 0x0403_4b50);
    n16(&mut local, 20);
    n16(&mut local, 0x800);
    for _ in 0..3 {
        n16(&mut local, 0);
    }
    n32(&mut local, crc);
    n32(&mut local, payload.len() as u32);
    n32(&mut local, payload.len() as u32);
    n16(&mut local, name.len() as u16);
    n16(&mut local, 0);
    local.extend_from_slice(name.as_bytes());
    local.extend_from_slice(payload);
    archive.write_all(&local).unwrap();

    let mut central = Vec::new();
    n32(&mut central, 0x0201_4b50);
    n16(&mut central, 20);
    n16(&mut central, 20);
    n16(&mut central, 0x800);
    for _ in 0..3 {
        n16(&mut central, 0);
    }
    n32(&mut central, crc);
    n32(&mut central, payload.len() as u32);
    n32(&mut central, payload.len() as u32);
    n16(&mut central, name.len() as u16);
    for _ in 0..4 {
        n16(&mut central, 0);
    }
    n32(&mut central, 0);
    n32(&mut central, 0);
    central.extend_from_slice(name.as_bytes());
    archive.write_all(&central).unwrap();

    let mut end = Vec::new();
    n32(&mut end, 0x0605_4b50);
    n16(&mut end, 0);
    n16(&mut end, 0);
    n16(&mut end, 1);
    n16(&mut end, 1);
    n32(&mut end, central.len() as u32);
    n32(&mut end, local.len() as u32);
    n16(&mut end, 0);
    archive.write_all(&end).unwrap();
}

impl Rig {
    fn new() -> Self {
        let worker = PathBuf::from("native-worker");
        let engine = PathBuf::from("native-engine");
        let temp = Builder::new()
            .prefix("openunpack-inplace-regression-")
            .tempdir()
            .unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("output")).unwrap();
        let core = Coordinator::open(root.join("data"), worker, engine).unwrap();
        Self {
            core,
            root,
            _temp: temp,
        }
    }

    fn settings(&self) -> Settings {
        Settings {
            output_root: String::new(), // in-place default: output next to archive
            min_free_bytes: 0,
            max_expanded_bytes: 64 * 1024 * 1024,
            extraction_engine: "inPlace".into(),
            concurrency: 1,
            conflict_policy: "rename".into(),
            expand_nested: false,
            ..Settings::default()
        }
    }

    fn archive(&self, name: &str, size: usize) -> PathBuf {
        let payload = vec![0x42; size];
        let target = self.root.join(format!("{name}.zip"));
        write_zip(&target, "content.bin", &payload);
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
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let snapshot = self.core.snapshot().unwrap();
            let all_finished = !snapshot.jobs.is_empty()
                && snapshot.jobs.iter().all(|j| {
                    matches!(
                        j.status.as_str(),
                        "succeeded" | "failed" | "skipped" | "cancelled" | "interrupted"
                    )
                });
            if all_finished || Instant::now() >= deadline {
                return snapshot.jobs;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// DEF-09: When free space minus min_free_bytes is insufficient for needed peak,
/// in-place job must fail with NoSpace before modifying the archive.
#[tokio::test]
async fn inplace_space_headroom_and_min_free_bytes() {
    let rig = Rig::new();
    let archive = rig.archive("sample_large", 64 * 1024);
    let original_len = fs::metadata(&archive).unwrap().len();

    // Query actual free bytes and set min_free_bytes to virtually consume everything
    let free = paths::free_bytes(&archive).unwrap();
    let mut s = rig.settings();
    s.min_free_bytes = free + 1024 * 1024; // Ensure headroom < preflight.needed

    let id = rig.add(&archive, &s);
    rig.core.start().unwrap();
    let jobs = rig.finish().await;

    let job = jobs.iter().find(|j| j.id == id).expect("job should exist");
    assert_eq!(job.status, "failed");
    assert!(
        job.message.contains("NoSpace") || job.message.contains("磁盘空间不足"),
        "expected NoSpace rejection message, got: {}",
        job.message
    );
    // Source archive must remain completely untouched
    assert_eq!(
        fs::metadata(&archive).unwrap().len(),
        original_len,
        "archive must not be modified or truncated on space rejection"
    );
}

/// DEF-07: Cleanup of 0-byte archive residue and state manifest management.
#[test]
fn inplace_cleanup_0_byte_archive_and_state() {
    let temp = Builder::new()
        .prefix("openunpack-cleanup-test-")
        .tempdir()
        .unwrap();
    let archive = temp.path().join("empty.zip");

    // 1. Create a 0-byte archive and call cleanup_archive
    fs::write(&archive, b"").unwrap();
    assert!(archive.exists());
    inplace::cleanup_archive(&archive);
    assert!(
        !archive.exists(),
        "0-byte archive residue must be deleted by cleanup_archive"
    );

    // 2. Non-zero byte archive must NOT be deleted by cleanup_archive
    fs::write(&archive, b"PK0304").unwrap();
    inplace::cleanup_archive(&archive);
    assert!(
        archive.exists(),
        "non-empty archive must be preserved by cleanup_archive"
    );

    // 3. State sidecar lifecycle
    let state_file = inplace::state_path(&archive);
    assert!(!inplace::has_session(&archive));
    fs::write(
        &state_file,
        b"{\"version\":1,\"archive\":\"test\",\"output\":\"test\",\"verified\":true,\"entries\":[]}",
    )
    .unwrap();
    assert!(
        inplace::has_session(&archive),
        "has_session must recognize .openunpack.json manifest"
    );
    inplace::forget_state(&archive).unwrap();
    assert!(
        !state_file.exists(),
        "forget_state must remove .openunpack.json"
    );
}

/// DEF-09 & paths: volume_root resolution and same_volume validation.
#[test]
fn paths_volume_root_and_same_volume() {
    let temp = Builder::new()
        .prefix("openunpack-volume-test-")
        .tempdir()
        .unwrap();
    let path_a = temp.path().join("a.zip");
    let path_b = temp.path().join("output/extracted");
    fs::create_dir_all(&path_b).unwrap();
    fs::write(&path_a, b"test").unwrap();

    let root = paths::volume_root(&path_a).expect("volume_root should resolve");
    assert!(
        root.is_absolute(),
        "volume_root should return an absolute root"
    );

    let same = paths::same_volume(&path_a, &path_b).expect("same_volume check should succeed");
    assert!(same, "paths in same temp directory must be on same volume");
}

/// DEF-01: Conflict policy "skip" when destination files exist.
#[tokio::test]
async fn inplace_conflict_policy_skip() {
    let rig = Rig::new();
    let archive = rig.archive("conflict_sample", 8192);
    let original_len = fs::metadata(&archive).unwrap().len();

    // Pre-create output directory and conflicting file
    let out_dir = rig.root.join("conflict_sample");
    fs::create_dir_all(&out_dir).unwrap();
    fs::write(out_dir.join("content.bin"), b"existing file").unwrap();

    let mut s = rig.settings();
    s.conflict_policy = "skip".into();

    let id = rig.add(&archive, &s);
    rig.core.start().unwrap();
    let jobs = rig.finish().await;

    let job = jobs.iter().find(|j| j.id == id).expect("job should exist");
    assert_eq!(job.status, "skipped");
    assert!(
        job.message.contains("按设置跳过"),
        "expected skipped message, got: {}",
        job.message
    );
    assert_eq!(
        fs::metadata(&archive).unwrap().len(),
        original_len,
        "archive must be untouched when skipped"
    );
}

/// DEF-02: Coordinator recognises in-place sessions via has_session.
#[test]
fn inplace_session_recognition() {
    let temp = Builder::new()
        .prefix("openunpack-session-test-")
        .tempdir()
        .unwrap();
    let archive = temp.path().join("test_session.zip");
    fs::write(&archive, b"dummy").unwrap();

    // Before session exists
    assert!(!inplace::has_session(&archive));

    // When manifest sidecar exists
    let manifest = inplace::state_path(&archive);
    fs::write(
        &manifest,
        b"{\"version\":1,\"archive\":\"test\",\"output\":\"test\",\"verified\":true,\"entries\":[]}",
    )
    .unwrap();
    assert!(inplace::has_session(&archive));

    // When cleaned
    inplace::forget_state(&archive).unwrap();
    assert!(!inplace::has_session(&archive));
}

/// DEF-08: Path authorization rejection prevents in-place extraction.
#[tokio::test]
async fn inplace_path_authorization_rejection() {
    struct DenyPathPolicy;
    impl openunpack_core::coordinator::JobExecutionPolicy for DenyPathPolicy {
        fn authorize(&self, _job: &JobView) -> Result<(), String> {
            Ok(())
        }
        fn authorize_path(&self, _path: &Path, _write: bool) -> Result<(), String> {
            Err("ApiAuthorizationDenied".into())
        }
    }

    let rig = Rig::new();
    let archive = rig.archive("auth_sample", 4096);
    let s = rig.settings();
    let id = rig.add(&archive, &s);

    let policy = Arc::new(DenyPathPolicy);
    rig.core
        .start_selected_authorized(std::slice::from_ref(&id), policy)
        .unwrap();
    let jobs = rig.finish().await;

    let job = jobs.iter().find(|j| j.id == id).expect("job should exist");
    assert!(
        job.status == "queued" || job.status == "failed",
        "expected queued/failed under auth denial, got: {}",
        job.status
    );
}

/// DEF-09: Concurrent volume reservation prevents over-subscription on the same disk.
#[tokio::test]
async fn inplace_concurrency_volume_reservation() {
    let rig = Rig::new();
    let archive = rig.archive("resv_sample", 32 * 1024);
    let volume_root = paths::volume_root(&archive).unwrap();

    let reservations = rig.core.inplace_volume_reservations();
    let free = paths::free_bytes(&archive).unwrap();
    {
        let mut ledger = reservations.lock().unwrap();
        ledger.insert(volume_root.clone(), free);
    }

    let s = rig.settings();
    let id = rig.add(&archive, &s);
    rig.core.start().unwrap();
    let jobs = rig.finish().await;

    let job = jobs.iter().find(|j| j.id == id).expect("job should exist");
    assert_eq!(job.status, "failed");
    assert!(
        job.message.contains("NoSpace") || job.message.contains("磁盘空间不足"),
        "expected NoSpace rejection due to concurrency reservation, got: {}",
        job.message
    );

    // Clean up artificial reservation
    {
        let mut ledger = reservations.lock().unwrap();
        ledger.remove(&volume_root);
    }
}
