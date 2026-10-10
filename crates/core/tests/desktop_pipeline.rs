//! Windows acceptance tests across Coordinator -> named pipes -> .NET Worker -> official 7-Zip.
//! All archives, passwords, data stores and outputs are synthetic and tempfile-owned.
//! Requires a built Worker plus tools/7zip/{7z.exe,7z.dll}; no fake backend is used.
#![cfg(windows)]

use openunpack_core::{
    coordinator::Coordinator,
    scanner,
    types::{JobView, PasswordInput, Settings},
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
        let root = temporary.path().canonicalize().unwrap();
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
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let snapshot = self.core.snapshot().unwrap();
            let job = snapshot.jobs.into_iter().find(|job| job.id == id).unwrap();
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
    let second_path = delivered(&second);
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
        b"same output remains unchanged"
    );
    let mut skip = rig.settings();
    skip.conflict_policy = "skip".into();
    let third_id = rig.add(&archive, skip);
    let third = rig.run(&third_id).await;
    assert_eq!(third.status, "skipped", "{}", third.message);
    assert!(third.output_path.is_none());
    assert!(third.workspace.is_none());
    assert_eq!(digest_file(&archive), original);
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
        second.nodes[0].attempts, 2,
        "one empty-password attempt then the content-associated candidate; filename-derived associations would need a third attempt"
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
    rig.file("payload/a.bin", &vec![0x71; 4 * 1024 * 1024]);
    rig.file("payload/zz-last.txt", b"locked copy fixture");
    let archive = rig.root.join("archives/writeback.zip");
    rig.pack(&rig.root.join("payload"), &archive, "zip", None, &[]);
    let mut settings = rig.settings();
    settings.cache_enabled = true;
    let id = rig.add(&archive, settings);
    rig.core.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
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
            "copy stage was never observed"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    };
    let pending = rig.finish(&id).await;
    drop(lock);
    assert_eq!(pending.status, "publishPending", "{}", pending.message);
    assert!(pending.output_path.is_none());
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
