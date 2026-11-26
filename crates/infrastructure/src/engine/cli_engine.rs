//! 基于 7-Zip CLI 的压缩包检视、完整性探测与全量/按需解压引擎。

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
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::process::Command;

mod probe;
mod process;

#[cfg(test)]
use probe::ListingParser;
pub use probe::{inspect_cli, probe_cli};
#[cfg(test)]
use process::parse_7z_progress_percent;
use process::*;
pub(crate) use process::{configure_7z_cmd, push_password};
pub use process::{find_7z_binary, has_7z_cli};

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
mod tests;
