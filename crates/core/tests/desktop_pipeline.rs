//! Windows acceptance tests across Coordinator -> named pipes -> .NET Worker -> official 7-Zip.
//! All archives, passwords, data stores and outputs are synthetic and tempfile-owned.
//! Requires a built Worker plus tools/7zip/{7z.exe,7z.dll}; no fake backend is used.
#![cfg(windows)]

use openunpack_core::{
    coordinator::Coordinator,
    scanner,
    types::{CopyFile, JobView, MAX_MESSAGE_BYTES, PasswordInput, Settings, WorkerRequest},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::{Builder, TempDir};

struct Rig {
    root: PathBuf,
    output: PathBuf,
    cache: PathBuf,
    seven_zip: PathBuf,
    core: Arc<Coordinator>,
    // Drop database/worker ownership before tempfile recursively removes the fixture tree on Windows.
    _temporary: TempDir,
}

async fn sampled_cache_bytes(cache: &Path) -> u64 {
    for attempt in 0..10 {
        let sample = walkdir::WalkDir::new(cache).into_iter().try_fold(
            0u64,
            |bytes, entry| -> Result<u64, walkdir::Error> {
                let entry = entry?;
                Ok(bytes
                    + if entry.file_type().is_file() {
                        entry.metadata()?.len()
                    } else {
                        0
                    })
            },
        );
        match sample {
            Ok(bytes) => return bytes,
            Err(error)
                if attempt < 9
                    && matches!(
                        error.io_error().and_then(|io| io.raw_os_error()),
                        Some(2 | 3 | 5 | 32)
                    ) =>
            {
                // Cleanup can unlink a directory after enumeration. Windows
                // also reports delete-pending directories as access denied.
                // Retry the entire sample; never count an unreadable tree as 0.
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(error) => panic!("cannot obtain a complete cache sample: {error}"),
        }
    }
    unreachable!()
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
        let seven_zip = workspace.join("tools/7zip/7z.exe");
        assert!(
            worker.is_file(),
            "Build the .NET Worker before running desktop_pipeline acceptance tests"
        );
        assert!(
            engine.is_file() && seven_zip.is_file(),
            "Prepare the official 7-Zip engine before running desktop_pipeline"
        );
        let temporary = Builder::new()
            .prefix("openunpack-pipeline-")
            .tempdir()
            .unwrap();
        // The system TEMP directory may grant other accounts Modify access.
        // Acceptance sources must use the same private ACL boundary as owned
        // application workspaces; unsafe shared-source recycling is tested apart.
        let private = temporary.path().join("private");
        openunpack_core::paths::DirectoryGuard::open(temporary.path())
            .unwrap()
            .create_private_child(&private)
            .unwrap();
        let root = private.canonicalize().unwrap();
        let output = root.join("output");
        let cache = root.join("ssd-work");
        fs::create_dir_all(&output).unwrap();
        fs::create_dir_all(&cache).unwrap();
        fs::create_dir_all(root.join("archives")).unwrap();
        let core = Coordinator::open(root.join("data"), worker, engine).unwrap();
        Self {
            _temporary: temporary,
            root,
            output,
            cache,
            seven_zip,
            core,
        }
    }

    fn settings(&self) -> Settings {
        Settings {
            output_root: self.output.to_string_lossy().into_owned(),
            cache_root: self.cache.to_string_lossy().into_owned(),
            min_free_bytes: 0,
            max_expanded_bytes: 256 * 1024 * 1024,
            cache_quota_bytes: 512 * 1024 * 1024,
            max_depth: 2,
            clean_empty_dirs: false,
            ..Settings::default()
        }
    }

    fn password(&self, label: &str, synthetic_secret: &str) -> String {
        self.core
            .vault()
            .unwrap()
            .upsert(PasswordInput {
                id: None,
                name: label.into(),
                secret: Some(synthetic_secret.into()),
                groups: vec![],
                notes: "Synthetic integration fixture".into(),
                enabled: true,
            })
            .unwrap()
            .id
    }

    fn file(&self, relative: &str, content: &[u8]) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    fn pack(
        &self,
        directory: &Path,
        target: &Path,
        format: &str,
        password: Option<&str>,
        extra: &[&str],
    ) {
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        let mut command = Command::new(&self.seven_zip);
        command
            .current_dir(directory)
            .args(["a", "-mx=0", "-y", "-bso0", "-bsp0", "-bse0"])
            .arg(format!("-t{format}"));
        if let Some(password) = password {
            command.arg(format!("-p{password}"));
            if format == "7z" {
                command.arg("-mhe=on");
            }
        }
        command.args(extra).arg(target).arg(".");
        // Do not print command arguments: even fixture passwords do not belong in logs.
        let status = command
            .status()
            .expect("official 7-Zip fixture packer must start");
        assert!(status.success(), "official 7-Zip fixture creation failed");
    }

    fn add(&self, archive: &Path, settings: Settings) -> String {
        self.core.save_settings(settings.clone()).unwrap();
        let result = self
            .core
            .scan(&[archive.to_string_lossy().into_owned()])
            .unwrap();
        assert_eq!(
            result.sources.len(),
            1,
            "fixture must resolve to one archive group"
        );
        let jobs = self.core.add_jobs(result.sources, settings).unwrap();
        assert_eq!(
            jobs.len(),
            1,
            "a terminal previous attempt must not block a new queue item"
        );
        jobs[0].id.clone()
    }

    async fn finish(&self, id: &str) -> JobView {
        self.finish_with_timeout(id, Duration::from_secs(90)).await
    }

    async fn finish_with_timeout(&self, id: &str, duration: Duration) -> JobView {
        let started = Instant::now();
        let deadline = started + duration;
        let mut last_log = Instant::now() - Duration::from_secs(10);
        let mut last_stage = String::new();
        loop {
            let snapshot = self.core.snapshot().unwrap();
            let job = snapshot.jobs.into_iter().find(|job| job.id == id).unwrap();
            if job.stage != last_stage || last_log.elapsed() >= Duration::from_secs(3) {
                eprintln!(
                    "[{:.1}s] status={} stage={} progress={:.2} msg={}",
                    started.elapsed().as_secs_f64(),
                    job.status,
                    job.stage,
                    job.progress,
                    job.message
                );
                last_stage = job.stage.clone();
                last_log = Instant::now();
            }
            if !snapshot.running
                && !matches!(job.status.as_str(), "queued" | "running" | "stopping")
            {
                return job;
            }
            if Instant::now() >= deadline {
                let _ = self.core.cancel(None);
                // Allow the supervisor to observe cancellation and reap the child before tempfile drops.
                let shutdown = Instant::now() + Duration::from_secs(10);
                while self.core.snapshot().unwrap().running && Instant::now() < shutdown {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                panic!(
                    "pipeline did not finish within its bound: status={} stage={} message={}",
                    job.status, job.stage, job.message
                );
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    }

    async fn run(&self, id: &str) -> JobView {
        self.core.start().unwrap();
        self.finish(id).await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn automatic_cache_with_unknown_contents_uses_direct_workspace_and_exact_output() {
    let rig = Rig::new();
    let expected = b"Native automatic-cache route must preserve every byte.";
    let input = rig.file("auto-input/payload.txt", expected);
    let archive = rig.root.join("archives/auto-direct.zip");
    rig.pack(input.parent().unwrap(), &archive, "zip", None, &[]);
    let archive_before = fs::read(&archive).unwrap();
    let mut settings = rig.settings();
    settings.cache_enabled = true;
    settings.cache_policy = "auto".into();
    settings.expand_nested = true;
    settings.verification = "strict".into();
    let id = rig.add(&archive, settings);
    rig.core.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut observed_direct_workspace = false;
    loop {
        let view = rig.core.snapshot().unwrap();
        let job = view.jobs.iter().find(|job| job.id == id).unwrap();
        if let Some(workspace) = &job.workspace {
            assert!(Path::new(workspace).starts_with(rig.output.join(".openunpack-work")));
            observed_direct_workspace = true;
        }
        if !view.running {
            break;
        }
        if Instant::now() >= deadline {
            rig.core.cancel(None).unwrap();
            let _ = rig.finish(&id).await;
            panic!("automatic-route fixture did not finish in its deadline");
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let finished = rig.finish(&id).await;
    assert_eq!(finished.status, "completed", "{}", finished.message);
    assert!(
        !finished.stage_times.contains_key("copying"),
        "automatic direct routing must not add a second same-volume full copy"
    );
    assert!(
        observed_direct_workspace,
        "the actual owned workspace must be observed"
    );
    assert_eq!(
        fs::read(Path::new(finished.output_path.as_ref().unwrap()).join("payload.txt")).unwrap(),
        expected
    );
    assert_eq!(fs::read(&archive).unwrap(), archive_before);
    let cache_leftovers: Vec<String> = fs::read_dir(&rig.cache)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(cache_leftovers.is_empty(), "cache workspace residue: {cache_leftovers:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_cached_resume_keeps_actual_cache_quota_after_automatic_route_changes() {
    let rig = Rig::new();
    let secret = "synthetic-legacy-resume-only";
    let inner_file = rig.file("legacy-inner/secret.txt", b"retained legacy fixture");
    let inner = rig.root.join("legacy-outer/protected.7z");
    rig.pack(
        inner_file.parent().unwrap(),
        &inner,
        "7z",
        Some(secret),
        &[],
    );
    rig.file("legacy-outer/retained.bin", &vec![0x43; 1024 * 1024]);
    let archive = rig.root.join("archives/legacy-resume.zip");
    rig.pack(inner.parent().unwrap(), &archive, "zip", None, &[]);
    let original = fs::read(&archive).unwrap();
    let mut settings = rig.settings();
    settings.cache_enabled = true;
    settings.cache_policy = "always".into();
    settings.cache_input = false;
    let id = rig.add(&archive, settings);
    rig.core.start().unwrap();
    let retained = rig.finish(&id).await;
    assert_eq!(retained.status, "needsPassword", "{}", retained.message);
    let work = PathBuf::from(retained.workspace.as_ref().unwrap());
    assert!(work.starts_with(rig.cache.join("OpenUnpack-cache")));
    assert!(retained.output_path.is_none());
    let deadline = Instant::now() + Duration::from_secs(5);
    while Arc::strong_count(&rig.core) != 1 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(Arc::strong_count(&rig.core), 1);
    let Rig {
        core,
        root,
        output,
        _temporary,
        ..
    } = rig;
    drop(core);
    // Old checkpoints have no cachePolicy; serde reads them as auto. The
    // currently selected route is then direct, but their bytes remain cached.
    let checkpoint = work.join("pipeline.json");
    let mut saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    saved["options"]
        .as_object_mut()
        .unwrap()
        .remove("cachePolicy");
    fs::write(&checkpoint, serde_json::to_vec_pretty(&saved).unwrap()).unwrap();
    let repository =
        openunpack_core::repository::Repository::open(&root.join("data/jobs.sqlite")).unwrap();
    let mut job = repository
        .jobs()
        .unwrap()
        .into_iter()
        .find(|job| job.id == id)
        .unwrap();
    job.options.cache_policy = "auto".into();
    job.options.cache_quota_bytes = 4096;
    repository.save_job(&job).unwrap();
    drop(repository);
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
    let reopened = Coordinator::open(
        root.join("data"),
        worker,
        workspace.join("tools/7zip/7z.dll"),
    )
    .unwrap();
    // Retrying uses the current application settings, not the row's old
    // options. Keep that real API behavior in the migration fixture.
    reopened.save_settings(job.options.clone()).unwrap();
    reopened
        .retry_with_password(&id, false, Some(secret.into()))
        .unwrap();
    reopened.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let resumed = loop {
        let view = reopened.snapshot().unwrap();
        let job = view.jobs.into_iter().find(|job| job.id == id).unwrap();
        if !view.running {
            break job;
        }
        if Instant::now() >= deadline {
            reopened.cancel(None).unwrap();
            panic!("legacy cached resume did not stop within its deadline");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!(
        resumed.status, "failed",
        "old cached bytes must remain quota-limited: {}",
        resumed.message
    );
    assert_eq!(
        resumed.message,
        "磁盘空间、缓存配额或展开限制不足，请调整后重试。"
    );
    assert!(resumed.output_path.is_none());
    assert!(!output.join("legacy-resume").exists());
    assert_eq!(
        resumed.workspace.as_deref(),
        Some(work.to_string_lossy().as_ref())
    );
    assert_eq!(fs::read(&archive).unwrap(), original);
    drop(reopened);
    drop(_temporary);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_transfer_lists_copy_and_readback_all_real_file_bytes() {
    let rig = Rig::new();
    let count = 16_000;
    let suffix = "a".repeat(165);
    for index in 0..count {
        let relative = format!("payload/{index:05}-{suffix}.txt");
        rig.file(&relative, &(index as u32).to_le_bytes());
    }
    let archive = rig.root.join("archives/large-list.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &[]);
    let original = digest_file(&archive);
    let mut settings = rig.settings();
    settings.cache_enabled = true;
    settings.cache_policy = "always".into();
    settings.verification = "strict".into();
    let expected_root = rig.output.join("large-list");
    let expected_records: Vec<_> = (0..count)
        .map(|index| {
            let relative = format!("{index:05}-{suffix}.txt");
            CopyFile {
                source_path: expected_root.join(&relative).to_string_lossy().into_owned(),
                relative_path: relative,
                size: 4,
                size_known: true,
                sha256: "a".repeat(64),
                is_directory: false,
                is_corrupted: false,
            }
        })
        .collect();
    let expected_wire = serde_json::to_vec(&serde_json::json!({
        "protocolVersion": 2, "requestId": "original-full-list", "operation": "readback",
        "operationId": "original-full-operation", "generation": 1,
        "destinationDirectory": expected_root.to_string_lossy(), "files": expected_records,
        "maxEntries": 100_000, "maxTotalBytes": 256 * 1024 * 1024,
        "maxWrittenBytes": 256 * 1024 * 1024, "verification": "strict"
    }))
    .unwrap()
    .len();
    assert!(
        expected_wire > MAX_MESSAGE_BYTES,
        "fixture must exceed the original request budget"
    );
    eprintln!(
        "large-list fixture: {count} real files, original request >= {expected_wire} bytes, IPC limit {MAX_MESSAGE_BYTES}"
    );
    let id = rig.add(&archive, settings);
    rig.core.start().unwrap();
    // This test creates, extracts, copies and reads back 16k real small files;
    // allow filesystem/antivirus overhead without changing any product budget.
    let job = rig.finish_with_timeout(&id, Duration::from_secs(600)).await;
    let output = delivered(&job);
    let mut records = Vec::with_capacity(count);
    for index in 0..count {
        let relative = format!("{index:05}-{suffix}.txt");
        let file = output.join(&relative);
        let expected = (index as u32).to_le_bytes();
        assert_eq!(
            fs::read(&file).unwrap(),
            expected,
            "file {index} must survive every batch"
        );
        records.push(CopyFile {
            source_path: file.to_string_lossy().into_owned(),
            relative_path: relative,
            size: expected.len() as u64,
            size_known: true,
            sha256: hex::encode(Sha256::digest(expected)),
            is_directory: false,
            is_corrupted: false,
        });
    }
    let unsplit: WorkerRequest = serde_json::from_value(serde_json::json!({
        "protocolVersion": 2, "requestId": "original-full-list", "operation": "readback",
        "operationId": "original-full-operation", "generation": 1,
        "destinationDirectory": output.to_string_lossy(), "files": records,
        "maxEntries": 100_000, "maxTotalBytes": 256 * 1024 * 1024,
        "maxWrittenBytes": 256 * 1024 * 1024, "verification": "strict"
    }))
    .unwrap();
    let original_wire_bytes = serde_json::to_vec(&unsplit).unwrap().len();
    assert!(
        original_wire_bytes > MAX_MESSAGE_BYTES,
        "fixture must trigger the original request limit, got {original_wire_bytes} bytes"
    );
    assert_eq!(
        walkdir::WalkDir::new(&output)
            .into_iter()
            .map(Result::unwrap)
            .filter(|entry| entry.file_type().is_file()
                && entry.file_name() != ".openunpack-owner.json")
            .count(),
        count
    );
    assert!(job.stage_times.contains_key("copying"));
    assert!(job.stage_times.contains_key("verifying"));
    assert!(job.verification_scope.contains("targetReadback"));
    assert!(
        job.workspace.is_none(),
        "successful batches must release owned scratch directories"
    );
    assert_eq!(
        digest_file(&archive),
        original,
        "source archive must stay unchanged"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paused_batch_finishes_after_all_remaining_jobs_are_cancelled() {
    let rig = Rig::new();
    rig.file("payload/large.bin", &vec![0x6du8; 32 * 1024 * 1024]);
    let archive = rig.root.join("archives/active.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &[]);
    let second_archive = rig.root.join("archives/pending.zip");
    fs::copy(&archive, &second_archive).unwrap();
    let first = rig.add(&archive, rig.settings());
    let pending = rig.add(&second_archive, rig.settings());
    rig.core.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = rig.core.snapshot().unwrap();
        if snapshot
            .jobs
            .iter()
            .any(|job| job.id == first && job.status == "running")
        {
            break;
        }
        assert!(Instant::now() < deadline, "first job must become active");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    rig.core.pause().unwrap();
    rig.core.cancel(Some(&pending)).unwrap();
    rig.core.cancel(Some(&first)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while rig.core.snapshot().unwrap().running && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let snapshot = rig.core.snapshot().unwrap();
    if snapshot.running {
        rig.core.cancel(None).unwrap(); // Reap the old failing implementation before fixture cleanup.
        rig.finish(&first).await;
    }
    assert!(
        !snapshot.running,
        "a paused queue with no remaining work must release running state"
    );
    for id in [&first, &pending] {
        let job = snapshot.jobs.iter().find(|job| &job.id == id).unwrap();
        assert_eq!(job.status, "cancelled");
        assert!(job.output_path.is_none());
    }
    assert!(archive.exists() && second_archive.exists());
}

fn digest_file(path: &Path) -> Vec<u8> {
    let mut file = fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let read = file.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    hash.finalize().to_vec()
}

fn delivered(job: &JobView) -> PathBuf {
    assert_eq!(
        job.status, "completed",
        "stage={} message={}",
        job.stage, job.message
    );
    PathBuf::from(
        job.output_path
            .as_ref()
            .expect("completed jobs must name their published result"),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssd_nested_distinct_passwords_preserves_paths_and_standard_scope() {
    let rig = Rig::new();
    let outer_id = rig.password("outer-fixture", "SYNTHETIC-OUTER-472");
    let inner_id = rig.password("inner-fixture", "SYNTHETIC-INNER-936");
    rig.file("inner-payload/docs/说明.txt", "合成内层内容\n".as_bytes());
    fs::create_dir_all(rig.root.join("inner-payload/empty")).unwrap();
    let inner = rig.root.join("outer-payload/资料/inner.7z");
    rig.pack(
        &rig.root.join("inner-payload"),
        &inner,
        "7z",
        Some("SYNTHETIC-INNER-936"),
        &[],
    );
    rig.file("outer-payload/README.txt", b"outer ordinary file\n");
    rig.file(
        "outer-payload/notes/readme.md",
        b"preserve ordinary paths\n",
    );
    let archive = rig.root.join("archives/nested.7z");
    rig.pack(
        &rig.root.join("outer-payload"),
        &archive,
        "7z",
        Some("SYNTHETIC-OUTER-472"),
        &[],
    );
    let original = digest_file(&archive);
    let mut settings = rig.settings();
    settings.cache_enabled = true;
    settings.cache_policy = "always".into();
    settings.cache_input = true;
    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;
    let output = delivered(&job);
    assert_eq!(
        fs::read(output.join("README.txt")).unwrap(),
        b"outer ordinary file\n"
    );
    assert_eq!(
        fs::read(output.join("notes/readme.md")).unwrap(),
        b"preserve ordinary paths\n"
    );
    assert_eq!(
        fs::read(output.join("资料/inner/docs/说明.txt")).unwrap(),
        "合成内层内容\n".as_bytes()
    );
    assert!(
        output.join("资料/inner/empty").is_dir(),
        "empty nested directories must be preserved"
    );
    assert!(
        !output.join("资料/inner.7z").exists(),
        "successfully expanded inner archive is consumed only in final mapping"
    );
    assert_eq!(
        digest_file(&archive),
        original,
        "original outer archive must remain byte-identical"
    );
    assert!(
        inner.exists(),
        "original fixture source outside extracted workspace is untouched"
    );
    assert_eq!(job.nodes.len(), 2);
    assert!(job.nodes.iter().all(|node| node.status == "completed"));
    assert!(job.stage_times.contains_key("caching"));
    assert!(job.stage_times.contains_key("nested"));
    assert!(
        !job.stage_times.contains_key("verifying"),
        "standard workflow must not add target reread"
    );
    assert_eq!(
        job.verification_scope,
        "archiveCrcAndStreamSha256+sourceStreamSha256"
    );
    assert!(
        job.workspace.is_none(),
        "successful pipeline must release owned cache"
    );
    let passwords = rig.core.vault().unwrap().views().unwrap();
    assert_eq!(
        passwords
            .iter()
            .find(|password| password.id == outer_id)
            .unwrap()
            .success_count,
        1
    );
    assert_eq!(
        passwords
            .iter()
            .find(|password| password.id == inner_id)
            .unwrap()
            .success_count,
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn strict_scope_includes_actual_target_readback_without_crediting_unused_password() {
    let rig = Rig::new();
    let password_id = rig.password("unused-fixture", "SYNTHETIC-NOT-NEEDED-164");
    rig.file(
        "payload/subdir/value.txt",
        b"strict target readback fixture",
    );
    let archive = rig.root.join("archives/strict.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &[]);
    let mut settings = rig.settings();
    settings.verification = "strict".into();
    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;
    let output = delivered(&job);
    assert_eq!(
        fs::read(output.join("subdir/value.txt")).unwrap(),
        b"strict target readback fixture"
    );
    assert_eq!(
        job.verification_scope,
        "archiveCrcAndStreamSha256+targetReadback"
    );
    assert!(
        job.stage_times.contains_key("verifying"),
        "strict workflow must execute the readback worker"
    );
    assert_eq!(
        rig.core
            .vault()
            .unwrap()
            .views()
            .unwrap()
            .iter()
            .find(|password| password.id == password_id)
            .unwrap()
            .success_count,
        0
    );
    assert!(archive.is_file());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exhausted_passwords_do_not_publish_any_output() {
    let rig = Rig::new();
    rig.password("wrong-fixture", "SYNTHETIC-WRONG-241");
    rig.file(
        "payload/secret.txt",
        b"synthetic content must not be published",
    );
    let archive = rig.root.join("archives/locked.7z");
    rig.pack(
        &rig.root.join("payload"),
        &archive,
        "7z",
        Some("SYNTHETIC-CORRECT-986"),
        &[],
    );
    let original = digest_file(&archive);
    let id = rig.add(&archive, rig.settings());
    let job = rig.run(&id).await;
    assert_eq!(job.status, "needsPassword", "{}", job.message);
    assert!(job.output_path.is_none());
    assert!(job.verification_scope.is_empty());
    assert!(
        fs::read_dir(&rig.output)
            .unwrap()
            .all(|entry| entry.unwrap().file_name() == ".openunpack-work"),
        "only the retained ordinary-mode workspace may exist before publication"
    );
    assert_eq!(digest_file(&archive), original);
    assert!(
        job.workspace.is_some(),
        "failed attempts keep their owned recovery workspace"
    );
    assert!(
        rig.core
            .vault()
            .unwrap()
            .views()
            .unwrap()
            .iter()
            .all(|password| password.success_count == 0)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_active_pipeline_keeps_original_and_never_claims_success() {
    let rig = Rig::new();
    let payload = rig.root.join("payload");
    fs::create_dir_all(&payload).unwrap();
    let mut file = fs::File::create(payload.join("large.bin")).unwrap();
    let block = [0x5cu8; 65536];
    for _ in 0..512 {
        file.write_all(&block).unwrap();
    }
    drop(file);
    let archive = rig.root.join("archives/cancel.7z");
    rig.pack(&payload, &archive, "7z", None, &[]);
    let original = digest_file(&archive);
    let id = rig.add(&archive, rig.settings());
    rig.core.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = rig.core.snapshot().unwrap();
        let job = snapshot.jobs.iter().find(|job| job.id == id).unwrap();
        if job.status == "running" {
            break;
        }
        assert!(Instant::now() < deadline, "fixture job never became active");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    rig.core.cancel(Some(&id)).unwrap();
    let job = rig.finish(&id).await;
    assert_eq!(
        job.status, "cancelled",
        "cancelled fixture must not finish as {}: {}",
        job.status, job.message
    );
    assert!(job.output_path.is_none());
    assert_eq!(digest_file(&archive), original);
    assert!(!rig.output.join("cancel").exists());
    assert!(
        !rig.core.snapshot().unwrap().running,
        "supervisor must finish cancellation and release the batch"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn delete_running_pipeline_stops_real_worker_without_reviving_or_blocking_other_jobs() {
    let rig = Rig::new();
    rig.file("payload-delete/data.bin", &vec![0x42; 64 * 1024 * 1024]);
    let archive = rig.root.join("archives/delete-running.zip");
    rig.pack(&rig.root.join("payload-delete"), &archive, "zip", None, &[]);
    let original = digest_file(&archive);
    rig.file("payload-keep/keep.txt", b"other task must still publish");
    let keep_archive = rig.root.join("archives/keep-running.zip");
    rig.pack(
        &rig.root.join("payload-keep"),
        &keep_archive,
        "zip",
        None,
        &[],
    );
    rig.file(
        "payload-cancel/cancel.txt",
        b"queued cancellation remains responsive",
    );
    let cancel_archive = rig.root.join("archives/cancel-queued.zip");
    rig.pack(
        &rig.root.join("payload-cancel"),
        &cancel_archive,
        "zip",
        None,
        &[],
    );
    let mut settings = rig.settings();
    settings.concurrency = 2;
    settings.cache_enabled = true;
    settings.cache_policy = "always".into();
    settings.cache_input = false;
    settings.source_cleanup = "delete".into();
    let id = rig.add(&archive, settings.clone());
    let keep_id = rig.add(&keep_archive, settings.clone());
    rig.core.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let workspace = loop {
        let snapshot = rig.core.snapshot().unwrap();
        let job = snapshot.jobs.iter().find(|job| job.id == id).unwrap();
        if job.stage == "extracting"
            && job
                .transfer_progress
                .as_ref()
                .is_some_and(|progress| progress.completed_bytes > 0)
        {
            rig.core.pause().unwrap();
            break PathBuf::from(job.workspace.as_ref().unwrap());
        }
        assert!(
            snapshot.running && Instant::now() < deadline,
            "real extraction transfer not observed: {} {} {}",
            job.status,
            job.stage,
            job.message
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    };
    let cancel_id = rig.add(&cancel_archive, settings);
    let deleting = rig.core.clone();
    let deleting_id = id.clone();
    let operation = tokio::spawn(async move { deleting.delete_task(&deleting_id).await });
    let cancelling = rig.core.clone();
    let another_id = cancel_id.clone();
    tokio::time::timeout(
        Duration::from_secs(3),
        tokio::task::spawn_blocking(move || cancelling.cancel(Some(&another_id))),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert!(rig.core.snapshot().unwrap().paused);
    tokio::time::timeout(Duration::from_secs(30), operation)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!workspace.exists());
    assert!(
        !rig.core
            .snapshot()
            .unwrap()
            .jobs
            .iter()
            .any(|job| job.id == id)
    );
    assert_eq!(
        digest_file(&archive),
        original,
        "delete request must preserve original archive"
    );
    assert!(!rig.output.join("delete-running").exists());
    rig.core.start().unwrap();
    let keep = rig.finish(&keep_id).await;
    assert_eq!(
        fs::read(delivered(&keep).join("keep.txt")).unwrap(),
        b"other task must still publish"
    );
    assert_eq!(rig.finish(&cancel_id).await.status, "cancelled");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !rig.core
            .snapshot()
            .unwrap()
            .jobs
            .iter()
            .any(|job| job.id == id)
    );
    let repository =
        openunpack_core::repository::Repository::open(&rig.root.join("data/jobs.sqlite")).unwrap();
    assert!(!repository.jobs().unwrap().iter().any(|job| job.id == id));
    assert!(
        !repository
            .publications()
            .unwrap()
            .iter()
            .any(|publication| publication.job_id == id)
    );
    // Exclusive rename verifies that all real Worker source handles were released.
    fs::rename(&archive, archive.with_extension("held-proof")).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_password_failed_real_pipeline_cleans_cache_and_keeps_vault_and_original() {
    let rig = Rig::new();
    rig.password("saved synthetic wrong password", "WRONG-FIXTURE-DELETE");
    rig.file(
        "payload-delete-secret/secret.txt",
        b"synthetic protected payload",
    );
    let archive = rig.root.join("archives/delete-password.7z");
    rig.pack(
        &rig.root.join("payload-delete-secret"),
        &archive,
        "7z",
        Some("CORRECT-FIXTURE-DELETE"),
        &[],
    );
    let original = digest_file(&archive);
    let vault_before = fs::read(rig.root.join("data/passwords.vault")).unwrap();
    let mut settings = rig.settings();
    settings.cache_enabled = true;
    settings.cache_policy = "always".into();
    let id = rig.add(&archive, settings);
    let failed = rig.run(&id).await;
    assert_eq!(failed.status, "needsPassword", "{}", failed.message);
    let work = PathBuf::from(failed.workspace.unwrap());
    assert!(work.exists());
    rig.core.delete_task(&id).await.unwrap();
    assert!(!work.exists());
    assert_eq!(digest_file(&archive), original);
    assert_eq!(
        fs::read(rig.root.join("data/passwords.vault")).unwrap(),
        vault_before
    );
    assert!(rig.core.snapshot().unwrap().jobs.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_completed_real_pipeline_preserves_published_bytes_and_source() {
    let rig = Rig::new();
    rig.file(
        "payload-delete-complete/delivered.txt",
        b"published result must survive record deletion",
    );
    let archive = rig.root.join("archives/delete-completed.zip");
    rig.pack(
        &rig.root.join("payload-delete-complete"),
        &archive,
        "zip",
        None,
        &[],
    );
    let original = digest_file(&archive);
    let id = rig.add(&archive, rig.settings());
    let finished = rig.run(&id).await;
    let output = delivered(&finished);
    let output_digest = digest_file(&output.join("delivered.txt"));
    rig.core.delete_task(&id).await.unwrap();
    assert_eq!(digest_file(&output.join("delivered.txt")), output_digest);
    assert_eq!(digest_file(&archive), original);
    assert!(rig.core.snapshot().unwrap().jobs.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn output_rename_and_skip_preserve_previous_results_and_original() {
    let rig = Rig::new();
    rig.file("payload/retained.txt", b"same output remains unchanged");
    let archive = rig.root.join("archives/repeat.7z");
    rig.pack(&rig.root.join("payload"), &archive, "7z", None, &[]);
    let original = digest_file(&archive);
    let first_id = rig.add(&archive, rig.settings());
    let first = rig.run(&first_id).await;
    let first_path = delivered(&first);
    let second_id = rig.add(&archive, rig.settings());
    let second = rig.run(&second_id).await;
    assert_eq!(
        second.status, "skipped",
        "identical content is detected before conflict policy"
    );
    assert!(second.output_path.is_none());
    assert!(second.workspace.is_none());
    assert_eq!(digest_file(&archive), original);
    // A genuinely different payload must honor rename without changing the first result.
    rig.file(
        "payload/retained.txt",
        b"a different payload for the second delivery",
    );
    fs::remove_file(&archive).unwrap();
    rig.pack(&rig.root.join("payload"), &archive, "7z", None, &[]);
    let replacement = digest_file(&archive);
    let renamed_id = rig.add(&archive, rig.settings());
    let renamed = rig.run(&renamed_id).await;
    let second_path = delivered(&renamed);
    assert_ne!(
        first_path, second_path,
        "rename must never overwrite the first delivery"
    );
    assert_eq!(
        fs::read(first_path.join("retained.txt")).unwrap(),
        b"same output remains unchanged"
    );
    assert_eq!(
        fs::read(second_path.join("retained.txt")).unwrap(),
        b"a different payload for the second delivery"
    );
    let mut skip = rig.settings();
    skip.conflict_policy = "skip".into();
    let third_id = rig.add(&archive, skip);
    let third = rig.run(&third_id).await;
    assert_eq!(third.status, "skipped", "{}", third.message);
    assert!(third.output_path.is_none());
    assert!(third.workspace.is_none());
    assert_eq!(digest_file(&archive), replacement);
    assert_eq!(
        fs::read_dir(&rig.output)
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_name() != ".openunpack-work")
            .count(),
        2,
        "skip must leave no third output or temporary directory"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn output_exclusions_preserve_kept_paths_and_empty_directories() {
    let rig = Rig::new();
    rig.file("payload/skip.url", b"synthetic excluded file");
    rig.file("payload/docs/keep.txt", b"keep this ordinary path");
    fs::create_dir_all(rig.root.join("payload/empty")).unwrap();
    let archive = rig.root.join("archives/filter.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &[]);
    let mut settings = rig.settings();
    settings.output_exclusions = vec!["*.url".into()];
    settings.expand_nested = false;
    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;
    let output = delivered(&job);
    assert!(!output.join("skip.url").exists());
    assert_eq!(
        fs::read(output.join("docs/keep.txt")).unwrap(),
        b"keep this ordinary path"
    );
    assert!(output.join("empty").is_dir());
    assert!(archive.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn filename_password_only_prioritizes_enabled_saved_group_candidates() {
    for (saved_group, enabled, should_deliver) in [
        (None, true, false),
        (Some("outside"), true, false),
        (Some("allowed"), false, false),
        (Some("allowed"), true, true),
    ] {
        let rig = Rig::new();
        let secret = "SYNTHETIC-FILENAME-913";
        rig.file("payload/value.txt", b"approved saved password only");
        let archive = rig.root.join(format!("archives/hint[密码_{secret}].7z"));
        rig.pack(&rig.root.join("payload"), &archive, "7z", Some(secret), &[]);
        if let Some(group) = saved_group {
            rig.core
                .update_visible_state(|| {
                    rig.core.vault()?.upsert(PasswordInput {
                        id: None,
                        name: "filename-matching fixture".into(),
                        secret: Some(secret.into()),
                        groups: vec![group.into()],
                        notes: "Synthetic integration fixture".into(),
                        enabled,
                    })
                })
                .unwrap();
        }
        let mut settings = rig.settings();
        settings.password_group = "allowed".into();
        let id = rig.add(&archive, settings);
        let job = rig.run(&id).await;
        if should_deliver {
            let output = delivered(&job);
            assert_eq!(
                fs::read(output.join("value.txt")).unwrap(),
                b"approved saved password only"
            );
            assert_eq!(
                job.nodes[0].password_label.as_deref(),
                Some("filename-matching fixture")
            );
        } else {
            assert_eq!(job.status, "needsPassword", "{}", job.message);
            assert!(job.output_path.is_none());
            assert_eq!(
                job.nodes[0].attempts, 0,
                "unapproved filename secret must never reach extraction"
            );
        }
        assert!(archive.exists());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_candidate_batches_continue_after_128_wrong_passwords() {
    let rig = Rig::new();
    for index in 0..128 {
        rig.password(
            &format!("a-{index:03}"),
            &format!("SYNTHETIC-WRONG-{index:03}-674"),
        );
    }
    let correct = rig.password("z-correct", "SYNTHETIC-SECOND-BATCH-185");
    rig.file("payload/value.txt", b"candidate in second finite batch");
    let archive = rig.root.join("archives/second-batch.7z");
    rig.pack(
        &rig.root.join("payload"),
        &archive,
        "7z",
        Some("SYNTHETIC-SECOND-BATCH-185"),
        &[],
    );
    let id = rig.add(&archive, rig.settings());
    let job = rig.run(&id).await;
    let output = delivered(&job);
    assert_eq!(
        fs::read(output.join("value.txt")).unwrap(),
        b"candidate in second finite batch"
    );
    assert_eq!(
        job.nodes[0].attempts, 1,
        "probe batches must not trigger repeated full extraction"
    );
    assert_eq!(job.nodes[0].password_label.as_deref(), Some("z-correct"));
    assert_eq!(
        rig.core
            .vault()
            .unwrap()
            .views()
            .unwrap()
            .into_iter()
            .find(|password| password.id == correct)
            .unwrap()
            .success_count,
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retired_inner_bytes_remaining_after_checkpoint_are_still_verified() {
    for tamper in [false, true] {
        let rig = Rig::new();
        rig.file("first-data/a.txt", b"verified sibling");
        rig.file("second-data/b.txt", b"awaiting password");
        let inner = rig.root.join("outer-payload/a.7z");
        rig.pack(&rig.root.join("first-data"), &inner, "7z", None, &[]);
        rig.pack(
            &rig.root.join("second-data"),
            &rig.root.join("outer-payload/b.7z"),
            "7z",
            Some("SYNTHETIC-RETIRED-724"),
            &[],
        );
        let archive = rig.root.join("archives/retired.7z");
        rig.pack(&rig.root.join("outer-payload"), &archive, "7z", None, &[]);
        let id = rig.add(&archive, rig.settings());
        let waiting = rig.run(&id).await;
        assert_eq!(waiting.status, "needsPassword", "{}", waiting.message);
        let work = Path::new(waiting.workspace.as_ref().unwrap());
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(work.join("pipeline.json")).unwrap()).unwrap();
        let retired = saved["retired_files"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|entry| entry.as_str().filter(|path| path.ends_with("a.7z")))
            .unwrap();
        let retired = retired.to_string();
        if !tamper {
            let mut legacy = saved.clone();
            legacy["version"] = serde_json::json!(1);
            legacy["retired_files"] = serde_json::json!([]);
            legacy.as_object_mut().unwrap().remove("input_retired");
            fs::write(
                work.join("pipeline.json"),
                serde_json::to_vec(&legacy).unwrap(),
            )
            .unwrap();
        }
        assert!(
            !Path::new(&retired).exists(),
            "the archive is retired only after a durable checkpoint"
        );
        // Recreate the bytes to simulate a crash between the saved retirement
        // intent and deletion, or a deletion failure. The sealed digest stays fixed.
        fs::copy(&inner, &retired).unwrap();
        if tamper {
            let mut bytes = fs::read(&retired).unwrap();
            let end = bytes.len() - 1;
            bytes[end] ^= 1;
            fs::write(&retired, bytes).unwrap();
        }
        rig.password("added", "SYNTHETIC-RETIRED-724");
        rig.core.retry(&id, false).unwrap();
        let resumed = rig.run(&id).await;
        if tamper {
            assert_eq!(resumed.status, "failed", "{}", resumed.message);
            assert!(resumed.output_path.is_none());
        } else {
            let output = delivered(&resumed);
            assert_eq!(
                fs::read(output.join("a/a.txt")).unwrap(),
                b"verified sibling"
            );
            assert!(!output.join("a.7z").exists());
        }
        assert!(archive.exists());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retired_input_cache_remaining_after_checkpoint_is_still_verified() {
    for tamper in [false, true] {
        let rig = Rig::new();
        rig.file("inner-data/value.txt", b"cache recovery fixture");
        rig.pack(
            &rig.root.join("inner-data"),
            &rig.root.join("outer-payload/locked.7z"),
            "7z",
            Some("SYNTHETIC-CACHE-528"),
            &[],
        );
        let archive = rig.root.join("archives/retired-cache.7z");
        rig.pack(&rig.root.join("outer-payload"), &archive, "7z", None, &[]);
        let mut settings = rig.settings();
        settings.cache_enabled = true;
        settings.cache_policy = "always".into();
        settings.cache_input = true;
        let id = rig.add(&archive, settings);
        let waiting = rig.run(&id).await;
        assert_eq!(waiting.status, "needsPassword", "{}", waiting.message);
        let work = Path::new(waiting.workspace.as_ref().unwrap());
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(work.join("pipeline.json")).unwrap()).unwrap();
        assert_eq!(saved["input_retired"], true);
        if !tamper {
            let mut legacy = saved.clone();
            legacy["version"] = serde_json::json!(1);
            legacy.as_object_mut().unwrap().remove("input_retired");
            fs::write(
                work.join("pipeline.json"),
                serde_json::to_vec(&legacy).unwrap(),
            )
            .unwrap();
        }
        let cached = Path::new(saved["input_files"][0]["sourcePath"].as_str().unwrap());
        assert!(
            !cached.exists(),
            "cached input must be released after root sealing"
        );
        fs::create_dir_all(cached.parent().unwrap()).unwrap();
        fs::copy(&archive, cached).unwrap();
        if tamper {
            let mut bytes = fs::read(cached).unwrap();
            let end = bytes.len() - 1;
            bytes[end] ^= 1;
            fs::write(cached, bytes).unwrap();
        }
        rig.password("added", "SYNTHETIC-CACHE-528");
        rig.core.retry(&id, false).unwrap();
        let resumed = rig.run(&id).await;
        if tamper {
            assert_eq!(resumed.status, "failed", "{}", resumed.message);
            assert!(resumed.output_path.is_none());
        } else {
            let output = delivered(&resumed);
            assert_eq!(
                fs::read(output.join("locked/value.txt")).unwrap(),
                b"cache recovery fixture"
            );
        }
        assert!(archive.exists());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selecting_first_real_split_volume_discovers_and_delivers_complete_group() {
    let rig = Rig::new();
    rig.file("payload/value.bin", &vec![0x41u8; 96 * 1024]);
    let archive = rig.root.join("archives/split.7z");
    rig.pack(&rig.root.join("payload"), &archive, "7z", None, &["-v8k"]);
    let first = rig.root.join("archives/split.7z.001");
    let scan = scanner::scan(&[first.to_string_lossy().into_owned()], false, &[]).unwrap();
    assert_eq!(scan.sources.len(), 1);
    assert!(
        scan.sources[0].volumes.len() > 1,
        "selecting a first volume must discover its adjacent approved siblings"
    );
    assert!(!scan.sources[0].missing_volume);
    let originals: Vec<_> = scan.sources[0]
        .volumes
        .iter()
        .map(|path| (PathBuf::from(path), digest_file(Path::new(path))))
        .collect();
    let id = rig.add(&first, rig.settings());
    let job = rig.run(&id).await;
    let output = delivered(&job);
    assert_eq!(
        fs::read(output.join("value.bin")).unwrap(),
        vec![0x41u8; 96 * 1024]
    );
    for (path, digest) in originals {
        assert_eq!(
            digest_file(&path),
            digest,
            "original split volume must remain unchanged"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recycle_bin_cleanup_when_enabled_and_verified() {
    let rig = Rig::new();
    rig.file("payload/data.txt", b"recycle bin verification payload\n");
    let archive = rig.root.join("archives/recycle.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &[]);
    let mut settings = rig.settings();
    settings.source_cleanup = "recycle".into();
    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;
    let output = delivered(&job);
    assert_eq!(
        fs::read(output.join("data.txt")).unwrap(),
        b"recycle bin verification payload\n"
    );
    assert!(
        !archive.exists(),
        "original archive must be removed from original location into recycle bin"
    );
    assert!(job.message.contains("回收站"));
}

async fn verify_three_layer_source_cleanup(cleanup: &str, cache_enabled: bool) {
    let rig = Rig::new();
    let payload: Vec<u8> = (0..48 * 1024).map(|index| (index % 251) as u8).collect();
    let payload_path = rig.file("inner-payload/leaf/payload.bin", &payload);
    rig.pack(
        &rig.root.join("inner-payload"),
        &rig.root.join("middle-payload/inner.7z"),
        "7z",
        None,
        &["-v8k"],
    );
    let inner_scan = scanner::scan(
        &[rig
            .root
            .join("middle-payload/inner.7z.001")
            .to_string_lossy()
            .into_owned()],
        false,
        &[],
    )
    .unwrap();
    assert_eq!(inner_scan.sources.len(), 1);
    assert!(inner_scan.sources[0].volumes.len() > 1);
    let mut preserved: Vec<_> = inner_scan.sources[0]
        .volumes
        .iter()
        .map(|path| (PathBuf::from(path), digest_file(Path::new(path))))
        .collect();
    rig.file(
        "middle-payload/middle.txt",
        b"ordinary middle-layer content",
    );
    let middle = rig.root.join("outer-payload/middle.zip");
    rig.pack(&rig.root.join("middle-payload"), &middle, "zip", None, &[]);
    preserved.push((middle.clone(), digest_file(&middle)));
    rig.file("outer-payload/outer.txt", b"ordinary outer-layer content");
    rig.pack(
        &rig.root.join("outer-payload"),
        &rig.root.join("archives/outer.7z"),
        "7z",
        None,
        &["-v16k"],
    );
    let first = rig.root.join("archives/outer.7z.001");
    let source_scan = scanner::scan(&[first.to_string_lossy().into_owned()], false, &[]).unwrap();
    assert_eq!(source_scan.sources.len(), 1);
    assert!(source_scan.sources[0].volumes.len() > 1);
    let source_volumes: Vec<_> = source_scan.sources[0]
        .volumes
        .iter()
        .map(|path| (PathBuf::from(path), digest_file(Path::new(path))))
        .collect();
    let unrelated = rig.root.join("archives/unrelated.zip");
    rig.pack(
        &rig.root.join("inner-payload"),
        &unrelated,
        "zip",
        None,
        &[],
    );
    preserved.push((unrelated.clone(), digest_file(&unrelated)));
    preserved.push((payload_path.clone(), digest_file(&payload_path)));

    let mut settings = rig.settings();
    settings.source_cleanup = cleanup.into();
    settings.expand_nested = true;
    settings.max_depth = 3;
    settings.cache_enabled = cache_enabled;
    settings.cache_policy = "always".into();
    let id = rig.add(&first, settings);
    let job = rig.run(&id).await;
    let output = delivered(&job);
    assert_eq!(job.nodes.len(), 3, "all three layers must be expanded");
    assert!(job.nodes.iter().all(|node| node.status == "completed"));
    assert_eq!(
        fs::read(output.join("middle/inner/leaf/payload.bin")).unwrap(),
        payload
    );
    assert_eq!(
        digest_file(&output.join("middle/inner/leaf/payload.bin")),
        digest_file(&payload_path)
    );
    assert_eq!(
        fs::read(output.join("middle/middle.txt")).unwrap(),
        b"ordinary middle-layer content"
    );
    assert_eq!(
        fs::read(output.join("outer.txt")).unwrap(),
        b"ordinary outer-layer content"
    );
    let mut published_files: Vec<_> = walkdir::WalkDir::new(&output)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| {
            entry.file_type().is_file() && entry.file_name() != ".openunpack-owner.json"
        })
        .map(|entry| {
            entry
                .path()
                .strip_prefix(&output)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    published_files.sort();
    assert_eq!(
        published_files,
        [
            "middle/inner/leaf/payload.bin",
            "middle/middle.txt",
            "outer.txt"
        ],
        "no consumed inner archive or split volume may remain in the result"
    );
    for (path, before) in source_volumes {
        if cleanup == "delete" {
            assert!(
                !path.exists(),
                "every selected outer source volume must be deleted"
            );
        } else {
            assert_eq!(
                digest_file(&path),
                before,
                "keep must retain every outer volume"
            );
        }
    }
    for (path, before) in preserved {
        assert_eq!(
            digest_file(&path),
            before,
            "cleanup must not touch unrelated archives or fixture inputs outside the workspace"
        );
    }
    if cleanup == "delete" {
        assert!(job.verification_scope.contains("targetReadback"));
        assert!(job.message.contains("源压缩包已删除"));
    }
    assert_eq!(job.stage_times.contains_key("caching"), cache_enabled);
    assert!(
        job.workspace.is_none(),
        "completed workspaces must be retired"
    );
    let cache_leftovers: Vec<String> = fs::read_dir(&rig.cache)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(cache_leftovers.is_empty(), "cache workspace residue: {cache_leftovers:?}");
    let direct_work = rig.output.join(".openunpack-work");
    if direct_work.exists() {
        assert_eq!(fs::read_dir(direct_work).unwrap().count(), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_cleanup_delete_removes_three_layer_archives_and_all_selected_volumes() {
    for cache_enabled in [false, true] {
        verify_three_layer_source_cleanup("delete", cache_enabled).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_cleanup_keep_retains_outer_volumes_after_three_layer_expansion() {
    for cache_enabled in [false, true] {
        verify_three_layer_source_cleanup("keep", cache_enabled).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unwrap_single_root_directory_when_enabled() {
    let rig = Rig::new();
    rig.file("payload/single-folder/a.txt", b"wrapped content a\n");
    rig.file("payload/single-folder/b.txt", b"wrapped content b\n");
    let archive = rig.root.join("archives/nested_wrapper.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &[]);
    let mut settings = rig.settings();
    settings.unwrap_single_root = true;
    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;
    let output = delivered(&job);
    assert_eq!(
        fs::read(output.join("a.txt")).unwrap(),
        b"wrapped content a\n"
    );
    assert_eq!(
        fs::read(output.join("b.txt")).unwrap(),
        b"wrapped content b\n"
    );
    assert!(
        !output.join("single-folder").exists(),
        "redundant wrapper folder must be unwrapped"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_nested_archives_with_same_stem_get_distinct_delivery_prefixes() {
    let rig = Rig::new();
    rig.file("inner-one/same.txt", b"first nested content");
    rig.file("inner-two/same.txt", b"second nested content");
    rig.pack(
        &rig.root.join("inner-one"),
        &rig.root.join("outer-payload/bundles/pack.7z"),
        "7z",
        None,
        &[],
    );
    rig.pack(
        &rig.root.join("inner-two"),
        &rig.root.join("outer-payload/bundles/pack.zip"),
        "zip",
        None,
        &[],
    );
    let archive = rig.root.join("archives/collision.7z");
    rig.pack(&rig.root.join("outer-payload"), &archive, "7z", None, &[]);
    let id = rig.add(&archive, rig.settings());
    let job = rig.run(&id).await;
    let output = delivered(&job);
    let mut contents = vec![];
    for entry in walkdir::WalkDir::new(output.join("bundles"))
        .into_iter()
        .map(Result::unwrap)
    {
        if entry.file_type().is_file() && entry.file_name() == "same.txt" {
            contents.push(fs::read(entry.path()).unwrap());
        }
    }
    contents.sort();
    let mut expected = vec![
        b"first nested content".to_vec(),
        b"second nested content".to_vec(),
    ];
    expected.sort();
    assert_eq!(
        contents, expected,
        "both nested contents require distinct final prefixes"
    );
    assert!(archive.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_selected_inner_reuses_verified_outer_and_successful_sibling() {
    let rig = Rig::new();
    let first_password = rig.password("first", "SYNTHETIC-FIRST-INNER-342");
    rig.file("first-data/first.txt", b"already verified sibling");
    rig.file("second-data/second.txt", b"retry only this inner archive");
    rig.pack(
        &rig.root.join("first-data"),
        &rig.root.join("outer-payload/a.7z"),
        "7z",
        Some("SYNTHETIC-FIRST-INNER-342"),
        &[],
    );
    rig.pack(
        &rig.root.join("second-data"),
        &rig.root.join("outer-payload/b.7z"),
        "7z",
        Some("SYNTHETIC-SECOND-INNER-564"),
        &[],
    );
    rig.file(
        "outer-payload/ordinary.txt",
        b"ordinary paths survive retries",
    );
    let archive = rig.root.join("archives/selected.7z");
    rig.pack(&rig.root.join("outer-payload"), &archive, "7z", None, &[]);
    let original = digest_file(&archive);
    let id = rig.add(&archive, rig.settings());
    let waiting = rig.run(&id).await;
    assert_eq!(waiting.status, "needsPassword", "{}", waiting.message);
    assert!(
        rig.core.history().unwrap().iter().any(|job| job.id == id),
        "a password-blocked job must be visible in retryable history"
    );
    let outer = waiting
        .nodes
        .iter()
        .find(|node| node.depth == 0)
        .unwrap()
        .clone();
    let sibling = waiting
        .nodes
        .iter()
        .find(|node| node.name == "a")
        .unwrap()
        .clone();
    let failed = waiting
        .nodes
        .iter()
        .find(|node| node.name == "b")
        .unwrap()
        .clone();
    assert_eq!(sibling.status, "completed");
    assert_eq!(failed.status, "needsPassword");
    assert!(
        Path::new(waiting.workspace.as_ref().unwrap()).starts_with(&rig.output),
        "ordinary mode must use the output volume instead of an implicit app-data cache"
    );
    // The first candidate revision changes after sealing. Successful sibling bytes
    // must be reused rather than decrypted again with the edited secret.
    rig.core
        .vault()
        .unwrap()
        .upsert(PasswordInput {
            id: Some(first_password.clone()),
            name: "first".into(),
            secret: Some("SYNTHETIC-EDITED-REVISION-821".into()),
            groups: vec![],
            notes: String::new(),
            enabled: true,
        })
        .unwrap();
    let second_password = rig.password("second", "SYNTHETIC-SECOND-INNER-564");
    rig.core.retry_node(&id, &failed.id).unwrap();
    let finished = rig.run(&id).await;
    let output = delivered(&finished);
    assert_eq!(
        fs::read(output.join("a/first.txt")).unwrap(),
        b"already verified sibling"
    );
    assert_eq!(
        fs::read(output.join("b/second.txt")).unwrap(),
        b"retry only this inner archive"
    );
    assert_eq!(
        fs::read(output.join("ordinary.txt")).unwrap(),
        b"ordinary paths survive retries"
    );
    assert_eq!(
        finished
            .nodes
            .iter()
            .find(|node| node.id == outer.id)
            .unwrap()
            .attempts,
        outer.attempts
    );
    assert_eq!(
        finished
            .nodes
            .iter()
            .find(|node| node.id == sibling.id)
            .unwrap()
            .attempts,
        sibling.attempts
    );
    assert!(
        finished
            .nodes
            .iter()
            .find(|node| node.id == failed.id)
            .unwrap()
            .attempts
            > failed.attempts
    );
    assert_eq!(digest_file(&archive), original);
    let views = rig.core.vault().unwrap().views().unwrap();
    assert_eq!(
        views
            .iter()
            .find(|password| password.id == first_password)
            .unwrap()
            .success_count,
        0,
        "stale sealed password revision must not be credited"
    );
    assert_eq!(
        views
            .iter()
            .find(|password| password.id == second_password)
            .unwrap()
            .success_count,
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_partial_delivery_retains_corrupt_inner_archive_and_ordinary_files() {
    let rig = Rig::new();
    let corrupt = b"7z\xbc\xaf\x27\x1cSYNTHETIC-CORRUPT-INNER";
    rig.file("outer-payload/broken.7z", corrupt);
    rig.file(
        "outer-payload/ordinary.txt",
        b"must be retained in partial delivery",
    );
    let archive = rig.root.join("archives/partial.zip");
    rig.pack(&rig.root.join("outer-payload"), &archive, "zip", None, &[]);
    let original = digest_file(&archive);
    let mut settings = rig.settings();
    settings.source_cleanup = "delete".into();
    let id = rig.add(&archive, settings);
    let waiting = rig.run(&id).await;
    assert_eq!(waiting.status, "failed", "{}", waiting.message);
    assert!(waiting.output_path.is_none());
    assert_eq!(
        digest_file(&archive),
        original,
        "failed nested extraction must preserve the outer archive before any retry"
    );
    let outer_attempts = waiting
        .nodes
        .iter()
        .find(|node| node.depth == 0)
        .unwrap()
        .attempts;
    rig.core.retry(&id, true).unwrap();
    let partial = rig.run(&id).await;
    assert_eq!(partial.status, "partial", "{}", partial.message);
    let output = Path::new(partial.output_path.as_ref().unwrap());
    assert_eq!(fs::read(output.join("broken.7z")).unwrap(), corrupt);
    assert_eq!(
        fs::read(output.join("ordinary.txt")).unwrap(),
        b"must be retained in partial delivery"
    );
    assert_eq!(
        partial
            .nodes
            .iter()
            .find(|node| node.depth == 0)
            .unwrap()
            .attempts,
        outer_attempts
    );
    assert_eq!(
        digest_file(&archive),
        original,
        "partial delivery must never trigger enabled source deletion"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn renamed_cached_archive_uses_content_association_ahead_of_global_password_scores() {
    use openunpack_core::types::SuccessEvent;
    let rig = Rig::new();
    let correct = rig.password("a-correct", "SYNTHETIC-RENAMED-CORRECT-159");
    let other = rig.password("b-other", "SYNTHETIC-OTHER-HIGH-SCORE-753");
    rig.file(
        "payload/data.txt",
        b"content fingerprint is independent of the filename",
    );
    let archive = rig.root.join("archives/original-name.7z");
    rig.pack(
        &rig.root.join("payload"),
        &archive,
        "7z",
        Some("SYNTHETIC-RENAMED-CORRECT-159"),
        &[],
    );
    let original = digest_file(&archive);
    let mut settings = rig.settings();
    settings.cache_enabled = true;
    settings.cache_policy = "always".into();
    settings.cache_input = true;
    let first_id = rig.add(&archive, settings.clone());
    let first = rig.run(&first_id).await;
    delivered(&first);
    let revision = rig
        .core
        .vault()
        .unwrap()
        .views()
        .unwrap()
        .into_iter()
        .find(|password| password.id == other)
        .unwrap()
        .revision;
    for _ in 0..5 {
        rig.core
            .vault()
            .unwrap()
            .record_success(&SuccessEvent {
                event_id: uuid::Uuid::new_v4().to_string(),
                password_id: other.clone(),
                password_revision: revision,
                fingerprint: None,
            })
            .unwrap();
    }
    let renamed = rig.root.join("archives/completely-renamed.7z");
    fs::rename(&archive, &renamed).unwrap();
    let second_id = rig.add(&renamed, settings);
    let second = rig.run(&second_id).await;
    delivered(&second);
    assert_eq!(
        second.nodes[0].attempts, 1,
        "single probe resolves content-associated password directly without repeated extraction attempts"
    );
    assert_eq!(digest_file(&renamed), original);
    assert_eq!(
        rig.core
            .vault()
            .unwrap()
            .views()
            .unwrap()
            .into_iter()
            .find(|password| password.id == correct)
            .unwrap()
            .success_count,
        2
    );
    assert_eq!(
        rig.core
            .vault()
            .unwrap()
            .views()
            .unwrap()
            .into_iter()
            .find(|password| password.id == other)
            .unwrap()
            .success_count,
        5,
        "global high-score password was not consumed because content-associated candidate took precedence"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn depth_limit_counts_outer_as_first_layer_and_prohibits_source_cleanup() {
    let rig = Rig::new();
    rig.file("deep-data/deep.txt", b"third level should not be expanded");
    rig.pack(
        &rig.root.join("deep-data"),
        &rig.root.join("inner-data/third.7z"),
        "7z",
        None,
        &[],
    );
    rig.pack(
        &rig.root.join("inner-data"),
        &rig.root.join("outer-payload/second.7z"),
        "7z",
        None,
        &[],
    );
    let archive = rig.root.join("archives/depth.7z");
    rig.pack(&rig.root.join("outer-payload"), &archive, "7z", None, &[]);
    let original = digest_file(&archive);
    let unexpanded = digest_file(&rig.root.join("inner-data/third.7z"));
    let mut settings = rig.settings();
    settings.source_cleanup = "delete".into();
    settings.max_depth = 2;
    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;
    let output = delivered(&job);
    assert_eq!(
        job.nodes.len(),
        2,
        "maxDepth=2 allows the outer archive and one inner level"
    );
    assert!(output.join("second/third.7z").is_file());
    assert_eq!(digest_file(&output.join("second/third.7z")), unexpanded);
    assert!(!output.join("second/third/deep.txt").exists());
    assert!(
        job.nodes
            .iter()
            .any(|node| node.message.contains("层数限制"))
    );
    assert!(
        archive.is_file(),
        "unexpanded deeper archives must suppress source cleanup"
    );
    assert_eq!(digest_file(&archive), original);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_single_archive_publishes_on_output_volume_without_a_second_full_copy() {
    let rig = Rig::new();
    rig.file("payload/ordinary.txt", b"no hidden second full copy");
    let archive = rig.root.join("archives/ordinary.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &[]);
    let id = rig.add(&archive, rig.settings());
    let job = rig.run(&id).await;
    let output = delivered(&job);
    assert_eq!(
        fs::read(output.join("ordinary.txt")).unwrap(),
        b"no hidden second full copy"
    );
    assert!(!job.stage_times.contains_key("copying"));
    assert!(!job.stage_times.contains_key("verifying"));
    assert_eq!(job.verification_scope, "archiveCrcAndStreamSha256");
    assert!(!rig.root.join("data/work").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tampered_sealed_result_is_never_reused_or_published() {
    let rig = Rig::new();
    rig.file("inner-data/locked.txt", b"still needs a saved password");
    rig.pack(
        &rig.root.join("inner-data"),
        &rig.root.join("outer-payload/locked.7z"),
        "7z",
        Some("SYNTHETIC-MISSING-THEN-ADDED-991"),
        &[],
    );
    rig.file("outer-payload/ordinary.txt", b"sealed original value");
    let archive = rig.root.join("archives/tamper.7z");
    rig.pack(&rig.root.join("outer-payload"), &archive, "7z", None, &[]);
    let id = rig.add(&archive, rig.settings());
    let waiting = rig.run(&id).await;
    assert_eq!(waiting.status, "needsPassword");
    let work = Path::new(waiting.workspace.as_ref().unwrap());
    let sealed = walkdir::WalkDir::new(work)
        .into_iter()
        .map(Result::unwrap)
        .find(|entry| entry.file_name() == "ordinary.txt")
        .unwrap();
    fs::write(sealed.path(), b"different file bytes").unwrap();
    rig.password("newly-added", "SYNTHETIC-MISSING-THEN-ADDED-991");
    rig.core.retry(&id, false).unwrap();
    let failed = rig.run(&id).await;
    assert_eq!(failed.status, "failed", "{}", failed.message);
    assert!(failed.output_path.is_none());
    assert!(failed.workspace.is_some());
    assert!(archive.exists());
    assert!(!rig.output.join("tamper").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn copy_failure_can_retry_writeback_without_reextracting_sealed_nodes() {
    use std::os::windows::fs::OpenOptionsExt;
    let rig = Rig::new();
    rig.file("payload/a.bin", &vec![0x71; 8 * 1024 * 1024]);
    rig.file("payload/zz-last.txt", b"locked copy fixture");
    let archive = rig.root.join("archives/writeback.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &[]);
    let mut settings = rig.settings();
    settings.cache_enabled = true;
    settings.cache_policy = "always".into();
    let id = rig.add(&archive, settings);
    rig.core.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let lock = loop {
        let snapshot = rig.core.snapshot().unwrap();
        let job = snapshot.jobs.iter().find(|job| job.id == id).unwrap();
        if job.stage == "copying" {
            let work = Path::new(job.workspace.as_ref().unwrap());
            let last = walkdir::WalkDir::new(work)
                .into_iter()
                .map(Result::unwrap)
                .find(|entry| entry.file_name() == "zz-last.txt")
                .unwrap();
            break fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(last.path())
                .expect("acquire synthetic copy-failure lock before the Worker opens it");
        }
        assert!(
            snapshot.running && Instant::now() < deadline,
            "copy stage was never observed: status={}, stage={}, running={}, msg={}",
            job.status,
            job.stage,
            snapshot.running,
            job.message
        );
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(1)).await;
    };
    let pending = rig.finish(&id).await;
    drop(lock);
    assert_eq!(pending.status, "publishPending", "{}", pending.message);
    assert!(pending.output_path.is_none());
    assert!(
        rig.core
            .delete_task(&id)
            .await
            .unwrap_err()
            .contains("发布结果尚未确认")
    );
    assert!(
        rig.core
            .snapshot()
            .unwrap()
            .jobs
            .iter()
            .any(|job| job.id == id)
    );
    assert!(Path::new(pending.workspace.as_ref().unwrap()).exists());
    let attempts = pending.nodes[0].attempts;
    rig.core.recover(&id, "retry").await.unwrap();
    let finished = rig.run(&id).await;
    let output = delivered(&finished);
    assert_eq!(
        finished.nodes[0].attempts, attempts,
        "writeback retry must reuse already verified extraction"
    );
    assert_eq!(
        fs::read(output.join("zz-last.txt")).unwrap(),
        b"locked copy fixture"
    );
    assert!(archive.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_jobs_execute_and_publish_with_concurrency_two() {
    let rig = Rig::new();
    rig.file("payload1/file1.txt", b"concurrent fixture payload 1");
    let archive1 = rig.root.join("archives/arch1.zip");
    rig.pack(&rig.root.join("payload1"), &archive1, "zip", None, &[]);

    rig.file("payload2/file2.txt", b"concurrent fixture payload 2");
    let archive2 = rig.root.join("archives/arch2.zip");
    rig.pack(&rig.root.join("payload2"), &archive2, "zip", None, &[]);

    let mut settings = rig.settings();
    settings.concurrency = 2;
    rig.core.save_settings(settings.clone()).unwrap();

    let id1 = rig.add(&archive1, settings.clone());
    let id2 = rig.add(&archive2, settings);

    rig.core.start().unwrap();

    let job1 = rig.finish(&id1).await;
    let job2 = rig.finish(&id2).await;

    let out1 = delivered(&job1);
    let out2 = delivered(&job2);

    assert_eq!(
        fs::read(out1.join("file1.txt")).unwrap(),
        b"concurrent fixture payload 1"
    );
    assert_eq!(
        fs::read(out2.join("file2.txt")).unwrap(),
        b"concurrent fixture payload 2"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_jobs_can_cancel_one_while_other_finishes() {
    let rig = Rig::new();
    rig.file("payload_large/data.bin", &vec![0x42; 8 * 1024 * 1024]);
    let archive_large = rig.root.join("archives/cancel_target.zip");
    rig.pack(
        &rig.root.join("payload_large"),
        &archive_large,
        "zip",
        None,
        &[],
    );

    rig.file("payload_small/fast.txt", b"fast finish payload");
    let archive_small = rig.root.join("archives/fast.zip");
    rig.pack(
        &rig.root.join("payload_small"),
        &archive_small,
        "zip",
        None,
        &[],
    );

    let mut settings = rig.settings();
    settings.concurrency = 2;
    rig.core.save_settings(settings.clone()).unwrap();

    let id_cancel = rig.add(&archive_large, settings.clone());
    let id_keep = rig.add(&archive_small, settings);

    rig.core.start().unwrap();

    // Cancel the first job specifically
    rig.core.cancel(Some(&id_cancel)).unwrap();

    let job_cancel = rig.finish(&id_cancel).await;
    let job_keep = rig.finish(&id_keep).await;

    assert_eq!(job_cancel.status, "cancelled");
    assert!(job_cancel.output_path.is_none());

    let out_keep = delivered(&job_keep);
    assert_eq!(
        fs::read(out_keep.join("fast.txt")).unwrap(),
        b"fast finish payload"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_jobs_same_stem_publish_without_conflict() {
    let rig = Rig::new();
    rig.file("dir1/item1.txt", b"item from dir1");
    let archive1 = rig.root.join("archives1/same_stem.zip");
    rig.pack(&rig.root.join("dir1"), &archive1, "zip", None, &[]);

    rig.file("dir2/item2.txt", b"item from dir2");
    let archive2 = rig.root.join("archives2/same_stem.zip");
    rig.pack(&rig.root.join("dir2"), &archive2, "zip", None, &[]);

    let mut settings = rig.settings();
    settings.concurrency = 2;
    settings.conflict_policy = "rename".into();
    rig.core.save_settings(settings.clone()).unwrap();

    let id1 = rig.add(&archive1, settings.clone());
    let id2 = rig.add(&archive2, settings);

    rig.core.start().unwrap();

    let job1 = rig.finish(&id1).await;
    let job2 = rig.finish(&id2).await;

    let out1 = delivered(&job1);
    let out2 = delivered(&job2);

    assert_ne!(out1, out2, "both outputs must be distinct folders");
    assert!(out1.exists());
    assert!(out2.exists());
    let (item1_found, item2_found) = (
        out1.join("item1.txt").exists() || out2.join("item1.txt").exists(),
        out1.join("item2.txt").exists() || out2.join("item2.txt").exists(),
    );
    assert!(item1_found && item2_found);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn salvage_extraction_when_corrupted_flags_salvaged_and_never_cleans_source() {
    let rig = Rig::new();
    rig.file("payload/intact.txt", b"intact good payload\n");
    rig.file(
        "payload/corrupt.txt",
        b"corrupted payload data will be modified\n",
    );
    let archive = rig.root.join("archives/salvage.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &["-mx0"]);

    let mut bytes = fs::read(&archive).unwrap();
    let target = b"corrupted payload data";
    let pos = bytes
        .windows(target.len())
        .position(|w| w == target)
        .expect("target bytes found");
    bytes[pos + 4] ^= 0xFF;
    bytes[pos + 5] ^= 0xAA;
    fs::write(&archive, bytes).unwrap();

    let mut settings = rig.settings();
    settings.salvage_corrupted = true;
    settings.source_cleanup = "delete".into();

    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;

    assert_eq!(
        job.status, "salvaged",
        "status must be salvaged, not completed or failed: {}",
        job.message
    );
    assert!(
        archive.exists(),
        "source archive MUST be preserved even when source_cleanup is delete"
    );
    assert!(job.output_path.is_some());
    let out = PathBuf::from(job.output_path.as_ref().unwrap());
    assert_eq!(
        fs::read(out.join("intact.txt")).unwrap(),
        b"intact good payload\n"
    );
    assert!(job.message.contains("抢救解压完成") || job.message.contains("CRC"));
}

fn write_zip_raw(path: &Path, names: &[&[u8]], payload: &[u8]) {
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
    let mut central = Vec::new();
    let mut offset = 0u32;
    for raw_name in names {
        let mut local = Vec::new();
        n32(&mut local, 0x0403_4b50);
        n16(&mut local, 20);
        n16(&mut local, 0); // bit 11 = 0
        for _ in 0..3 {
            n16(&mut local, 0);
        }
        n32(&mut local, crc);
        n32(&mut local, payload.len() as u32);
        n32(&mut local, payload.len() as u32);
        n16(&mut local, raw_name.len() as u16);
        n16(&mut local, 0);
        local.extend_from_slice(raw_name);
        local.extend_from_slice(payload);
        archive.write_all(&local).unwrap();
        n32(&mut central, 0x0201_4b50);
        n16(&mut central, 20);
        n16(&mut central, 20);
        n16(&mut central, 0); // bit 11 = 0
        for _ in 0..3 {
            n16(&mut central, 0);
        }
        n32(&mut central, crc);
        n32(&mut central, payload.len() as u32);
        n32(&mut central, payload.len() as u32);
        n16(&mut central, raw_name.len() as u16);
        for _ in 0..4 {
            n16(&mut central, 0);
        }
        n32(&mut central, 0);
        n32(&mut central, offset);
        central.extend_from_slice(raw_name);
        offset += local.len() as u32;
    }
    archive.write_all(&central).unwrap();
    let mut end = Vec::new();
    n32(&mut end, 0x0605_4b50);
    n16(&mut end, 0);
    n16(&mut end, 0);
    n16(&mut end, names.len() as u16);
    n16(&mut end, names.len() as u16);
    n32(&mut end, central.len() as u32);
    n32(&mut end, offset);
    n16(&mut end, 0);
    archive.write_all(&end).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clean_empty_dirs_prunes_empty_nested_directories_and_preserves_files() {
    let rig = Rig::new();
    rig.file("payload/docs/readme.txt", b"docs readme file\n");
    fs::create_dir_all(rig.root.join("payload/empty_branch/sub_empty")).unwrap();
    fs::create_dir_all(rig.root.join("payload/another_empty")).unwrap();
    let archive = rig.root.join("archives/with_empty_dirs.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &[]);

    let mut settings = rig.settings();
    settings.clean_empty_dirs = true;

    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;

    assert_eq!(
        job.status, "completed",
        "job must be completed: {}",
        job.message
    );
    let out = delivered(&job);
    assert_eq!(
        fs::read(out.join("docs/readme.txt")).unwrap(),
        b"docs readme file\n"
    );
    assert!(out.join("docs").is_dir());
    assert!(
        !out.join("empty_branch").exists(),
        "empty_branch must be pruned"
    );
    assert!(
        !out.join("another_empty").exists(),
        "another_empty must be pruned"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_fix_encoding_renames_gbk_files_on_disk() {
    let rig = Rig::new();
    let archive = rig.root.join("archives/gbk_extract.zip");
    let gbk_name = [
        0xb2, 0xe2, 0xca, 0xd4, 0xc4, 0xbf, 0xc2, 0xbc, b'/', 0xcb, 0xb5, 0xc3, 0xf7, 0xce, 0xc4,
        0xb5, 0xb5, b'.', b't', b'x', b't',
    ];
    write_zip_raw(&archive, &[&gbk_name], b"gbk extraction payload\n");

    let mut settings = rig.settings();
    settings.auto_fix_encoding = true;

    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;

    assert_eq!(
        job.status, "completed",
        "job must be completed: {}",
        job.message
    );
    let out = delivered(&job);
    assert_eq!(
        fs::read(out.join("测试目录/说明文档.txt")).unwrap(),
        b"gbk extraction payload\n",
        "file must be extracted with decoded UTF-8 name"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cache_known_metadata_allows_high_compression_without_ratio_guess_limit() {
    let rig = Rig::new();
    let payload = vec![0u8; 48 * 1024 * 1024];
    rig.file("ratio-payload/large.bin", &payload);
    let archive = rig.root.join("archives/high-ratio.7z");
    rig.pack(
        &rig.root.join("ratio-payload"),
        &archive,
        "7z",
        None,
        &["-mx=7"],
    );
    assert!(fs::metadata(&archive).unwrap().len() < 1024 * 1024);
    let mut settings = rig.settings();
    settings.cache_enabled = true;
    settings.cache_policy = "always".into();
    settings.cache_input = false;
    settings.cache_quota_bytes = 80 * 1024 * 1024;
    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;
    assert_eq!(job.status, "completed", "{}", job.message);
    assert_eq!(
        fs::read(delivered(&job).join("large.bin")).unwrap(),
        payload
    );
    assert!(archive.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cache_waiting_jobs_finish_without_manual_restart_and_stay_within_quota() {
    let rig = Rig::new();
    let payload = vec![0x5Au8; 24 * 1024 * 1024];
    rig.file("cache-one/item.bin", &payload);
    rig.file("cache-two/item.bin", &payload);
    let first = rig.root.join("archives/cache-one.7z");
    let second = rig.root.join("archives/cache-two.7z");
    rig.pack(&rig.root.join("cache-one"), &first, "7z", None, &[]);
    rig.pack(&rig.root.join("cache-two"), &second, "7z", None, &[]);
    let mut settings = rig.settings();
    settings.cache_enabled = true;
    settings.cache_policy = "always".into();
    settings.cache_input = false;
    settings.concurrency = 2;
    settings.cache_quota_bytes = 36 * 1024 * 1024;
    settings.verification = "strict".into();
    let quota = settings.cache_quota_bytes;
    let first_id = rig.add(&first, settings.clone());
    let second_id = rig.add(&second, settings);
    rig.core.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut peak = 0u64;
    loop {
        let observed = sampled_cache_bytes(&rig.cache).await;
        peak = peak.max(observed);
        assert!(
            observed <= quota,
            "cache quota exceeded: {observed} > {quota}"
        );
        let snapshot = rig.core.snapshot().unwrap();
        if !snapshot.running {
            break;
        }
        if Instant::now() >= deadline {
            let _ = rig.core.cancel(None);
            rig.finish_with_timeout(&first_id, Duration::from_secs(15))
                .await;
            panic!("cache waiters did not finish automatically");
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let mut retried = false;
    for id in [&first_id, &second_id] {
        let job = rig.finish(id).await;
        retried |= job.attempts > 1;
        assert_eq!(job.status, "completed", "{}", job.message);
        assert_eq!(fs::read(delivered(&job).join("item.bin")).unwrap(), payload);
    }
    assert!(peak >= payload.len() as u64);
    let topology = openunpack_core::paths::probe_disk_topology(&first, &rig.output);
    if !topology.source_is_hdd && !topology.target_is_hdd {
        assert!(
            retried,
            "fixture must actually contend for cache quota on parallel SSD paths"
        );
    }
    eprintln!("cache_peak_bytes={peak} quota_bytes={quota} automatic_retry={retried}");
    assert!(first.exists() && second.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lz4_delivery_scope_preserves_frame_validation_in_direct_cache_and_nested_modes() {
    let rig = Rig::new();
    let payload = b"LZ4 synthetic scope fixture";
    // FLG=0x60, BD=0x40 has descriptor checksum 0x82; the frame contains
    // one uncompressed block and EndMark, without a declared content size.
    let mut frame = vec![0x04, 0x22, 0x4D, 0x18, 0x60, 0x40, 0x82];
    frame.extend_from_slice(&(0x80000000u32 | payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&[0u8; 4]);
    for (index, cached, strict) in [(0, false, false), (1, false, true), (2, true, true)] {
        let archive = rig.file(&format!("archives/frame-{index}.txt.lz4"), &frame);
        let mut settings = rig.settings();
        settings.cache_enabled = cached;
        settings.cache_policy = "always".into();
        settings.cache_input = cached;
        settings.cache_quota_bytes = 2 * 1024 * 1024;
        if strict {
            settings.verification = "strict".into();
        }
        let id = rig.add(&archive, settings);
        let job = rig.run(&id).await;
        assert_eq!(job.status, "completed", "{}", job.message);
        assert!(
            job.verification_scope
                .contains("frameValidationAndStreamSha256")
        );
        assert!(!job.verification_scope.contains("archiveCrc"));
        assert_eq!(job.verification_scope.contains("targetReadback"), strict);
        assert_eq!(
            fs::read(delivered(&job).join(format!("frame-{index}.txt"))).unwrap(),
            payload
        );
    }
    rig.file("mixed-payload/inner.txt.lz4", &frame);
    let archive = rig.root.join("archives/mixed-scope.7z");
    rig.pack(&rig.root.join("mixed-payload"), &archive, "7z", None, &[]);
    let mut settings = rig.settings();
    settings.verification = "strict".into();
    let id = rig.add(&archive, settings);
    let job = rig.run(&id).await;
    assert_eq!(job.status, "completed", "{}", job.message);
    assert!(job.verification_scope.contains("archiveCrcAndStreamSha256"));
    assert!(
        job.verification_scope
            .contains("frameValidationAndStreamSha256")
    );
    assert!(job.verification_scope.contains("targetReadback"));
    let archive = rig.file("archives/recovery-scope.txt.lz4", &frame);
    let id = rig.add(&archive, rig.settings());
    let connection = rusqlite::Connection::open(rig.root.join("data/jobs.sqlite")).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_scope_commit BEFORE UPDATE ON jobs WHEN NEW.status = 'completed' BEGIN SELECT RAISE(FAIL, 'synthetic commit failure'); END;").unwrap();
    let pending = rig.run(&id).await;
    assert_eq!(pending.status, "publishPending", "{}", pending.message);
    let journal: serde_json::Value = serde_json::from_slice(
        &fs::read(Path::new(pending.workspace.as_ref().unwrap()).join("publication.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        journal["verification_scope"],
        "frameValidationAndStreamSha256"
    );
    connection
        .execute_batch("DROP TRIGGER fail_scope_commit;")
        .unwrap();
    drop(connection);
    rig.core.recover(&id, "verify").await.unwrap();
    let recovered = rig.finish(&id).await;
    assert_eq!(recovered.status, "completed");
    assert_eq!(
        recovered.verification_scope,
        "frameValidationAndStreamSha256+targetReadback"
    );
    assert_eq!(
        fs::read(delivered(&recovered).join("recovery-scope.txt")).unwrap(),
        payload
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_size_cache_jobs_complete_after_whole_job_reservation_is_released() {
    let rig = Rig::new();
    // Unknown content size; each compressed block expands to 64 KiB of A.
    let mut block = vec![0x1f, b'A', 1, 0];
    block.extend_from_slice(&[255; 256]);
    block.push(231);
    block.push(0x50);
    block.extend_from_slice(b"AAAAA");
    let mut frame = vec![4, 0x22, 0x4d, 0x18, 0x60, 0x40, 0x82];
    for _ in 0..96 {
        frame.extend_from_slice(&(block.len() as u32).to_le_bytes());
        frame.extend_from_slice(&block);
    }
    frame.extend_from_slice(&[0; 4]);
    let first = rig.file("archives/unknown-one.bin.lz4", &frame);
    let second = rig.file("archives/unknown-two.bin.lz4", &frame);
    let mut settings = rig.settings();
    settings.concurrency = 2;
    settings.cache_enabled = true;
    settings.cache_policy = "always".into();
    settings.cache_input = false;
    settings.cache_quota_bytes = 10 * 1024 * 1024;
    settings.verification = "strict".into();
    let quota = settings.cache_quota_bytes;
    let first_id = rig.add(&first, settings.clone());
    let second_id = rig.add(&second, settings);
    rig.core.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut peak = 0;
    loop {
        let occupied = sampled_cache_bytes(&rig.cache).await;
        peak = peak.max(occupied);
        assert!(occupied <= quota, "quota exceeded: {occupied}");
        if !rig.core.snapshot().unwrap().running {
            break;
        }
        if Instant::now() >= deadline {
            let _ = rig.core.cancel(None);
            rig.finish_with_timeout(&first_id, Duration::from_secs(15))
                .await;
            panic!("unknown-size cache jobs did not finish");
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let mut retried = false;
    for (id, name) in [
        (&first_id, "unknown-one.bin"),
        (&second_id, "unknown-two.bin"),
    ] {
        let job = rig.finish(id).await;
        retried |= job.attempts > 1;
        assert_eq!(job.status, "completed", "{}", job.message);
        assert_eq!(
            fs::read(delivered(&job).join(name)).unwrap(),
            vec![b'A'; 6 * 1024 * 1024]
        );
    }
    let topology = openunpack_core::paths::probe_disk_topology(&first, &rig.output);
    if !topology.source_is_hdd && !topology.target_is_hdd {
        assert!(retried);
    }
    assert!(peak >= 6 * 1024 * 1024);
    eprintln!("unknown_cache_peak={peak} quota={quota} retried={retried}");
    assert!(first.exists() && second.exists());
}
