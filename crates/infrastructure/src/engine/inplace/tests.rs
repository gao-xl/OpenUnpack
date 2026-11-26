//! 就地解压引擎预检、断点续传、空间约束与边车状态清理单元测试。

use super::*;
use std::io::Write;

use zip::write::SimpleFileOptions;

fn sample_zip(dir: &Path) -> PathBuf {
    let path = dir.join("game.zip");
    let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for index in 0..6u8 {
        writer
            .start_file(format!("data/{index}.bin"), options)
            .unwrap();
        writer
            .write_all(&vec![index; 120_000 + usize::from(index)])
            .unwrap();
    }
    writer.finish().unwrap();
    path
}

fn request(archive: &Path, output: &Path) -> Request {
    Request {
        archive: archive.to_path_buf(),
        output: output.to_path_buf(),
        passwords: Vec::new(),
        keep_free: 0,
        replace: false,
        verify_first: true,
        max_entries: 10_000,
        max_expanded_bytes: 1 << 30,
    }
}

#[test]
fn preflight_reports_an_in_place_run_fits_where_a_copy_would_not() {
    let dir = tempfile::tempdir().unwrap();
    let archive = sample_zip(dir.path());
    let output = dir.path().join("game");
    let plan = preflight(&request(&archive, &output)).unwrap();
    assert_eq!(plan.state, PreflightState::Fresh);
    assert_eq!(plan.entries, 6);
    assert!(plan.reclaims);
    assert!(plan.needed < plan.bytes);
    assert!(plan.blocker(10_000, 1 << 30).is_none());
}

#[test]
fn execute_frees_the_archive_and_writes_every_entry() {
    let dir = tempfile::tempdir().unwrap();
    let archive = sample_zip(dir.path());
    let output = dir.path().join("game");
    let progress = Arc::new(Progress::default());
    let stop = Arc::new(AtomicBool::new(false));
    let result = execute(&request(&archive, &output), &progress, &stop).unwrap();
    assert!(result.is_finished());
    assert!(
        fs::metadata(&archive).map(|m| m.len() == 0).unwrap_or(true),
        "archive is consumed"
    );
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
    let manifest = manifest(&archive, &output).unwrap();
    assert_eq!(manifest.iter().filter(|f| !f.is_directory).count(), 6);
    assert!(
        manifest
            .iter()
            .filter(|f| !f.is_directory)
            .all(|f| !f.sha256.is_empty())
    );
    assert!(
        read_state(&archive).is_some(),
        "state sidecar is retained until publication commit"
    );
    forget_state(&archive).unwrap();
    assert!(read_state(&archive).is_none());
}

#[test]
fn verify_first_refuses_a_damaged_archive_without_touching_it() {
    let dir = tempfile::tempdir().unwrap();
    let archive = sample_zip(dir.path());
    let mut bytes = fs::read(&archive).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xff;
    fs::write(&archive, &bytes).unwrap();
    let output = dir.path().join("game");
    let progress = Arc::new(Progress::default());
    let stop = Arc::new(AtomicBool::new(false));
    let error = execute(&request(&archive, &output), &progress, &stop).unwrap_err();
    assert_eq!(error.code, "Corrupt");
    assert_eq!(fs::metadata(&archive).unwrap().len(), bytes.len() as u64);
    assert!(!output.exists());
}

#[test]
fn a_paused_run_resumes_where_it_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let archive = sample_zip(dir.path());
    let output = dir.path().join("game");
    let stop = Arc::new(AtomicBool::new(false));
    stop.store(true, Ordering::Relaxed);
    let progress = Arc::new(Progress::default());
    let paused = execute(&request(&archive, &output), &progress, &stop).unwrap();
    assert_eq!(paused, Executed::Paused);
    stop.store(false, Ordering::Relaxed);
    let resumed = execute(&request(&archive, &output), &progress, &stop).unwrap();
    assert!(resumed.is_finished());
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
}

#[test]
fn test_journal_transaction_lifecycle_and_reconcile() {
    let dir = tempfile::tempdir().unwrap();
    let archive = sample_zip(dir.path());
    let output = dir.path().join("game");
    let stop = Arc::new(AtomicBool::new(false));
    stop.store(true, Ordering::Relaxed);
    let progress = Arc::new(Progress::default());

    // 1. 中途暂停模拟断电/崩溃
    let paused = execute(&request(&archive, &output), &progress, &stop).unwrap();
    assert_eq!(paused, Executed::Paused);

    // 2. 验证 .openunpack-journal 事务文件生成且包含状态
    assert!(journal_path(&archive).exists(), "journal file must exist");
    let journal = read_journal(&archive).expect("must parse journal");
    assert_eq!(journal.phase, JournalPhase::Interrupted);
    assert_eq!(journal.entries.len(), 6);

    // 3. 自愈检测
    assert!(
        reconcile_journal(&archive).unwrap(),
        "reconcile must confirm resumable session"
    );

    // 4. 接续解压至完成
    stop.store(false, Ordering::Relaxed);
    let resumed = execute(&request(&archive, &output), &progress, &stop).unwrap();
    assert!(resumed.is_finished());

    let final_journal = read_journal(&archive).expect("must read finished journal");
    assert_eq!(final_journal.phase, JournalPhase::Committed);

    // 5. 提交后清理验证
    forget_state(&archive).unwrap();
    assert!(
        !journal_path(&archive).exists(),
        "journal must be deleted on commit"
    );
    assert!(
        !state_path(&archive).exists(),
        "state sidecar must be deleted on commit"
    );
}

#[test]
fn test_journal_cancellation_marks_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let archive = sample_zip(dir.path());
    let output = dir.path().join("game");
    let stop = Arc::new(AtomicBool::new(false));
    stop.store(true, Ordering::Relaxed);
    let progress = Arc::new(Progress::default());

    let _ = execute(&request(&archive, &output), &progress, &stop).unwrap();
    assert!(journal_path(&archive).exists());

    mark_journal_cancelled(&archive).unwrap();
    let j = read_journal(&archive).unwrap();
    assert_eq!(j.phase, JournalPhase::Cancelled);
}
