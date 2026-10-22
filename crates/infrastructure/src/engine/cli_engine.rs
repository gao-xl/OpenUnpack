use super::path_safety::{matches_exclusion, matches_inclusion, validate_relative_path};
use openunpack_protocol::worker::{FileRecord, WorkerEvent, WorkerRequest};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};
use tokio::process::Command;

/// Whether this 7-Zip build accepts a password on stdin.
/// 0 = not probed, 1 = stdin works, 2 = unsupported (fall back to argv).
static STDIN_PASSWORD: AtomicU8 = AtomicU8::new(0);
/// A build that ignores stdin may block on a console prompt; bound the probe.
const STDIN_PASSWORD_TIMEOUT: Duration = Duration::from_secs(30);
/// Measured on 2026-10-21 (7-Zip 26.04, both `7zr` and the full `7za`): a
/// password written to a redirected stdin is ignored and extraction fails with
/// "Wrong password?" even when the secret is correct. Evidence:
/// `artifacts/p2-09-stdin-proof/stdin-probe-result-*.json`. The stdin channel is
/// therefore opt-in only, for builds that do support it.
const STDIN_PASSWORD_ENV: &str = "OPENUNPACK_7Z_STDIN";

pub fn find_7z_binary() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("OPENUNPACK_7Z_PATH") {
        let p = PathBuf::from(custom);
        // Only trust an explicit absolute path: a relative value resolves against
        // the current working directory, which an opened archive folder controls.
        if p.is_absolute() && p.is_file() {
            return Some(p);
        }
    }

    // Common install locations (absolute, no PATH lookup).
    let absolute_candidates = [
        #[cfg(windows)]
        r"C:\Program Files\7-Zip\7z.exe",
        #[cfg(windows)]
        r"C:\Program Files (x86)\7-Zip\7z.exe",
        #[cfg(not(windows))]
        "/usr/bin/7z",
        #[cfg(not(windows))]
        "/usr/bin/7za",
        #[cfg(not(windows))]
        "/usr/local/bin/7z",
    ];
    for candidate in absolute_candidates {
        let p = Path::new(candidate);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }

    // Bare names are resolved against PATH directories only. Never let the
    // current directory satisfy a bare name: `CreateProcess` searches it first,
    // so a `7z.exe` dropped next to a downloaded archive would win.
    #[cfg(windows)]
    let names = ["7z.exe", "7za.exe", "7zr.exe"];
    #[cfg(not(windows))]
    let names = ["7z", "7za", "7zr", "p7zip"];

    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn has_7z_cli() -> bool {
    find_7z_binary().is_some()
}

fn stdin_password_supported() -> bool {
    std::env::var(STDIN_PASSWORD_ENV).is_ok() && STDIN_PASSWORD.load(Ordering::Relaxed) != 2
}

/// Adds the password switch. In stdin mode the value is omitted entirely so the
/// secret never reaches the process command line.
fn push_password(cmd: &mut Command, password: Option<&str>, use_stdin: bool) {
    match (password, use_stdin) {
        (Some(_), true) => {
            cmd.arg("-p");
        }
        (Some(pwd), false) => {
            cmd.arg(format!("-p{pwd}"));
        }
        (None, _) => {
            cmd.arg("-p-");
        }
    }
}

