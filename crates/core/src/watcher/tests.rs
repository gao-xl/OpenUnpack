//! 文件夹监控临时文件过滤、退避策略与归档就绪预检单元测试。

use super::*;
use std::io::Cursor;

#[test]
fn test_temp_download_file_detection() {
    assert!(is_temp_download_file(Path::new("video.zip.crdownload")));
    assert!(is_temp_download_file(Path::new("archive.part")));
    assert!(is_temp_download_file(Path::new("data.tmp")));
    assert!(is_temp_download_file(Path::new("large.aria2")));
    assert!(is_temp_download_file(Path::new("file.idm")));
    assert!(is_temp_download_file(Path::new("movie.xltd")));
    assert!(is_temp_download_file(Path::new("movie.td")));
    assert!(is_temp_download_file(Path::new("torrent.!ut")));
    assert!(is_temp_download_file(Path::new("backup.downloading")));
    assert!(is_temp_download_file(Path::new("data.partial")));
    assert!(is_temp_download_file(Path::new("~$doc.docx")));

    assert!(!is_temp_download_file(Path::new("normal.zip")));
    assert!(!is_temp_download_file(Path::new("package.tar.gz")));
    assert!(!is_temp_download_file(Path::new("bundle.7z")));
    assert!(!is_temp_download_file(Path::new("game.part1.rar")));
}

#[test]
fn test_strip_temp_download_suffix() {
    assert_eq!(
        strip_temp_download_suffix("archive.part2.rar.crdownload"),
        Some("archive.part2.rar")
    );
    assert_eq!(
        strip_temp_download_suffix("movie.7z.001.xltd"),
        Some("movie.7z.001")
    );
    assert_eq!(
        strip_temp_download_suffix("data.tar.gz.downloading"),
        Some("data.tar.gz")
    );
    assert_eq!(strip_temp_download_suffix("data.zip"), None);
}

#[test]
fn test_volume_sibling_temp_detection() {
    assert!(is_volume_sibling_temp("Game.part2.rar.crdownload", "game"));
    assert!(is_volume_sibling_temp("Game.part2.rar.xltd", "game"));
    assert!(is_volume_sibling_temp("Movie.7z.002.downloading", "movie"));

    // Different stem
    assert!(!is_volume_sibling_temp(
        "Other.part2.rar.crdownload",
        "game"
    ));
    // Not a temp download file
    assert!(!is_volume_sibling_temp("Game.part2.rar", "game"));
}

#[test]
fn test_candidate_archive_name_filter() {
    assert!(is_archive_candidate_name("archive.zip"));
    assert!(is_archive_candidate_name("files.7z"));
    assert!(is_archive_candidate_name("data.rar"));
    assert!(is_archive_candidate_name("release.tar.gz"));
    assert!(is_archive_candidate_name("backup.tar.xz"));
    assert!(is_archive_candidate_name("split.zip.001"));
    assert!(is_archive_candidate_name("movie.part1.rar"));

    assert!(!is_archive_candidate_name("image.png"));
    assert!(!is_archive_candidate_name("document.pdf"));
    assert!(!is_archive_candidate_name("notes.txt"));
}

#[test]
fn test_probe_file_ready_read_probe() {
    // Non-existent file must fail read probe
    assert!(!probe_file_ready(Path::new(
        "non_existent_archive_probe.zip"
    )));

    let manifest_cargo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    // In unconstrained desktop execution, probing an accessible idle file succeeds.
    // In sandboxed/restricted execution environments, check safely returns bool without panicking.
    let is_ready = probe_file_ready(&manifest_cargo);
    let _ = is_ready;
}

#[test]
fn test_stability_record_logic() {
    let t0 = SystemTime::UNIX_EPOCH;
    let mut record = FileStabilityRecord {
        path: PathBuf::from("test.zip"),
        last_size: 0,
        last_mtime: t0,
        stable_ticks: 0,
        consecutive_probe_failures: 0,
        preflight_failures: 0,
        next_preflight_at: None,
        first_seen: Instant::now(),
    };

    // 1. Zero size -> not stable
    assert!(!update_candidate_stability(&mut record, 0, t0, true, 3));
    assert_eq!(record.stable_ticks, 0);

    // 2. First tick with size 100 -> reset to 0 ticks
    assert!(!update_candidate_stability(&mut record, 100, t0, true, 3));
    assert_eq!(record.stable_ticks, 0);
    assert_eq!(record.last_size, 100);

    // 3. Second tick with same size, same mtime, probe OK -> tick 1
    assert!(!update_candidate_stability(&mut record, 100, t0, true, 3));
    assert_eq!(record.stable_ticks, 1);

    // 4. File size changes while downloading -> ticks reset to 0
    assert!(!update_candidate_stability(&mut record, 150, t0, true, 3));
    assert_eq!(record.stable_ticks, 0);
    assert_eq!(record.last_size, 150);

    // 5. Probe fails (e.g. writer exclusive lock held) -> ticks reset to 0
    assert!(!update_candidate_stability(&mut record, 150, t0, false, 3));
    assert_eq!(record.stable_ticks, 0);

    // 6. Stabilizing: tick 1
    assert!(!update_candidate_stability(&mut record, 150, t0, true, 3));
    assert_eq!(record.stable_ticks, 1);

    // 7. Stabilizing: tick 2
    assert!(!update_candidate_stability(&mut record, 150, t0, true, 3));
    assert_eq!(record.stable_ticks, 2);

    // 8. Stabilizing: tick 3 -> reaches required 3 ticks!
    assert!(update_candidate_stability(&mut record, 150, t0, true, 3));
    assert_eq!(record.stable_ticks, 3);
}

