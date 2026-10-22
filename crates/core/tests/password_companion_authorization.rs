#![cfg(windows)]

use openunpack_core::scanner::{
    sniff_accompanying_passwords, sniff_accompanying_passwords_authorized,
};
use std::{fs, path::Path, process::Command};

fn archive(directory: &Path) -> std::path::PathBuf {
    let archive = directory.join("sample.zip");
    fs::write(&archive, b"synthetic metadata-only archive").unwrap();
    archive
}

#[test]
fn file_only_archive_permission_does_not_authorize_password_companions() {
    let directory = tempfile::tempdir().unwrap();
    let archive = archive(directory.path());
    fs::write(
        directory.path().join("sample.txt"),
        b"synthetic-stem-secret",
    )
    .unwrap();
    fs::write(
        directory.path().join("password.txt"),
        b"synthetic-generic-secret",
    )
    .unwrap();

    let denied = sniff_accompanying_passwords_authorized(&archive, |candidate| {
        candidate == archive.as_path()
    });
    assert!(denied.is_empty());
    // The trusted native entry point still discovers the same companions.
    let trusted = sniff_accompanying_passwords(&archive);
    assert!(trusted.iter().any(|hint| hint == "synthetic-stem-secret"));
    assert!(
        trusted
            .iter()
            .any(|hint| hint == "synthetic-generic-secret")
    );
}

#[test]
fn denied_optional_companions_do_not_hide_other_authorized_hints() {
    let directory = tempfile::tempdir().unwrap();
    let archive = archive(directory.path());
    let approved = directory.path().join("sample.txt");
    let denied = directory.path().join("password.txt");
    fs::write(&approved, b"synthetic-approved").unwrap();
    fs::write(&denied, b"synthetic-denied").unwrap();
    let mut checked = Vec::new();
    let hints = sniff_accompanying_passwords_authorized(&archive, |candidate| {
        checked.push(candidate.to_path_buf());
        candidate == approved.as_path()
    });
    assert_eq!(hints, vec!["synthetic-approved"]);
    assert!(checked.contains(&approved));
    assert!(checked.contains(&denied));
}

#[test]
fn policy_is_rechecked_with_the_real_companion_handle_pinned_before_content_read() {
    let directory = tempfile::tempdir().unwrap();
    let archive = archive(directory.path());
    let password = directory.path().join("password.txt");
    fs::write(&password, b"synthetic-revoked-before-read").unwrap();
    let mut checks = 0;
    let hints = sniff_accompanying_passwords_authorized(&archive, |candidate| {
        if candidate != password.as_path() {
            return false;
        }
        checks += 1;
        if checks == 2 {
            // This write normally succeeds in the synthetic fixture. It must
            // be excluded by the exact read handle, not an attribute sample.
            assert!(fs::OpenOptions::new().write(true).open(candidate).is_err());
            return false;
        }
        true
    });
    assert_eq!(checks, 2);
    assert!(hints.is_empty());
    // The read lease was released after the skipped candidate.
    assert!(fs::OpenOptions::new().write(true).open(password).is_ok());
}

#[test]
fn companion_reader_accepts_64_kib_but_rejects_a_larger_file() {
    let directory = tempfile::tempdir().unwrap();
    let archive = archive(directory.path());
    let password = directory.path().join("password.txt");
    let mut bytes = b"synthetic-boundary\n".to_vec();
    bytes.resize(64 * 1024, b' ');
    fs::write(&password, &bytes).unwrap();
    assert_eq!(
        sniff_accompanying_passwords(&archive),
        vec!["synthetic-boundary"]
    );
    bytes.push(b' ');
    fs::write(&password, &bytes).unwrap();
    assert!(sniff_accompanying_passwords(&archive).is_empty());
}

#[test]
fn a_reparse_parent_cannot_redirect_companion_reads_even_when_policy_allows_the_name() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("outside");
    fs::create_dir(&target).unwrap();
    archive(&target);
    fs::write(target.join("password.txt"), b"synthetic-outside-secret").unwrap();
    let junction = directory.path().join("alias");
    let creation = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .output()
        .expect("junction fixture command must start");
    assert!(
        creation.status.success(),
        "junction fixture must be created"
    );
    assert!(
        sniff_accompanying_passwords_authorized(&junction.join("sample.zip"), |_| true).is_empty()
    );
    assert!(sniff_accompanying_passwords(&junction.join("sample.zip")).is_empty());
    // Remove only the junction entry. The synthetic target remains intact.
    fs::remove_dir(&junction).unwrap();
    assert!(target.join("password.txt").exists());
}
