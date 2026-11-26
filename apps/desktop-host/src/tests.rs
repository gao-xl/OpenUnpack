use super::*;
use openunpack_protocol::{ConflictStrategy, ToolboxKind};

fn assert_host_captures_watcher_runtime(runtime: &tokio::runtime::Handle) {
    assert!(tokio::runtime::Handle::try_current().is_err());
    let directory = tempfile::tempdir().unwrap();
    let worker = directory.path().join("OpenUnpack.Worker.exe");
    let engine = directory.path().join("7z.dll");
    fs::write(&worker, b"synthetic worker, no jobs are executed").unwrap();
    fs::write(&engine, b"synthetic engine, no jobs are executed").unwrap();
    let core = open_coordinator_on_runtime(directory.path().join("data"), worker, engine, runtime)
        .unwrap();
    assert!(tokio::runtime::Handle::try_current().is_err());
    let started = std::thread::spawn({
        let core = core.clone();
        move || core.start()
    })
    .join()
    .expect("the watcher thread must not panic");
    assert_eq!(started, Ok(()));
    runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while core.snapshot().unwrap().running {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the idle batch must release its running latch");
    });
}

#[test]
fn desktop_initialization_captures_the_framework_runtime_for_watcher_threads() {
    let runtime = tauri::async_runtime::handle();
    assert_host_captures_watcher_runtime(runtime.inner());
}

#[test]
fn headless_initialization_captures_its_runtime_for_watcher_threads() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    assert_host_captures_watcher_runtime(runtime.handle());
}

#[test]
fn import_encodings_preserve_unicode_and_reject_invalid_bytes() {
    let text = "  中文𠀀口令  \r\n";
    let (gb, _, errors) = encoding_rs::GB18030.encode(text);
    assert!(!errors);
    assert_eq!(decode_import_bytes(&gb, "gb18030").unwrap(), text);
    let utf16: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    assert_eq!(decode_import_bytes(&utf16, "utf-16le").unwrap(), text);
    assert_eq!(decode_import_bytes(&utf16[2..], "UTF-16LE").unwrap(), text);
    assert_eq!(decode_import_bytes(text.as_bytes(), "utf-8").unwrap(), text);
    assert!(decode_import_bytes(&[0x00, 0xD8], "utf-16le").is_err());
    assert!(decode_import_bytes(&[0x01], "utf-16le").is_err());
    assert!(decode_import_bytes(&[0xFE, 0xFF, 0x00, 0x61], "utf-16le").is_err());
    assert!(decode_import_bytes(&[0xFF], "utf-8").is_err());
    assert!(decode_import_bytes(&[0x81], "gb18030").is_err());
}

#[test]
fn test_origin_verification() {
    assert!(is_trusted_origin_url("tauri://localhost"));
    assert!(is_trusted_origin_url("tauri://localhost/index.html"));
    assert!(is_trusted_origin_url("http://tauri.localhost"));
    assert!(is_trusted_origin_url("https://tauri.localhost"));
    assert!(!is_trusted_origin_url("http://attacker.com"));
    assert!(!is_trusted_origin_url("https://attacker.localhost"));
    assert!(!is_trusted_origin_url("http://user:pass@tauri.localhost"));
    assert!(!is_trusted_origin_url("http://tauri.localhost:8080"));
    assert!(!is_trusted_origin_url("file:///C:/malicious.html"));
}

#[test]
fn test_path_containment_helper() {
    let base = PathBuf::from("C:\\Users\\Public\\OpenUnpack");
    let child = PathBuf::from("C:\\Users\\Public\\OpenUnpack\\sub\\file.txt");
    let outside = PathBuf::from("C:\\Users\\Public\\Other\\file.txt");
    let prefix_spoof = PathBuf::from("C:\\Users\\Public\\OpenUnpackSecret\\file.txt");

    assert!(path_starts_with_case_insensitive(&base, &base));
    assert!(path_starts_with_case_insensitive(&child, &base));
    assert!(!path_starts_with_case_insensitive(&outside, &base));
    assert!(!path_starts_with_case_insensitive(&prefix_spoof, &base));
}

