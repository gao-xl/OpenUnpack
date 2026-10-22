#![cfg(windows)]

use openunpack_core::{
    types::{CopyFile, WorkerEvent, WorkerRequest},
    worker::WorkerClient,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

fn client() -> WorkerClient {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let executable = std::env::var_os("OPENUNPACK_TEST_WORKER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.join("src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe")
        });
    let engine = std::env::var_os("OPENUNPACK_TEST_ENGINE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("tools/7zip/7z.dll"));
    assert!(
        executable.is_file(),
        "Build the Release .NET Worker before this integration test."
    );
    assert!(
        engine.is_file(),
        "Install the official test engine before this integration test."
    );
    WorkerClient { executable, engine }
}

fn request(operation: &str, destination: &Path) -> WorkerRequest {
    WorkerRequest {
        protocol_version: 1,
        request_id: uuid::Uuid::new_v4().to_string(),
        operation: operation.into(),
        operation_id: None,
        generation: None,
        archive_path: String::new(),
        destination_directory: destination.to_string_lossy().into_owned(),
        password: None,
        allowed_volumes: vec![],
        max_entries: 100,
        max_total_bytes: 10 * 1024 * 1024,
        max_written_bytes: 10 * 1024 * 1024,
        min_free_bytes: 0,
        files: vec![],
        verification: "standard".into(),
        exclusions: vec![],
        inclusions: vec![],
        candidate_passwords: vec![],
        candidates: vec![],
        engine_threads: 0,
        worker_priority: String::new(),
        salvage_corrupted: false,
    }
}

async fn run(client: &WorkerClient, request: WorkerRequest) -> WorkerEvent {
    tokio::time::timeout(
        Duration::from_secs(30),
        client.run(request, Arc::new(AtomicBool::new(false)), Arc::new(|_| {})),
    )
    .await
    .expect("Worker completion deadline")
    .expect("Worker transport and exit confirmation")
}

fn stored_zip(path: &Path, name: &str, content: &[u8]) {
    fn n16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn n32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut crc = 0xffff_ffffu32;
    for byte in content {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    crc = !crc;
    let mut bytes = Vec::new();
    n32(&mut bytes, 0x0403_4b50);
    n16(&mut bytes, 20);
    for _ in 0..4 {
        n16(&mut bytes, 0);
    }
    n32(&mut bytes, crc);
    n32(&mut bytes, content.len() as u32);
    n32(&mut bytes, content.len() as u32);
    n16(&mut bytes, name.len() as u16);
    n16(&mut bytes, 0);
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(content);
    let central_offset = bytes.len() as u32;
    n32(&mut bytes, 0x0201_4b50);
    n16(&mut bytes, 20);
    n16(&mut bytes, 20);
    for _ in 0..4 {
        n16(&mut bytes, 0);
    }
    n32(&mut bytes, crc);
    n32(&mut bytes, content.len() as u32);
    n32(&mut bytes, content.len() as u32);
    n16(&mut bytes, name.len() as u16);
    for _ in 0..4 {
        n16(&mut bytes, 0);
    }
    n32(&mut bytes, 0);
    n32(&mut bytes, 0);
    bytes.extend_from_slice(name.as_bytes());
    let central_size = bytes.len() as u32 - central_offset;
    n32(&mut bytes, 0x0605_4b50);
    n16(&mut bytes, 0);
    n16(&mut bytes, 0);
    n16(&mut bytes, 1);
    n16(&mut bytes, 1);
    n32(&mut bytes, central_size);
    n32(&mut bytes, central_offset);
    n16(&mut bytes, 0);
    fs::write(path, bytes).unwrap();
}

#[tokio::test]
async fn native_extract_copy_and_target_readback_through_authenticated_pipes() {
    let folder = tempfile::tempdir().unwrap();
    let archive = folder.path().join("archive.zip");
    let content = "真实 Rust 到 C# 的原生解压验证".as_bytes();
    stored_zip(&archive, "./folder/text.txt", content);
    let client = client();
    let unpacked = folder.path().join("unpacked");
    let mut extract = request("extract", &unpacked);
    extract.archive_path = archive.to_string_lossy().into_owned();
    extract.allowed_volumes = vec![extract.archive_path.clone()];
    let extracted = run(&client, extract).await;
    assert_eq!(extracted.kind, "completed", "{}", extracted.error_code);
    assert!(!extracted.password_used);
    assert_eq!(extracted.files.len(), 1);
    assert_eq!(
        extracted.files[0].sha256,
        hex::encode(Sha256::digest(content))
    );
    assert_eq!(fs::read(unpacked.join("folder/text.txt")).unwrap(), content);

    let target = folder.path().join("target");
    let mut copy = request("copy", &target);
    copy.files = extracted
        .files
        .iter()
        .map(|file| CopyFile {
            source_path: unpacked
                .join(&file.relative_path)
                .to_string_lossy()
                .into_owned(),
            relative_path: file.relative_path.clone(),
            size: file.size,
            size_known: true,
            sha256: file.sha256.clone(),
            is_directory: file.is_directory,
            is_corrupted: file.is_corrupted,
        })
        .collect();
    let copied = run(&client, copy).await;
    assert_eq!(copied.kind, "completed", "{}", copied.error_code);
    assert_eq!(copied.verification_scope, "sourceStreamSha256");
    let mut readback = request("readback", &target);
    readback.files = copied
        .files
        .iter()
        .map(|file| CopyFile {
            source_path: target
                .join(&file.relative_path)
                .to_string_lossy()
                .into_owned(),
            relative_path: file.relative_path.clone(),
            size: file.size,
            size_known: true,
            sha256: file.sha256.clone(),
            is_directory: file.is_directory,
            is_corrupted: file.is_corrupted,
        })
        .collect();
    let verified = run(&client, readback.clone()).await;
    assert_eq!(verified.kind, "completed", "{}", verified.error_code);
    assert_eq!(verified.verification_scope, "targetReadback");
    fs::write(target.join("folder/text.txt"), b"changed").unwrap();
    let mismatch = run(&client, readback).await;
    assert_eq!(mismatch.kind, "error");
    assert_eq!(mismatch.error_code, "Corrupt");
    assert!(archive.is_file(), "Original archive is preserved.");
}

#[tokio::test]
async fn unsafe_mapping_is_a_terminal_error_with_confirmed_process_exit() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("source.txt");
    fs::write(&source, b"fixture").unwrap();
    let destination = folder.path().join("output");
    let mut copy = request("copy", &destination);
    copy.files = vec![CopyFile {
        source_path: source.to_string_lossy().into_owned(),
        relative_path: "../escaped.txt".into(),
        size: 7,
        size_known: true,
        sha256: String::new(),
        is_directory: false,
        is_corrupted: false,
    }];
    let result = run(&client(), copy).await;
    assert_eq!(result.kind, "error");
    assert_eq!(result.error_code, "UnsafePath");
    assert!(!folder.path().join("escaped.txt").exists());
    assert!(source.is_file());
}

#[tokio::test]
async fn cancellation_remains_responsive_during_transfer_and_releases_file_handles() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("large.bin");
    File::create(&source)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let destination = folder.path().join("partial");
    let cancel = Arc::new(AtomicBool::new(false));
    let observed = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let seen = observed.clone();
    let progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync> = Arc::new(move |event| {
        if event.kind == "progress" {
            seen.store(true, Ordering::Release);
            flag.store(true, Ordering::Release);
        }
    });
    let mut copy = request("copy", &destination);
    copy.max_written_bytes = 2 * 1024 * 1024 * 1024;
    copy.files = vec![CopyFile {
        source_path: source.to_string_lossy().into_owned(),
        relative_path: "large.bin".into(),
        size: 1024 * 1024 * 1024,
        size_known: true,
        sha256: String::new(),
        is_directory: false,
        is_corrupted: false,
    }];
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        client().run(copy, cancel, progress),
    )
    .await
    .expect("Cancellation deadline")
    .expect("Confirmed worker exit");
    assert!(observed.load(Ordering::Acquire));
    assert_eq!(result.kind, "cancelled");
    assert_eq!(result.error_code, "Cancelled");
    // A Windows rename/delete cannot succeed while the Worker retains FileShare.None.
    let partial = destination.join("large.bin");
    if partial.exists() {
        fs::remove_file(&partial).expect("Worker output handle was released before returning.");
    }
    fs::remove_file(&source).expect("Worker source handle was released before returning.");
}