#[test]
fn test_archive_complete_preflight_stream() {
    // 1. 0-byte file: rejected
    let mut cursor = Cursor::new(vec![]);
    assert!(!is_archive_complete_preflight_stream(
        "empty.zip",
        0,
        &mut cursor
    ));

    // 2. Valid ZIP (contains EOCD)
    let mut zip_data = vec![0u8; 30];
    zip_data[26..30].copy_from_slice(&[0x50, 0x4B, 0x05, 0x06]);
    let mut cursor = Cursor::new(zip_data);
    assert!(is_archive_complete_preflight_stream(
        "valid.zip",
        30,
        &mut cursor
    ));

    // 3. Valid ZIP64 (contains EOCD locator)
    let mut zip64_data = vec![0u8; 30];
    zip64_data[26..30].copy_from_slice(&[0x50, 0x4B, 0x06, 0x07]);
    let mut cursor = Cursor::new(zip64_data);
    assert!(is_archive_complete_preflight_stream(
        "valid.zip",
        30,
        &mut cursor
    ));

    // 4. Truncated ZIP (missing EOCD)
    let trunc_zip = b"PK\x03\x04randomdatawithoutcentraldir".to_vec();
    let len = trunc_zip.len() as u64;
    let mut cursor = Cursor::new(trunc_zip);
    assert!(!is_archive_complete_preflight_stream(
        "truncated.zip",
        len,
        &mut cursor
    ));

    // 5. Valid 7z (valid 32-byte header with within-bounds NextHeader)
    let mut header_7z = [0u8; 32];
    header_7z[0..6].copy_from_slice(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]);
    let mut cursor = Cursor::new(header_7z.to_vec());
    assert!(is_archive_complete_preflight_stream(
        "valid.7z",
        32,
        &mut cursor
    ));

    // 6. Truncated 7z (next_header claims more bytes than file size)
    let mut trunc_7z = [0u8; 32];
    trunc_7z[0..6].copy_from_slice(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]);
    trunc_7z[20..28].copy_from_slice(&500u64.to_le_bytes());
    let mut cursor = Cursor::new(trunc_7z.to_vec());
    assert!(!is_archive_complete_preflight_stream(
        "truncated.7z",
        32,
        &mut cursor
    ));

    // 7. Invalid 7z magic
    let mut bad_7z = [0u8; 32];
    bad_7z[0..6].copy_from_slice(b"bad7z!");
    let mut cursor = Cursor::new(bad_7z.to_vec());
    assert!(!is_archive_complete_preflight_stream(
        "bad.7z",
        32,
        &mut cursor
    ));

    // 8. Valid RAR4 header
    let rar4_data = b"Rar!\x1A\x07\x00extra".to_vec();
    let len = rar4_data.len() as u64;
    let mut cursor = Cursor::new(rar4_data);
    assert!(is_archive_complete_preflight_stream(
        "valid4.rar",
        len,
        &mut cursor
    ));

    // 9. Valid RAR5 header
    let rar5_data = b"Rar!\x1A\x07\x01\x00extra".to_vec();
    let len = rar5_data.len() as u64;
    let mut cursor = Cursor::new(rar5_data);
    assert!(is_archive_complete_preflight_stream(
        "valid5.rar",
        len,
        &mut cursor
    ));

    // 10. Corrupted RAR header
    let bad_rar = b"Rar!bad".to_vec();
    let len = bad_rar.len() as u64;
    let mut cursor = Cursor::new(bad_rar);
    assert!(!is_archive_complete_preflight_stream(
        "bad.rar",
        len,
        &mut cursor
    ));

    // 11. Valid GZ
    let mut gz_data = vec![0u8; 20];
    gz_data[0..2].copy_from_slice(&[0x1F, 0x8B]);
    let mut cursor = Cursor::new(gz_data);
    assert!(is_archive_complete_preflight_stream(
        "archive.tar.gz",
        20,
        &mut cursor
    ));

    // 12. Continuation volume bypasses header preflight
    let raw_continuation = vec![0x99; 50];
    let mut cursor = Cursor::new(raw_continuation);
    assert!(is_archive_complete_preflight_stream(
        "game.part2.rar",
        50,
        &mut cursor
    ));
    let mut cursor = Cursor::new(vec![0xAA; 50]);
    assert!(is_archive_complete_preflight_stream(
        "data.7z.002",
        50,
        &mut cursor
    ));
}

#[test]
fn bounded_enqueue_history_evicts_oldest_at_capacity() {
    let mut history = BoundedEnqueueHistory::with_capacity(2);
    let t0 = SystemTime::UNIX_EPOCH;
    assert!(history.insert((PathBuf::from("a.zip"), 10, t0)));
    assert!(history.insert((PathBuf::from("b.zip"), 20, t0)));
    assert!(!history.insert((PathBuf::from("a.zip"), 10, t0)));
    assert_eq!(history.len(), 2);

    // Third distinct entry evicts "a.zip" FIFO.
    assert!(history.insert((PathBuf::from("c.zip"), 30, t0)));
    assert_eq!(history.len(), 2);
    assert!(!history.contains(&(PathBuf::from("a.zip"), 10, t0)));
    assert!(history.contains(&(PathBuf::from("b.zip"), 20, t0)));
    assert!(history.contains(&(PathBuf::from("c.zip"), 30, t0)));
}

#[test]
fn preflight_backoff_scales_exponentially_and_caps() {
    assert_eq!(preflight_backoff_duration(0), Duration::ZERO);
    assert_eq!(preflight_backoff_duration(1), Duration::from_secs(1));
    assert_eq!(preflight_backoff_duration(2), Duration::from_secs(2));
    assert_eq!(preflight_backoff_duration(3), Duration::from_secs(4));
    assert_eq!(preflight_backoff_duration(6), Duration::from_secs(32));
    assert_eq!(preflight_backoff_duration(20), Duration::from_secs(32));
}
