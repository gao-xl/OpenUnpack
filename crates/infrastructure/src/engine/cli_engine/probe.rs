//! 7-Zip 列表流式解析（ListingParser）、目录检视与完整性探测。

use super::*;

/// was consulted. Parsing line by line keeps the resident set proportional to
/// `max_entries` instead of to the archive's claim.
pub(super) struct ListingParser {
    pub(super) files: Vec<FileRecord>,
    pub(super) cur_path: String,
    pub(super) cur_size: u64,
    pub(super) cur_is_dir: bool,
    pub(super) cur_is_encrypted: bool,
    pub(super) max_entries: u64,
    pub(super) max_total_bytes: u64,
    pub(super) total_bytes: u64,
}

impl ListingParser {
    pub(super) fn new(max_entries: u64, max_total_bytes: u64) -> Self {
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

    pub(super) fn push_line(&mut self, line: &str) -> Result<(), String> {
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

    pub(super) fn flush(&mut self) -> Result<(), String> {
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

    pub(super) fn finish(mut self) -> Result<Vec<FileRecord>, String> {
        self.flush()?;
        Ok(self.files)
    }
}

/// Drains a child pipe into a bounded buffer.
///
/// An unread pipe blocks the child once the OS buffer fills (~64 KiB on
/// Windows), so every piped stream must be consumed even when its content is
/// only kept for failure classification.
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

pub(super) async fn inspect_cli_once(
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
                    let decoded_line = super::super::oem_codepage::decode_oem_text(&line);
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
        let stderr_text = super::super::oem_codepage::decode_oem_text(&stderr_bytes);
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
) -> Result<super::super::ProbeOutcome, String> {
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
                        let err_str =
                            super::super::oem_codepage::decode_oem_text(&sample_out.stderr);
                        let out_str =
                            super::super::oem_codepage::decode_oem_text(&sample_out.stdout);
                        let combined = format!("{err_str}\n{out_str}");
                        if combined.contains("CRC Failed")
                            || combined.contains("Data Error")
                            || combined.contains("Unavailable data")
                            || combined.contains("Headers Error")
                        {
                            return Ok(super::super::ProbeOutcome::Corrupt);
                        }
                    }
                }
                return Ok(super::super::ProbeOutcome::NoPasswordNeeded);
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
            return Ok(super::super::ProbeOutcome::MissingVolume);
        }
        Err(e) if e == "Corrupt" => {
            return Ok(super::super::ProbeOutcome::Corrupt);
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
                return Ok(super::super::ProbeOutcome::NoPasswordNeeded);
            }
            let err_str = super::super::oem_codepage::decode_oem_text(&null_out.stderr);
            let out_str = super::super::oem_codepage::decode_oem_text(&null_out.stdout);
            let combined = format!("{err_str}\n{out_str}");
            if combined.contains("Missing volume") || combined.contains("Cannot find volume") {
                return Ok(super::super::ProbeOutcome::MissingVolume);
            }
            if combined.contains("Unsupported Method") {
                return Ok(super::super::ProbeOutcome::Unsupported);
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
                return Ok(super::super::ProbeOutcome::Corrupt);
            }
        }
    }

    // 2. Try each candidate password via stdin (never on argv)
    if candidates.is_empty() {
        return Ok(if requires_password {
            super::super::ProbeOutcome::WrongPassword
        } else {
            super::super::ProbeOutcome::Corrupt
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
                return Ok(super::super::ProbeOutcome::Success(
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

    Ok(super::super::ProbeOutcome::WrongPassword)
}