#[tokio::test]
async fn aborting_outer_worker_future_releases_progress_capture_and_confirms_process_exit() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("large.bin");
    File::create(&source)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let destination = folder.path().join("partial");
    let mut copy = request("copy", &destination);
    copy.max_written_bytes = 2 * 1024 * 1024 * 1024;
    copy.files = vec![CopyFile {
        source_path: source.to_string_lossy().into_owned(),
        relative_path: "large.bin".into(),
        size: 1024 * 1024 * 1024,
        size_known: true,
        sha256: String::new(),
        is_directory: false,
        is_corrupted: false,
    }];
    let request_id = copy.request_id.clone();
    let marker = Arc::new(());
    let weak = Arc::downgrade(&marker);
    let observed = Arc::new(tokio::sync::Notify::new());
    let ready = observed.clone();
    let progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync> = Arc::new(move |event| {
        let _marker = &marker;
        if event.kind == "progress" && event.completed_bytes > 0 {
            ready.notify_one();
        }
    });
    let client = client();
    client.retain_request_receipt(&request_id).unwrap();
    let running_client = client.clone();
    let operation = tokio::spawn(async move {
        running_client
            .run(copy, Arc::new(AtomicBool::new(false)), progress)
            .await
    });
    tokio::time::timeout(Duration::from_secs(30), observed.notified())
        .await
        .expect("actual Worker transfer progress before abort");
    assert!(
        !operation.is_finished(),
        "the regression must abort a live outer operation"
    );
    let before = client.request_exit_receipt(&request_id).unwrap();
    assert!(!before.exit_confirmed && !before.processes.is_empty());
    operation.abort();
    assert!(matches!(operation.await, Err(error) if error.is_cancelled()));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let exited = before.processes.iter().all(|(pid, created)| {
                openunpack_core::worker::process_identity_exited(*pid, *created).unwrap_or(false)
            });
            if exited && weak.upgrade().is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("cancelled Worker exits and all progress callback owners are dropped");
    assert!(
        client
            .request_exit_receipt(&request_id)
            .unwrap()
            .exit_confirmed
    );
    client.forget_confirmed_request(&request_id);
    assert_eq!(fs::metadata(&source).unwrap().len(), 1024 * 1024 * 1024);
    fs::remove_file(&source).expect("aborted Worker released its source handle");
}
