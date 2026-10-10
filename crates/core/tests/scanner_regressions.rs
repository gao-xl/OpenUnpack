use openunpack_core::scanner::{matches_exclusion, scan};
use std::fs;

#[test]
fn test_question_mark_wildcard_unicode() {
    let patterns_double = vec!["??.zip".to_string()];
    assert!(matches_exclusion("资料.zip", &patterns_double));

    let patterns_single = vec!["?.zip".to_string()];
    assert!(!matches_exclusion("中文.zip", &patterns_single));
    assert!(matches_exclusion("文.zip", &patterns_single));
}

#[test]
fn exclude_directory_prunes_descendants() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let ignored_dir = temp_dir.path().join("ignored");
    let keep_dir = temp_dir.path().join("keep");
    fs::create_dir_all(&ignored_dir).expect("failed to create ignored dir");
    fs::create_dir_all(&keep_dir).expect("failed to create keep dir");

    let ignored_zip = ignored_dir.join("a.zip");
    let keep_zip = keep_dir.join("b.zip");
    fs::write(&ignored_zip, b"").expect("failed to write ignored a.zip");
    fs::write(&keep_zip, b"").expect("failed to write keep b.zip");

    let root_str = temp_dir.path().to_str().expect("valid utf-8").to_string();
    let exclusions = vec!["ignored".to_string()];
    let result = scan(&[root_str], true, &exclusions).expect("scan should succeed");

    assert_eq!(result.sources.len(), 1);
    assert!(result.sources[0].path.ends_with("b.zip"));
}

#[test]
fn test_split_archive_missing_volume() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let vol1 = temp_dir.path().join("data.7z.001");
    let vol3 = temp_dir.path().join("data.7z.003");
    fs::write(&vol1, b"").expect("failed to write data.7z.001");
    fs::write(&vol3, b"").expect("failed to write data.7z.003");

    let vol3_str = vol3.to_str().expect("valid utf-8").to_string();
    let result = scan(&[vol3_str], false, &[]).expect("scan should succeed");

    assert_eq!(result.sources.len(), 1);
    let group = &result.sources[0];
    assert!(group.missing_volume);
    assert_eq!(group.volumes.len(), 2);
    assert!(group.path.ends_with("001"));
}

#[test]
fn cancelled_scan_stops_without_returning_a_partial_success() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("fixture.zip"), b"").unwrap();
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    assert!(
        openunpack_core::scanner::scan_with_cancel(
            &[temp.path().to_string_lossy().into_owned()],
            true,
            &[],
            &cancelled
        )
        .is_err()
    );
}

#[test]
fn extreme_numeric_suffix_does_not_walk_billions_of_missing_numbers() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["fixture.7z.001", "fixture.7z.4294967295"] {
        std::fs::write(temp.path().join(name), b"").unwrap();
    }
    let started = std::time::Instant::now();
    let scan = scan(&[temp.path().to_string_lossy().into_owned()], false, &[]).unwrap();
    assert!(scan.sources[0].missing_volume);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}
