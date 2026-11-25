//! Chaos and Extreme Boundary Automation Tests for OpenUnpack 1.0.
//!
//! Covers:
//! 1. Power-loss / SIGKILL crash recovery: .openunpack-journal transaction reconciliation,
//!    in-place state restoration, breakpoint resumption, and 0-byte archive residue cleanup.
//! 2. Decompression Bomb (42.zip): high compression ratio, entry count explosion, total size budget,
//!    and directory depth exhaustion.
//! 3. Zip-Slip / Path Traversal: parent dir traversal, absolute/UNC paths, Windows reserved device names,
//!    null bytes, colons, and trailing dots/spaces.
//! 4. Long Paths (>260 characters): Windows MAX_PATH boundaries, verbatim (\\?\) prefix support,
//!    deep extraction hierarchy, drive letter resolution.
//! 5. Sleep / Wake-up / Transient I/O: SleepBlockerGuard lifecycle, transient sharing violation (os error 32/33/5/145)
//!    retries, worker cancellation during slow I/O, and missing/disconnected volume handling.

#![cfg(windows)]

use openunpack_core::paths as core_paths;
use openunpack_infrastructure::{
    engine::{
        execute_native_worker,
        inplace::{self, JOURNAL_VERSION, JournalPhase, ManifestEntry, OpenUnpackJournal},
        path_safety::validate_relative_path,
    },
    paths as infra_paths,
};
use openunpack_protocol::worker::{WORKER_PROTOCOL_VERSION_V2, WorkerEvent, WorkerRequest};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tempfile::Builder;
use zip::{ZipWriter, write::SimpleFileOptions};

struct ScratchDir {
    _temp: Option<tempfile::TempDir>,
    path: PathBuf,
}

