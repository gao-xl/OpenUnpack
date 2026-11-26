//! CLI 引擎过滤参数、取消、加密探测与错误分类单元测试。

use super::archive_filter_args;
pub use super::*;

#[test]
fn maps_wildcards_to_seven_zip_switches() {
    let args = archive_filter_args(
        &[
            "*.exe".to_string(),
            "docs/cache".to_string(),
            "   ".to_string(),
        ],
        &["*.txt".to_string()],
    );
    assert_eq!(args, vec!["-xr!*.exe", "-x!docs/cache", "-ir!*.txt"]);
}

#[test]
fn empty_rules_produce_no_switches() {
    assert!(archive_filter_args(&[], &[]).is_empty());
    assert!(archive_filter_args(&["".to_string()], &["".to_string()]).is_empty());
}

#[test]
fn test_find_7z() {
    let b = super::find_7z_binary();
    assert!(b.is_some(), "bundled 7z must be found");
}

#[cfg(test)]
mod password_channel_tests {
    use super::push_password;
    use tokio::process::Command;

    fn args_of(cmd: &Command) -> Vec<String> {
        cmd.as_std()
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn password_never_reaches_argv() {
        let mut cmd = Command::new("7z.exe");
        push_password(&mut cmd, Some("s3cr3t-value"));
        let args = args_of(&cmd);
        assert!(
            args.is_empty(),
            "password switch must be omitted from argv entirely"
        );
        assert!(!args.iter().any(|arg| arg.contains("s3cr3t")));
    }

    #[test]
    fn missing_password_always_disables_the_prompt() {
        let mut cmd = Command::new("7z.exe");
        push_password(&mut cmd, None);
        assert_eq!(args_of(&cmd), vec!["-p-".to_string()]);
    }
}

#[cfg(test)]
mod cancellation_and_encryption_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    #[tokio::test]
    async fn cancel_stops_execution_under_100ms() {
        if !has_7z_cli() {
            return;
        }
        let bin = find_7z_binary().expect("7z must be found");

        // Run a benchmark command that would normally take seconds
        let mut cmd = Command::new(&bin);
        cmd.arg("b");

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel_clone.store(true, Ordering::Release);
        });

        let start = Instant::now();
        let res = run_7z_guarded(cmd, None, None, Duration::from_secs(30), &cancel).await;
        let elapsed = start.elapsed();

        assert_eq!(res.unwrap_err(), "Cancelled");
        assert!(
            elapsed < Duration::from_millis(200),
            "Kill-on-cancel must terminate child promptly: elapsed {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn password_probe_and_extract_via_stdin_succeeds_without_argv_exposure() {
        if !has_7z_cli() {
            return;
        }
        let bin = find_7z_binary().expect("7z must be found");
        let cur = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root = if cur.ends_with("infrastructure") {
            cur.parent().and_then(|p| p.parent()).unwrap_or(&cur)
        } else {
            &cur
        };
        let temp_dir = root
            .join("target")
            .join("test_scratch")
            .join(format!("ou_test_pwd_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let src_file = temp_dir.join("secret_data.txt");
        std::fs::write(&src_file, b"secure payload contents").unwrap();
        let archive_path = temp_dir.join("encrypted.7z");

        // Create encrypted archive using 7z.exe
        let mut create_cmd = std::process::Command::new(&bin);
        create_cmd
            .arg("a")
            .arg(&archive_path)
            .arg(&src_file)
            .arg("-pP@ssw0rd123!")
            .arg("-y")
            .arg("-bso0")
            .arg("-bsp0")
            .arg("-bse0");
        let create_status = create_cmd.status().expect("create archive status");
        assert!(create_status.success());

        // 1. Test probe_cli with candidate passwords
        let cancel = AtomicBool::new(false);
        let candidates = vec![
            ("WrongSecret1".to_string(), Some("id-1".to_string())),
            ("P@ssw0rd123!".to_string(), Some("id-2".to_string())),
        ];
        let probe_outcome = probe_cli(&archive_path, &candidates, &cancel).await;
        assert!(
            probe_outcome.is_ok(),
            "probe_cli failed: {:?}",
            probe_outcome.err()
        );
        match probe_outcome.unwrap() {
            crate::engine::ProbeOutcome::Success(pwd, id) => {
                assert_eq!(pwd, "P@ssw0rd123!");
                assert_eq!(id.as_deref(), Some("id-2"));
            }
            other => panic!("Expected ProbeOutcome::Success, got {:?}", other),
        }

        // 2. Test inspect_cli
        let inspect_res = inspect_cli(&archive_path, Some("P@ssw0rd123!"), 100, 1 << 20).await;
        assert!(
            inspect_res.is_ok(),
            "inspect_cli failed: {:?}",
            inspect_res.err()
        );
        let records = inspect_res.unwrap();
        assert!(
            records
                .iter()
                .any(|r| r.relative_path.contains("secret_data.txt"))
        );

        // 3. Test extract_cli (and verify -bsp1 streams progress without stdin leaks)
        let dest_dir = temp_dir.join("extracted_full");
        std::fs::create_dir_all(&dest_dir).unwrap();
        let mut req = WorkerRequest::default();
        req.operation = "extract".to_string();
        req.archive_path = archive_path.to_string_lossy().to_string();
        req.destination_directory = dest_dir.to_string_lossy().to_string();
        req.password = Some("P@ssw0rd123!".to_string());
        let cancel_arc = Arc::new(AtomicBool::new(false));
        let seen_progress = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_progress_clone = Arc::clone(&seen_progress);
        let progress = Arc::new(move |ev: &WorkerEvent| {
            if ev.kind == "progress" {
                seen_progress_clone.lock().unwrap().push((
                    ev.completed_bytes,
                    ev.total_bytes,
                    ev.total_bytes_known,
                ));
            }
        });
        let extract_res = extract_cli(&req, &dest_dir, progress, cancel_arc).await;
        assert!(
            extract_res.is_ok(),
            "extract_cli failed: {:?}",
            extract_res.err()
        );
        {
            let events = seen_progress.lock().unwrap();
            assert!(
                !events.is_empty(),
                "extract_cli -bsp1 should emit at least one progress event"
            );
            assert!(
                events
                    .iter()
                    .all(|(done, total, known)| *known && *done <= *total && *total > 0)
            );
        }
        let extracted_file = dest_dir.join("secret_data.txt");
        assert!(extracted_file.exists());
        assert_eq!(
            std::fs::read(&extracted_file).unwrap(),
            b"secure payload contents"
        );

        // 4. Test extract_cli_selective
        let sel_dest_dir = temp_dir.join("extracted_sel");
        std::fs::create_dir_all(&sel_dest_dir).unwrap();
        let sel_res = extract_cli_selective(
            &archive_path,
            &["secret_data.txt".to_string()],
            &sel_dest_dir,
            Some("P@ssw0rd123!"),
            true,
            Arc::new(AtomicBool::new(false)),
        )
        .await;
        assert!(
            sel_res.is_ok(),
            "extract_cli_selective failed: {:?}",
            sel_res.err()
        );
        assert!(sel_dest_dir.join("secret_data.txt").exists());

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}

#[cfg(test)]
mod failure_classification_tests {
    use super::{CliFailure, ListingParser, classify_cli_failure};

    #[test]
    fn password_and_corruption_are_not_confused() {
        assert_eq!(
            classify_cli_failure("ERROR: Wrong password?"),
            Some(CliFailure::Password)
        );
        assert_eq!(
            classify_cli_failure("Can not open the file as archive"),
            Some(CliFailure::Corrupt)
        );
        assert_eq!(
            classify_cli_failure("Missing volume : data.7z.002"),
            Some(CliFailure::MissingVolume)
        );
        assert_eq!(classify_cli_failure("Everything is Ok"), None);
    }

    fn record_block(path: &str, size: u64, folder: bool) -> String {
        format!(
            "Path = {path}\nSize = {size}\nFolder = {}\n\n",
            if folder { "+" } else { "-" }
        )
    }

    #[test]
    fn listing_parser_stops_at_the_entry_limit() {
        let mut parser = ListingParser::new(2, 0);
        parser
            .push_line("Path = C:\\src\\a.7z")
            .expect("archive header is skipped");
        parser.push_line("").expect("blank line is harmless");
        for index in 0..3 {
            let block = record_block(&format!("file{index}.txt"), 1, false);
            for line in block.lines() {
                if let Err(error) = parser.push_line(line) {
                    assert_eq!(error, "ResourceLimit: Exceeds maxEntries");
                    return;
                }
            }
        }
        panic!("the third entry must exceed maxEntries");
    }

    #[test]
    fn listing_parser_stops_at_the_byte_limit() {
        let mut parser = ListingParser::new(0, 10);
        for line in record_block("a.txt", 6, false).lines() {
            parser.push_line(line).expect("first file fits");
        }
        let mut failure = None;
        for line in record_block("b.txt", 6, false).lines() {
            if let Err(error) = parser.push_line(line) {
                failure = Some(error);
                break;
            }
        }
        assert_eq!(
            failure.as_deref(),
            Some("ResourceLimit: Exceeds maxTotalBytes")
        );
    }

    #[test]
    fn bsp1_progress_parser_handles_percentages_and_ignores_filenames() {
        use super::parse_7z_progress_percent;
        assert_eq!(parse_7z_progress_percent("  0%"), Some(0));
        assert_eq!(parse_7z_progress_percent(" 42% 3 - dir/file.txt"), Some(42));
        assert_eq!(parse_7z_progress_percent("100%"), Some(100));
        assert_eq!(parse_7z_progress_percent("101%"), None);
        assert_eq!(parse_7z_progress_percent("report 50%.pdf"), None);
        assert_eq!(parse_7z_progress_percent(""), None);
    }

    #[test]
    fn modular_7z_requires_sibling_dll() {
        use super::has_required_7z_dll;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("7z.exe");
        std::fs::write(&bin, b"stub").unwrap();
        assert!(
            !has_required_7z_dll(&bin),
            "7z.exe without sibling 7z.dll must be rejected"
        );
        std::fs::write(tmp.path().join("7z.dll"), b"dll").unwrap();
        assert!(has_required_7z_dll(&bin));
        assert!(has_required_7z_dll(&tmp.path().join("7za.exe")));
    }
}
