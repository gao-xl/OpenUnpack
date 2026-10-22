//! Native RR-01 regressions: archive-level corruption must survive Worker -> Core.
//! Sources, output and deletion policy are confined to synthetic private fixtures.
//! Set OPENUNPACK_TEST_WORKER/OPENUNPACK_TEST_ENGINE to the frozen candidate payload.
#![cfg(windows)]

use openunpack_core::{
    coordinator::Coordinator,
    paths::DirectoryGuard,
    types::{JobView, Settings},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::{Builder, TempDir};

const FIRST: &[u8] = b"synthetic-audit-data:first.txt\n";
const SECOND: &[u8] = b"synthetic-audit-data:second.txt\n";

struct Rig {
    root: PathBuf,
    output: PathBuf,
    seven_zip: PathBuf,
    core: Arc<Coordinator>,
    // Coordinator/Worker ownership drops before the private fixture tree.
    _temporary: TempDir,
}

impl Rig {
    fn new() -> Self {
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let worker = std::env::var_os("OPENUNPACK_TEST_WORKER")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                repository
                    .join("src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe")
            });
        let engine = std::env::var_os("OPENUNPACK_TEST_ENGINE")
            .map(PathBuf::from)
            .unwrap_or_else(|| repository.join("tools/7zip/7z.dll"));
        let seven_zip = repository.join("tools/7zip/7z.exe");
        assert!(
            worker.is_file() && engine.is_file() && seven_zip.is_file(),
            "build the current Worker and prepare the official native engine"
        );
        let temporary = Builder::new()
            .prefix("openunpack-archive-integrity-")
            .tempdir()
            .unwrap();
        let private = temporary.path().join("private");
        DirectoryGuard::open(temporary.path())
            .unwrap()
            .create_private_child(&private)
            .unwrap();
        let root = private.canonicalize().unwrap();
        let output = root.join("output");
        for directory in [&output, &root.join("archives"), &root.join("cache")] {
            fs::create_dir(directory).unwrap();
        }
        let core = Coordinator::open(root.join("data"), worker, engine).unwrap();
        Self {
            root,
            output,
            seven_zip,
            core,
            _temporary: temporary,
        }
    }

    fn settings(&self, salvage: bool, cleanup: &str) -> Settings {
        Settings {
            output_root: self.output.to_string_lossy().into_owned(),
            cache_root: self.root.join("cache").to_string_lossy().into_owned(),
            min_free_bytes: 0,
            max_expanded_bytes: 16 * 1024 * 1024,
            cache_quota_bytes: 64 * 1024 * 1024,
            expand_nested: false,
            clean_empty_dirs: false,
            salvage_corrupted: salvage,
            source_cleanup: cleanup.into(),
            ..Settings::default()
        }
    }

    fn archive(&self, name: &str, length: Option<usize>) -> PathBuf {
        let mut bytes = synthetic_tar();
        if let Some(length) = length {
            bytes.truncate(length);
        }
        let archive = self.root.join("archives").join(name);
        fs::write(&archive, bytes).unwrap();
        let result = Command::new(&self.seven_zip)
            .args(["t", "-y", "-bd"])
            .arg(&archive)
            .output()
            .expect("independent official 7-Zip must start");
        assert_eq!(
            result.status.code(),
            Some(if length.is_some() { 2 } else { 0 }),
            "independent fixture integrity: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        archive
    }

    async fn run(&self, archive: &Path, settings: Settings) -> JobView {
        self.core.save_settings(settings.clone()).unwrap();
        let scan = self
            .core
            .scan(&[archive.to_string_lossy().into_owned()])
            .unwrap();
        assert_eq!(scan.sources.len(), 1);
        let jobs = self.core.add_jobs(scan.sources, settings).unwrap();
        assert_eq!(jobs.len(), 1);
        let id = &jobs[0].id;
        self.core.start().unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let snapshot = self.core.snapshot().unwrap();
            let job = snapshot.jobs.into_iter().find(|job| &job.id == id).unwrap();
            if !snapshot.running
                && !matches!(job.status.as_str(), "queued" | "running" | "stopping")
            {
                return job;
            }
            if Instant::now() >= deadline {
                self.core.cancel(None).unwrap();
                let shutdown = Instant::now() + Duration::from_secs(10);
                while self.core.snapshot().unwrap().running && Instant::now() < shutdown {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                panic!(
                    "archive-integrity pipeline deadline: status={} stage={} message={}",
                    job.status, job.stage, job.message
                );
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }
}

fn synthetic_tar() -> Vec<u8> {
    let mut archive = Vec::new();
    for (name, payload) in [("first.txt", FIRST), ("second.txt", SECOND)] {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", payload.len()).as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        archive.extend_from_slice(&header);
        archive.extend_from_slice(payload);
        archive.resize(archive.len().div_ceil(512) * 512, 0);
    }
    archive.resize(archive.len() + 1024, 0);
    archive
}

fn digest(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).unwrap()).to_vec()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn complete_tar_publishes_both_exact_members_with_full_quality() {
    let rig = Rig::new();
    let archive = rig.archive("complete.tar", None);
    let before = digest(&archive);
    let job = rig.run(&archive, rig.settings(false, "keep")).await;
    assert_eq!(job.status, "completed", "{}", job.message);
    assert!(job.verification_scope.contains("archiveCrcAndStreamSha256"));
    assert!(!job.verification_scope.contains("salvaged"));
    let output = PathBuf::from(job.output_path.expect("complete TAR must publish"));
    assert_eq!(fs::read(output.join("first.txt")).unwrap(), FIRST);
    assert_eq!(fs::read(output.join("second.txt")).unwrap(), SECOND);
    assert_eq!(digest(&archive), before);
}

async fn damaged_tar_stays_unpublished_and_preserves_source(length: usize, name: &str) {
    let rig = Rig::new();
    let archive = rig.archive(name, Some(length));
    let before = digest(&archive);
    let job = rig.run(&archive, rig.settings(false, "delete")).await;
    assert_eq!(job.status, "failed", "{}", job.message);
    assert!(
        job.output_path.is_none(),
        "corrupt standard extraction must not publish"
    );
    assert!(
        job.verification_scope.is_empty(),
        "failed extraction must not claim verified output"
    );
    // A failed attempt can retain its marked workspace for recovery, but must
    // not publish any user-visible result directory.
    for entry in fs::read_dir(&rig.output).unwrap() {
        let entry = entry.unwrap();
        assert_eq!(entry.file_name(), ".openunpack-work");
        assert!(entry.file_type().unwrap().is_dir());
        if let Some(work) = &job.workspace {
            let work = Path::new(work);
            assert!(work.starts_with(entry.path()));
            openunpack_core::paths::owned(work, &job.id, None).unwrap();
        } else {
            assert_eq!(fs::read_dir(entry.path()).unwrap().count(), 0);
        }
    }
    assert!(
        archive.is_file(),
        "corrupt archive must survive delete-source policy"
    );
    assert_eq!(digest(&archive), before);
}

async fn damaged_tar_salvage_preserves_quality_and_source(length: usize, name: &str) {
    let rig = Rig::new();
    let archive = rig.archive(name, Some(length));
    let before = digest(&archive);
    let job = rig.run(&archive, rig.settings(true, "delete")).await;
    assert_eq!(job.status, "salvaged", "{}", job.message);
    assert!(job.verification_scope.contains("salvagedWithCrcWarning"));
    assert!(!job.verification_scope.contains("archiveCrcAndStreamSha256"));
    assert!(job.nodes.iter().any(|node| node.status == "salvaged"));
    let output = PathBuf::from(
        job.output_path
            .expect("explicit salvage should publish recovered files"),
    );
    assert_eq!(fs::read(output.join("first.txt")).unwrap(), FIRST);
    if length == 1100 {
        assert!(
            !output.join("second.txt").exists(),
            "lost member must not be fabricated"
        );
    } else {
        assert_eq!(fs::read(output.join("second.txt")).unwrap(), SECOND);
    }
    assert!(
        archive.is_file(),
        "salvaged archive must survive delete-source policy"
    );
    assert_eq!(digest(&archive), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn truncated_member_header_is_not_published_or_deleted_in_standard_mode() {
    damaged_tar_stays_unpublished_and_preserves_source(1100, "partial-header.tar").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn truncated_member_header_salvage_never_becomes_complete_or_deletes_source() {
    damaged_tar_salvage_preserves_quality_and_source(1100, "partial-header.tar").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_tar_end_is_not_published_or_deleted_in_standard_mode() {
    damaged_tar_stays_unpublished_and_preserves_source(2048, "no-end.tar").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_tar_end_salvage_never_becomes_complete_or_deletes_source() {
    damaged_tar_salvage_preserves_quality_and_source(2048, "no-end.tar").await;
}
