mod cache_container_tests {
    use super::super::{
        RetirementError, mark_owned, owned, prune_empty_cache_container, remove_owned,
        remove_owned_retrying, retry_retirement,
    };

    #[test]
    fn owned_workspace_retirement_prunes_cache_only_after_the_last_job() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let container = cache.join("OpenUnpack-cache");
        std::fs::create_dir_all(&container).unwrap();
        let first = container.join("job-1");
        let second = container.join("job-2");
        mark_owned(&first, "first", "").unwrap();
        mark_owned(&second, "second", "manifest-kept").unwrap();
        std::fs::create_dir_all(first.join("nested/deep")).unwrap();
        std::fs::write(first.join("nested/deep/retire.txt"), b"retire").unwrap();
        std::fs::write(second.join("keep.txt"), b"keep").unwrap();

        remove_owned(&first, "first").unwrap();
        assert!(!first.exists());
        assert!(container.is_dir(), "another job still needs the container");
        owned(&second, "second", Some("manifest-kept")).unwrap();
        assert_eq!(std::fs::read(second.join("keep.txt")).unwrap(), b"keep");

        remove_owned(&second, "second").unwrap();
        assert!(!second.exists());
        assert!(
            !container.exists(),
            "retiring the last real workspace must release its handles before pruning"
        );
        assert!(cache.is_dir(), "the user's cache root must stay");
    }

    #[test]
    fn owned_workspace_retirement_keeps_a_non_cache_parent() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join(".openunpack-work");
        std::fs::create_dir(&parent).unwrap();
        let work = parent.join("job");
        mark_owned(&work, "job", "").unwrap();

        remove_owned(&work, "job").unwrap();
        assert!(!work.exists());
        assert!(parent.is_dir(), "only the named cache container is pruned");
    }

    #[test]
    fn empty_cache_container_is_pruned_but_a_shared_one_is_kept() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let container = cache.join("OpenUnpack-cache");
        let job = container.join("job-1");
        std::fs::create_dir_all(&job).unwrap();
        prune_empty_cache_container(&job);
        assert!(
            container.is_dir(),
            "a container with a live workspace must stay"
        );
        std::fs::remove_dir(&job).unwrap();
        prune_empty_cache_container(&job);
        assert!(!container.exists(), "the empty container must not linger");
        assert!(
            cache.is_dir(),
            "the user cache root itself is never removed"
        );
    }

    #[test]
    fn a_held_workspace_is_retired_once_the_handle_is_released() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ,
        };

        let root = tempfile::tempdir().unwrap();
        let container = root.path().join("OpenUnpack-cache");
        let work = container.join("job-1");
        std::fs::create_dir_all(&container).unwrap();
        mark_owned(&work, "job", "").unwrap();
        std::fs::write(work.join("payload.bin"), b"bytes").unwrap();

        // Hold the root without FILE_SHARE_DELETE: the first attempt fails the
        // way a sibling job or an on-access scanner would make it fail.
        let blocker = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&work)
            .unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            drop(blocker);
        });

        remove_owned_retrying(&work, "job").expect("a released handle must not block retirement");
        releaser.join().unwrap();
        assert!(!work.exists(), "the retried retirement must finish");
    }

    #[test]
    fn transient_retirement_is_retried_and_terminal_reasons_are_not() {
        let mut calls = 0;
        let result = retry_retirement(|| {
            calls += 1;
            if calls < 3 {
                Err("工作目录清理失败，可稍后重试。".into())
            } else {
                Ok(())
            }
        });
        assert!(result.is_ok(), "a transient conflict must be retried");
        assert_eq!(calls, 3, "the transient retry budget must be bounded");

        let mut exhausted = 0;
        let result = retry_retirement(|| {
            exhausted += 1;
            Err("工作目录清理失败，可稍后重试。".into())
        });
        assert!(result.is_err(), "an exhausted retry budget must surface");
        assert_eq!(
            exhausted, 4,
            "the final attempt must run after every bounded retry"
        );

        let mut refusals = 0;
        let result = retry_retirement(|| {
            refusals += 1;
            Err("工作目录标记不匹配，已停止清理。".into())
        });
        assert_eq!(
            result.unwrap_err(),
            "工作目录标记不匹配，已停止清理。",
            "the durable reason must reach the caller"
        );
        assert_eq!(refusals, 1, "ownership refusals must never be retried");

        let mut concurrent_attempts = 0;
        let result = retry_retirement(|| {
            concurrent_attempts += 1;
            Err(RetirementError::terminal(
                "工作目录在清理过程中发生并发变动，已停止清理。",
            ))
        });
        assert_eq!(
            result.unwrap_err(),
            "工作目录在清理过程中发生并发变动，已停止清理。",
            "concurrent modification must never be retried"
        );
        assert_eq!(
            concurrent_attempts, 1,
            "concurrent modification must stop immediately"
        );
    }
}