/// Runs 7z, feeding the password on stdin when requested. `kill_on_drop` plus a
/// timeout keeps a build that ignores stdin (and waits on a console prompt) from
/// hanging the worker forever.
async fn run_7z(
    mut cmd: Command,
    password: Option<&str>,
    use_stdin: bool,
) -> Result<std::process::Output, String> {
    if !use_stdin || password.is_none() {
        return cmd
            .output()
            .await
            .map_err(|e| format!("7zExecutionFailed: {e}"));
    }
    cmd.stdin(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("7zExecutionFailed: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let mut line = password.unwrap_or_default().as_bytes().to_vec();
        line.push(b'\n');
        let _ = stdin.write_all(&line).await;
        let _ = stdin.shutdown().await;
    }
    match tokio::time::timeout(STDIN_PASSWORD_TIMEOUT, child.wait_with_output()).await {
        Ok(result) => result.map_err(|e| format!("7zExecutionFailed: {e}")),
        Err(_) => Err("7zPasswordProbeTimeout".into()),
    }
}

pub async fn inspect_cli(
    archive_path: &Path,
    password: Option<&str>,
) -> Result<Vec<FileRecord>, String> {
    let bin = find_7z_binary().ok_or_else(|| "7zNotFound".to_string())?;

    let build = |use_stdin: bool| {
        let mut cmd = Command::new(&bin);
        cmd.arg("l").arg("-slt");
        push_password(&mut cmd, password, use_stdin);
        cmd.arg(archive_path);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd
    };

    let use_stdin = password.is_some() && stdin_password_supported();
    let output = match run_7z(build(use_stdin), password, use_stdin).await {
        Ok(output) if output.status.success() => {
            if use_stdin {
                STDIN_PASSWORD.store(1, Ordering::Relaxed);
            }
            output
        }
        Ok(_) if use_stdin => {
            // Either the password is wrong or this build ignored stdin. Only an
            // argv retry can tell them apart, and its outcome is what we report.
            let retry = run_7z(build(false), password, false).await?;
            STDIN_PASSWORD.store(
                if retry.status.success() { 2 } else { 1 },
                Ordering::Relaxed,
            );
            retry
        }
        Ok(output) => output,
        Err(_) if use_stdin => {
            STDIN_PASSWORD.store(2, Ordering::Relaxed);
            run_7z(build(false), password, false).await?
        }
        Err(error) => return Err(error),
    };
    if !output.status.success() {
        return Err("InspectFailedOrEncrypted".into());
    }

    let mut files = Vec::new();
    let reader = BufReader::new(&output.stdout[..]);

    let mut cur_path = String::new();
    let mut cur_size: u64 = 0;
    let mut cur_is_dir = false;
    let mut cur_is_encrypted = false;

    for line_res in reader.lines() {
        let line = line_res.map_err(|e| format!("ParseError: {e}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !cur_path.is_empty() {
                if let Ok(rel) = validate_relative_path(&cur_path) {
                    files.push(FileRecord {
                        relative_path: rel,
                        size: cur_size,
                        size_known: true,
                        sha256: String::new(),
                        is_directory: cur_is_dir,
                        is_encrypted: cur_is_encrypted,
                        is_corrupted: false,
                    });
                }
                cur_path.clear();
                cur_size = 0;
                cur_is_dir = false;
                cur_is_encrypted = false;
            }
            continue;
        }

        if let Some(val) = trimmed.strip_prefix("Path = ") {
            cur_path = val.to_string();
        } else if let Some(val) = trimmed.strip_prefix("Size = ") {
            cur_size = val.parse::<u64>().unwrap_or(0);
        } else if let Some(val) = trimmed.strip_prefix("Folder = ") {
            cur_is_dir = val == "+" || val == "1" || val.eq_ignore_ascii_case("true");
        } else if let Some(val) = trimmed.strip_prefix("Encrypted = ") {
            cur_is_encrypted = val == "+" || val == "1" || val.eq_ignore_ascii_case("true");
        }
    }

    if !cur_path.is_empty()
        && let Ok(rel) = validate_relative_path(&cur_path)
    {
        files.push(FileRecord {
            relative_path: rel,
            size: cur_size,
            size_known: true,
            sha256: String::new(),
            is_directory: cur_is_dir,
            is_encrypted: cur_is_encrypted,
            is_corrupted: false,
        });
    }

    Ok(files)
}

pub async fn probe_cli(
    archive_path: &Path,
    candidates: &[(String, Option<String>)],
    cancel: &AtomicBool,
) -> Result<Option<(String, Option<String>)>, String> {
    let bin = find_7z_binary().ok_or_else(|| "7zNotFound".to_string())?;

    for (candidate_secret, candidate_id) in candidates {
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }

        let mut cmd = Command::new(&bin);
        cmd.arg("t")
            .arg(format!("-p{}", candidate_secret))
            .arg("-y")
            .arg(archive_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if let Ok(output) = cmd.output().await
            && output.status.success()
        {
            return Ok(Some((candidate_secret.clone(), candidate_id.clone())));
        }
    }

    Ok(None)
}

/// Translates settings wildcard rules into 7-Zip switches so the CLI engine
/// honours the same include/exclude contract as the native ZIP engine.
/// Bare names are applied recursively (matching `matches_exclusion`'s
/// basename semantics); patterns containing a separator stay path-anchored.
fn archive_filter_args(exclusions: &[String], inclusions: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    for raw in exclusions {
        let pattern = raw.trim();
        if pattern.is_empty() {
            continue;
        }
        let recursive = !pattern.contains('/') && !pattern.contains('\\');
        args.push(format!(
            "-x{}!{}",
            if recursive { "r" } else { "" },
            pattern
        ));
    }
    for raw in inclusions {
        let pattern = raw.trim();
        if pattern.is_empty() {
            continue;
        }
        let recursive = !pattern.contains('/') && !pattern.contains('\\');
        args.push(format!(
            "-i{}!{}",
            if recursive { "r" } else { "" },
            pattern
        ));
    }
    args
}

pub async fn extract_cli(
    request: &WorkerRequest,
    destination: &Path,
    _progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<FileRecord>, String> {
    let bin = find_7z_binary().ok_or_else(|| "7zNotFound".to_string())?;
    let password = request.password.as_deref();

    let build = |use_stdin: bool| {
        let mut cmd = Command::new(&bin);
        cmd.arg("x")
            .arg(format!("-o{}", destination.display()))
            .arg("-y");
        for filter in archive_filter_args(&request.exclusions, &request.inclusions) {
            cmd.arg(filter);
        }
        push_password(&mut cmd, password, use_stdin);
        cmd.arg(&request.archive_path);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd
    };

    let use_stdin = password.is_some() && stdin_password_supported();
    let output = match run_7z(build(use_stdin), password, use_stdin).await {
        Ok(output) if output.status.success() => {
            if use_stdin {
                STDIN_PASSWORD.store(1, Ordering::Relaxed);
            }
            output
        }
        Ok(_) if use_stdin => {
            // Either the password is wrong or this build ignored stdin. Only an
            // argv retry can tell them apart, and its outcome is what we report.
            let retry = run_7z(build(false), password, false).await?;
            STDIN_PASSWORD.store(
                if retry.status.success() { 2 } else { 1 },
                Ordering::Relaxed,
            );
            retry
        }
        Ok(output) => output,
        Err(_) if use_stdin => {
            STDIN_PASSWORD.store(2, Ordering::Relaxed);
            run_7z(build(false), password, false).await?
        }
        Err(error) => return Err(error),
    };
    let had_warning_or_error = !output.status.success();
    if had_warning_or_error {
        let err_str = String::from_utf8_lossy(&output.stderr);
        if err_str.contains("Wrong password") || err_str.contains("password") {
            return Err("PasswordMismatchOrCorrupt".into());
        }
        if !request.salvage_corrupted {
            return Err("ExtractionFailed".into());
        }
    }

    // Discover extracted files, sanitize via two-pass parity gate, and build FileRecords
    let mut extracted_records = Vec::new();
    let mut items_to_delete = Vec::new();
    for entry in walkdir::WalkDir::new(destination)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }
        let path = entry.path();
        if path == destination {
            continue;
        }

        let rel = match path.strip_prefix(destination) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        let metadata = entry
            .metadata()
            .map_err(|e| format!("MetadataError: {e}"))?;
        let is_dir = metadata.is_dir();

        // Parity Gate 1: Check exclusions
        if matches_exclusion(&rel, &request.exclusions) {
            items_to_delete.push((path.to_path_buf(), is_dir));
            continue;
        }

        // Parity Gate 2: Check inclusions
        if !matches_inclusion(&rel, is_dir, &request.inclusions) {
            items_to_delete.push((path.to_path_buf(), is_dir));
            continue;
        }

        let size = if is_dir { 0 } else { metadata.len() };

        let sha256 = if is_dir {
            String::new()
        } else {
            let mut file = File::open(path).map_err(|e| format!("IoError: {e}"))?;
            let mut hasher = Sha256::new();
            let mut buf = [0u8; 32 * 1024];
            loop {
                match file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => hasher.update(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(format!("IoError: {e}")),
                }
            }
            hex::encode(hasher.finalize())
        };

        extracted_records.push(FileRecord {
            relative_path: rel,
            size,
            size_known: true,
            sha256,
            is_directory: is_dir,
            is_encrypted: request.password.is_some(),
            is_corrupted: false,
        });
    }

    // Clean up any files/directories that were excluded by policy but written by 7z CLI.
    // Sort descending by path length so that deepest leaf items are deleted before parent folders.
    items_to_delete.sort_by_key(|item| std::cmp::Reverse(item.0.as_os_str().len()));
    for (to_del, is_dir) in items_to_delete {
        if is_dir {
            let _ = std::fs::remove_dir(&to_del);
        } else {
            let _ = std::fs::remove_file(&to_del);
        }
    }

    if extracted_records.is_empty() && had_warning_or_error {
        return Err("ExtractionFailed: 无有效文件可解压".into());
    }

    Ok(extracted_records)
}

