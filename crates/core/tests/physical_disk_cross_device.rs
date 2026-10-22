//! Physical device acceptance for SSD cache routing on real hardware.
//! Ground truth for the development machine (Windows `Get-Disk`):
//!   C: = SAMSUNG MZVLB512HBJQ NVMe SSD (disk 1), D: = TOSHIBA MQ04ABD200 SATA HDD (disk 0).
//! Both tests are `#[ignore]` by default: they require D: (physical HDD) and only run on request:
//!   cargo test -p openunpack-core --test physical_disk_cross_device -- --ignored --test-threads=1
#![cfg(windows)]

use openunpack_core::{
    coordinator::Coordinator,
    paths::{self, probe_disk_topology, probe_physical_device_number},
    route::{ExtractionRoute, RouteDecider},
    types::Settings,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn pd_root() -> PathBuf {
    std::env::var_os("OPENUNPACK_PD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("D:\\"))
        .canonicalize()
        .expect("OPENUNPACK_PD_ROOT must be an existing parent; it is never deleted")
}

struct OwnedFixture {
    root: PathBuf,
    owner: String,
}

impl OwnedFixture {
    fn new(parent: &Path, label: &str) -> Self {
        let parent = parent.canonicalize().unwrap();
        paths::check_path(&parent).unwrap();
        let owner = uuid::Uuid::new_v4().to_string();
        let root = parent.join(format!("openunpack-physical-{label}-{owner}"));
        assert!(!root.exists(), "physical fixture must be a new UUID child");
        paths::mark_owned(&root, &owner, "physical-disk-acceptance").unwrap();
        assert_eq!(root.parent(), Some(parent.as_path()));
        Self { root, owner }
    }

    fn cleanup(self) {
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(&self.owner),
            "refuse cleanup outside the newly created fixture identity"
        );
        paths::remove_owned(&self.root, &self.owner).unwrap();
    }
}

fn sha_of(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn synthetic_bytes(index: usize, len: usize) -> Vec<u8> {
    (0..len).map(|j| ((index * 97 + j) % 251) as u8).collect()
}

#[test]
#[ignore = "requires a physical HDD on D: (dev machine ground truth)"]
fn disk_topology_matches_physical_ground_truth() {
    let c = Path::new("C:\\");
    let d = Path::new("D:\\");
    assert!(d.exists(), "D: drive with the physical HDD is required");

    let same = probe_disk_topology(
        Path::new("D:\\openunpack-accept\\a.zip"),
        Path::new("D:\\openunpack-accept\\out"),
    );
    let cross = probe_disk_topology(
        Path::new("D:\\openunpack-accept\\a.zip"),
        Path::new("C:\\Users\\Public"),
    );

    // Ground truth: D: is the SATA HDD, C: is the NVMe SSD; both report device numbers.
    assert!(
        same.source_is_hdd,
        "D: must probe as HDD (ground truth: TOSHIBA MQ04ABD200)"
    );
    assert!(same.target_is_hdd, "target on D: must probe as HDD");
    assert!(
        same.is_same_physical_disk,
        "source and target on D: share one physical disk"
    );
    assert_eq!(same.source_device_number, same.target_device_number);

    assert!(
        !cross.target_is_hdd,
        "C: must probe as SSD (ground truth: SAMSUNG MZVLB512HBJQ)"
    );
    assert!(
        !cross.is_same_physical_disk,
        "C: and D: are different physical disks"
    );
    assert_ne!(cross.source_device_number, cross.target_device_number);
    assert_eq!(probe_physical_device_number(c), cross.target_device_number);
    assert_eq!(probe_physical_device_number(d), cross.source_device_number);

    // Real topology input must select the SSD cache route for a same-HDD job.
    let probe = openunpack_core::route::ArchiveProbeSummary {
        total_size_bytes: 8 * 1024 * 1024,
        estimated_extracted_bytes: 16 * 1024 * 1024,
        entry_count: 40,
        has_nested_archives: false,
        is_source_on_hdd: same.source_is_hdd,
        is_target_on_hdd: same.target_is_hdd,
        is_same_physical_disk: same.is_same_physical_disk,
    };
    let settings = Settings {
        cache_enabled: true,
        cache_policy: "always".into(),
        cache_root: std::env::temp_dir().to_string_lossy().into_owned(),
        min_free_bytes: Settings::default().min_free_bytes,
        ..Settings::default()
    };
    let route = RouteDecider::decide(
        &probe,
        &settings,
        Path::new("D:\\openunpack-accept\\out"),
        u64::MAX / 2,
    );
    assert!(
        matches!(route, ExtractionRoute::SsdCache { .. }),
        "same-HDD source and target with cache enabled must route through the SSD workspace, got {route:?}"
    );
    eprintln!("topology: same={same:?}\n cross={cross:?}\n route={route:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "real HDD -> SSD cache -> HDD pipeline run; requires D: and the built Worker"]
async fn cross_disk_hdd_source_ssd_cache_roundtrip() {
    assert!(
        Path::new("D:\\").exists(),
        "D: drive with the physical HDD is required"
    );
    let workspace = workspace_root();
    let worker = std::env::var_os("OPENUNPACK_TEST_WORKER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            workspace
                .join("src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe")
        });
    let engine = workspace.join("tools/7zip/7z.dll");
    let seven_zip = workspace.join("tools/7zip/7z.exe");
    assert!(
        worker.is_file(),
        "build the .NET Worker first: {}",
        worker.display()
    );
    assert!(
        engine.is_file() && seven_zip.is_file(),
        "official 7-Zip tools required"
    );

    // Never remove/reuse fixed children of a user-selected root. Each run creates
    // an exclusive private UUID directory and removes only that owned identity.
    // A failed run is retained, rather than deleting evidence during unwinding.
    let hdd_fixture = OwnedFixture::new(&pd_root(), "hdd");
    let root = &hdd_fixture.root;
    assert!(
        paths::free_bytes(root).unwrap() >= 1024u64.pow(3) + 32 * 1024 * 1024,
        "physical fixture must preserve at least 1 GiB of free space"
    );
    let tree = root.join("source-tree");
    fs::create_dir(&tree).unwrap();
    let mut expected: BTreeMap<String, String> = BTreeMap::new();
    const FILES: usize = 12;
    const LEN: usize = 512 * 1024;
    for index in 0..FILES {
        let bytes = synthetic_bytes(index, LEN);
        let name = format!("pd-file-{index:02}.bin");
        fs::write(tree.join(&name), &bytes).unwrap();
        expected.insert(name, sha_of(&bytes));
    }
    let archive = root.join("pd-source.zip");
    let status = Command::new(&seven_zip)
        .current_dir(&tree)
        .args(["a", "-mx=1", "-y", "-bso0", "-bsp0", "-bse0", "-tzip"])
        .arg(&archive)
        .arg(".")
        .status()
        .expect("7z must start");
    assert!(status.success(), "fixture archive creation failed");

    // Data store and SSD cache workspace live on the NVMe (C: via TEMP).
    let ssd_fixture = OwnedFixture::new(&std::env::temp_dir(), "ssd");
    assert!(
        paths::free_bytes(&ssd_fixture.root).unwrap() >= 1024u64.pow(3) + 32 * 1024 * 1024,
        "cache fixture must preserve at least 1 GiB of free space"
    );
    let cache_dir = ssd_fixture.root.join("cache");
    fs::create_dir(&cache_dir).unwrap();
    let output_root = root.join("output");
    fs::create_dir_all(&output_root).unwrap();
    let actual_hdd = probe_disk_topology(&archive, &output_root);
    let actual_cache = probe_disk_topology(&archive, &cache_dir);
    assert!(
        actual_hdd.source_is_hdd && actual_hdd.target_is_hdd && actual_hdd.is_same_physical_disk,
        "selected fixture parent must really be on one physical HDD"
    );
    assert!(
        actual_cache.target_device_number.is_some()
            && !actual_cache.target_is_hdd
            && !actual_cache.is_same_physical_disk,
        "TEMP cache must really be on a separate physical SSD"
    );

    let core = Arc::new(Coordinator::open(ssd_fixture.root.join("data"), worker, engine).unwrap());
    let settings = Settings {
        // Native pickers return canonical extended paths, even for short names.
        output_root: output_root
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        cache_root: cache_dir
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        cache_enabled: true,
        cache_policy: "always".into(),
        // Exercise the user's default reserve; zero bypasses Worker space checks.
        min_free_bytes: Settings::default().min_free_bytes,
        max_expanded_bytes: 256 * 1024 * 1024,
        cache_quota_bytes: 512 * 1024 * 1024,
        ..Settings::default()
    };
    assert_eq!(settings.min_free_bytes, 1024u64.pow(3));
    assert!(settings.cache_root.starts_with(r"\\?\"));
    assert!(settings.output_root.starts_with(r"\\?\"));
    core.save_settings(settings.clone()).unwrap();
    let scanned = core
        .scan(&[archive.to_string_lossy().into_owned()])
        .unwrap();
    assert_eq!(scanned.sources.len(), 1);
    let jobs = core.add_jobs(scanned.sources, settings).unwrap();
    assert_eq!(jobs.len(), 1);
    let job_id = jobs[0].id.clone();
    core.start().unwrap();

    let deadline = Instant::now() + Duration::from_secs(240);
    let mut max_cache_used = 0u64;
    let job;
    loop {
        let snapshot = core.snapshot().unwrap();
        max_cache_used = max_cache_used.max(snapshot.cache_used_bytes);
        let current = snapshot
            .jobs
            .into_iter()
            .find(|job| job.id == job_id)
            .unwrap();
        if !snapshot.running
            && !matches!(current.status.as_str(), "queued" | "running" | "stopping")
        {
            job = current;
            break;
        }
        assert!(
            Instant::now() < deadline,
            "cross-disk pipeline timed out: {current:?}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(job.status, "completed", "job failed: {}", job.message);
    assert!(
        max_cache_used > 0,
        "SSD cache workspace was never engaged on a same-HDD job (cache_used stayed 0)"
    );

    // Published bytes on the HDD must match every source file exactly.
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for entry in walkdir::WalkDir::new(&output_root)
        .into_iter()
        .map(Result::unwrap)
    {
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("pd-file-") {
                let bytes = fs::read(entry.path()).unwrap();
                seen.insert(name, sha_of(&bytes));
            }
        }
    }
    assert_eq!(
        seen, expected,
        "published bytes must equal the physical-HDD source files"
    );

    // Cache workspace must drain back to zero after publish/cleanup.
    let drain_deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let used = core.snapshot().unwrap().cache_used_bytes;
        if used == 0 {
            break;
        }
        assert!(
            Instant::now() < drain_deadline,
            "cache_used_bytes never returned to 0 (last={used})"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    eprintln!(
        "cross-disk ok: files={FILES}, max_cache_used={max_cache_used}, archive={}",
        archive.display()
    );
    drop(core);
    ssd_fixture.cleanup();
    hdd_fixture.cleanup();
}