use super::*;

#[test]
fn excessive_path_depth_is_rejected_before_creating_any_directory() {
    assert!(relative(&vec!["a"; 128].join("/")).is_ok());
    assert_eq!(
        relative(&vec!["a"; 129].join("/")).unwrap_err(),
        "ResourceLimit"
    );
    let fixture = tempfile::tempdir().unwrap();
    let base = fixture.path().join("not-created");
    let mut excessive = base.clone();
    for _ in 0..257 {
        excessive.push("a");
    }
    assert!(DirectoryGuard::create(&excessive).is_err());
    assert!(!base.exists());
}

#[test]
fn native_handle_rename_and_tree_publication_keep_sources_locked() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("file");
    fs::write(&file_path, b"locked").unwrap();
    let parent = DirectoryGuard::open(temp.path()).unwrap();
    let mut file = parent.lock_entry(&file_path, true).unwrap();
    file.rename_no_replace(&parent, &temp.path().join("renamed"))
        .unwrap();
    drop(file);
    let source = temp.path().join("source");
    drop(parent.create_private_child(&source).unwrap());
    fs::create_dir_all(source.join("child")).unwrap();
    fs::write(source.join("child/data"), b"stable bytes").unwrap();
    let mut tree = LockedTree::open(&source).unwrap();
    assert_eq!(tree.entry_count(), 2);
    assert!(fs::write(source.join("child/data"), b"wrong").is_err());
    assert!(fs::rename(&source, temp.path().join("swapped")).is_err());
    assert!(
        tree.root_mut()
            .rename_no_replace(&parent, &temp.path().join("published"))
            .is_err(),
        "NTFS requires child leases to close before directory rename"
    );
    tree.rename_no_replace(&parent, &temp.path().join("published"))
        .unwrap();
    assert_eq!(
        fs::read(temp.path().join("published/child/data")).unwrap(),
        b"stable bytes"
    );
}

#[test]
fn native_ancestor_leases_block_rename_then_release_it() {
    let temp = tempfile::tempdir().unwrap();
    let ancestor = temp.path().join("ancestor");
    let leaf = ancestor.join("child/grandchild");
    let guard = DirectoryGuard::create(&leaf).unwrap();
    assert!(fs::rename(&ancestor, temp.path().join("replacement")).is_err());
    assert!(fs::remove_dir(&leaf).is_err());
    drop(guard);
    fs::rename(&ancestor, temp.path().join("replacement")).unwrap();
}

#[test]
fn cleanup_failure_after_marker_deletion_restores_same_owner_for_retry() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    mark_owned(&work, "job", "manifest-kept").unwrap();
    fs::create_dir_all(work.join("child/grandchild")).unwrap();
    fs::write(work.join("child/grandchild/data"), b"old").unwrap();
    assert!(
        remove_owned_with_hook(&work, "job", |root| {
            fs::write(root.join("concurrent-new-child"), b"keep").unwrap();
        })
        .is_err()
    );
    assert_eq!(
        fs::read(work.join("concurrent-new-child")).unwrap(),
        b"keep"
    );
    owned(&work, "job", Some("manifest-kept")).unwrap();
    remove_owned(&work, "job").unwrap();
    assert!(!work.exists());
}

#[test]
fn owned_children_cleanup_and_readonly_files_keep_authority() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    mark_owned(&work, "job", "").unwrap();
    fs::create_dir_all(work.join("candidate/deep")).unwrap();
    let file = work.join("candidate/deep/data");
    fs::write(&file, b"remove").unwrap();
    let mut readonly = fs::metadata(&file).unwrap().permissions();
    readonly.set_readonly(true);
    fs::set_permissions(&file, readonly).unwrap();
    remove_file_inside_owned(&file, &work, "job").unwrap();
    remove_inside_owned(&work.join("candidate"), &work, "job").unwrap();
    assert!(!work.join("candidate").exists());
    owned(&work, "job", None).unwrap();
    remove_owned(&work, "job").unwrap();
}

#[test]
fn repeated_owner_validation_does_not_consume_the_shared_file_cursor() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    mark_owned(&work, "job", "manifest").unwrap();
    let tree = LockedTree::open(&work).unwrap();
    tree.verify_owned("job", Some("manifest")).unwrap();
    tree.verify_owned("job", Some("manifest")).unwrap();
}

