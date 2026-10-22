use openunpack_application::SystemEnvironment;
use openunpack_infrastructure::WindowsSystemEnvironment;

#[test]
fn workspace_usage_counts_real_nested_file_bytes_and_missing_workspace_is_zero() {
    let root = tempfile::tempdir().unwrap();
    let nested = root.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(root.path().join("one"), [0u8; 19]).unwrap();
    std::fs::write(nested.join("two"), [0u8; 23]).unwrap();
    let environment = WindowsSystemEnvironment::new();
    assert_eq!(
        environment
            .workspace_size(root.path().to_str().unwrap())
            .unwrap(),
        42
    );
    assert_eq!(
        environment
            .workspace_size(root.path().join("absent").to_str().unwrap())
            .unwrap(),
        0
    );
}

#[cfg(windows)]
#[test]
fn workspace_usage_refuses_a_junction_instead_of_following_external_files() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("outside.bin"), [0u8; 11]).unwrap();
    let junction = root.path().join("redirect");
    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not create isolated regression junction: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result = WindowsSystemEnvironment::new().workspace_size(root.path().to_str().unwrap());
    std::fs::remove_dir(junction).unwrap();
    assert!(result.is_err());
    assert!(outside.path().join("outside.bin").is_file());
}