#[test]
fn toolbox_native_access_guard_enforces_directory_authorization() {
    let temp = tempfile::tempdir().unwrap();
    let canon = canonicalize_existing(&temp.path().to_string_lossy()).unwrap();
    let unauth_temp = tempfile::tempdir().unwrap();
    let unauth_canon = canonicalize_existing(&unauth_temp.path().to_string_lossy()).unwrap();

    let access = Mutex::new(NativeAccess::default());

    let unauth_opts = ToolboxOptions {
        kind: ToolboxKind::NormalizeExtensions,
        source_directory: unauth_canon.to_string_lossy().into_owned(),
        target_directory: None,
        recursive: true,
        remove_empty_folders: false,
        conflict_strategy: ConflictStrategy::Rename,
        rename_rule: None,
    };

    // Unauthorized access must be rejected
    assert!(authorize_toolbox_options(&access, unauth_opts).is_err());

    // Grant access
    {
        let mut guard = access.lock().unwrap();
        guard.grant_read(canon.clone(), true);
        guard.grant_write_dir(canon.clone());
    }

    let auth_opts = ToolboxOptions {
        kind: ToolboxKind::NormalizeExtensions,
        source_directory: canon.to_string_lossy().into_owned(),
        target_directory: None,
        recursive: true,
        remove_empty_folders: false,
        conflict_strategy: ConflictStrategy::Rename,
        rename_rule: None,
    };

    // Authorized access must succeed
    let authorized = authorize_toolbox_options(&access, auth_opts).unwrap();
    assert_eq!(authorized.source_directory, canon.to_string_lossy());
}

#[test]
fn strip_verbatim_prefix_cleans_extended_paths() {
    assert_eq!(
        strip_verbatim_prefix(Path::new(r"\\?\C:\Users\im\Desktop")),
        PathBuf::from(r"C:\Users\im\Desktop")
    );
    assert_eq!(
        strip_verbatim_prefix(Path::new(r"\\?\UNC\server\share\sub")),
        PathBuf::from(r"\\server\share\sub")
    );
    assert_eq!(
        strip_verbatim_prefix(Path::new(r"C:\Users\im\Desktop")),
        PathBuf::from(r"C:\Users\im\Desktop")
    );
    assert_eq!(
        strip_verbatim_prefix(Path::new(r"\\server\share\sub")),
        PathBuf::from(r"\\server\share\sub")
    );
}

