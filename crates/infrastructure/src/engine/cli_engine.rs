use super::path_safety::{matches_exclusion, matches_inclusion, validate_relative_path};
use super::process_job::ProcessJob;
use openunpack_protocol::worker::{FileRecord, WorkerEvent, WorkerRequest};
use sha2::{Digest, Sha256};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    },
    time::Duration,
};
use tokio::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Timeout for archive integrity probe operations.
const CLI_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout for archive directory listing/inspection operations.
const CLI_INSPECT_TIMEOUT: Duration = Duration::from_secs(60);
/// Execution timeout for general CLI operations to prevent deadlocks on hang.
const CLI_DEFAULT_TIMEOUT: Duration = Duration::from_secs(3600);
/// Upper bound on captured 7-Zip stderr. Only the classification keywords are
/// needed, so a hostile archive cannot grow the buffer through error spam.
const STDERR_CAPTURE_LIMIT: usize = 8 * 1024;
/// Ensures full `7z.exe` binaries sit alongside their required `7z.dll`.
fn has_required_7z_dll(bin_path: &Path) -> bool {
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
enum CliFailure {
    Password,
    MissingVolume,
    Corrupt,
}

fn classify_cli_failure(text: &str) -> Option<CliFailure> {
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
struct GuardedOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
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
type ProgressSink = Arc<dyn Fn(u8) + Send + Sync>;

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

async fn run_7z_guarded(
    cmd: Command,
    password: Option<&str>,
    volume: Option<(&Path, u64)>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<GuardedOutput, String> {
    run_7z_guarded_with_progress(cmd, password, volume, timeout, cancel, None).await
}

async fn run_7z_guarded_with_progress(
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

/// Incremental `7z l -slt` parser.
///
/// The listing used to be buffered in full before parsing, so an archive that
/// merely *declares* millions of entries could exhaust memory before any limit
/// was consulted. Parsing line by line keeps the resident set proportional to
/// `max_entries` instead of to the archive's claim.
struct ListingParser {
    files: Vec<FileRecord>,
    cur_path: String,
    cur_size: u64,
    cur_is_dir: bool,
    cur_is_encrypted: bool,
    max_entries: u64,
    max_total_bytes: u64,
    total_bytes: u64,
}

impl ListingParser {
    fn new(max_entries: u64, max_total_bytes: u64) -> Self {
        Self {
            files: Vec::new(),
            cur_path: String::new(),
            cur_size: 0,
            cur_is_dir: false,
            cur_is_encrypted: false,
            max_entries,
            max_total_bytes,
            total_bytes: 0,
        }
    }

    fn push_line(&mut self, line: &str) -> Result<(), String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return self.flush();
        }
        if let Some(val) = trimmed.strip_prefix("Path = ") {
            self.cur_path = val.to_string();
        } else if let Some(val) = trimmed.strip_prefix("Size = ") {
            self.cur_size = val.parse::<u64>().unwrap_or(0);
        } else if let Some(val) = trimmed.strip_prefix("Folder = ") {
            self.cur_is_dir = val == "+" || val == "1" || val.eq_ignore_ascii_case("true");
        } else if let Some(val) = trimmed.strip_prefix("Encrypted = ") {
            self.cur_is_encrypted = val == "+" || val == "1" || val.eq_ignore_ascii_case("true");
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        if self.cur_path.is_empty() {
            return Ok(());
        }
        let relative_path = validate_relative_path(&self.cur_path);
        // The first block describes the archive itself and has an absolute
        // path, so it is dropped here rather than counted against the budget.
        if let Ok(relative_path) = relative_path {
            if self.max_entries > 0 && self.files.len() as u64 >= self.max_entries {
                return Err("ResourceLimit: Exceeds maxEntries".into());
            }
            self.total_bytes = self.total_bytes.saturating_add(self.cur_size);
            if self.max_total_bytes > 0 && self.total_bytes > self.max_total_bytes {
                return Err("ResourceLimit: Exceeds maxTotalBytes".into());
            }
            self.files.push(FileRecord {
                relative_path,
                size: self.cur_size,
                size_known: true,
                sha256: String::new(),
                is_directory: self.cur_is_dir,
                is_encrypted: self.cur_is_encrypted,
                is_corrupted: false,
            });
        }
        self.cur_path.clear();
        self.cur_size = 0;
        self.cur_is_dir = false;
        self.cur_is_encrypted = false;
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<FileRecord>, String> {
        self.flush()?;
        Ok(self.files)
    }
}

/// Drains a child pipe into a bounded buffer.
///
/// An unread pipe blocks the child once the OS buffer fills (~64 KiB on
/// Windows), so every piped stream must be consumed even when its content is
/// only kept for failure classification.
async fn drain_bounded<R>(pipe: Option<R>) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    drain_stdout_stream(pipe, None).await
}

/// Drains a child pipe into a bounded buffer while optionally parsing `-bsp1`
/// progress tokens separated by CR, LF, or backspace.
async fn drain_stdout_stream<R>(pipe: Option<R>, on_percent: Option<ProgressSink>) -> Vec<u8>
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
                                let segment = super::oem_codepage::decode_oem_text(&segment_buf);
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
        let segment = super::oem_codepage::decode_oem_text(&segment_buf);
        if let Some(pct) = parse_7z_progress_percent(&segment) {
            sink(pct);
        }
    }
    collected
}

pub async fn inspect_cli(
    archive_path: &Path,
    password: Option<&str>,
    max_entries: u64,
    max_total_bytes: u64,
) -> Result<Vec<FileRecord>, String> {
    let bin = find_7z_binary().ok_or_else(|| "7zNotFound".to_string())?;

    let mut cmd = configure_7z_cmd(&bin);
    cmd.arg("l").arg("-slt").arg("-sccUTF-8");
    push_password(&mut cmd, password);
    cmd.arg("--");
    cmd.arg(archive_path);

    inspect_cli_once(cmd, password, max_entries, max_total_bytes).await
}

async fn inspect_cli_once(
    mut cmd: Command,
    password: Option<&str>,
    max_entries: u64,
    max_total_bytes: u64,
) -> Result<Vec<FileRecord>, String> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if password.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd.spawn().map_err(|e| format!("7zExecutionFailed: {e}"))?;
    let _job = ProcessJob::attach(&child);
    let stdout_pipe = child.stdout.take();
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

    let parsed = tokio::time::timeout(CLI_INSPECT_TIMEOUT, async move {
        let Some(pipe) = stdout_pipe else {
            return Ok(Vec::new());
        };
        use tokio::io::AsyncBufReadExt;
        let mut reader = tokio::io::BufReader::new(pipe);
        let mut parser = ListingParser::new(max_entries, max_total_bytes);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    // 7-Zip writes the console code page into a redirected pipe;
                    // decode_oem_text safely decodes UTF-8 as well as Windows OEM/ANSI codepages.
                    let decoded_line = super::oem_codepage::decode_oem_text(&line);
                    parser.push_line(&decoded_line)?;
                }
                Err(error) => return Err(format!("IoError: {error}")),
            }
        }
        parser.finish()
    })
    .await;

    let files = match parsed {
        Ok(Ok(files)) => files,
        Ok(Err(error)) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stderr_task.await;
            return Err(error);
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stderr_task.await;
            return Err("7zExecutionTimeout".into());
        }
    };

    let status = child
        .wait()
        .await
        .map_err(|e| format!("7zExecutionFailed: {e}"))?;
    let stderr_bytes = stderr_task.await.unwrap_or_default();
    if !status.success() {
        let stderr_text = super::oem_codepage::decode_oem_text(&stderr_bytes);
        return Err(match classify_cli_failure(&stderr_text) {
            Some(CliFailure::Password) => "WrongPassword",
            Some(CliFailure::MissingVolume) => "MissingVolume",
            Some(CliFailure::Corrupt) => "Corrupt",
            None => "InspectFailed",
        }
        .into());
    }
    Ok(files)
}

