//! Native Worker failures and graceful cancellation/reopen, using private UUID fixtures.
//! These cases do not establish physical removal, network interruption, OS disk-full,
//! abrupt Host crash, clean-machine installation or native GUI acceptance.
#![cfg(windows)]

use openunpack_core::{
    coordinator::Coordinator,
    paths::DirectoryGuard,
    types::{CopyFile, JobView, Settings, WorkerEvent, WorkerRequest},
    worker::{WorkerClient, process_identity_exited},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::{Duration, Instant},
};
use tempfile::{Builder, TempDir};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};

// PID attribution in the cancellation case requires one operation from this test executable.
static TEST_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Rig {
    root: PathBuf,
    worker: PathBuf,
    engine: PathBuf,
    seven_zip: PathBuf,
    temporary: Mutex<Option<TempDir>>,
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
        let engine = repository.join("tools/7zip/7z.dll");
        let seven_zip = repository.join("tools/7zip/7z.exe");
        assert!(worker.is_file() && engine.is_file() && seven_zip.is_file());
        let temporary = Builder::new()
            .prefix(&format!("openunpack-release-{}-", uuid::Uuid::new_v4()))
            .tempdir()
            .unwrap();
        let private = temporary.path().join("private");
        DirectoryGuard::open(temporary.path())
            .unwrap()
            .create_private_child(&private)
            .unwrap();
        let root = private.canonicalize().unwrap();
        for directory in ["input", "archives", "output", "cache"] {
            fs::create_dir(root.join(directory)).unwrap();
        }
        fs::write(
            root.join(".release-qa-owner"),
            uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
        Self {
            root,
            worker,
            engine,
            seven_zip,
            temporary: Mutex::new(Some(temporary)),
        }
    }

    fn preserve(&self) {
        if let Some(temporary) = self.temporary.lock().unwrap().take() {
            eprintln!(
                "unconfirmed exit; private diagnostic fixture retained: {:?}",
                temporary.keep()
            );
        }
    }

    fn client(&self) -> WorkerClient {
        WorkerClient {
            executable: self.worker.clone(),
            engine: self.engine.clone(),
        }
    }

    fn coordinator(&self) -> Arc<Coordinator> {
        Coordinator::open(
            self.root.join("data"),
            self.worker.clone(),
            self.engine.clone(),
        )
        .unwrap()
    }

    fn pack(&self, name: &str, mebibytes: usize) -> PathBuf {
        let payload = self.root.join("input/data.bin");
        let mut file = File::create(payload).unwrap();
        let block = vec![0x53; 1024 * 1024];
        for _ in 0..mebibytes {
            file.write_all(&block).unwrap();
        }
        drop(file);
        let archive = self.root.join(format!("archives/{name}.zip"));
        assert!(
            Command::new(&self.seven_zip)
                .current_dir(self.root.join("input"))
                .args(["a", "-tzip", "-mx=0", "-y", "-bso0", "-bsp0", "-bse0"])
                .arg(&archive)
                .arg(".")
                .status()
                .unwrap()
                .success(),
            "official fixture packer failed"
        );
        archive
    }

    async fn run(&self, request: WorkerRequest) -> WorkerEvent {
        let client = self.client();
        let id = request.request_id.clone();
        client.retain_request_receipt(&id).unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(60),
            client.run(request, Arc::new(AtomicBool::new(false)), Arc::new(|_| {})),
        )
        .await;
        let receipt = client.request_exit_receipt(&id);
        if !receipt
            .as_ref()
            .is_some_and(|receipt| receipt.exit_confirmed)
        {
            self.preserve();
        }
        let event = result
            .expect("native operation deadline")
            .expect("native transport and confirmed exit");
        let receipt = receipt.expect("native operation must leave an exit receipt");
        assert!(receipt.exit_confirmed);
        assert!(
            !receipt.processes.is_empty(),
            "a real Worker must have started"
        );
        for (pid, created) in receipt.processes {
            assert!(process_identity_exited(pid, created).unwrap());
        }
        client.forget_confirmed_request(&id);
        event
    }

    async fn finish(&self, core: &Arc<Coordinator>, id: &str, bound: Duration) -> JobView {
        let deadline = Instant::now() + bound;
        loop {
            let snapshot = core.snapshot().unwrap();
            let job = snapshot.jobs.into_iter().find(|job| job.id == id).unwrap();
            if !snapshot.running {
                return job;
            }
            if Instant::now() >= deadline {
                core.cancel(None).unwrap();
                let exit_deadline = Instant::now() + Duration::from_secs(10);
                while core.snapshot().unwrap().running && Instant::now() < exit_deadline {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                if core.snapshot().unwrap().running {
                    self.preserve();
                }
                panic!(
                    "pipeline deadline: status={} stage={} message={}",
                    job.status, job.stage, job.message
                );
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
}

fn digest(path: &Path) -> String {
    let mut file = File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    hex::encode(hash.finalize())
}

fn request(operation: &str, destination: &Path) -> WorkerRequest {
    serde_json::from_value(serde_json::json!({
        "protocolVersion": 2, "requestId": uuid::Uuid::new_v4().to_string(),
        "operationId": uuid::Uuid::new_v4().to_string(), "generation": 1,
        "operation": operation, "destinationDirectory": destination,
        "maxEntries": 1000, "maxTotalBytes": 256 * 1024 * 1024,
        "maxWrittenBytes": 256 * 1024 * 1024, "minFreeBytes": 0,
        "verification": "strict", "engineThreads": 1, "workerPriority": "belowNormal"
    }))
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn moved_source_is_reported_by_real_worker_and_retry_after_restore_is_exact() {
    let _serial = TEST_GATE.lock().await;
    let rig = Rig::new();
    let archive = rig.pack("moved-source", 2);
    let source_hash = digest(&archive);
    let payload_hash = digest(&rig.root.join("input/data.bin"));
    let destination = rig.root.join("output/moved-source");
    let moved = archive.with_extension("held");
    fs::rename(&archive, &moved).unwrap();
    let mut extract = request("extract", &destination);
    extract.archive_path = archive.to_string_lossy().into_owned();
    extract.allowed_volumes = vec![extract.archive_path.clone()];
    let failed = rig.run(extract).await;
    assert_eq!(failed.kind, "error");
    // The Worker pins all approved source handles before opening the archive engine.
    // A moved root file fails that filesystem lease as IoError; MissingVolume
    // describes missing parts diagnosed later by the archive/volume resolver.
    assert_eq!(failed.error_code, "IoError");
    assert!(!destination.join("data.bin").exists());
    assert_eq!(digest(&moved), source_hash);
    fs::rename(&moved, &archive).unwrap();
    let mut extract = request("extract", &destination);
    extract.archive_path = archive.to_string_lossy().into_owned();
    extract.allowed_volumes = vec![extract.archive_path.clone()];
    let completed = rig.run(extract).await;
    assert_eq!(completed.kind, "completed", "{}", completed.error_code);
    assert_eq!(completed.files.len(), 1);
    assert_eq!(digest(&destination.join("data.bin")), payload_hash);
    assert_eq!(digest(&archive), source_hash);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn occupied_target_fails_real_copy_without_overwrite_and_releases_source_handles() {
    let _serial = TEST_GATE.lock().await;
    let rig = Rig::new();
    let archive = rig.pack("occupied-target", 2);
    let source_hash = digest(&archive);
    let payload = rig.root.join("input/data.bin");
    let payload_hash = digest(&payload);
    let output = rig.root.join("output/data.bin");
    fs::write(&output, b"previous successful output must survive").unwrap();
    let existing_hash = digest(&output);
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&output)
        .unwrap();
    let mut copy = request("copy", &rig.root.join("output"));
    copy.files = vec![CopyFile {
        source_path: payload.to_string_lossy().into_owned(),
        relative_path: "data.bin".into(),
        size: fs::metadata(&payload).unwrap().len(),
        size_known: true,
        sha256: payload_hash.clone(),
        is_directory: false,
        is_corrupted: false,
    }];
    let failed = rig.run(copy).await;
    drop(held);
    assert_eq!(failed.kind, "error");
    assert_eq!(failed.error_code, "IoError");
    assert_eq!(digest(&output), existing_hash);
    assert_eq!(digest(&payload), payload_hash);
    assert_eq!(digest(&archive), source_hash);
    let exclusive = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&payload)
        .unwrap();
    drop(exclusive);
}

fn live_worker_identities() -> Vec<(u32, u64)> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    assert_ne!(snapshot, INVALID_HANDLE_VALUE);
    let mut processes = vec![];
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) };
    while more != 0 {
        let end = entry
            .szExeFile
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(entry.szExeFile.len());
        if entry.th32ParentProcessID == std::process::id()
            && String::from_utf16_lossy(&entry.szExeFile[..end])
                .eq_ignore_ascii_case("OpenUnpack.Worker.exe")
        {
            let handle =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID) };
            if !handle.is_null() {
                let mut created = FILETIME::default();
                let mut exit = FILETIME::default();
                let mut kernel = FILETIME::default();
                let mut user = FILETIME::default();
                if unsafe {
                    GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user)
                } != 0
                {
                    let created = (u64::from(created.dwHighDateTime) << 32)
                        | u64::from(created.dwLowDateTime);
                    if process_identity_exited(entry.th32ProcessID, created)
                        .is_ok_and(|exited| !exited)
                    {
                        processes.push((entry.th32ProcessID, created));
                    }
                }
                unsafe {
                    CloseHandle(handle);
                }
            }
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    unsafe {
        CloseHandle(snapshot);
    }
    processes
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_native_extraction_reopens_as_cancelled_and_can_retry_without_false_publication()
{
    let _serial = TEST_GATE.lock().await;
    let rig = Rig::new();
    let archive = rig.pack("cancel-reopen", 128);
    let archive_hash = digest(&archive);
    let payload_hash = digest(&rig.root.join("input/data.bin"));
    let sentinel = rig.root.join("output/existing-result.txt");
    fs::write(&sentinel, b"unrelated existing output").unwrap();
    let sentinel_hash = digest(&sentinel);
    let core = rig.coordinator();
    let settings = Settings {
        output_root: rig.root.join("output").to_string_lossy().into_owned(),
        cache_root: rig.root.join("cache").to_string_lossy().into_owned(),
        cache_enabled: true,
        cache_policy: "always".into(),
        cache_input: false,
        expand_nested: false,
        min_free_bytes: 0,
        concurrency: 1,
        max_expanded_bytes: 256 * 1024 * 1024,
        cache_quota_bytes: 512 * 1024 * 1024,
        verification: "strict".into(),
        clean_empty_dirs: false,
        ..Settings::default()
    };
    core.save_settings(settings.clone()).unwrap();
    let sources = core
        .scan(&[archive.to_string_lossy().into_owned()])
        .unwrap()
        .sources;
    let id = core.add_jobs(sources, settings).unwrap().remove(0).id;
    core.start().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let processes = loop {
        let snapshot = core.snapshot().unwrap();
        let job = snapshot.jobs.iter().find(|job| job.id == id).unwrap();
        if job.stage == "extracting"
            && job
                .transfer_progress
                .as_ref()
                .is_some_and(|progress| progress.completed_bytes > 0)
        {
            let processes = live_worker_identities();
            if processes.len() == 1 {
                break processes;
            }
        }
        if !snapshot.running || Instant::now() >= deadline {
            core.cancel(None).unwrap();
            let _ = rig.finish(&core, &id, Duration::from_secs(10)).await;
            panic!(
                "real extraction progress and one attributable Worker were not observed: {} {}",
                job.status, job.stage
            );
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    };
    core.cancel(Some(&id)).unwrap();
    let cancelled = rig.finish(&core, &id, Duration::from_secs(5)).await;
    assert_eq!(cancelled.status, "cancelled", "{}", cancelled.message);
    assert!(cancelled.output_path.is_none());
    assert!(!rig.root.join("output/cancel-reopen").exists());
    for (pid, created) in processes {
        assert!(process_identity_exited(pid, created).unwrap());
    }
    assert_eq!(digest(&archive), archive_hash);
    assert_eq!(digest(&sentinel), sentinel_hash);
    let stopped = Instant::now() + Duration::from_secs(5);
    while Arc::strong_count(&core) != 1 && Instant::now() < stopped {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        Arc::strong_count(&core),
        1,
        "no background run may retain the previous Coordinator"
    );
    drop(core);
    let reopened = rig.coordinator();
    let persisted = reopened
        .snapshot()
        .unwrap()
        .jobs
        .into_iter()
        .find(|job| job.id == id)
        .unwrap();
    assert_eq!(persisted.status, "cancelled");
    assert!(persisted.output_path.is_none());
    assert_eq!(persisted.workspace, cancelled.workspace);
    reopened.retry(&id, false).unwrap();
    reopened.start().unwrap();
    let completed = rig.finish(&reopened, &id, Duration::from_secs(90)).await;
    assert_eq!(completed.status, "completed", "{}", completed.message);
    assert_eq!(
        digest(&PathBuf::from(completed.output_path.unwrap()).join("data.bin")),
        payload_hash
    );
    assert_eq!(digest(&archive), archive_hash);
    assert_eq!(digest(&sentinel), sentinel_hash);
    assert!(live_worker_identities().is_empty());
}