#[test]
fn locked_child_prevents_cleanup_before_any_entry_is_removed() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    mark_owned(&work, "job", "").unwrap();
    fs::write(work.join("unlocked"), b"preserved").unwrap();
    fs::write(work.join("locked"), b"busy").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(work.join("locked"))
        .unwrap();
    assert!(remove_owned(&work, "job").is_err());
    assert_eq!(fs::read(work.join("unlocked")).unwrap(), b"preserved");
    assert_eq!(fs::read(work.join("locked")).unwrap(), b"busy");
    owned(&work, "job", None).unwrap();
    drop(lock);
    remove_owned(&work, "job").unwrap();
}

#[test]
fn existing_descendant_junction_blocks_cleanup_and_preserves_external_files() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("work");
    let outside = temp.path().join("outside");
    mark_owned(&work, "job", "").unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"outside preserved").unwrap();
    let candidate = work.join("candidate");
    fs::create_dir(&candidate).unwrap();
    let writer = fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .share_mode(
            FILE_SHARE_READ
                | FILE_SHARE_WRITE
                | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE,
        )
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(&candidate)
        .unwrap();
    set_test_junction(&writer, &outside).unwrap();
    assert!(remove_owned(&work, "job").is_err());
    assert!(remove_inside_owned(&candidate, &work, "job").is_err());
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"outside preserved"
    );
    owned(&work, "job", None).unwrap();
    clear_test_junction(&writer);
    drop(writer);
    remove_owned(&work, "job").unwrap();
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"outside preserved"
    );
}

#[test]
fn native_json_commit_overwrites_only_named_config_and_ownership_tag_never_replaces() {
    let temp = tempfile::tempdir().unwrap();
    let settings = temp.path().join("nested/settings.json");
    atomic_json(&settings, &serde_json::json!({"value":1})).unwrap();
    atomic_json(&settings, &serde_json::json!({"value":2})).unwrap();
    assert_eq!(
        read_json::<serde_json::Value>(&settings).unwrap()["value"],
        2
    );
    assert_eq!(fs::read_dir(settings.parent().unwrap()).unwrap().count(), 1);
    let work = temp.path().join("work");
    mark_owned(&work, "job", "manifest").unwrap();
    assert!(tag_existing(&work, "other", "other-manifest").is_err());
    owned(&work, "job", Some("manifest")).unwrap();
    assert_eq!(fs::read_dir(&work).unwrap().count(), 1);
}

fn set_test_junction(file: &fs::File, target: &Path) -> io::Result<()> {
    let display: Vec<u16> = target.as_os_str().encode_wide().collect();
    let substitute: Vec<u16> = format!(r"\??\{}", target.display())
        .encode_utf16()
        .collect();
    let mut path_buffer = substitute.clone();
    path_buffer.push(0);
    path_buffer.extend(&display);
    path_buffer.push(0);
    let mut bytes = Vec::new();
    bytes.extend(0xA000_0003u32.to_le_bytes());
    bytes.extend(((path_buffer.len() * 2 + 8) as u16).to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    for value in [
        0,
        (substitute.len() * 2) as u16,
        ((substitute.len() + 1) * 2) as u16,
        (display.len() * 2) as u16,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    for character in path_buffer {
        bytes.extend(character.to_le_bytes());
    }
    let mut written = 0;
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            0x0009_00A4,
            bytes.as_ptr().cast(),
            bytes.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut written,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn clear_test_junction(file: &fs::File) {
    let mut bytes = 0xA000_0003u32.to_le_bytes().to_vec();
    bytes.extend([0u8; 4]);
    let mut written = 0;
    assert_ne!(
        unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                0x0009_00AC,
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut written,
                std::ptr::null_mut(),
            )
        },
        0,
        "{}",
        io::Error::last_os_error()
    );
}

#[test]
fn attribute_only_junction_mutation_cannot_redirect_relative_native_operations() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pinned");
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"outside preserved").unwrap();
    let guard = DirectoryGuard::create(&path).unwrap();
    let writer = fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .share_mode(
            FILE_SHARE_READ
                | FILE_SHARE_WRITE
                | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE,
        )
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(&path)
        .unwrap();
    set_test_junction(&writer, &outside).unwrap();
    assert!(
        DirectoryGuard::open(&path).is_err(),
        "an existing junction is rejected"
    );
    assert!(
        !guard
            .entries()
            .unwrap()
            .iter()
            .any(|name| name == "sentinel")
    );
    assert!(
        guard.create_file(&path.join("created-by-object")).is_err(),
        "native child create rejects a reparse-mutated parent instead of redirecting it"
    );
    assert!(!outside.join("created-by-object").exists());
    clear_test_junction(&writer);
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"outside preserved"
    );
}