impl ScratchDir {
    fn new(prefix: &str) -> Self {
        let temp = Builder::new()
            .prefix(&format!("ou-chaos-{prefix}-"))
            .tempdir()
            .unwrap();
        let path = temp.path().canonicalize().unwrap();
        Self {
            _temp: Some(temp),
            path,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

fn make_worker_request(op: &str, archive: &Path, dest: &Path) -> WorkerRequest {
    let mut req = WorkerRequest::default();
    req.protocol_version = WORKER_PROTOCOL_VERSION_V2;
    req.request_id = format!("req-{}", uuid::Uuid::new_v4().simple());
    req.operation = op.to_string();
    req.archive_path = archive.to_string_lossy().to_string();
    req.destination_directory = dest.to_string_lossy().to_string();
    req
}

fn create_standard_zip(path: &Path, files: &[(&str, &[u8])]) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

// =========================================================================
// 1. .openunpack-journal Power-Loss / Crash Recovery & Resumption
// =========================================================================

#[test]
fn test_chaos_power_loss_journal_reconciliation_and_resumption() {
    let scratch = ScratchDir::new("power-loss");
    let archive = scratch.path().join("power_loss_target.zip");
    create_standard_zip(
        &archive,
        &[("data.bin", b"important data that was mid-extraction")],
    );

    // 1. Simulate process abruptly killed / power cut during in-place extraction:
    // Journal is left on disk in InProgress phase.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let journal = OpenUnpackJournal {
        version: JOURNAL_VERSION,
        archive: archive.to_string_lossy().to_string(),
        output: scratch
            .path()
            .join("power_loss_target")
            .to_string_lossy()
            .to_string(),
        phase: JournalPhase::InProgress,
        verified: true,
        total_bytes: 1000,
        committed_bytes: 500,
        updated_at: now - 30, // 30 seconds ago
        entries: vec![ManifestEntry {
            path: "data.bin".into(),
            size: 1000,
            dir: false,
        }],
    };

    // Write journal directly as if in-flight
    inplace::write_journal(&archive, &journal).expect("failed to write mock in-progress journal");
    assert!(
        inplace::has_session(&archive),
        "session must be recognized when journal exists"
    );

    // 2. Perform crash recovery / reconciliation (called when OpenUnpack discovers an orphaned run)
    let can_resume = inplace::reconcile_journal(&archive).expect("reconcile_journal failed");
    assert!(
        can_resume,
        "interrupted in-progress journal must be eligible for resumption"
    );

    // 3. Verify journal phase changed to Interrupted and timestamp updated
    let reconciled =
        inplace::read_journal(&archive).expect("journal must exist after reconciliation");
    assert_eq!(
        reconciled.phase,
        JournalPhase::Interrupted,
        "phase must transition from InProgress to Interrupted"
    );
    assert!(
        reconciled.updated_at >= now,
        "updated_at must be refreshed during reconciliation"
    );
    assert_eq!(reconciled.committed_bytes, 500);
    assert_eq!(reconciled.entries.len(), 1);

    // 4. Test explicit cancellation marks journal Cancelled and preserves state
    inplace::mark_journal_cancelled(&archive).expect("mark_journal_cancelled failed");
    let cancelled = inplace::read_journal(&archive).expect("journal must exist");
    assert_eq!(cancelled.phase, JournalPhase::Cancelled);

    // 5. Clean up session and verify complete forget
    inplace::forget_journal(&archive).expect("forget_journal failed");
    assert!(!inplace::has_session(&archive));
}

#[test]
fn test_chaos_corrupted_or_truncated_journal_tolerance() {
    let scratch = ScratchDir::new("corrupt-journal");
    let archive = scratch.path().join("crash_mid_journal_write.zip");
    fs::write(&archive, b"PK0304dummy").unwrap();

    let j_path = inplace::journal_path(&archive);

    // Scenario A: 0-byte journal file (power cut right after file create / truncation)
    fs::write(&j_path, b"").unwrap();
    assert!(
        inplace::read_journal(&archive).is_none(),
        "0-byte journal must return None safely"
    );
    let res = inplace::reconcile_journal(&archive).unwrap();
    assert!(!res, "0-byte journal cannot resume");

    // Scenario B: Truncated JSON (power cut mid-write)
    fs::write(&j_path, br#"{"version":1,"archive":"test","output":"#).unwrap();
    assert!(
        inplace::read_journal(&archive).is_none(),
        "truncated journal must not panic"
    );

    // Scenario C: Arbitrary corrupted garbage bytes
    fs::write(&j_path, [0xFF, 0xFE, 0x00, 0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
    assert!(
        inplace::read_journal(&archive).is_none(),
        "binary garbage journal must return None safely"
    );

    // Forget journal cleans up corrupted file without error
    inplace::forget_journal(&archive).expect("forget_journal should cleanly remove corrupt file");
    assert!(!j_path.exists());
}

#[test]
fn test_chaos_zero_byte_archive_residue_cleanup_lifecycle() {
    let scratch = ScratchDir::new("zero-byte-residue");
    let archive = scratch.path().join("completed_archive.zip");

    // Simulate in-place extraction that completely drained archive to 0 bytes
    fs::write(&archive, b"").unwrap();
    assert_eq!(fs::metadata(&archive).unwrap().len(), 0);

    // Write journal sidecar
    let journal = OpenUnpackJournal {
        version: JOURNAL_VERSION,
        archive: archive.to_string_lossy().to_string(),
        output: scratch.path().join("out").to_string_lossy().to_string(),
        phase: JournalPhase::Committed,
        verified: true,
        total_bytes: 100,
        committed_bytes: 100,
        updated_at: 1000,
        entries: Vec::new(),
    };
    inplace::write_journal(&archive, &journal).unwrap();
    assert!(inplace::journal_path(&archive).exists());

    // cleanup_archive cleans up 0-byte archive
    inplace::cleanup_archive(&archive);
    assert!(
        !archive.exists(),
        "0-byte archive residue must be purged after completion"
    );

    // Forget journal removes the completed journal sidecar
    inplace::forget_journal(&archive).unwrap();
    assert!(!inplace::journal_path(&archive).exists());
}

// =========================================================================
// 2. Decompression Bomb (42.zip) Multi-Vector Defense
// =========================================================================

#[tokio::test]
async fn test_chaos_decompression_bomb_ratio_defense() {
    let scratch = ScratchDir::new("bomb-ratio");
    let archive = scratch.path().join("ratio_bomb.zip");

    // Create a high-compression ratio archive: 5 MB of repeating zeros deflates to ~5 KB
    let zeros = vec![0u8; 5 * 1024 * 1024];
    create_standard_zip(&archive, &[("payload.bin", &zeros)]);

    let archive_size = fs::metadata(&archive).unwrap().len();
    assert!(
        archive_size < 50 * 1024,
        "Archive should be highly compressed, got {} bytes",
        archive_size
    );

    let dest = scratch.path().join("extracted");
    fs::create_dir_all(&dest).unwrap();

    let mut request = make_worker_request("extract", &archive, &dest);
    // Budget: allow at most 256 KB written
    request.max_written_bytes = 256 * 1024;

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(|_: &WorkerEvent| {});

    let result = execute_native_worker(request, progress, cancel).await;
    assert!(
        result.is_err(),
        "Decompression bomb exceeding max_written_bytes must be aborted"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("ResourceLimit") && err.contains("maxWrittenBytes"),
        "Expected maxWrittenBytes resource limit, got: {err}"
    );

    // Verify disk space was protected: the output file must NOT be 5 MB
    let out_file = dest.join("payload.bin");
    if out_file.exists() {
        let written = fs::metadata(&out_file).unwrap().len();
        assert!(
            written <= 512 * 1024,
            "Written file size must not blow up disk space (got {written} bytes, expected <= 512KB)"
        );
    }
}

#[tokio::test]
async fn test_chaos_decompression_bomb_entry_count_defense() {
    let scratch = ScratchDir::new("bomb-entries");
    let archive = scratch.path().join("entry_bomb.zip");

    // Create an archive with 25 entries
    let entries: Vec<(String, Vec<u8>)> = (0..25)
        .map(|i| {
            (
                format!("file_{i:03}.txt"),
                format!("content {i}").into_bytes(),
            )
        })
        .collect();
    let entries_refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    create_standard_zip(&archive, &entries_refs);

    let dest = scratch.path().join("out");
    fs::create_dir_all(&dest).unwrap();

    // 1. Inspect with max_entries = 10 -> rejected
    let mut req_inspect = make_worker_request("inspect", &archive, &dest);
    req_inspect.max_entries = 10;
    let err_inspect = execute_native_worker(
        req_inspect,
        Arc::new(|_| {}),
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap_err();
    assert!(
        err_inspect.contains("ResourceLimit") && err_inspect.contains("maxEntries"),
        "Inspect must reject entry-count bomb: {err_inspect}"
    );

    // 2. Extract with max_entries = 10 -> rejected
    let mut req_extract = make_worker_request("extract", &archive, &dest);
    req_extract.max_entries = 10;
    let err_extract = execute_native_worker(
        req_extract,
        Arc::new(|_| {}),
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap_err();
    assert!(
        err_extract.contains("ResourceLimit") && err_extract.contains("maxEntries"),
        "Extract must reject entry-count bomb: {err_extract}"
    );
}

#[tokio::test]
async fn test_chaos_decompression_bomb_total_bytes_inspect_defense() {
    let scratch = ScratchDir::new("bomb-total-bytes");
    let archive = scratch.path().join("large_declared.zip");
    create_standard_zip(
        &archive,
        &[
            ("part1.bin", &[1u8; 100_000]),
            ("part2.bin", &[2u8; 100_000]),
        ],
    );

    let dest = scratch.path().join("out");
    let mut req = make_worker_request("inspect", &archive, &dest);
    req.max_total_bytes = 50_000; // Total uncompressed is 200,000 > 50,000

    let err = execute_native_worker(req, Arc::new(|_| {}), Arc::new(AtomicBool::new(false)))
        .await
        .unwrap_err();

    assert!(
        err.contains("ResourceLimit") && err.contains("maxTotalBytes"),
        "Expected maxTotalBytes ResourceLimit error, got: {err}"
    );
}

#[test]
fn test_chaos_path_depth_bomb_defense() {
    // 130 levels of nested directory traversal
    let mut deep_path = String::new();
    for i in 0..130 {
        deep_path.push_str(&format!("level_{i}/"));
    }
    deep_path.push_str("target.txt");

    let res = validate_relative_path(&deep_path);
    assert!(
        res.is_err(),
        "Deep nested path hierarchy (>128) must be rejected"
    );
    assert!(
        res.unwrap_err()
            .contains("ResourceLimit: Path hierarchy exceeds safe depth"),
        "Expected Path hierarchy safe depth limit error"
    );
}

// =========================================================================
// 3. Zip-Slip & Path Traversal Full-Spectrum Attack Defense
// =========================================================================

#[test]
fn test_chaos_zip_slip_parent_dir_and_escapes() {
    let evil_paths = [
        "../../evil.txt",
        "..\\\\..\\\\evil.txt",
        "sub/../../../secret.sys",
        "./../../outside.bin",
        "foo/bar/../../../../../../windows/system32/calc.exe",
        "..",
        "../",
    ];

    for path in &evil_paths {
        let res = validate_relative_path(path);
        assert!(
            res.is_err(),
            "Path traversal '{path}' must be rejected by validate_relative_path"
        );
        let err = res.unwrap_err();
        assert!(
            err.contains("UnsafePath"),
            "Error for '{path}' must be UnsafePath, got: {err}"
        );
    }
}

#[test]
fn test_chaos_zip_slip_absolute_and_unc_escapes() {
    let evil_paths = [
        "/etc/passwd",
        "/root/.ssh/id_rsa",
        r"\Windows\System32\drivers\etc\hosts",
        r"C:\Windows\System32\calc.exe",
        r"D:\data\target.bin",
        r"\\server\share\evil.dll",
    ];

    for path in &evil_paths {
        let res = validate_relative_path(path);
        assert!(res.is_err(), "Absolute/UNC path '{path}' must be rejected");
        let err = res.unwrap_err();
        assert!(
            err.contains("UnsafePath"),
            "Error for '{path}' must be UnsafePath, got: {err}"
        );
    }
}

#[test]
fn test_chaos_zip_slip_windows_reserved_device_names() {
    let device_names = [
        "CON",
        "PRN",
        "AUX",
        "NUL",
        "CLOCK$",
        "CONIN$",
        "CONOUT$",
        "COM1",
        "COM2",
        "COM3",
        "COM4",
        "COM5",
        "COM6",
        "COM7",
        "COM8",
        "COM9",
        "LPT1",
        "LPT2",
        "LPT3",
        "LPT4",
        "LPT5",
        "LPT6",
        "LPT7",
        "LPT8",
        "LPT9",
        "con.txt",
        "aux.dat",
        "prn.zip",
        "COM1.exe",
        "NUL.tar",
        "sub/con.txt",
        "sub/nested/aux.dat",
    ];

    for dev in &device_names {
        let res = validate_relative_path(dev);
        assert!(res.is_err(), "Windows device name '{dev}' must be rejected");
        assert!(
            res.unwrap_err()
                .contains("UnsafePath: Reserved system device name"),
            "Expected Reserved system device name error for '{dev}'"
        );
    }
}

#[test]
fn test_chaos_zip_slip_illegal_characters_and_trailing_aliases() {
    let invalid_paths = [
        "file\0name.txt",
        "test:alternate_stream",
        "trailing_dot.",
        "trailing_space ",
        "sub/trailing. /file.txt",
        "sub/dot./file.txt",
    ];

    for path in &invalid_paths {
        let res = validate_relative_path(path);
        assert!(
            res.is_err(),
            "Path with illegal chars or trailing space/dot '{path}' must be rejected"
        );
        assert!(
            res.unwrap_err().contains("UnsafePath"),
            "Expected UnsafePath for '{path}'"
        );
    }
}

// =========================================================================
// 4. Long Paths (>260 characters) Boundary & Compatibility Defense
// =========================================================================

#[tokio::test]
async fn test_chaos_long_path_over_260_chars_extraction() {
    let scratch = ScratchDir::new("long-path");
    let archive = scratch.path().join("long_path_archive.zip");

    // Construct a deeply nested relative path whose total target path will exceed 260 chars.
    let seg1 = "a".repeat(50);
    let seg2 = "b".repeat(50);
    let seg3 = "c".repeat(50);
    let seg4 = "d".repeat(50);
    let seg5 = "e".repeat(50);
    let long_rel = format!("{seg1}/{seg2}/{seg3}/{seg4}/{seg5}/deep_payload.txt");

    assert!(
        long_rel.len() > 260,
        "Relative path alone is {} chars (>260)",
        long_rel.len()
    );

    // 1. Verify validate_relative_path accepts this valid deep path
    let validated = validate_relative_path(&long_rel)
        .expect("valid long path within depth limit must be accepted");
    assert_eq!(validated, long_rel);

    // 2. Create archive containing this long path entry
    create_standard_zip(
        &archive,
        &[(&long_rel, b"deeply nested content in long path archive")],
    );

    let dest = scratch.path().join("out");
    fs::create_dir_all(&dest).unwrap();

    let expected_dest_file = dest
        .join(&seg1)
        .join(&seg2)
        .join(&seg3)
        .join(&seg4)
        .join(&seg5)
        .join("deep_payload.txt");
    let full_path_len = expected_dest_file.to_string_lossy().len();
    assert!(
        full_path_len > 260,
        "Full target file path is {} chars (> 260 MAX_PATH)",
        full_path_len
    );

    // 3. Extract using native worker
    let request = make_worker_request("extract", &archive, &dest);
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(|_: &WorkerEvent| {});

    let event = execute_native_worker(request, progress, cancel)
        .await
        .expect("Extraction of long path (>260 chars) must succeed on modern Windows");

    assert_eq!(event.kind, "completed");
    assert_eq!(event.files.len(), 1);
    assert_eq!(event.files[0].relative_path, long_rel);

    // 4. Verify file content on disk
    let content =
        fs::read_to_string(&expected_dest_file).expect("Must be able to read extracted deep file");
    assert_eq!(content, "deeply nested content in long path archive");
}

#[test]
fn test_chaos_long_path_verbatim_normalization_and_drive_resolution() {
    let standard_path = Path::new(r"C:\nested\folder\file.zip");
    let verbatim_path = Path::new(r"\\?\C:\nested\folder\file.zip");
    let unc_verbatim_path = Path::new(r"\\?\UNC\storage.server\share\file.zip");

    assert_eq!(core_paths::drive_letter_of(standard_path), Some('C'));
    assert_eq!(core_paths::drive_letter_of(verbatim_path), Some('C'));
    assert_eq!(core_paths::drive_letter_of(unc_verbatim_path), None);

    // volume_root resolution
    let vol_standard = core_paths::volume_root(standard_path).unwrap();
    let vol_verbatim = core_paths::volume_root(verbatim_path).unwrap();
    assert_eq!(vol_standard, PathBuf::from("C:\\"));
    assert_eq!(vol_verbatim, PathBuf::from(r"\\?\C:\"));
    assert!(vol_standard.is_absolute());
}

// =========================================================================
// 5. Sleep / Wake-up / Transient I/O & Worker Interruption Self-Healing
// =========================================================================

#[test]
fn test_chaos_sleep_blocker_guard_lifecycle_and_concurrency() {
    // Acquire the sleep blocker guard (SetThreadExecutionState ES_CONTINUOUS | ES_SYSTEM_REQUIRED)
    let guard1 = infra_paths::SleepBlockerGuard::acquire();
    // Concurrent acquisition from a second worker/coordinator task
    let guard2 = infra_paths::SleepBlockerGuard::acquire();

    std::thread::sleep(Duration::from_millis(50));

    // Release guards in reverse order
    drop(guard2);
    drop(guard1);
    // Verified: No deadlocks, thread crashes or panic on drop
}

#[test]
fn test_chaos_transient_io_error_classification_and_retries() {
    // 1. Verify sharing violation and lock contention are classified as retryable
    assert!(infra_paths::transient_retirement(
        "os error 32: The process cannot access the file because it is being used by another process."
    ));
    assert!(infra_paths::transient_retirement(
        "os error 33: The process cannot access the file because another process has locked a portion of the file."
    ));
    assert!(infra_paths::transient_retirement(
        "os error 5: Access is denied."
    ));
    assert!(infra_paths::transient_retirement(
        "os error 145: The directory is not empty."
    ));
    assert!(infra_paths::transient_retirement(
        "目标目录正被占用，可稍后重试"
    ));

    // 2. Verify non-retryable durable failures are NOT treated as transient
    assert!(!infra_paths::transient_retirement(
        "WorkerExitUnconfirmed: process died unexpectedly"
    ));
    assert!(!infra_paths::transient_retirement(
        "缺少独占标记，拒绝危险清理"
    ));
    assert!(!infra_paths::transient_retirement(
        "检测到并发变动，已停止清理"
    ));

    // 3. Test retry_retirement successfully rides out transient locks
    let mut attempts = 0;
    let res = infra_paths::retry_retirement(|| {
        attempts += 1;
        if attempts < 3 {
            Err(infra_paths::RetirementError {
                message: "os error 32: sharing violation".into(),
                retryable: true,
            })
        } else {
            Ok(())
        }
    });
    assert!(
        res.is_ok(),
        "retry_retirement must succeed once transient lock clears"
    );
    assert_eq!(attempts, 3);
}

#[tokio::test]
async fn test_chaos_worker_cancellation_during_inflight_io() {
    let scratch = ScratchDir::new("cancellation-io");
    let archive = scratch.path().join("cancellation_archive.zip");

    create_standard_zip(
        &archive,
        &[
            ("file1.bin", &[1u8; 100_000]),
            ("file2.bin", &[2u8; 100_000]),
            ("file3.bin", &[3u8; 100_000]),
        ],
    );

    let dest = scratch.path().join("out");
    fs::create_dir_all(&dest).unwrap();

    // 1. Pre-cancellation test: verify immediate cancellation response
    let req1 = make_worker_request("extract", &archive, &dest);
    let cancel1 = Arc::new(AtomicBool::new(true));
    let res1 = execute_native_worker(req1, Arc::new(|_| {}), cancel1).await;
    assert_eq!(res1.unwrap_err(), "Cancelled");

    // 2. Mid-flight cancellation test
    let req2 = make_worker_request("extract", &archive, &dest);
    let cancel2 = Arc::new(AtomicBool::new(false));
    let cancel2_clone = cancel2.clone();

    let progress = Arc::new(move |_: &WorkerEvent| {
        cancel2_clone.store(true, Ordering::Release);
    });

    // Spawn cancellation trigger right after execution starts
    let cancel_trigger = cancel2.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1)).await;
        cancel_trigger.store(true, Ordering::Release);
    });

    let res2 = execute_native_worker(req2, progress, cancel2).await;
    assert!(res2.is_err(), "Worker should report Err when cancelled");
    assert_eq!(res2.unwrap_err(), "Cancelled");
}

#[test]
fn test_chaos_missing_or_unmounted_volume_safety() {
    // Drive X: does not exist on this machine
    let non_existent_drive = Path::new(r"X:\nonexistent\volume\archive.zip");

    // 1. volume_root fails cleanly without panic because volume does not exist
    let root_res = core_paths::volume_root(non_existent_drive);
    assert!(
        root_res.is_err(),
        "Non-existent drive must fail volume_root cleanly"
    );

    // 2. free_bytes fails cleanly without panic because drive does not exist
    let free_res = core_paths::free_bytes(non_existent_drive);
    assert!(
        free_res.is_err(),
        "Non-existent drive must fail free_bytes cleanly"
    );
}
