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
    assert!(!dest.join("evil.txt").exists());
    assert!(!dir.path().parent().unwrap().join("evil.txt").exists());
}

#[tokio::test]
async fn zip_safety_and_entry_budget_errors_are_not_retried_by_another_engine() {
    let dir = TestDir::new();
    let zip_path = dir.path().join("sample.zip");
    create_sample_zip(&zip_path);
    let dest = dir.path().join("out");
    for operation in ["inspect", "extract"] {
        let mut request = make_request(
            operation,
            &zip_path.to_string_lossy(),
            &dest.to_string_lossy(),
        );
        request.max_entries = 1;
        let error = execute_native_worker(
            request,
            Arc::new(|_: &WorkerEvent| {}),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap_err();
        assert!(error.starts_with("ResourceLimit"), "{operation}: {error}");
        assert!(!dest.join("sub/nested.txt").exists());
        assert!(!dest.join("sub/skip.tmp").exists());
    }
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

#[tokio::test]
async fn test_unix_symlink_entry_safely_skipped_in_zip() {
    let dir = TestDir::new();
    let zip_path = dir.path().join("with_symlink.zip");

    {
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);

        let options_normal =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("healthy.txt", options_normal).unwrap();
        zip.write_all(b"Normal file content").unwrap();

        let options_symlink =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.add_symlink("my_symlink", "../healthy.txt", options_symlink)
            .unwrap();

        zip.finish().unwrap();
    }

    let dest = dir.path().join("out");
    let request = make_request(
        "extract",
        &zip_path.to_string_lossy(),
        &dest.to_string_lossy(),
    );

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(|_ev: &WorkerEvent| {});

    let result = execute_native_worker(request, progress, cancel).await;
    assert!(
        result.is_ok(),
        "Zip with symlink should safely skip symlink and succeed: {:?}",
        result.err()
    );

    let event = result.unwrap();
    assert!(event.files.iter().any(|f| f.relative_path == "healthy.txt"));
    assert!(!event.files.iter().any(|f| f.relative_path == "my_symlink"));
    assert!(dest.join("healthy.txt").exists());
}

#[tokio::test]
async fn test_hyphenated_archive_name_cli_option_injection_defense() {
    use openunpack_infrastructure::engine::cli_engine::{extract_cli, has_7z_cli, inspect_cli};

    if !has_7z_cli() {
        eprintln!("Skipping test: 7z binary not found");
        return;
    }

    let dir = TestDir::new();
    // Test archive named with leading hyphen that mimics a 7z switch (-sdel)
    let hyphen_zip = dir.path().join("-sdel.zip");
    create_sample_zip(&hyphen_zip);

    assert!(hyphen_zip.exists(), "Hyphen archive must exist on disk");

    // 1. inspect_cli with -sdel.zip
    let inspect_res = inspect_cli(&hyphen_zip, None, 10_000, 1 << 30).await;
    assert!(
        inspect_res.is_ok(),
        "inspect_cli must succeed on -sdel.zip: {:?}",
        inspect_res.err()
    );
    let files = inspect_res.unwrap();
    assert!(files.iter().any(|f| f.relative_path == "hello.txt"));

    // 2. extract_cli with -sdel.zip
    let dest = dir.path().join("extracted_hyphen");
    fs::create_dir_all(&dest).unwrap();

    let req = make_request(
        "extract",
        &hyphen_zip.to_string_lossy(),
        &dest.to_string_lossy(),
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(|_ev: &WorkerEvent| {});

    let extract_res = extract_cli(&req, &dest, progress, cancel).await;
    assert!(
        extract_res.is_ok(),
        "extract_cli must succeed on -sdel.zip: {:?}",
        extract_res.err()
    );

    // Verify file extracted
    assert!(
        dest.join("hello.txt").exists(),
        "hello.txt must be extracted"
    );
    // Verify -sdel did NOT delete the archive file!
    assert!(
        hyphen_zip.exists(),
        "-sdel switch must not have been triggered; archive must still exist!"
    );

    // 3. extract_cli_selective with -sdel.zip
    use openunpack_infrastructure::engine::cli_engine::extract_cli_selective;
    let selective_dest = dir.path().join("extracted_selective");
    fs::create_dir_all(&selective_dest).unwrap();

    let selective_res = extract_cli_selective(
        &hyphen_zip,
        &["hello.txt".to_string()],
        &selective_dest,
        None,
        false,
        Arc::new(AtomicBool::new(false)),
    )
    .await;
    assert!(
        selective_res.is_ok(),
        "extract_cli_selective must succeed on -sdel.zip: {:?}",
        selective_res.err()
    );
    assert!(
        selective_dest.join("hello.txt").exists(),
        "hello.txt must exist after selective extract"
    );
    assert!(
        hyphen_zip.exists(),
        "Archive must still exist after selective extract"
    );
}

#[tokio::test]
async fn test_empty_archive_and_all_excluded_semantic_parity() {
    let dir = TestDir::new();

    // 1. Create an empty zip archive (0 entries)
    let empty_zip_path = dir.path().join("empty.zip");
    {
        let file = File::create(&empty_zip_path).unwrap();
        let zip = ZipWriter::new(file);
        zip.finish().unwrap();
    }

    let dest_empty_zip = dir.path().join("empty_zip_out");
    fs::create_dir_all(&dest_empty_zip).unwrap();
    let req_empty_zip = make_request(
        "extract",
        &empty_zip_path.to_string_lossy(),
        &dest_empty_zip.to_string_lossy(),
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(|_ev: &WorkerEvent| {});

    let res_empty_zip =
        execute_native_worker(req_empty_zip, progress.clone(), cancel.clone()).await;
    assert!(
        res_empty_zip.is_ok(),
        "Empty ZIP extraction must succeed with empty file list without misreporting CorruptArchive: {:?}",
        res_empty_zip.err()
    );
    assert_eq!(res_empty_zip.unwrap().files.len(), 0);

    // 2. Create a zip with files, then extract with an exclusion pattern that filters all files
    let zip_with_files = dir.path().join("sample_all_excluded.zip");
    create_sample_zip(&zip_with_files);

    let dest_all_excl = dir.path().join("all_excluded_out");
    fs::create_dir_all(&dest_all_excl).unwrap();
    let mut req_all_excl = make_request(
        "extract",
        &zip_with_files.to_string_lossy(),
        &dest_all_excl.to_string_lossy(),
    );
    req_all_excl.exclusions = vec!["*.*".to_string()];

    let res_all_excl = execute_native_worker(req_all_excl, progress.clone(), cancel.clone()).await;
    assert!(
        res_all_excl.is_ok(),
        "ZIP with all entries excluded must succeed with 0 files extracted: {:?}",
        res_all_excl.err()
    );
    assert_eq!(res_all_excl.unwrap().files.len(), 0);

    // 3. Test CLI extractor on empty zip if 7z is present
    use openunpack_infrastructure::engine::cli_engine::{extract_cli, has_7z_cli};
    if has_7z_cli() {
        let dest_cli = dir.path().join("empty_cli_out");
        fs::create_dir_all(&dest_cli).unwrap();
        let req_cli = make_request(
            "extract",
            &empty_zip_path.to_string_lossy(),
            &dest_cli.to_string_lossy(),
        );
        let res_cli = extract_cli(&req_cli, &dest_cli, progress, cancel).await;
        assert!(
            res_cli.is_ok(),
            "CLI engine on empty zip must succeed with empty records: {:?}",
            res_cli.err()
        );
        assert_eq!(res_cli.unwrap().len(), 0);
    }
}

#[tokio::test]
async fn test_windows_oem_non_ascii_filename_extraction() {
    use openunpack_infrastructure::engine::cli_engine::{extract_cli, has_7z_cli, inspect_cli};
    if !has_7z_cli() {
        eprintln!("Skipping test: 7z binary not found");
        return;
    }

    let dir = TestDir::new();
    let bin = openunpack_infrastructure::engine::cli_engine::find_7z_binary().unwrap();

    let zh_file = dir.path().join("批量解压缩_中文报告.txt");
    let ja_file = dir.path().join("日本語テストファイル.txt");
    fs::write(&zh_file, "中文测试内容").unwrap();
    fs::write(&ja_file, "日本語テスト内容").unwrap();

    let archive_path = dir.path().join("non_ascii_test.7z");
    let mut create_cmd = std::process::Command::new(&bin);
    create_cmd
        .arg("a")
        .arg(&archive_path)
        .arg(&zh_file)
        .arg(&ja_file)
        .arg("-y")
        .arg("-bso0")
        .arg("-bsp0")
        .arg("-bse0");
    let create_status = create_cmd.status().expect("create non-ascii archive");
    assert!(create_status.success());

    // 1. inspect_cli must decode Chinese & Japanese filenames accurately without corruption
    let inspect_res = inspect_cli(&archive_path, None, 100, 1 << 30).await;
    assert!(
        inspect_res.is_ok(),
        "inspect_cli failed on non-ascii 7z archive: {:?}",
        inspect_res.err()
    );
    let records = inspect_res.unwrap();
    assert!(
        records
            .iter()
            .any(|r| r.relative_path.contains("批量解压缩_中文报告.txt")),
        "Chinese filename must be decoded properly, found: {:?}",
        records.iter().map(|r| &r.relative_path).collect::<Vec<_>>()
    );
    assert!(
        records
            .iter()
            .any(|r| r.relative_path.contains("日本語テストファイル.txt")),
        "Japanese filename must be decoded properly, found: {:?}",
        records.iter().map(|r| &r.relative_path).collect::<Vec<_>>()
    );

    // 2. extract_cli must extract and discover files with exact matching paths
    let dest = dir.path().join("extracted_non_ascii");
    fs::create_dir_all(&dest).unwrap();
    let req = make_request(
        "extract",
        &archive_path.to_string_lossy(),
        &dest.to_string_lossy(),
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(|_ev: &WorkerEvent| {});

    let extract_res = extract_cli(&req, &dest, progress, cancel).await;
    assert!(
        extract_res.is_ok(),
        "extract_cli failed on non-ascii archive: {:?}",
        extract_res.err()
    );
    let extracted = extract_res.unwrap();
    assert!(
        extracted
            .iter()
            .any(|r| r.relative_path.contains("批量解压缩_中文报告.txt")),
        "Extracted records must contain Chinese filename, found: {:?}",
        extracted
            .iter()
            .map(|r| &r.relative_path)
            .collect::<Vec<_>>()
    );
    assert!(
        extracted
            .iter()
            .any(|r| r.relative_path.contains("日本語テストファイル.txt")),
        "Extracted records must contain Japanese filename, found: {:?}",
        extracted
            .iter()
            .map(|r| &r.relative_path)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_zipcrypto_false_positive_and_salvage_corrupted_regression() {
    use openunpack_infrastructure::engine::{
        ProbeOutcome,
        cli_engine::{find_7z_binary, has_7z_cli},
        zip_engine::{extract_zip, probe_zip},
    };
    if !has_7z_cli() {
        return;
    }

    let dir = TestDir::new();
    let bin = find_7z_binary().unwrap();
    let payload_file = dir.path().join("secret.txt");
    let expected_payload =
        b"Authentic ZipCrypto payload that must not be corrupted by a 1/256 header collision!";
    fs::write(&payload_file, expected_payload).unwrap();

    let zip_path = dir.path().join("zipcrypto_stored.zip");
    let status = std::process::Command::new(&bin)
        .args([
            "a",
            "-tzip",
            "-mx=0",
            "-mem=ZipCrypto",
            "-pRealSecret#2026",
            "-y",
            "-bso0",
            "-bsp0",
            "-bse0",
        ])
        .arg(&zip_path)
        .arg(&payload_file)
        .status()
        .unwrap();
    assert!(status.success());

    // Generate 400 wrong passwords (guaranteeing at least one 1/256 ZipCrypto header collision)
    // followed by the real password at the very end.
    let mut candidates: Vec<(String, Option<String>)> = (0..400)
        .map(|i| (format!("wrong-candidate-{i}"), Some(format!("id-{i}"))))
        .collect();
    candidates.push(("RealSecret#2026".to_string(), Some("real-id".to_string())));

    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = probe_zip(&zip_path, &candidates, &cancel).unwrap();
    match outcome {
        ProbeOutcome::Success(secret, id) => {
            assert_eq!(secret, "RealSecret#2026");
            assert_eq!(id.as_deref(), Some("real-id"));
        }
        other => panic!("Expected ProbeOutcome::Success with real password, got {other:?}"),
    }

    // Also verify extract_zip with salvage_corrupted = true rejects wrong passwords
    // (including any 1/256 colliding password) instead of writing corrupted garbage!
    let dest_wrong = dir.path().join("dest_wrong");
    fs::create_dir_all(&dest_wrong).unwrap();
    for i in 0..300 {
        let mut req = make_request(
            "extract",
            &zip_path.to_string_lossy(),
            &dest_wrong.to_string_lossy(),
        );
        req.salvage_corrupted = true;
        req.password = Some(format!("wrong-candidate-{i}"));
        let res = extract_zip(
            &req,
            &dest_wrong,
            Arc::new(|_: &WorkerEvent| {}),
            cancel.clone(),
        );
        assert!(
            res.is_err(),
            "Wrong password wrong-candidate-{i} must NEVER succeed as salvaged!"
        );
    }
}

#[tokio::test]
async fn test_probe_cli_chinese_password_and_corrupt_unencrypted_archive() {
    use openunpack_infrastructure::engine::{
        ProbeOutcome,
        cli_engine::{find_7z_binary, has_7z_cli, probe_cli},
    };
    if !has_7z_cli() {
        return;
    }

    let dir = TestDir::new();
    let bin = find_7z_binary().unwrap();
    let payload_file = dir.path().join("data.txt");
    fs::write(
        &payload_file,
        b"Payload protected by Chinese UTF-8 password",
    )
    .unwrap();

    let cn_archive = dir.path().join("cn_pass.7z");
    let status = std::process::Command::new(&bin)
        .args([
            "a",
            "-t7z",
            "-p测试密码123",
            "-mhe=on",
            "-y",
            "-bso0",
            "-bsp0",
            "-bse0",
        ])
        .arg(&cn_archive)
        .arg(&payload_file)
        .status()
        .unwrap();
    assert!(status.success());

    let cancel = Arc::new(AtomicBool::new(false));
    let candidates = vec![
        ("wrong1".to_string(), Some("c1".to_string())),
        ("测试密码123".to_string(), Some("c2".to_string())),
    ];
    let outcome = probe_cli(&cn_archive, &candidates, &cancel).await.unwrap();
    match outcome {
        ProbeOutcome::Success(secret, id) => {
            assert_eq!(secret, "测试密码123");
            assert_eq!(id.as_deref(), Some("c2"));
        }
        other => panic!("Expected ProbeOutcome::Success for Chinese password, got {other:?}"),
    }

    // Create an unencrypted 7z archive and corrupt its payload bytes so 7z t reports CRC Failed
    let plain_archive = dir.path().join("plain_corrupt.7z");
    let big_payload = dir.path().join("big.txt");
    fs::write(&big_payload, vec![b'A'; 4096]).unwrap();
    let status = std::process::Command::new(&bin)
        .args(["a", "-t7z", "-mx=0", "-y", "-bso0", "-bsp0", "-bse0"])
        .arg(&plain_archive)
        .arg(&big_payload)
        .status()
        .unwrap();
    assert!(status.success());

    let mut raw = fs::read(&plain_archive).unwrap();
    if let Some(pos) = raw.windows(16).position(|w| w == [b'A'; 16]) {
        raw[pos] ^= 0xFF;
        fs::write(&plain_archive, &raw).unwrap();
    }
    let corrupt_outcome = probe_cli(&plain_archive, &candidates, &cancel)
        .await
        .unwrap();
    assert!(
        matches!(corrupt_outcome, ProbeOutcome::Corrupt),
        "Unencrypted archive with CRC failure must return ProbeOutcome::Corrupt, got {corrupt_outcome:?}"
    );
}

#[tokio::test]
async fn test_probe_cli_fast_metadata_unencrypted_and_targeted_candidate() {
    use openunpack_infrastructure::engine::{
        ProbeOutcome,
        cli_engine::{find_7z_binary, has_7z_cli, probe_cli},
    };
    if !has_7z_cli() {
        return;
    }

    let dir = TestDir::new();
    let bin = find_7z_binary().unwrap();
    let cancel = Arc::new(AtomicBool::new(false));

    // 1. Fast metadata inspection for healthy unencrypted archive
    let plain_healthy = dir.path().join("plain_healthy.7z");
    let file1 = dir.path().join("healthy.txt");
    fs::write(&file1, b"Hello Healthy Unencrypted Archive").unwrap();
    let status = std::process::Command::new(&bin)
        .args(["a", "-t7z", "-mx=0", "-y", "-bso0", "-bsp0", "-bse0"])
        .arg(&plain_healthy)
        .arg(&file1)
        .status()
        .unwrap();
    assert!(status.success());

    let candidates = vec![
        ("wrong1".to_string(), Some("id-1".to_string())),
        ("wrong2".to_string(), Some("id-2".to_string())),
    ];
    let outcome_plain = probe_cli(&plain_healthy, &candidates, &cancel)
        .await
        .unwrap();
    assert!(
        matches!(outcome_plain, ProbeOutcome::NoPasswordNeeded),
        "Expected ProbeOutcome::NoPasswordNeeded, got {outcome_plain:?}"
    );

    // 2. Targeted candidate trial on encrypted archive (unencrypted header, encrypted payload)
    let encrypted_archive = dir.path().join("encrypted_target.7z");
    let enc_file = dir.path().join("secret.txt");
    fs::write(&enc_file, b"Targeted encrypted content").unwrap();
    let status = std::process::Command::new(&bin)
        .args([
            "a",
            "-t7z",
            "-pTargetP@ss998",
            "-mhe=off",
            "-y",
            "-bso0",
            "-bsp0",
            "-bse0",
        ])
        .arg(&encrypted_archive)
        .arg(&enc_file)
        .status()
        .unwrap();
    assert!(status.success());

    let candidates_enc = vec![
        ("wrong-first".to_string(), Some("id-wrong".to_string())),
        ("TargetP@ss998".to_string(), Some("id-correct".to_string())),
    ];
    let outcome_enc = probe_cli(&encrypted_archive, &candidates_enc, &cancel)
        .await
        .unwrap();
    match outcome_enc {
        ProbeOutcome::Success(secret, id) => {
            assert_eq!(secret, "TargetP@ss998");
            assert_eq!(id.as_deref(), Some("id-correct"));
        }
        other => panic!("Expected ProbeOutcome::Success, got {other:?}"),
    }
}
