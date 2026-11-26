#[test]
fn preview_digest_binds_file_identity_even_when_size_and_times_match() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FileBasicInfo, SetFileInformationByHandle,
    };
    let fixture = tempfile::tempdir().unwrap();
    let source = fixture.path().join("source.txt");
    fs::write(&source, b"same bytes").unwrap();
    let options = ToolboxOptions {
        kind: ToolboxKind::BatchRename,
        source_directory: fixture.path().to_string_lossy().into_owned(),
        target_directory: None,
        recursive: false,
        remove_empty_folders: false,
        conflict_strategy: ConflictStrategy::Error,
        rename_rule: Some(RenamePatternRule {
            search: String::new(),
            replace: String::new(),
            case_sensitive: false,
            use_regex: false,
            prefix: Some("changed-".into()),
            suffix: None,
            numbering_start: None,
            numbering_digits: None,
        }),
    };
    let preview = preview_toolbox(&options).unwrap();
    assert_eq!(preview.plans.len(), 1);
    let before = compute_preview_digest(&options, &preview.plans);
    let metadata = fs::metadata(&source).unwrap();
    fs::rename(&source, fixture.path().join("preserved-original.txt")).unwrap();
    fs::write(&source, b"same bytes").unwrap();
    let replacement = fs::OpenOptions::new().write(true).open(&source).unwrap();
    let information = FILE_BASIC_INFO {
        CreationTime: metadata.creation_time() as i64,
        LastWriteTime: metadata.last_write_time() as i64,
        ..FILE_BASIC_INFO::default()
    };
    assert_ne!(
        unsafe {
            SetFileInformationByHandle(
                replacement.as_raw_handle() as _,
                FileBasicInfo,
                &information as *const _ as _,
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0
    );
    drop(replacement);
    assert_ne!(before, compute_preview_digest(&options, &preview.plans));
}
#[test]
fn case_only_move_uses_the_same_object_for_both_rename_steps() {
    let fixture = tempfile::tempdir().unwrap();
    let source = fixture.path().join("SOURCE.txt");
    let target = fixture.path().join("source.txt");
    fs::write(&source, b"case-preserving content").unwrap();
    safe_move_file(&source, &target, true).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"case-preserving content");
    let names: Vec<_> = fs::read_dir(fixture.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("source.txt")]);
}
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
#[test]
#[ignore = "requires an explicitly supplied second writable volume"]
fn cross_volume_transfer_uses_locked_source_and_preserves_existing_target() {
    let other =
        std::env::var_os("OPENUNPACK_TOOLBOX_SECOND_VOLUME").expect("second volume required");
    let source_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::Builder::new()
        .prefix("openunpack-xvol-regression-")
        .tempdir_in(other)
        .unwrap();
    let source = source_dir.path().join("source.bin");
    let target = target_dir.path().join("target.bin");
    std::fs::write(&source, b"owned source payload").unwrap();
    let a: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    assert_eq!(
        unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 0) },
        0,
        "volumes must be distinct"
    );
    assert_eq!(unsafe { GetLastError() }, ERROR_NOT_SAME_DEVICE);
    safe_move_file(&source, &target, false).unwrap();
    assert!(!source.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"owned source payload");
    std::fs::write(&source, b"second owned source").unwrap();
    assert!(safe_move_file(&source, &target, false).is_err());
    assert_eq!(std::fs::read(&source).unwrap(), b"second owned source");
    assert_eq!(std::fs::read(&target).unwrap(), b"owned source payload");
    assert_eq!(
        std::fs::read_dir(target_dir.path()).unwrap().count(),
        1,
        "no stale temporary copies"
    );
}

use super::*;
use tempfile::tempdir;

#[test]
fn cleanup_collects_before_mutating_and_rejects_excessive_depth() {
    let dir = tempdir().unwrap();
    let mut path = dir.path().to_path_buf();
    for _ in 0..=MAX_TOOLBOX_DEPTH {
        path.push("d");
        fs::create_dir(&path).unwrap();
    }
    assert!(cleanup_directories(dir.path(), Instant::now()).is_err());
    assert!(
        path.is_dir(),
        "an incomplete cleanup must preserve every directory"
    );
    assert!(check_toolbox_budget(Instant::now(), MAX_TOOLBOX_ENTRIES + 1, 1).is_err());
    assert!(
        check_toolbox_budget(
            Instant::now() - TOOLBOX_TIME_LIMIT - Duration::from_secs(1),
            0,
            1
        )
        .is_err()
    );
}

