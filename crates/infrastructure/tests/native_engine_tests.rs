use openunpack_infrastructure::engine::execute_native_worker;
use openunpack_protocol::worker::{WORKER_PROTOCOL_VERSION_V2, WorkerEvent, WorkerRequest};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

struct TestDir {
    _temp: Option<tempfile::TempDir>,
    path: PathBuf,
}
impl TestDir {
    fn new() -> Self {
        let current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root = current
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(&current);
        let base = root.join("target").join("test_scratch");
        let _ = fs::create_dir_all(&base);
        let p = base.join(format!("op-ne-{}", uuid::Uuid::new_v4().simple()));
        let _ = fs::create_dir_all(&p);
        Self {
            _temp: None,
            path: p,
        }
    }
    fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        if self._temp.is_none() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn create_sample_zip(path: &std::path::Path) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);

    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("hello.txt", options).unwrap();
    zip.write_all(b"Hello from Rust Native Engine!").unwrap();

    zip.start_file("sub/nested.txt", options).unwrap();
    zip.write_all(b"Nested file content 1234567890").unwrap();

    zip.start_file("sub/skip.tmp", options).unwrap();
    zip.write_all(b"Temporary file that should be excluded")
        .unwrap();

    zip.finish().unwrap();
}

fn make_request(op: &str, archive: &str, dest: &str) -> WorkerRequest {
    let mut req = WorkerRequest::default();
    req.protocol_version = WORKER_PROTOCOL_VERSION_V2;
    req.request_id = format!("req-{}", uuid::Uuid::new_v4().simple());
    req.operation = op.to_string();
    req.archive_path = archive.to_string();
    req.destination_directory = dest.to_string();
    req
}

#[tokio::test]
async fn test_native_engine_inspect() {
    let dir = TestDir::new();
    let zip_path = dir.path().join("sample.zip");
    create_sample_zip(&zip_path);

    let dest = dir.path().join("out");
    fs::create_dir_all(&dest).unwrap();

    let request = make_request(
        "inspect",
        &zip_path.to_string_lossy(),
        &dest.to_string_lossy(),
    );

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(|_ev: &WorkerEvent| {});

    let event = execute_native_worker(request, progress, cancel)
        .await
        .expect("inspect failed");
    assert_eq!(event.kind, "completed");
    assert_eq!(event.files.len(), 3);

    let paths: Vec<&str> = event
        .files
        .iter()
        .map(|f| f.relative_path.as_str())
        .collect();
    assert!(paths.contains(&"hello.txt"));
    assert!(paths.contains(&"sub/nested.txt"));
    assert!(paths.contains(&"sub/skip.tmp"));
}

#[tokio::test]
async fn test_native_engine_extract_with_exclusions() {
    let dir = TestDir::new();
    let zip_path = dir.path().join("sample.zip");
    create_sample_zip(&zip_path);

    let dest = dir.path().join("extracted");

    let mut request = make_request(
        "extract",
        &zip_path.to_string_lossy(),
        &dest.to_string_lossy(),
    );
    request.exclusions = vec!["*.tmp".into()];

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(|_ev: &WorkerEvent| {});

    let event = execute_native_worker(request, progress, cancel)
        .await
        .expect("extract failed");
    assert_eq!(event.kind, "completed");

    // "sub/skip.tmp" should have been excluded
    let paths: Vec<&str> = event
        .files
        .iter()
        .map(|f| f.relative_path.as_str())
        .collect();
    assert!(paths.contains(&"hello.txt"));
    assert!(paths.contains(&"sub/nested.txt"));
    assert!(!paths.contains(&"sub/skip.tmp"));

    // Verify disk content
    let hello_content = fs::read_to_string(dest.join("hello.txt")).unwrap();
    assert_eq!(hello_content, "Hello from Rust Native Engine!");

    let nested_content = fs::read_to_string(dest.join("sub/nested.txt")).unwrap();
    assert_eq!(nested_content, "Nested file content 1234567890");

    assert!(!dest.join("sub/skip.tmp").exists());
}

#[tokio::test]
async fn test_native_engine_zip_slip_prevention() {
    let dir = TestDir::new();
    let zip_path = dir.path().join("malicious.zip");

    // Craft a zip with ../ traversal
    let file = File::create(&zip_path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("../../evil.txt", options).unwrap();
    zip.write_all(b"pwned").unwrap();
    zip.finish().unwrap();

    let dest = dir.path().join("out");

    let request = make_request(
        "extract",
        &zip_path.to_string_lossy(),
        &dest.to_string_lossy(),
    );

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(|_ev: &WorkerEvent| {});

    let result = execute_native_worker(request, progress, cancel).await;
    assert!(result.is_err(), "Zip Slip attack must be rejected!");
    assert!(result.unwrap_err().contains("UnsafePath"));
}

#[tokio::test]
async fn test_native_engine_salvage_corrupted_partial_recovery() {
    let dir = TestDir::new();
    let zip_path = dir.path().join("partially_corrupted.zip");

    // 1. Build a valid zip with two entries
    {
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        zip.start_file("healthy.txt", options).unwrap();
        zip.write_all(b"This is intact data that must be salvaged.")
            .unwrap();

        zip.start_file("damaged.txt", options).unwrap();
        zip.write_all(b"Damaged payload that will fail CRC.")
            .unwrap();

        zip.finish().unwrap();
    }

    // 2. Corrupt bytes belonging to damaged.txt payload in the zip file
    let mut raw_bytes = fs::read(&zip_path).unwrap();
    if let Some(pos) = raw_bytes.windows(7).position(|w| w == b"Damaged") {
        raw_bytes[pos] = b'X';
        raw_bytes[pos + 1] = b'Y';
        fs::write(&zip_path, &raw_bytes).unwrap();
    }

    let dest = dir.path().join("salvaged_out");
    fs::create_dir_all(&dest).unwrap();

    // Without salvage mode -> extraction fails due to CRC mismatch
    let mut normal_req = make_request(
        "extract",
        &zip_path.to_string_lossy(),
        &dest.to_string_lossy(),
    );
    normal_req.salvage_corrupted = false;
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(|_ev: &WorkerEvent| {});

    let normal_result = execute_native_worker(normal_req, progress.clone(), cancel.clone()).await;
    assert!(
        normal_result.is_err(),
        "Non-salvage mode must reject corrupted zip"
    );

    // With salvage mode -> extracts healthy.txt intact, marks damaged.txt as corrupted, returns Ok
    let mut salvage_req = make_request(
        "extract",
        &zip_path.to_string_lossy(),
        &dest.to_string_lossy(),
    );
    salvage_req.salvage_corrupted = true;

    let salvage_result = execute_native_worker(salvage_req, progress, cancel).await;
    assert!(
        salvage_result.is_ok(),
        "Salvage mode must succeed and recover readable data"
    );

    let event = salvage_result.unwrap();
    let healthy_file = event
        .files
        .iter()
        .find(|f| f.relative_path == "healthy.txt")
        .expect("healthy file present");
    assert!(!healthy_file.is_corrupted);

    let damaged_file = event
        .files
        .iter()
        .find(|f| f.relative_path == "damaged.txt")
        .expect("damaged file present");
    assert!(damaged_file.is_corrupted);

    // Verify healthy file was actually written to disk intact
    let healthy_content = fs::read_to_string(dest.join("healthy.txt")).unwrap();
    assert_eq!(
        healthy_content,
        "This is intact data that must be salvaged."
    );
}