#[test]
fn test_prune_empty_directories_cleans_multilevel_empty_dirs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sandbox");
    fs::create_dir_all(root.join("empty_a/empty_b/empty_c")).unwrap();
    fs::create_dir_all(root.join("keep_a/keep_b")).unwrap();
    fs::write(root.join("keep_a/keep_b/file.txt"), b"keep me").unwrap();

    assert!(root.join("empty_a/empty_b/empty_c").exists());
    assert!(root.join("keep_a/keep_b/file.txt").exists());

    let pruned = prune_empty_directories(&root).unwrap();
    assert_eq!(pruned, 3); // empty_c, empty_b, empty_a

    assert!(!root.join("empty_a").exists());
    assert!(root.join("keep_a/keep_b/file.txt").exists());
    assert!(root.exists(), "Root must never be deleted");
}

#[test]
fn native_sleep_requests_are_released_on_their_own_thread_after_guard_moves() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = calls.clone();
    let guard = SleepBlockerGuard::with_execution_state(move |flags| {
        let result = unsafe { SetThreadExecutionState(flags) };
        recorded
            .lock()
            .unwrap()
            .push((std::thread::current().id(), flags, result));
        result
    });
    let second = SleepBlockerGuard::acquire();
    std::thread::spawn(move || drop(guard)).join().unwrap();
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, calls[1].0);
    assert_ne!(calls[0].0, std::thread::current().id());
    assert_eq!(calls[0].1, ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
    assert_eq!(calls[1].1, ES_CONTINUOUS);
    assert!(calls.iter().all(|call| call.2 != 0));
    drop(second);
}
#[test]
fn rejects_windows_aliases_and_traversal() {
    for p in [
        "../a",
        "C:/x",
        "a:ads",
        "a/../b",
        "NUL.txt",
        "folder/COM1",
        "folder/COM¹.txt",
        "LPT²",
        "CON .txt",
        "CONIN$",
        "x.",
        OWNER_FILE,
        "/abs",
    ] {
        assert!(relative(p).is_err(), "{p}")
    }
    assert!(relative("中文/file.txt").is_ok());
}
#[test]
fn owned_cleanup_cannot_remove_unmarked() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("work");
    fs::create_dir(&p).unwrap();
    assert!(remove_owned(&p, "id").is_err());
    assert!(p.exists());
    fs::remove_dir(&p).unwrap();
    mark_owned(&p, "id", "").unwrap();
    assert!(remove_owned(&p, "wrong").is_err());
    remove_owned(&p, "id").unwrap();
    assert!(!p.exists());
}
#[test]
fn publication_never_replaces_an_existing_empty_directory() {
    let d = tempfile::tempdir().unwrap();
    let source = d.path().join("source");
    let target = d.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(source.join("data"), b"synthetic committed file").unwrap();
    assert!(publish_directory(&source, &target).is_err());
    assert!(source.join("data").is_file());
    assert!(target.is_dir());
    assert!(!target.join("data").exists());
}
#[test]
fn unresolved_worker_marker_blocks_all_workspace_cleanup() {
    let d = tempfile::tempdir().unwrap();
    let work = d.path().join("work");
    mark_owned(&work, "job", "").unwrap();
    fs::create_dir(work.join("candidate")).unwrap();
    fs::write(work.join(".openunpack-exit-unconfirmed.json"), b"{}").unwrap();
    assert!(remove_owned(&work, "job").is_err());
    assert!(remove_inside_owned(&work.join("candidate"), &work, "job").is_err());
    assert!(work.join("candidate").is_dir());
}
#[test]
fn test_unc_path_stripping() {
    let p_unc = r"\\?\UNC\server\share\file.zip";
    let clean = if let Some(stripped) = p_unc.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", stripped)
    } else if let Some(stripped) = p_unc.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        p_unc.to_string()
    };
    assert_eq!(clean, r"\\server\share\file.zip");

    let p_drive = r"\\?\C:\Users\test\file.zip";
    let clean_drive = if let Some(stripped) = p_drive.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", stripped)
    } else if let Some(stripped) = p_drive.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        p_drive.to_string()
    };
    assert_eq!(clean_drive, r"C:\Users\test\file.zip");
}

#[test]
fn test_drive_letter_of_supports_verbatim_and_standard_paths() {
    assert_eq!(
        drive_letter_of(Path::new(r"C:\Users\test\file.zip")),
        Some('C')
    );
    assert_eq!(
        drive_letter_of(Path::new(r"\\?\C:\Users\test\file.zip")),
        Some('C')
    );
    assert_eq!(
        drive_letter_of(Path::new(r"c:\archives\pack.7z")),
        Some('C')
    );
    assert_eq!(
        drive_letter_of(Path::new(r"\\?\d:\data\file.rar")),
        Some('D')
    );
    assert_eq!(drive_letter_of(Path::new(r"\\server\share\file.zip")), None);
    assert_eq!(
        drive_letter_of(Path::new(r"\\?\UNC\server\share\file.zip")),
        None
    );
}