#[test]
fn test_normalize_extensions_compound_and_magic() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // 1. Broken .tar.gz.tar -> .tar.gz
    let f1 = root.join("release.tar.gz.tar");
    fs::write(&f1, [0x1F, 0x8B, 0x08, 0x00, 0x00]).unwrap();

    // 2. .tgz -> .tar.gz
    let f2 = root.join("packages.tgz");
    fs::write(&f2, [0x1F, 0x8B, 0x08, 0x00, 0x00]).unwrap();

    // 3. Disguised 7z named .bin -> .7z
    let f3 = root.join("backup.bin");
    fs::write(&f3, [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0x00, 0x04]).unwrap();

    // 4. Normal .zip remains untouched
    let f4 = root.join("normal.zip");
    fs::write(&f4, [0x50, 0x4B, 0x03, 0x04, 0x14, 0x00]).unwrap();

    let opts = ToolboxOptions {
        kind: ToolboxKind::NormalizeExtensions,
        source_directory: root.to_string_lossy().into_owned(),
        target_directory: None,
        recursive: false,
        remove_empty_folders: false,
        conflict_strategy: ConflictStrategy::Rename,
        rename_rule: None,
    };

    let preview = preview_toolbox(&opts).unwrap();
    assert_eq!(preview.total_scanned, 4);
    assert_eq!(preview.total_planned, 3);

    let exec = execute_toolbox(&ToolboxExecuteRequest {
        options: opts,
        approved_paths: Some(
            preview
                .plans
                .iter()
                .map(|p| p.source_path.clone())
                .collect(),
        ),
        token: Some(preview.token),
    })
    .unwrap();
    assert_eq!(exec.success_count, 3);
    assert!(root.join("release.tar.gz").exists());
    assert!(root.join("packages.tar.gz").exists());
    assert!(root.join("backup.7z").exists());
    assert!(root.join("normal.zip").exists());
}

#[test]
fn test_batch_rename_preserves_compound_ext_and_numbers() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::write(root.join("[raw]_episode_a.tar.gz"), b"data1").unwrap();
    fs::write(root.join("[raw]_episode_b.tar.gz"), b"data2").unwrap();

    let opts = ToolboxOptions {
        kind: ToolboxKind::BatchRename,
        source_directory: root.to_string_lossy().into_owned(),
        target_directory: None,
        recursive: false,
        remove_empty_folders: false,
        conflict_strategy: ConflictStrategy::Rename,
        rename_rule: Some(RenamePatternRule {
            search: "[raw]_".into(),
            replace: "".into(),
            case_sensitive: false,
            use_regex: false,
            prefix: Some("clean_".into()),
            suffix: None,
            numbering_start: Some(1),
            numbering_digits: Some(2),
        }),
    };

    let preview = preview_toolbox(&opts).unwrap();
    assert_eq!(preview.total_planned, 2);
    assert_eq!(preview.plans[0].proposed_name, "clean_episode_a_01.tar.gz");
    assert_eq!(preview.plans[1].proposed_name, "clean_episode_b_02.tar.gz");

    let exec = execute_toolbox(&ToolboxExecuteRequest {
        options: opts,
        approved_paths: Some(
            preview
                .plans
                .iter()
                .map(|p| p.source_path.clone())
                .collect(),
        ),
        token: Some(preview.token),
    })
    .unwrap();
    assert_eq!(exec.success_count, 2);
    assert!(root.join("clean_episode_a_01.tar.gz").exists());
    assert!(root.join("clean_episode_b_02.tar.gz").exists());
}