#[test]
fn sanitize_explorer_target_rejects_switches_and_reserved_devices() {
    assert!(sanitize_explorer_target(Path::new("-select,C:\\Windows")).is_err());
    assert!(sanitize_explorer_target(Path::new("/e,C:\\Windows")).is_err());
    assert!(sanitize_explorer_target(Path::new("C:\\Windows\n")).is_err());
    assert!(sanitize_explorer_target(Path::new(r"C:\CON")).is_err());
    assert!(sanitize_explorer_target(Path::new(r"C:\Windows\AUX\sub")).is_err());
    assert!(sanitize_explorer_target(Path::new(r"relative\path")).is_err());

    let temp = tempfile::tempdir().unwrap();
    let canon = temp.path().canonicalize().unwrap();
    let safe = sanitize_explorer_target(&canon).unwrap();
    assert!(!safe.to_string_lossy().starts_with(r"\\?\"));
    assert!(safe.is_dir());
}

#[test]
fn inplace_extraction_requires_parent_write_permission() {
    let temp = tempfile::tempdir().unwrap();
    let test_file = temp.path().join("archive.zip");
    fs::write(&test_file, b"test content").unwrap();
    let file_canon = canonicalize_existing(&test_file.to_string_lossy()).unwrap();
    let parent_canon = canonicalize_existing(&temp.path().to_string_lossy()).unwrap();

    let access = Arc::new(Mutex::new(NativeAccess::new()));

    // Case 1: only read granted on the file itself, no write granted on parent
    {
        let mut guard = access.lock().unwrap();
        guard.grant_read(file_canon.clone(), false);
    }

    let guard = access.lock().unwrap().clone();
    assert!(guard.is_readable(&file_canon));
    assert!(!guard.is_writable_dir(&parent_canon));

    // Case 2: grant write to parent
    {
        let mut guard = access.lock().unwrap();
        guard.grant_write_dir(parent_canon.clone());
    }
    let guard = access.lock().unwrap().clone();
    assert!(guard.is_writable_dir(&parent_canon));
}

#[cfg(windows)]
#[test]
fn reparse_tag_policy_allows_cloud_placeholders_and_rejects_junctions() {
    use super::{is_allowed_cloud_reparse_tag, verify_no_reparse_ancestors};
    assert!(is_allowed_cloud_reparse_tag(0x9000_001A));
    assert!(is_allowed_cloud_reparse_tag(0x9000_301A));
    assert!(is_allowed_cloud_reparse_tag(0x8000_0021));
    assert!(!is_allowed_cloud_reparse_tag(0xA000_0003));
    assert!(!is_allowed_cloud_reparse_tag(0xA000_000C));

    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target_dir");
    let junction = temp.path().join("junction_dir");
    fs::create_dir_all(&target).unwrap();
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .output()
        .unwrap()
        .status;
    if status.success() {
        assert!(
            verify_no_reparse_ancestors(&junction).is_err(),
            "NTFS junction should be rejected by verify_no_reparse_ancestors"
        );
        assert!(verify_no_reparse_ancestors(&target).is_ok());
    }
}

mod exclusion_preview_tests {
    use super::super::{preview_exclusion_matches, validate_exclusion_preview};

    #[test]
    fn preview_rule_limit_accepts_257_to_512_utf8_bytes() {
        for pattern in [
            "a".repeat(257),
            "a".repeat(512),
            format!("{}aa", "中".repeat(170)),
        ] {
            let result =
                preview_exclusion_matches(vec![pattern.clone()], vec![pattern], None).unwrap();
            assert!(result[0].matched);
        }
        for pattern in ["a".repeat(513), "中".repeat(171)] {
            assert!(preview_exclusion_matches(vec![], vec![pattern], None).is_err());
        }
    }

    #[test]
    fn preview_scope_defaults_to_legacy_input_and_routes_output_to_worker() {
        let child = "temp/deep/file.bin".to_string();
        let rule = vec!["temp".into()];
        assert!(
            !preview_exclusion_matches(vec![child.clone()], rule.clone(), None).unwrap()[0].matched
        );
        assert!(
            validate_exclusion_preview(&["temp/".into(), child.clone()], &rule, Some("output"))
                .is_ok()
        );
        assert!(preview_exclusion_matches(vec![child], rule, Some("output")).is_err());
        assert!(preview_exclusion_matches(vec![], vec![], Some("unknown")).is_err());
    }
}

mod headless_args_tests {
    use super::super::{canonicalize_for_save, collect_flag_values, mask_token};
    use std::path::Path;

    #[test]
    fn collects_repeated_flag_values_and_skips_blanks() {
        let args: Vec<String> = [
            "--read-root",
            "D://in",
            "--read-root",
            "   ",
            "--write-root",
            "D://out",
        ]
        .iter()
        .map(|value| value.to_string())
        .collect();
        assert_eq!(collect_flag_values(&args, "--read-root"), vec!["D://in"]);
        assert_eq!(collect_flag_values(&args, "--write-root"), vec!["D://out"]);
        assert!(collect_flag_values(&args, "--missing").is_empty());
    }

    #[test]
    fn masks_tokens_without_revealing_the_body() {
        let token =
            "oup_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let masked = mask_token(token);
        assert!(masked.starts_with("oup_01"));
        assert!(masked.ends_with(&token[token.len() - 4..]));
        assert!(!masked.contains(&token[10..50]));
        assert_eq!(mask_token("short"), "***");
    }

    #[test]
    fn canonicalize_for_save_supports_multilevel_uncreated_directories() {
        let dir = tempfile::tempdir().unwrap();
        let target_path = dir.path().join("sub1").join("sub2").join("backup.opbak");
        let result = canonicalize_for_save(&target_path.to_string_lossy()).unwrap();
        assert!(result.ends_with(Path::new("sub1").join("sub2").join("backup.opbak")));
        assert!(result.is_absolute());
    }
}