pub async fn probe_cli(
    archive_path: &Path,
    candidates: &[(String, Option<String>)],
    cancel: &AtomicBool,
) -> Result<super::ProbeOutcome, String> {
    let bin = find_7z_binary().ok_or_else(|| "7zNotFound".to_string())?;

    if cancel.load(Ordering::Acquire) {
        return Err("Cancelled".into());
    }

    // 1. Fast metadata inspection via `7z l -slt -p-` (typically < 50ms)
    // Avoids the full CRC32 decompression test of `7z t -p-` on large unencrypted archives.
    let inspect_result = inspect_cli(archive_path, None, 100_000, 100 * 1024 * 1024 * 1024).await;

    let requires_password;
    let mut header_encrypted = false;
    let mut target_file: Option<String> = None;

    match inspect_result {
        Ok(files) => {
            let mut encrypted_files: Vec<&FileRecord> = files
                .iter()
                .filter(|f| f.is_encrypted && !f.is_directory)
                .collect();
            if encrypted_files.is_empty() {
                // Not a single entry is encrypted and headers are readable.
                // Verify a tiny sample entry (<= 32 MiB) to confirm payload integrity in
                // milliseconds without decompressing the entire archive.
                if let Some(sample) = files
                    .iter()
                    .filter(|f| !f.is_directory && f.size <= 32 * 1024 * 1024)
                    .min_by_key(|f| (f.size == 0, f.size))
                {
                    let mut sample_cmd = configure_7z_cmd(&bin);
                    sample_cmd
                        .arg("t")
                        .arg("-p-")
                        .arg("-y")
                        .arg("-sccUTF-8")
                        .arg("--")
                        .arg(archive_path)
                        .arg(&sample.relative_path);
                    if let Ok(sample_out) =
                        run_7z_guarded(sample_cmd, None, None, CLI_PROBE_TIMEOUT, cancel).await
                        && !sample_out.status.success()
                    {
                        let err_str = super::oem_codepage::decode_oem_text(&sample_out.stderr);
                        let out_str = super::oem_codepage::decode_oem_text(&sample_out.stdout);
                        let combined = format!("{err_str}\n{out_str}");
                        if combined.contains("CRC Failed")
                            || combined.contains("Data Error")
                            || combined.contains("Unavailable data")
                            || combined.contains("Headers Error")
                        {
                            return Ok(super::ProbeOutcome::Corrupt);
                        }
                    }
                }
                return Ok(super::ProbeOutcome::NoPasswordNeeded);
            }
            requires_password = true;
            encrypted_files.sort_by_key(|f| (f.size == 0, f.size));
            target_file = Some(encrypted_files[0].relative_path.clone());
        }
        Err(e) if e == "WrongPassword" => {
            requires_password = true;
            header_encrypted = true;
        }
        Err(e) if e == "MissingVolume" => {
            return Ok(super::ProbeOutcome::MissingVolume);
        }
        Err(e) if e == "Corrupt" => {
            return Ok(super::ProbeOutcome::Corrupt);
        }
        Err(e) if e == "Cancelled" => {
            return Err("Cancelled".into());
        }
        Err(_) => {
            // Fallback for archives where `7z l -slt` failed (e.g. unknown format)
            let mut null_cmd = configure_7z_cmd(&bin);
            null_cmd
                .arg("t")
                .arg("-p-")
                .arg("-y")
                .arg("-sccUTF-8")
                .arg("--")
                .arg(archive_path);

            let null_out =
                match run_7z_guarded(null_cmd, None, None, CLI_PROBE_TIMEOUT, cancel).await {
                    Ok(out) => out,
                    Err(e) if e == "Cancelled" => return Err("Cancelled".into()),
                    Err(e) if e == "7zExecutionTimeout" => return Err("7zProbeTimeout".into()),
                    Err(e) => return Err(e),
                };

            if null_out.status.success() {
                return Ok(super::ProbeOutcome::NoPasswordNeeded);
            }
            let err_str = super::oem_codepage::decode_oem_text(&null_out.stderr);
            let out_str = super::oem_codepage::decode_oem_text(&null_out.stdout);
            let combined = format!("{err_str}\n{out_str}");
            if combined.contains("Missing volume") || combined.contains("Cannot find volume") {
                return Ok(super::ProbeOutcome::MissingVolume);
            }
            if combined.contains("Unsupported Method") {
                return Ok(super::ProbeOutcome::Unsupported);
            }
            requires_password = combined.contains("Wrong password")
                || combined.contains("password")
                || combined.contains("encrypted")
                || combined.contains("Can not open encrypted archive")
                || combined.contains("Cannot open encrypted archive");
            if !requires_password
                && (combined.contains("CRC Failed")
                    || combined.contains("Data Error")
                    || combined.contains("Unavailable data")
                    || combined.contains("Can not open as archive")
                    || combined.contains("Can't open as archive")
                    || combined.contains("Cannot open the file as")
                    || combined.contains("Is not archive")
                    || combined.contains("Headers Error")
                    || combined.contains("Unexpected end"))
            {
                return Ok(super::ProbeOutcome::Corrupt);
            }
        }
    }

    // 2. Try each candidate password via stdin (never on argv)
    if candidates.is_empty() {
        return Ok(if requires_password {
            super::ProbeOutcome::WrongPassword
        } else {
            super::ProbeOutcome::Corrupt
        });
    }

    let mut saw_status_verdict = false;
    let mut last_error: Option<String> = None;
    for (candidate_secret, candidate_id) in candidates {
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }

        let mut cmd = configure_7z_cmd(&bin);
        if header_encrypted {
            cmd.arg("l").arg("-slt").arg("-sccUTF-8");
        } else {
            cmd.arg("t").arg("-y").arg("-sccUTF-8");
        }
        push_password(&mut cmd, Some(candidate_secret));
        cmd.arg("--").arg(archive_path);
        if let Some(target) = &target_file {
            cmd.arg(target);
        }

        match run_7z_guarded(cmd, Some(candidate_secret), None, CLI_PROBE_TIMEOUT, cancel).await {
            Ok(output) if output.status.success() => {
                return Ok(super::ProbeOutcome::Success(
                    candidate_secret.clone(),
                    candidate_id.clone(),
                ));
            }
            Ok(_) => saw_status_verdict = true,
            Err(e) if e == "Cancelled" => return Err("Cancelled".into()),
            Err(error) => last_error = Some(error),
        }
    }

    // Not one attempt produced an exit-status verdict: the run was broken
    // (timeout/spawn), not the passwords. Never report WrongPassword here —
    // a slow archive would otherwise be mislabelled as a wrong password.
    if !saw_status_verdict {
        return Err(last_error.unwrap_or_else(|| "ProbeFailed".into()));
    }

    Ok(super::ProbeOutcome::WrongPassword)
}