/// Selectively extracts designated entries from any 7-Zip supported archive format.
pub async fn extract_cli_selective(
    archive_path: &Path,
    entry_paths: &[String],
    destination: &Path,
    password: Option<&str>,
    flatten: bool,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<String>, String> {
    let bin = find_7z_binary().ok_or_else(|| "7zNotFound".to_string())?;

    let mut cmd = Command::new(&bin);
    cmd.arg(if flatten { "e" } else { "x" })
        .arg(format!("-o{}", destination.display()))
        .arg("-y");

    push_password(&mut cmd, password, false);
    cmd.arg(archive_path);

    for entry in entry_paths {
        let trimmed = entry.trim();
        if !trimmed.is_empty() {
            cmd.arg(trimmed);
        }
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("7zSpawnError: {e}"))?;
    let status = child
        .wait()
        .await
        .map_err(|e| format!("7zWaitError: {e}"))?;

    if cancel.load(Ordering::Acquire) {
        return Err("Cancelled".into());
    }

    if !status.success() {
        return Err("ExtractionFailed: 7-Zip selective extraction failed".into());
    }

    let mut extracted_paths = Vec::new();
    for entry in walkdir::WalkDir::new(destination)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.path() != destination && entry.file_type().is_file() {
            extracted_paths.push(entry.path().to_string_lossy().into_owned());
        }
    }

    Ok(extracted_paths)
}