#[test]
fn test_flatten_directory_with_conflict_and_empty_folder_cleanup() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let sub1 = root.join("sub1").join("deep");
    let sub2 = root.join("sub2");
    fs::create_dir_all(&sub1).unwrap();
    fs::create_dir_all(&sub2).unwrap();

    fs::write(sub1.join("readme.txt"), b"from_sub1").unwrap();
    fs::write(sub2.join("readme.txt"), b"from_sub2").unwrap();
    fs::write(sub2.join("unique.zip"), b"zip_content").unwrap();

    let opts = ToolboxOptions {
        kind: ToolboxKind::FlattenDirectory,
        source_directory: root.to_string_lossy().into_owned(),
        target_directory: None,
        recursive: true,
        remove_empty_folders: true,
        conflict_strategy: ConflictStrategy::Rename,
        rename_rule: None,
    };

    let preview = preview_toolbox(&opts).unwrap();
    assert_eq!(preview.total_planned, 3);
    assert_eq!(preview.conflicts_count, 1);

    let exec = execute_toolbox(&ToolboxExecuteRequest {
        options: opts,
        approved_paths: Some(
            preview
                .plans
                .iter()
                .map(|p| p.source_path.clone())
                .collect(),
        ),
        token: Some(preview.token),
    })
    .unwrap();
    assert_eq!(exec.success_count, 3);
    assert!(exec.cleaned_folders_count >= 2, "{exec:?}");
    assert!(root.join("readme.txt").exists());
    assert!(root.join("readme_1.txt").exists());
    assert!(root.join("unique.zip").exists());
    assert!(!root.join("sub1").exists());
    assert!(!root.join("sub2").exists());
}

#[test]
fn test_s04_system_directory_guards_verbatim_and_canonical() {
    let windows = PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()));
    assert!(
        validate_toolbox_directory(&windows).is_err(),
        "Windows dir must be rejected"
    );

    if let Ok(canon) = fs::canonicalize(&windows) {
        assert!(
            validate_toolbox_directory(&canon).is_err(),
            "Canonicalized \\\\?\\ Windows dir must be rejected"
        );
    }

    // Windows-backup or similar should NOT be falsely rejected
    let dir = tempdir().unwrap();
    let safe_dir = dir.path().join("Windows-backup");
    fs::create_dir(&safe_dir).unwrap();
    assert!(
        validate_toolbox_directory(&safe_dir).is_ok(),
        "Windows-backup must NOT be rejected"
    );
}

#[test]
fn test_s05_preview_token_binding_and_rejection_on_mismatch() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let file_path = root.join("safe.txt");
    fs::write(&file_path, b"test content").unwrap();

    let mut opts = ToolboxOptions {
        kind: ToolboxKind::BatchRename,
        source_directory: root.to_string_lossy().into_owned(),
        target_directory: None,
        recursive: false,
        remove_empty_folders: false,
        conflict_strategy: ConflictStrategy::Error,
        rename_rule: Some(RenamePatternRule {
            search: String::new(),
            replace: String::new(),
            case_sensitive: false,
            use_regex: false,
            prefix: Some("preview-".into()),
            suffix: None,
            numbering_start: None,
            numbering_digits: None,
        }),
    };

    let preview = preview_toolbox(&opts).unwrap();
    let approved = preview
        .plans
        .iter()
        .map(|p| p.source_path.clone())
        .collect::<Vec<_>>();

    // 1. Missing token -> rejected
    let res_no_token = execute_toolbox(&ToolboxExecuteRequest {
        options: opts.clone(),
        approved_paths: Some(approved.clone()),
        token: None,
    });
    assert!(res_no_token.is_err());

    // 2. Modified options (e.g. prefix changed from preview- to unpreviewed-) -> token mismatch, rejected
    opts.rename_rule.as_mut().unwrap().prefix = Some("unpreviewed-".into());
    let res_mismatched = execute_toolbox(&ToolboxExecuteRequest {
        options: opts.clone(),
        approved_paths: Some(approved.clone()),
        token: Some(preview.token.clone()),
    });
    assert!(
        res_mismatched.is_err(),
        "Must reject execution with mismatched preview token"
    );
    assert!(!root.join("unpreviewed-safe.txt").exists());

    // 3. Re-preview with new options -> valid execution
    let new_preview = preview_toolbox(&opts).unwrap();
    let res_valid = execute_toolbox(&ToolboxExecuteRequest {
        options: opts.clone(),
        approved_paths: Some(approved.clone()),
        token: Some(new_preview.token.clone()),
    });
    assert!(res_valid.is_ok());
    assert!(root.join("unpreviewed-safe.txt").exists());

    // 4. Replay attack: reusing new_preview.token -> rejected because token is single-use
    let res_replay = execute_toolbox(&ToolboxExecuteRequest {
        options: opts.clone(),
        approved_paths: Some(approved.clone()),
        token: Some(new_preview.token.clone()),
    });
    assert!(
        res_replay.is_err(),
        "Replay of consumed token must be rejected"
    );

    // 5. Forged token not generated by preview -> rejected
    let res_forged = execute_toolbox(&ToolboxExecuteRequest {
        options: opts,
        approved_paths: Some(approved),
        token: Some("fake-forged-token-12345".into()),
    });
    assert!(res_forged.is_err(), "Forged token must be rejected");
}