fn is_safe_filter_pattern(pattern: &str) -> bool {
    !pattern.is_empty()
        && !pattern.contains('!')
        && !pattern.starts_with('@')
        && !pattern.chars().any(char::is_control)
}

/// Translates settings wildcard rules into 7-Zip switches so the CLI engine
/// honours the same include/exclude contract as the native ZIP engine.
/// Bare names are applied recursively (matching `matches_exclusion`'s
/// basename semantics); patterns containing a separator stay path-anchored.
fn archive_filter_args(exclusions: &[String], inclusions: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    for raw in exclusions {
        let pattern = raw.trim();
        if !is_safe_filter_pattern(pattern) {
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
        if !is_safe_filter_pattern(pattern) {
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

/// Computes a stable compressed-byte progress denominator from the primary
/// archive file and any registered volume files.
fn estimate_archive_total_bytes(request: &WorkerRequest) -> u64 {
    let mut seen = std::collections::HashSet::new();
    let mut total = 0u64;
    for path_str in std::iter::once(&request.archive_path).chain(request.allowed_volumes.iter()) {
        let p = Path::new(path_str);
        if seen.insert(p.to_path_buf())
            && let Ok(meta) = std::fs::metadata(p)
        {
            total = total.saturating_add(meta.len());
        }
    }
    total.max(100)
}

pub async fn extract_cli(
    request: &WorkerRequest,
    destination: &Path,
    progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<FileRecord>, String> {
    if cancel.load(Ordering::Acquire) {
        return Err("Cancelled".into());
    }

    let bin = find_7z_binary().ok_or_else(|| "7zNotFound".to_string())?;
    let password = request.password.as_deref();

    let mut cmd = configure_7z_cmd(&bin);
    cmd.arg("x")
        .arg(format!("-o{}", destination.display()))
        .arg("-y")
        .arg("-bsp1")
        .arg("-sccUTF-8")
        .arg("-snl-")
        .arg("-snh-");
    for filter in archive_filter_args(&request.exclusions, &request.inclusions) {
        cmd.arg(filter);
    }
    push_password(&mut cmd, password);
    cmd.arg("--");
    cmd.arg(&request.archive_path);

    // Refuse to start when the volume is already below the safety margin: 7-Zip
    // would only fail halfway through and leave a partial tree behind.
    if request.min_free_bytes > 0
        && let Ok(free) = crate::paths::free_bytes(destination)
        && free < request.min_free_bytes
    {
        return Err("ResourceLimit: 目标磁盘剩余空间低于「保留空闲空间」下限，未开始解压；请清理磁盘或调低该设置".into());
    }
    let volume = Some((destination, request.min_free_bytes));

    let total_bytes = estimate_archive_total_bytes(request);
    let started = std::time::Instant::now();
    let last_pct = Arc::new(AtomicU8::new(u8::MAX));
    let last_emit_ms = Arc::new(AtomicU64::new(0));
    let req_snapshot = request.clone();
    let progress_cb = Arc::clone(&progress);
    let progress_sink: ProgressSink = Arc::new(move |pct: u8| {
        let prev = last_pct.load(Ordering::Relaxed);
        if prev != u8::MAX && pct <= prev {
            return;
        }
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let prev_ms = last_emit_ms.load(Ordering::Relaxed);
        let due = prev == u8::MAX || pct == 100 || elapsed_ms >= prev_ms.saturating_add(100);
        if !due {
            return;
        }
        last_pct.store(pct, Ordering::Relaxed);
        last_emit_ms.store(elapsed_ms, Ordering::Relaxed);
        let mut ev = super::make_event(&req_snapshot, "progress");
        ev.total_bytes = total_bytes;
        ev.total_bytes_known = true;
        ev.completed_bytes = ((total_bytes as u128 * u128::from(pct)) / 100) as u64;
        progress_cb(&ev);
    });

    let output = run_7z_guarded_with_progress(
        cmd,
        password,
        volume,
        CLI_DEFAULT_TIMEOUT,
        &cancel,
        Some(progress_sink),
    )
    .await?;
    let had_warning_or_error = !output.status.success();
    if had_warning_or_error {
        let err_str = super::oem_codepage::decode_oem_text(&output.stderr);
        let out_str = super::oem_codepage::decode_oem_text(&output.stdout);
        let combined = format!("{err_str}\n{out_str}");
        if let Some(failure) = classify_cli_failure(&combined) {
            return Err(match failure {
                CliFailure::Password => "PasswordMismatchOrCorrupt",
                CliFailure::MissingVolume => "MissingVolume",
                CliFailure::Corrupt => "Corrupt",
            }
            .into());
        }
        if !request.salvage_corrupted {
            return Err("Corrupt".into());
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

        if rel.starts_with(".openunpack-") {
            continue;
        }

        // Use symlink_metadata so dangling or inaccessible links don't trigger error
        let symlink_meta = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(_) => {
                items_to_delete.push((path.to_path_buf(), false));
                continue;
            }
        };

        #[cfg(windows)]
        let is_unsupported_link =
            (symlink_meta.file_attributes() & 0x400 != 0) || symlink_meta.file_type().is_symlink();
        #[cfg(not(windows))]
        let is_unsupported_link = symlink_meta.file_type().is_symlink();

        if is_unsupported_link {
            // Unsupported link (symlink, junction, reparse point) - safely remove and skip
            items_to_delete.push((path.to_path_buf(), symlink_meta.is_dir()));
            continue;
        }

        // Check if relative path conforms to safe paths contract
        if crate::paths::relative(&rel).is_err() {
            // Unsupported path segment (reserved Windows names, illegal characters, etc.)
            items_to_delete.push((path.to_path_buf(), symlink_meta.is_dir()));
            continue;
        }

        let is_dir = symlink_meta.is_dir();

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

        let size = if is_dir { 0 } else { symlink_meta.len() };

        let sha256 = if is_dir {
            String::new()
        } else {
            let mut file = File::open(path).map_err(|e| format!("IoError: {e}"))?;
            let mut hasher = Sha256::new();
            let mut buf = vec![0u8; 32 * 1024];
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
        return Err("Corrupt: 无有效文件可解压".into());
    }

    // 7-Zip cannot be told to stop at a byte budget, so the budget is asserted
    // once the tree is known. The workspace owner performs the actual cleanup:
    // deleting here would bypass its ownership manifest.
    if request.max_entries > 0 && extracted_records.len() as u64 > request.max_entries {
        return Err("ResourceLimit: Exceeds maxEntries".into());
    }
    if request.max_written_bytes > 0 {
        let written: u64 = extracted_records
            .iter()
            .filter(|record| !record.is_directory)
            .map(|record| record.size)
            .fold(0u64, u64::saturating_add);
        if written > request.max_written_bytes {
            return Err("ResourceLimit: Exceeds maxWrittenBytes".into());
        }
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
    if cancel.load(Ordering::Acquire) {
        return Err("Cancelled".into());
    }

    let bin = find_7z_binary().ok_or_else(|| "7zNotFound".to_string())?;

    let mut cmd = configure_7z_cmd(&bin);
    cmd.arg(if flatten { "e" } else { "x" })
        .arg(format!("-o{}", destination.display()))
        .arg("-y")
        .arg("-sccUTF-8")
        .arg("-snl-")
        .arg("-snh-");

    push_password(&mut cmd, password);
    cmd.arg("--");
    cmd.arg(archive_path);

    for entry in entry_paths {
        let trimmed = entry.trim();
        if !trimmed.is_empty() {
            cmd.arg(trimmed);
        }
    }

    // stdout carries only the progress meter here; stderr is drained by a task
    // below. An undrained pipe blocks the child once the ~64 KiB buffer fills,
    // which used to hang this wait loop forever.
    cmd.stdout(Stdio::null()).stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    if password.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }

    let mut child = cmd.spawn().map_err(|e| format!("7zSpawnError: {e}"))?;
    let _job = ProcessJob::attach(&child);
    let stderr_pipe = child.stderr.take();
    let stderr_drain = tokio::spawn(async move {
        let Some(mut pipe) = stderr_pipe else {
            return Vec::new();
        };
        use tokio::io::AsyncReadExt;
        let mut collected = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match pipe.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    // Keep a bounded tail: enough to classify the failure,
                    // never enough to grow with a hostile archive.
                    let keep = (STDERR_CAPTURE_LIMIT - collected.len()).min(read);
                    collected.extend_from_slice(&chunk[..keep]);
                }
            }
        }
        collected
    });
    if let Some(pwd) = password
        && let Some(mut stdin) = child.stdin.take()
    {
        use tokio::io::AsyncWriteExt;
        let mut line = pwd.as_bytes().to_vec();
        line.push(b'\n');
        let _ = stdin.write_all(&line).await;
        let _ = stdin.shutdown().await;
    }

    let deadline = tokio::time::Instant::now() + CLI_DEFAULT_TIMEOUT;
    let status = loop {
        if cancel.load(Ordering::Acquire) {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stderr_drain.await;
            return Err("Cancelled".into());
        }
        match child.try_wait().map_err(|e| format!("7zWaitError: {e}"))? {
            Some(status) => break status,
            None => {
                if tokio::time::Instant::now() >= deadline {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    let _ = stderr_drain.await;
                    return Err("7zExecutionTimeout".into());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    };
    let stderr_bytes = stderr_drain.await.unwrap_or_default();

    if cancel.load(Ordering::Acquire) {
        return Err("Cancelled".into());
    }

    if !status.success() {
        let stderr_text = super::oem_codepage::decode_oem_text(&stderr_bytes);
        return Err(match classify_cli_failure(&stderr_text) {
            Some(CliFailure::Password) => "PasswordMismatchOrCorrupt",
            Some(CliFailure::MissingVolume) => "MissingVolume",
            Some(CliFailure::Corrupt) => "Corrupt",
            None => "ExtractionFailed: 7-Zip selective extraction failed",
        }
        .into());
    }

    let mut extracted_paths = Vec::new();
    let mut unsafe_links_to_delete = Vec::new();
    for entry in walkdir::WalkDir::new(destination)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path == destination {
            continue;
        }
        let Ok(symlink_meta) = std::fs::symlink_metadata(path) else {
            continue;
        };
        #[cfg(windows)]
        let is_unsupported_link =
            (symlink_meta.file_attributes() & 0x400 != 0) || symlink_meta.file_type().is_symlink();
        #[cfg(not(windows))]
        let is_unsupported_link = symlink_meta.file_type().is_symlink();

        if is_unsupported_link {
            unsafe_links_to_delete.push((path.to_path_buf(), symlink_meta.is_dir()));
            continue;
        }

        if let Ok(rel) = path.strip_prefix(destination) {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if crate::paths::relative(&rel_str).is_err() {
                unsafe_links_to_delete.push((path.to_path_buf(), symlink_meta.is_dir()));
                continue;
            }
        }

        if symlink_meta.is_file() {
            extracted_paths.push(path.to_string_lossy().into_owned());
        }
    }

    unsafe_links_to_delete.sort_by_key(|item| std::cmp::Reverse(item.0.as_os_str().len()));
    for (to_del, is_dir) in unsafe_links_to_delete {
        if is_dir {
            let _ = std::fs::remove_dir(&to_del);
        } else {
            let _ = std::fs::remove_file(&to_del);
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

    #[test]
    fn test_find_7z() {
        let b = super::find_7z_binary();
        assert!(b.is_some(), "bundled 7z must be found");
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
                seen_progress_clone
                    .lock()
                    .unwrap()
                    .push((ev.completed_bytes, ev.total_bytes, ev.total_bytes_known));
            }
        });
        let extract_res = extract_cli(&req, &dest_dir, progress, cancel_arc).await;
        assert!(
            extract_res.is_ok(),
            "extract_cli failed: {:?}",
            extract_res.err()
        );
        let events = seen_progress.lock().unwrap();
        assert!(
            !events.is_empty(),
            "extract_cli -bsp1 should emit at least one progress event"
        );
        assert!(events.iter().all(|(done, total, known)| *known && *done <= *total && *total > 0));
        drop(events);
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