#[cfg(test)]
mod tests {
    use super::archive_filter_args;

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
    fn stdin_mode_keeps_the_password_out_of_argv() {
        let mut cmd = Command::new("7z.exe");
        push_password(&mut cmd, Some("s3cr3t-value"), true);
        assert_eq!(args_of(&cmd), vec!["-p".to_string()]);
        assert!(!args_of(&cmd).iter().any(|arg| arg.contains("s3cr3t")));
    }

    #[test]
    fn argv_mode_preserves_the_legacy_switch_for_builds_without_stdin_support() {
        let mut cmd = Command::new("7z.exe");
        push_password(&mut cmd, Some("s3cr3t-value"), false);
        assert_eq!(args_of(&cmd), vec!["-ps3cr3t-value".to_string()]);
    }

    #[test]
    fn missing_password_always_disables_the_prompt() {
        let mut cmd = Command::new("7z.exe");
        push_password(&mut cmd, None, true);
        assert_eq!(args_of(&cmd), vec!["-p-".to_string()]);
    }
}

#[cfg(test)]
mod stdin_policy_tests {
    use super::{STDIN_PASSWORD_ENV, stdin_password_supported};

    #[test]
    fn stdin_channel_is_opt_in_by_default() {
        // 7-Zip 26.04 ignores redirected stdin (see artifacts/p2-09-stdin-proof),
        // so the argv path must stay the default unless an operator opts in.
        unsafe { std::env::remove_var(STDIN_PASSWORD_ENV) };
        assert!(!stdin_password_supported());
        unsafe { std::env::set_var(STDIN_PASSWORD_ENV, "1") };
        assert!(stdin_password_supported());
        unsafe { std::env::remove_var(STDIN_PASSWORD_ENV) };
    }
}