#[test]
fn test_s06_atomic_no_overwrite_rejection() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let f1 = root.join("file1.txt");
    let f2 = root.join("file2.txt");
    fs::write(&f1, b"content 1").unwrap();
    fs::write(&f2, b"content 2").unwrap();

    // Attempting to move file1 to existing file2 with safe_move_file must fail without replacing file2
    let err = safe_move_file(&f1, &f2, false);
    assert!(
        err.is_err(),
        "Must refuse to overwrite existing destination"
    );
    assert_eq!(
        fs::read(&f2).unwrap(),
        b"content 2",
        "Destination content must NOT be overwritten"
    );
    assert!(f1.exists(), "Source must still exist");
}

#[test]
fn test_fix_archive_extensions_direct() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // 1. Fake .mp4 with ZIP magic -> should be directly renamed to .zip
    let f1 = root.join("video.mp4");
    fs::write(&f1, [0x50, 0x4B, 0x03, 0x04, 0x14, 0x00, 0x00, 0x00]).unwrap();

    // 2. Double-extension disguised file: movie.7z.bak with 7z magic -> should be directly renamed to movie.7z
    let f2 = root.join("movie.7z.bak");
    fs::write(&f2, [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0x00, 0x04]).unwrap();

    // 3. Normal .rar that already has correct extension -> untouched
    let f3 = root.join("document.rar");
    fs::write(&f3, [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00]).unwrap();

    // 4. Plain text file -> unrecognized
    let f4 = root.join("notes.txt");
    fs::write(&f4, b"just some text").unwrap();

    // 5. Disguised compound archive: archive.tar.gz.mp4 with ZIP magic -> should be cleaned to archive.zip
    let f5 = root.join("archive.tar.gz.mp4");
    fs::write(&f5, [0x50, 0x4B, 0x03, 0x04, 0x14, 0x00, 0x00, 0x00]).unwrap();

    let paths = vec![
        f1.to_string_lossy().into_owned(),
        f2.to_string_lossy().into_owned(),
        f3.to_string_lossy().into_owned(),
        f4.to_string_lossy().into_owned(),
        f5.to_string_lossy().into_owned(),
    ];

    let results = fix_archive_extensions(&paths).unwrap();
    assert_eq!(results.len(), 5);

    // video.mp4 -> video.zip
    let r1 = results
        .iter()
        .find(|r| r.original_path == paths[0])
        .unwrap();
    assert_eq!(r1.status, "renamed");
    assert_eq!(r1.proposed_name.as_deref(), Some("video.zip"));
    assert!(root.join("video.zip").exists());
    assert!(!root.join("video.mp4").exists());

    // movie.7z.bak -> movie.7z
    let r2 = results
        .iter()
        .find(|r| r.original_path == paths[1])
        .unwrap();
    assert_eq!(r2.status, "renamed");
    assert_eq!(r2.proposed_name.as_deref(), Some("movie.7z"));
    assert!(root.join("movie.7z").exists());
    assert!(!root.join("movie.7z.bak").exists());

    // document.rar -> already_correct
    let r3 = results
        .iter()
        .find(|r| r.original_path == paths[2])
        .unwrap();
    assert_eq!(r3.status, "already_correct");
    assert!(root.join("document.rar").exists());

    // notes.txt -> unrecognized
    let r4 = results
        .iter()
        .find(|r| r.original_path == paths[3])
        .unwrap();
    assert_eq!(r4.status, "unrecognized");
    assert!(root.join("notes.txt").exists());

    // archive.tar.gz.mp4 -> archive.zip
    let r5 = results
        .iter()
        .find(|r| r.original_path == paths[4])
        .unwrap();
    assert_eq!(r5.status, "renamed");
    assert_eq!(r5.proposed_name.as_deref(), Some("archive.zip"));
    assert!(root.join("archive.zip").exists());
    assert!(!root.join("archive.tar.gz.mp4").exists());
}
