//! Real Windows acceptance against small pinned MIT-licensed upstream archives.
//! Passwords are public fixture constants; payload executables are bytes only and
//! never launched. Every published file is checked against independent upstream
//! Original-file SHA-256, not just an engine return code or nonempty directory.
#![cfg(windows)]

use openunpack_core::{
    coordinator::Coordinator,
    types::{JobView, PasswordInput, Settings},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::TempDir;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    files: Vec<FixtureFile>,
    expected_files: Vec<ExpectedFile>,
}
#[derive(Deserialize)]
struct FixtureFile {
    file: String,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedFile {
    relative_path: String,
    size: u64,
    sha256: String,
    #[serde(default)]
    variants: Vec<ExpectedVariant>,
}
#[derive(Deserialize)]
struct ExpectedVariant {
    size: u64,
    sha256: String,
}

struct Rig {
    root: PathBuf,
    fixtures: PathBuf,
    output: PathBuf,
    seven_zip: PathBuf,
    manifest: Manifest,
    core: Arc<Coordinator>,
    _temporary: TempDir,
}
impl Rig {
    fn new() -> Self {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let fixtures = workspace.join("tests/fixtures/upstream-archives");
        let manifest: Manifest = serde_json::from_slice(
            &fs::read(fixtures.join("provenance.json"))
                .expect("pinned fixtures are part of the source tree"),
        )
        .unwrap();
        for record in &manifest.files {
            assert_eq!(
                digest(&fixtures.join(&record.file)),
                record.sha256,
                "upstream fixture integrity: {}",
                record.file
            );
        }
        let worker = std::env::var_os("OPENUNPACK_TEST_WORKER")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                workspace
                    .join("src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe")
            });
        let engine = workspace.join("tools/7zip/7z.dll");
        assert!(
            worker.is_file() && engine.is_file(),
            "prepare the official engine and build the real .NET Worker before format_matrix"
        );
        let temporary = tempfile::Builder::new()
            .prefix("openunpack-formats-")
            .tempdir()
            .unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let output = root.join("output");
        fs::create_dir(&output).unwrap();
        let core = Coordinator::open(root.join("data"), worker, engine).unwrap();
        Self {
            root,
            fixtures,
            output,
            manifest,
            core,
            seven_zip: workspace.join("tools/7zip/7z.exe"),
            _temporary: temporary,
        }
    }
    fn settings(&self) -> Settings {
        Settings {
            output_root: self.output.to_string_lossy().into_owned(),
            min_free_bytes: 0,
            expand_nested: false,
            max_expanded_bytes: 16 * 1024 * 1024,
            ..Settings::default()
        }
    }
    fn copy_family(&self, names: &[String]) -> Vec<PathBuf> {
        let input = self.root.join("archives");
        fs::create_dir_all(&input).unwrap();
        names
            .iter()
            .map(|name| {
                let path = input.join(name);
                fs::copy(self.fixtures.join(name), &path).unwrap();
                path
            })
            .collect()
    }
    fn saved_password(&self) -> String {
        self.core
            .vault()
            .unwrap()
            .upsert(PasswordInput {
                id: None,
                name: "public-upstream-fixture-password".into(),
                secret: Some("test".into()),
                groups: vec![],
                notes: "Public synthetic SharpCompress regression fixture".into(),
                enabled: true,
            })
            .unwrap()
            .id
    }
    async fn run(&self, path: &Path, expected_volumes: usize) -> JobView {
        let settings = self.settings();
        self.core.save_settings(settings.clone()).unwrap();
        let scan = self
            .core
            .scan(&[path.to_string_lossy().into_owned()])
            .unwrap();
        assert_eq!(
            scan.sources.len(),
            1,
            "one selected volume must become exactly one root job"
        );
        assert_eq!(scan.sources[0].volumes.len(), expected_volumes);
        assert!(!scan.sources[0].missing_volume);
        let originals: Vec<_> = scan.sources[0]
            .volumes
            .iter()
            .map(|path| (PathBuf::from(path), digest(Path::new(path))))
            .collect();
        let id = self
            .core
            .add_jobs(scan.sources, settings)
            .unwrap()
            .remove(0)
            .id;
        self.core.start().unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        let job = loop {
            let snapshot = self.core.snapshot().unwrap();
            let job = snapshot.jobs.into_iter().find(|job| job.id == id).unwrap();
            if !snapshot.running {
                break job;
            }
            if Instant::now() > deadline {
                let _ = self.core.cancel(None);
                panic!(
                    "real format pipeline exceeded its bound: {} {}",
                    job.stage, job.status
                );
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert_eq!(
            job.status,
            "completed",
            "{}: stage={} message={}",
            path.file_name().unwrap().to_string_lossy(),
            job.stage,
            job.message
        );
        for (path, hash) in originals {
            assert_eq!(digest(&path), hash, "source volume changed");
        }
        job
    }
    fn verify_upstream_original(&self, job: &JobView) {
        let output = Path::new(job.output_path.as_ref().unwrap());
        let files: Vec<_> = walkdir::WalkDir::new(output)
            .into_iter()
            .map(Result::unwrap)
            .filter(|entry| {
                entry.file_type().is_file()
                    && entry.file_name() != openunpack_core::paths::OWNER_FILE
            })
            .collect();
        assert_eq!(
            files.len(),
            self.manifest.expected_files.len(),
            "all expected upstream payload files must be delivered, with no additional payload"
        );
        for expected in &self.manifest.expected_files {
            let path = output.join(&expected.relative_path);
            assert!(
                path.is_file(),
                "expected path {} missing in {:?}",
                expected.relative_path,
                files.iter().map(|entry| entry.path()).collect::<Vec<_>>()
            );
            let size = fs::metadata(&path).unwrap().len();
            let actual = digest(&path);
            let matched = (size == expected.size && actual == expected.sha256)
                || expected
                    .variants
                    .iter()
                    .any(|variant| size == variant.size && actual == variant.sha256);
            assert!(
                matched,
                "independent upstream byte checksum mismatch: {} ({} bytes)",
                expected.relative_path, size
            );
        }
        assert!(
            job.workspace.is_none(),
            "verified successful format job must release its workspace"
        );
    }
}
fn digest(path: &Path) -> String {
    let mut file = fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    hex::encode(hash.finalize())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tar_with_mz_filename_is_scanned_and_delivered_as_archive_not_executable() {
    let rig = Rig::new();
    let content = b"TAR filename MZ is data, not an executable prefix";
    for (name, ustar) in [("mz-ustar.bin", true), ("mz-v7.bin", false)] {
        let mut header = [0u8; 512];
        header[..11].copy_from_slice(b"MZ-file.txt");
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", content.len()).as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].fill(b' ');
        if ustar {
            header[156] = b'0';
            header[257..263].copy_from_slice(b"ustar\0");
            header[263..265].copy_from_slice(b"00");
        }
        let checksum: u32 = header.iter().map(|&b| u32::from(b)).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(content);
        bytes.resize(2048, 0);
        let archive = rig.root.join(name);
        fs::write(&archive, bytes).unwrap();
        let job = rig.run(&archive, 1).await;
        let output = Path::new(job.output_path.as_ref().unwrap());
        assert_eq!(fs::read(output.join("MZ-file.txt")).unwrap(), content);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rar4_and_rar5_plain_and_both_encryption_modes_match_independent_payload_bytes() {
    let rig = Rig::new();
    let password = rig.saved_password();
    for name in [
        "Rar.rar",
        "Rar5.rar",
        "Rar.encrypted_filesOnly.rar",
        "Rar.encrypted_filesAndHeader.rar",
        "Rar5.encrypted_filesOnly.rar",
        "Rar5.encrypted_filesAndHeader.rar",
    ] {
        let paths = rig.copy_family(&[name.into()]);
        let job = rig.run(&paths[0], 1).await;
        rig.verify_upstream_original(&job);
    }
    assert_eq!(
        rig.core
            .vault()
            .unwrap()
            .views()
            .unwrap()
            .into_iter()
            .find(|item| item.id == password)
            .unwrap()
            .success_count,
        4,
        "only the four encrypted archives actually use and credit the saved fixture password"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rar_part_volumes_and_traditional_r00_family_accept_middle_volume_selection() {
    let rig = Rig::new();
    rig.saved_password();
    for (stem, count) in [
        ("Rar4.multi", 7),
        ("Rar5.multi", 6),
        ("Rar.EncryptedParts", 6),
    ] {
        let names: Vec<_> = (1..=count)
            .map(|n| format!("{stem}.part{n:02}.rar"))
            .collect();
        let paths = rig.copy_family(&names);
        let job = rig.run(&paths[2], count).await;
        rig.verify_upstream_original(&job);
    }
    let names: Vec<_> = std::iter::once("Rar2.multi.rar".into())
        .chain((0..=5).map(|n| format!("Rar2.multi.r{n:02}")))
        .collect();
    let paths = rig.copy_family(&names);
    let job = rig.run(&paths[4], 7).await;
    rig.verify_upstream_original(&job);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn z01_zip_volume_groups_from_infozip_and_winzip_match_independent_payload_bytes() {
    let rig = Rig::new();
    for stem in ["Infozip.nocomp.multi", "WinZip26.nocomp.multi"] {
        let names = vec![format!("{stem}.z01"), format!("{stem}.zip")];
        let paths = rig.copy_family(&names);
        let job = rig.run(&paths[0], 2).await;
        rig.verify_upstream_original(&job);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn numeric_zip_volumes_are_really_extracted_when_a_middle_volume_is_selected() {
    let rig = Rig::new();
    let payload = rig.root.join("synthetic");
    fs::create_dir(&payload).unwrap();
    fs::write(payload.join("data.bin"), vec![0x57u8; 96 * 1024]).unwrap();
    let archives = rig.root.join("archives");
    fs::create_dir_all(&archives).unwrap();
    let archive = archives.join("numeric.zip");
    let status = Command::new(&rig.seven_zip)
        .current_dir(&payload)
        .args([
            "a", "-tzip", "-mx=0", "-v8k", "-y", "-bso0", "-bsp0", "-bse0",
        ])
        .arg(&archive)
        .arg(".")
        .status()
        .unwrap();
    assert!(status.success(), "official fixture generator failed");
    let volumes: Vec<_> = fs::read_dir(&archives)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(volumes.len() > 2);
    let job = rig
        .run(&archives.join("numeric.zip.003"), volumes.len())
        .await;
    assert_eq!(
        fs::read(Path::new(job.output_path.as_ref().unwrap()).join("data.bin")).unwrap(),
        vec![0x57u8; 96 * 1024]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn batch_zip_and_7z_with_realtime_progress_broadcast() {
    let rig = Rig::new();
    let payload_a = rig.root.join("synthetic_a");
    let payload_b = rig.root.join("synthetic_b");
    fs::create_dir_all(&payload_a).unwrap();
    fs::create_dir_all(&payload_b).unwrap();
    let bytes_a = vec![0x41u8; 128 * 1024]; // 128 KB of 'A'
    let bytes_b = vec![0x42u8; 128 * 1024]; // 128 KB of 'B'
    fs::write(payload_a.join("file_a.txt"), &bytes_a).unwrap();
    fs::write(payload_b.join("file_b.txt"), &bytes_b).unwrap();

    let archives = rig.root.join("batch_archives");
    fs::create_dir_all(&archives).unwrap();
    let zip_archive = archives.join("batch_archive.zip");
    let sz_archive = archives.join("batch_archive.7z");

    let status_zip = Command::new(&rig.seven_zip)
        .current_dir(&payload_a)
        .args(["a", "-tzip", "-mx=5", "-y", "-bso0", "-bsp0", "-bse0"])
        .arg(&zip_archive)
        .arg(".")
        .status()
        .unwrap();
    assert!(status_zip.success(), "7z creating zip failed");

    let status_7z = Command::new(&rig.seven_zip)
        .current_dir(&payload_b)
        .args(["a", "-t7z", "-mx=5", "-y", "-bso0", "-bsp0", "-bse0"])
        .arg(&sz_archive)
        .arg(".")
        .status()
        .unwrap();
    assert!(status_7z.success(), "7z creating 7z failed");

    let settings = rig.settings();
    rig.core.save_settings(settings.clone()).unwrap();

    let scan = rig
        .core
        .scan(&[
            zip_archive.to_string_lossy().into_owned(),
            sz_archive.to_string_lossy().into_owned(),
        ])
        .unwrap();
    assert_eq!(
        scan.sources.len(),
        2,
        "must discover both ZIP and 7z sources"
    );

    let mut rx = rig.core.subscribe();

    let added_jobs = rig.core.add_jobs(scan.sources, settings).unwrap();
    assert_eq!(added_jobs.len(), 2);
    let job_ids: Vec<_> = added_jobs.iter().map(|j| j.id.clone()).collect();

    rig.core.start().unwrap();

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut notification_count = 0u64;
    let mut saw_positive_progress = false;

    loop {
        tokio::select! {
            res = rx.recv() => {
                if res.is_ok() {
                    notification_count += 1;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }

        let snapshot = rig.core.snapshot().unwrap();
        for job in &snapshot.jobs {
            if job_ids.contains(&job.id) && job.progress > 0.0 {
                saw_positive_progress = true;
            }
        }

        let all_done = snapshot
            .jobs
            .iter()
            .filter(|j| job_ids.contains(&j.id))
            .all(|j| j.status == "completed" || j.status == "failed");

        if all_done && !snapshot.running {
            break;
        }

        if Instant::now() > deadline {
            let _ = rig.core.cancel(None);
            panic!("Batch ZIP & 7z extraction timed out");
        }
    }

    assert!(
        notification_count > 0,
        "realtime state-changed broadcast must notify subscribers"
    );
    assert!(
        saw_positive_progress,
        "must observe positive progress during extraction"
    );

    let final_snapshot = rig.core.snapshot().unwrap();
    for job_id in &job_ids {
        let job = final_snapshot
            .jobs
            .iter()
            .find(|j| j.id == *job_id)
            .expect("job found");
        assert_eq!(job.status, "completed", "job {} should complete", job_id);
        assert_eq!(job.stage, "finished");
        assert!(
            (job.progress - 1.0).abs() < f64::EPSILON,
            "final progress must be 1.0"
        );

        let out = Path::new(job.output_path.as_ref().expect("output path present"));
        if job.source.path.ends_with(".zip") {
            assert_eq!(fs::read(out.join("file_a.txt")).unwrap(), bytes_a);
        } else if job.source.path.ends_with(".7z") {
            assert_eq!(fs::read(out.join("file_b.txt")).unwrap(), bytes_b);
        }
    }
}
