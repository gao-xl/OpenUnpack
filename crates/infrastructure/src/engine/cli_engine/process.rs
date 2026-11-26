//! 7-Zip 子进程创建、受限标准流读取与进度行解析。

use super::*;

#[cfg(windows)]
pub(super) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Timeout for archive integrity probe operations.
pub(super) const CLI_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout for archive directory listing/inspection operations.
pub(super) const CLI_INSPECT_TIMEOUT: Duration = Duration::from_secs(60);
/// Execution timeout for general CLI operations to prevent deadlocks on hang.
pub(super) const CLI_DEFAULT_TIMEOUT: Duration = Duration::from_secs(3600);
/// Upper bound on captured 7-Zip stderr. Only the classification keywords are
/// needed, so a hostile archive cannot grow the buffer through error spam.
pub(super) const STDERR_CAPTURE_LIMIT: usize = 8 * 1024;
/// Ensures full `7z.exe` binaries sit alongside their required `7z.dll`.
pub(super) fn has_required_7z_dll(bin_path: &Path) -> bool {
    let is_modular_7z = bin_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("7z.exe"));
    if is_modular_7z {
        bin_path.with_file_name("7z.dll").is_file()
    } else {
        true
    }
}

/// Builds a `7z` child command configured to hide any transient console window
/// on Windows desktop runs (`CREATE_NO_WINDOW`).
pub(crate) fn configure_7z_cmd(bin: &Path) -> Command {
    let mut cmd = Command::new(bin);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

pub fn find_7z_binary() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("OPENUNPACK_7Z_PATH") {
        let p = PathBuf::from(custom);
        // Only trust an explicit absolute path: a relative value resolves against
        // the current working directory, which an opened archive folder controls.
        if p.is_absolute() && p.is_file() && has_required_7z_dll(&p) {
            return Some(p);
        }
    }

    // Bundled 7-Zip alongside the running executable
    if let Ok(current_exe) = std::env::current_exe()
        && let Some(exe_dir) = current_exe.parent()
    {
        let relative_candidates = [
            exe_dir.join("7z.exe"),
            exe_dir.join("tools/7zip/7z.exe"),
            exe_dir.join("../tools/7zip/7z.exe"),
            exe_dir.join("../../tools/7zip/7z.exe"),
            exe_dir.join("../../../tools/7zip/7z.exe"),
            exe_dir.join("../../../../tools/7zip/7z.exe"),
        ];
        for candidate in relative_candidates {
            if let Ok(canonical) = candidate.canonicalize()
                && canonical.is_file()
                && has_required_7z_dll(&canonical)
            {
                return Some(canonical);
            }
        }
    }

    // Cargo workspace check during development / tests
    let manifest_tool = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/7zip/7z.exe");
    if manifest_tool.is_file()
        && let Ok(canonical) = manifest_tool.canonicalize()
        && has_required_7z_dll(&canonical)
    {
        return Some(canonical);
    }

    // Relative check if current_dir is workspace root or sub-crate (debug builds only)
    #[cfg(debug_assertions)]
    for rel in [
        "tools/7zip/7z.exe",
        "../../tools/7zip/7z.exe",
        "../tools/7zip/7z.exe",
    ] {
        let p = Path::new(rel);
        if p.is_file()
            && let Ok(canonical) = p.canonicalize()
            && has_required_7z_dll(&canonical)
        {
            return Some(canonical);
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
        if p.is_file() && has_required_7z_dll(p) {
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
            if candidate.is_file() && has_required_7z_dll(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn has_7z_cli() -> bool {
    find_7z_binary().is_some()
}

/// Adds password switches for 7-Zip CLI.
///
/// 7-Zip prompts for a password on stdin when no `-p` switch is specified on the command line.
/// Passing `-p` with no value sets the password to empty string `""` (which suppresses stdin prompt),
/// while passing `-p<secret>` leaks the secret in plaintext to the process command line (visible via WMI/EDR).
/// Passing `-p-` explicitly disables password prompting.
///
/// Therefore, when a password is provided (`Some`), we omit any `-p` argument from argv entirely
/// and pipe the secret exclusively through standard input. When no password is provided (`None`),
/// we pass `-p-` to immediately fail on password-protected archives instead of hanging on prompt.
pub(crate) fn push_password(cmd: &mut Command, password: Option<&str>) {
    if password.is_none() {
        cmd.arg("-p-");
    }
}

/// Machine-readable reason behind a non-zero 7-Zip exit code.
///
/// 7-Zip exposes no error codes of its own, so the console text is the only
/// signal. It is kept in one place: three copies of this ladder previously
/// drifted apart, and one of them classified a password failure as corruption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CliFailure {
    Password,
    MissingVolume,
    Corrupt,
}

pub(super) fn classify_cli_failure(text: &str) -> Option<CliFailure> {
    if text.contains("Wrong password")
        || text.contains("password")
        || text.contains("encrypted")
        || text.contains("Can not open encrypted archive")
        || text.contains("Cannot open encrypted archive")
    {
        return Some(CliFailure::Password);
    }
    if text.contains("Missing volume") || text.contains("Cannot find volume") {
        return Some(CliFailure::MissingVolume);
    }
    if text.contains("Can not open as archive")
        || text.contains("Can't open as archive")
        || text.contains("Can not open the file as")
        || text.contains("Cannot open the file as")
        || text.contains("Is not archive")
        || text.contains("Headers Error")
        || text.contains("Unexpected end")
    {
        return Some(CliFailure::Corrupt);
    }
    None
}

/// Result of a guarded 7-Zip run: bounded captures, never the full output.
#[derive(Debug)]
pub(super) struct GuardedOutput {
    pub(super) status: std::process::ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}

/// Runs 7z while watching the destination's free space.
///
/// `7z x` gives no per-file progress here, so a decompression bomb cannot be
/// stopped by counting bytes mid-stream. Watching the volume instead is what
/// keeps an archive that declares a small size from filling the disk: the child
/// is killed as soon as free space falls under the operator's safety margin.
///
/// This entry point is extraction-only (probing uses `run_7z` and its 30s
/// window), so the deadline is always `CLI_DEFAULT_TIMEOUT` — a long
/// password-protected unpack must never inherit the probe cap and be pushed
/// into a mid-run argv fallback that re-exposes the secret on the command line.
pub(super) type ProgressSink = Arc<dyn Fn(u8) + Send + Sync>;

/// Extracts the leading integer percentage (`0..=100`) from one `-bsp1`
/// carriage-return segment (e.g. `"  42% - dir/file"` or `"100%"`), without
/// misreading `%` characters embedded later inside a filename.
pub(crate) fn parse_7z_progress_percent(segment: &str) -> Option<u8> {
    let trimmed = segment.trim_start();
    let (digits, _) = trimmed.split_once('%')?;
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let pct = digits.parse::<u8>().ok()?;
    (pct <= 100).then_some(pct)
}

pub(super) async fn run_7z_guarded(
    cmd: Command,
    password: Option<&str>,
    volume: Option<(&Path, u64)>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<GuardedOutput, String> {
    run_7z_guarded_with_progress(cmd, password, volume, timeout, cancel, None).await
}

pub(super) async fn run_7z_guarded_with_progress(
    mut cmd: Command,
    password: Option<&str>,
    volume: Option<(&Path, u64)>,
    timeout: Duration,
    cancel: &AtomicBool,
    on_percent: Option<ProgressSink>,
) -> Result<GuardedOutput, String> {
    if cancel.load(Ordering::Acquire) {
        return Err("Cancelled".into());
    }

    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if password.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd.spawn().map_err(|e| format!("7zExecutionFailed: {e}"))?;
    // Held until this function returns: dropping the guard closes the job
    // handle, and KILL_ON_JOB_CLOSE then reaps 7-Zip even when the host dies.
    let _job = ProcessJob::attach(&child);
    let stdout_task = tokio::spawn(drain_stdout_stream(child.stdout.take(), on_percent));
    let stderr_task = tokio::spawn(drain_bounded(child.stderr.take()));
    if let Some(pwd) = password
        && let Some(mut stdin) = child.stdin.take()
    {
        use tokio::io::AsyncWriteExt;
        let mut line = pwd.as_bytes().to_vec();
        line.push(b'\n');
        let _ = stdin.write_all(&line).await;
        let _ = stdin.shutdown().await;
    }

    let deadline = tokio::time::Instant::now() + timeout;
    let mut next_space_check = tokio::time::Instant::now();
    let status = loop {
        match child
            .try_wait()
            .map_err(|e| format!("7zExecutionFailed: {e}"))?
        {
            Some(status) => break status,
            None => {
                if cancel.load(Ordering::Acquire) {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    let _ = stdout_task.await;
                    let _ = stderr_task.await;
                    return Err("Cancelled".into());
                }
                let now = tokio::time::Instant::now();
                if now >= deadline {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    let _ = stdout_task.await;
                    let _ = stderr_task.await;
                    return Err("7zExecutionTimeout".to_string());
                }
                if let Some((path, min_free)) = volume
                    && min_free > 0
                    && now >= next_space_check
                {
                    next_space_check = now + Duration::from_millis(500);
                    if let Ok(free) = crate::paths::free_bytes(path)
                        && free < min_free
                    {
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        let _ = stdout_task.await;
                        let _ = stderr_task.await;
                        return Err(
                            "ResourceLimit: 解压过程中磁盘剩余空间跌破「保留空闲空间」下限，已中止"
                                .into(),
                        );
                    }
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    };

    Ok(GuardedOutput {
        status,
        stdout: stdout_task.await.unwrap_or_default(),
        stderr: stderr_task.await.unwrap_or_default(),
    })
}

pub(super) async fn drain_bounded<R>(pipe: Option<R>) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    drain_stdout_stream(pipe, None).await
}

/// Drains a child pipe into a bounded buffer while optionally parsing `-bsp1`
/// progress tokens separated by CR, LF, or backspace.
pub(super) async fn drain_stdout_stream<R>(
    pipe: Option<R>,
    on_percent: Option<ProgressSink>,
) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(mut pipe) = pipe else {
        return Vec::new();
    };
    use tokio::io::AsyncReadExt;
    let mut collected = Vec::new();
    let mut segment_buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let slice = &chunk[..read];
                // Retain the bounded tail when output grows large so final
                // failure diagnostics at the end of a long run stay intact.
                if collected.len() + slice.len() <= STDERR_CAPTURE_LIMIT {
                    collected.extend_from_slice(slice);
                } else if slice.len() >= STDERR_CAPTURE_LIMIT {
                    collected.clear();
                    collected.extend_from_slice(&slice[slice.len() - STDERR_CAPTURE_LIMIT..]);
                } else {
                    let drop_n = collected.len() + slice.len() - STDERR_CAPTURE_LIMIT;
                    collected.drain(..drop_n);
                    collected.extend_from_slice(slice);
                }
                if let Some(sink) = &on_percent {
                    for &byte in slice {
                        if matches!(byte, b'\r' | b'\n' | 0x08) {
                            if !segment_buf.is_empty() {
                                let segment =
                                    super::super::oem_codepage::decode_oem_text(&segment_buf);
                                if let Some(pct) = parse_7z_progress_percent(&segment) {
                                    sink(pct);
                                }
                                segment_buf.clear();
                            }
                        } else if segment_buf.len() < 512 {
                            segment_buf.push(byte);
                        }
                    }
                }
            }
        }
    }
    if let Some(sink) = &on_percent
        && !segment_buf.is_empty()
    {
        let segment = super::super::oem_codepage::decode_oem_text(&segment_buf);
        if let Some(pct) = parse_7z_progress_percent(&segment) {
            sink(pct);
        }
    }
    collected
}
