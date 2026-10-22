#![cfg(windows)]

use openunpack_application::WorkerSupervisor;
use openunpack_infrastructure::LocalWorkerSupervisor;
use openunpack_protocol::{CopyFile, WorkerRequest};
use std::fs::{self, File};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn native_supervisor_termination_joins_worker_and_releases_source_and_output_handles() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let executable =
        root.join("src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe");
    let engine = root.join("tools/7zip/7z.dll");
    assert!(
        executable.is_file(),
        "Build the native Release worker before this test"
    );
    let client = openunpack_infrastructure::worker::WorkerClient {
        executable: executable.clone(),
        engine: engine.clone(),
    };
    let unconfirmed_before = client.unconfirmed_processes();
    let supervisor = LocalWorkerSupervisor::new(executable, engine);
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("large.bin");
    File::create(&source)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let destination = dir.path().join("output");
    let (handle, requests, events) = supervisor.start_session("native-cancel").unwrap();
    let request = WorkerRequest {
        protocol_version: 1,
        request_id: "native-supervisor-cancel".into(),
        operation: "copy".into(),
        operation_id: None,
        generation: None,
        archive_path: String::new(),
        destination_directory: destination.to_string_lossy().into_owned(),
        password: None,
        allowed_volumes: vec![],
        max_entries: 100,
        max_total_bytes: 2 * 1024 * 1024 * 1024,
        max_written_bytes: 2 * 1024 * 1024 * 1024,
        min_free_bytes: 0,
        files: vec![CopyFile {
            source_path: source.to_string_lossy().into_owned(),
            relative_path: "large.bin".into(),
            size: 1024 * 1024 * 1024,
            size_known: true,
            sha256: String::new(),
            is_directory: false,
            is_corrupted: false,
        }],
        verification: "standard".into(),
        exclusions: vec![],
        inclusions: vec![],
        candidate_passwords: vec![],
        candidates: vec![],
        engine_threads: 0,
        worker_priority: String::new(),
        salvage_corrupted: false,
    };
    requests.send(request.clone()).unwrap();
    let started = Instant::now();
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(30).saturating_sub(started.elapsed()))
            .unwrap();
        if event.kind == "progress" {
            break;
        }
        assert!(
            !matches!(event.kind.as_str(), "failed" | "error" | "completed"),
            "expected active transfer before cancellation: {} {}",
            event.kind,
            event.error_code
        );
    }
    supervisor
        .terminate(&handle)
        .expect("termination must confirm native process and thread exit");
    assert_eq!(
        client.unconfirmed_processes(),
        unconfirmed_before,
        "no WorkerClient exit-timeout failure may be hidden by joining the supervisor thread"
    );
    assert!(requests.send(request).is_err()); // no later queued operation can launch
    let remaining: Vec<_> = events.try_iter().collect();
    assert!(remaining.iter().any(|event| event.kind == "cancelled"));
    let partial = destination.join("large.bin");
    if partial.exists() {
        fs::remove_file(partial).expect("native output handle released before terminate returns");
    }
    fs::remove_file(source).expect("native source handle released before terminate returns");
}
