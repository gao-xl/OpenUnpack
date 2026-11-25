use super::path_safety::{matches_exclusion, matches_inclusion, validate_relative_path};
use openunpack_protocol::worker::{FileRecord, WorkerEvent, WorkerRequest};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufWriter, Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use zip::ZipArchive;

pub fn inspect_zip(
    archive_path: &Path,
    _password: Option<&str>,
    max_entries: u64,
    max_total_bytes: u64,
) -> Result<Vec<FileRecord>, String> {
    let file = File::open(archive_path).map_err(|e| format!("IoError: {e}"))?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("CorruptArchive: {e}"))?;

    let count = archive.len();
    if max_entries > 0 && count as u64 > max_entries {
        return Err("ResourceLimit: Exceeds maxEntries".into());
    }

    let mut records = Vec::with_capacity(count);
    let mut total_bytes: u64 = 0;

    for i in 0..count {
        let entry = archive
            .by_index_raw(i)
            .map_err(|e| format!("ZipReadError: {e}"))?;
        // zip 9 returns a Result: legacy code-page names are not valid UTF-8, so
        // decode via OEM/ANSI transcoding adapter instead of lossy UTF-8 replacement.
        let name = match entry.name() {
            Ok(name) => name.to_string(),
            Err(_) => super::oem_codepage::decode_oem_text(entry.name_raw()),
        };
        let is_dir = entry.is_dir();
        let size = entry.size();
        let is_encrypted = entry.encrypted();

        total_bytes = total_bytes.saturating_add(size);
        if max_total_bytes > 0 && total_bytes > max_total_bytes {
            return Err("ResourceLimit: Exceeds maxTotalBytes".into());
        }

        // Skip Unix symbolic links (S_IFLNK = 0o120000)
        if entry
            .unix_mode()
            .is_some_and(|m| (m & 0o170000) == 0o120000)
            || ((entry.external_attributes() >> 16) & 0o170000 == 0o120000)
        {
            continue;
        }

        // Clean relative path
        let relative_path = match validate_relative_path(&name) {
            Ok(p) => p,
            Err(_) if is_dir => continue,
            Err(e) => return Err(e),
        };

        records.push(FileRecord {
            relative_path,
            size,
            size_known: true,
            sha256: String::new(),
            is_directory: is_dir,
            is_encrypted,
            is_corrupted: false,
        });
    }

    Ok(records)
}

pub fn probe_zip(
    archive_path: &Path,
    candidates: &[(String, Option<String>)],
    cancel: &AtomicBool,
) -> Result<super::ProbeOutcome, String> {
    let file = File::open(archive_path).map_err(|e| format!("IoError: {e}"))?;
    let mut archive = match ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => return Err(format!("CorruptArchive: {e}")),
    };

    // Collect encrypted file entries and sort by uncompressed size (preferring
    // non-empty entries first) so we can verify full stream decompression + CRC32
    // at EOF with minimal I/O while defeating the 1/256 ZipCrypto header collision.
    let mut encrypted_entries: Vec<(usize, u64)> = Vec::new();
    for i in 0..archive.len() {
        match archive.by_index_raw(i) {
            Ok(entry) => {
                if entry.encrypted() && !entry.is_dir() {
                    encrypted_entries.push((i, entry.size()));
                }
            }
            Err(e) => return Err(format!("ZipReadError: {e}")),
        }
    }

    if encrypted_entries.is_empty() {
        // No encrypted entry found; no password needed
        return Ok(super::ProbeOutcome::NoPasswordNeeded);
    }

    encrypted_entries.sort_by_key(|&(_, size)| (size == 0, size));
    let verify_indices: Vec<usize> = encrypted_entries
        .iter()
        .take(2)
        .map(|&(idx, _)| idx)
        .collect();

    let mut probe_buf = vec![0u8; 64 * 1024];
    'candidate_loop: for (candidate_secret, candidate_id) in candidates {
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }

        for &target_idx in &verify_indices {
            match archive.by_index_decrypt(target_idx, candidate_secret.as_bytes()) {
                Ok(mut reader) => loop {
                    if cancel.load(Ordering::Acquire) {
                        return Err("Cancelled".into());
                    }
                    match reader.read(&mut probe_buf) {
                        Ok(0) => break,
                        Ok(_) => continue,
                        Err(_) => continue 'candidate_loop,
                    }
                },
                Err(_) => continue 'candidate_loop,
            }
        }

        return Ok(super::ProbeOutcome::Success(
            candidate_secret.clone(),
            candidate_id.clone(),
        ));
    }

    Ok(super::ProbeOutcome::WrongPassword)
}

pub fn extract_zip(
    request: &WorkerRequest,
    destination: &Path,
    progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<FileRecord>, String> {
    let archive_path = Path::new(&request.archive_path);
    let file = File::open(archive_path).map_err(|e| format!("IoError: {e}"))?;
    let archive = ZipArchive::new(file).map_err(|e| format!("CorruptArchive: {e}"))?;

    let dest = destination.to_path_buf();
    extract_zip_with_sink(archive, request, progress, cancel, move |rel, is_dir| {
        let target_path = dest.join(rel);
        if is_dir {
            fs::create_dir_all(&target_path)?;
            Ok(None)
        } else {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let file = File::create(&target_path)?;
            Ok(Some(Box::new(BufWriter::with_capacity(64 * 1024, file))))
        }
    })
}

pub fn extract_zip_with_sink<R: Read + std::io::Seek, F>(
    mut archive: ZipArchive<R>,
    request: &WorkerRequest,
    progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
    mut sink: F,
) -> Result<Vec<FileRecord>, String>
where
    F: FnMut(&str, bool) -> std::io::Result<Option<Box<dyn Write>>>,
{
    let mut extracted_records = Vec::new();
    let count = archive.len();
    let mut completed_bytes: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut last_progress = Instant::now();

    // Calculate total bytes
    for i in 0..count {
        if let Ok(entry) = archive.by_index_raw(i)
            && !entry.is_dir()
        {
            total_bytes = total_bytes.saturating_add(entry.size());
        }
    }

    let max_entries = if request.max_entries > 0 {
        request.max_entries
    } else {
        1_000_000
    };
    let max_written = if request.max_written_bytes > 0 {
        request.max_written_bytes
    } else {
        u64::MAX
    };
    let mut verified_encrypted_password = false;

    for i in 0..count {
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }

        if extracted_records.len() as u64 >= max_entries {
            return Err("ResourceLimit: Exceeds maxEntries".into());
        }

        let is_dir;
        let size;
        let relative_path;
        let is_encrypted;

        {
            let raw_entry = archive
                .by_index_raw(i)
                .map_err(|e| format!("ZipError: {e}"))?;
            let name = match raw_entry.name() {
                Ok(name) => name.to_string(),
                Err(_) => super::oem_codepage::decode_oem_text(raw_entry.name_raw()),
            };
            is_dir = raw_entry.is_dir();
            size = raw_entry.size();
            is_encrypted = raw_entry.encrypted();

            let is_symlink = raw_entry
                .unix_mode()
                .is_some_and(|m| (m & 0o170000) == 0o120000)
                || ((raw_entry.external_attributes() >> 16) & 0o170000 == 0o120000);
            if is_symlink {
                continue;
            }

            // Early check exclusions using raw name
            if matches_exclusion(&name, &request.exclusions) {
                continue;
            }

            match validate_relative_path(&name) {
                Ok(p) => relative_path = p,
                Err(_) if is_dir => continue,
                Err(e) => return Err(e),
            };
        }

        // Check exclusions
        if matches_exclusion(&relative_path, &request.exclusions) {
            continue;
        }

        // Check inclusions
        if !matches_inclusion(&relative_path, is_dir, &request.inclusions) {
            continue;
        }

        if is_dir {
            sink(&relative_path, true).map_err(|e| format!("SinkError: {e}"))?;
            extracted_records.push(FileRecord {
                relative_path,
                size: 0,
                size_known: true,
                sha256: String::new(),
                is_directory: true,
                is_encrypted,
                is_corrupted: false,
            });
            continue;
        }

        let mut entry_corrupted = false;

        // Open entry with password if requested
        let entry_reader_res = if is_encrypted {
            let pwd = request
                .password
                .as_deref()
                .ok_or_else(|| "NeedsPassword".to_string())?;
            archive
                .by_index_decrypt(i, pwd.as_bytes())
                .map_err(|_| "PasswordMismatchOrCorrupt".to_string())
        } else {
            archive
                .by_index(i)
                .map_err(|e| format!("ZipEntryError: {e}"))
        };

        let entry_reader = match entry_reader_res {
            Ok(r) => Some(r),
            Err(e) => {
                // Never treat a missing or rejected password on an encrypted entry
                // as a salvageable file unless the password has already been proven
                // valid on an earlier encrypted entry in this archive.
                if request.salvage_corrupted && (!is_encrypted || verified_encrypted_password) {
                    entry_corrupted = true;
                    None
                } else {
                    return Err(e);
                }
            }
        };

        let mut actual_written = 0u64;
        let mut sha256_hex = String::new();

        if let Some(mut reader) = entry_reader {
            let mut out_file = sink(&relative_path, false)
                .map_err(|e| format!("SinkError: {e}"))?
                .ok_or_else(|| "SinkDidNotProvideWriter".to_string())?;
            let mut hasher = Sha256::new();
            let mut buf = [0u8; 64 * 1024];

            loop {
                if cancel.load(Ordering::Acquire) {
                    return Err("Cancelled".into());
                }
                let read_bytes = match reader.read(&mut buf) {
                    Ok(n) => n,
                    Err(e) => {
                        if is_encrypted && !verified_encrypted_password {
                            return Err("PasswordMismatchOrCorrupt".into());
                        }
                        if request.salvage_corrupted {
                            entry_corrupted = true;
                            break;
                        } else {
                            return Err(format!("ZipReadError: {e}"));
                        }
                    }
                };
                if read_bytes == 0 {
                    if is_encrypted {
                        verified_encrypted_password = true;
                    }
                    break;
                }
                let next_completed = completed_bytes.saturating_add(read_bytes as u64);
                if next_completed > max_written {
                    return Err("ResourceLimit: Exceeds maxWrittenBytes".into());
                }

                out_file
                    .write_all(&buf[..read_bytes])
                    .map_err(|e| format!("IoError: {e}"))?;
                hasher.update(&buf[..read_bytes]);
                completed_bytes = next_completed;
                actual_written = actual_written.saturating_add(read_bytes as u64);

                if last_progress.elapsed() >= Duration::from_millis(50) {
                    let mut prog_ev = super::make_event(request, "progress");
                    prog_ev.completed_bytes = completed_bytes;
                    prog_ev.total_bytes = total_bytes;
                    prog_ev.total_bytes_known = true;
                    progress(&prog_ev);
                    last_progress = Instant::now();
                }
            }
            out_file.flush().map_err(|e| format!("IoError: {e}"))?;
            sha256_hex = hex::encode(hasher.finalize());
        }

        extracted_records.push(FileRecord {
            relative_path,
            size: if entry_corrupted {
                actual_written
            } else {
                size
            },
            size_known: true,
            sha256: sha256_hex,
            is_directory: false,
            is_encrypted,
            is_corrupted: entry_corrupted,
        });
    }

    // When the archive has 0 entries (empty archive) or all entries were excluded/filtered,
    // extraction completes with an empty file record list without misreporting CorruptArchive.
    // If the archive actually had non-directory entries but every one failed/was corrupted and
    // salvage was not requested, the loop has already returned Err earlier.
    Ok(extracted_records)
}

/// Selectively extracts designated entries from a ZIP archive.
pub fn extract_zip_selective(
    archive_path: &Path,
    entry_paths: &[String],
    destination: &Path,
    password: Option<&str>,
    flatten: bool,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<String>, String> {
    let file = File::open(archive_path).map_err(|e| format!("IoError: {e}"))?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("CorruptArchive: {e}"))?;

    let count = archive.len();
    let requested: std::collections::HashSet<String> = entry_paths
        .iter()
        .map(|p| p.replace('\\', "/").trim_start_matches('/').to_string())
        .collect();

    let mut extracted_paths = Vec::new();

    for i in 0..count {
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }

        let is_dir;
        let relative_path;
        let is_encrypted;

        {
            let raw_entry = archive
                .by_index_raw(i)
                .map_err(|e| format!("ZipError: {e}"))?;
            let name = match raw_entry.name() {
                Ok(name) => name.to_string(),
                Err(_) => super::oem_codepage::decode_oem_text(raw_entry.name_raw()),
            };
            is_dir = raw_entry.is_dir();
            is_encrypted = raw_entry.encrypted();

            // Skip Unix symbolic links (S_IFLNK = 0o120000)
            if raw_entry
                .unix_mode()
                .is_some_and(|m| (m & 0o170000) == 0o120000)
                || ((raw_entry.external_attributes() >> 16) & 0o170000 == 0o120000)
            {
                continue;
            }

            match validate_relative_path(&name) {
                Ok(p) => relative_path = p,
                Err(_) if is_dir => continue,
                Err(e) => return Err(e),
            };
        }

        let is_target = requested.contains(&relative_path)
            || requested
                .iter()
                .any(|req| relative_path.starts_with(&format!("{req}/")));

        if !is_target {
            continue;
        }

        let target_relative = if flatten && !is_dir {
            relative_path
                .rsplit('/')
                .next()
                .unwrap_or(&relative_path)
                .to_string()
        } else {
            relative_path.clone()
        };

        let mut out_path = destination.join(&target_relative);

        if is_dir {
            fs::create_dir_all(&out_path).map_err(|e| format!("MkdirError: {e}"))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("MkdirError: {e}"))?;
        }

        if flatten && out_path.exists() {
            let parent = out_path.parent().unwrap_or(destination);
            let stem = out_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("extracted");
            let ext = out_path.extension().and_then(|s| s.to_str());
            let mut found = false;
            for suffix in 1u32..=10_000 {
                let candidate_name = match ext {
                    Some(e) if !e.is_empty() => format!("{stem} ({suffix}).{e}"),
                    _ => format!("{stem} ({suffix})"),
                };
                let candidate_path = parent.join(candidate_name);
                if !candidate_path.exists() {
                    out_path = candidate_path;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(
                    "DestinationCollisionExhausted: 重名冲突次数超过上限，已中止防止覆盖".into(),
                );
            }
        }

        let mut entry_reader = if is_encrypted {
            if let Some(pwd) = password {
                archive
                    .by_index_decrypt(i, pwd.as_bytes())
                    .map_err(|e| format!("PasswordMismatch: {e}"))?
            } else {
                return Err("NeedsPassword".into());
            }
        } else {
            archive
                .by_index(i)
                .map_err(|e| format!("ZipReadError: {e}"))?
        };

        let mut outfile = File::create(&out_path).map_err(|e| format!("IoError: {e}"))?;
        if let Err(e) = std::io::copy(&mut entry_reader, &mut outfile).and_then(|_| outfile.flush())
        {
            drop(outfile);
            let _ = fs::remove_file(&out_path);
            return Err(format!("WriteError: {e}"));
        }
        extracted_paths.push(out_path.to_string_lossy().into_owned());
    }

    Ok(extracted_paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    #[test]
    fn test_zip_salvage_corrupted_partial_recovery() {
        let mut buffer = Cursor::new(Vec::new());

        // 1. Create a zip with two files
        {
            let mut zip = ZipWriter::new(&mut buffer);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

            zip.start_file("healthy.txt", options).unwrap();
            zip.write_all(b"Healthy data here.").unwrap();

            zip.start_file("broken.txt", options).unwrap();
            zip.write_all(b"Damaged content here.").unwrap();

            zip.finish().unwrap();
        }

        // 2. Corrupt "Damaged" payload bytes in the zip buffer
        let mut raw = buffer.into_inner();
        if let Some(pos) = raw.windows(7).position(|w| w == b"Damaged") {
            raw[pos] = b'X';
            raw[pos + 1] = b'Y';
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(|_ev: &WorkerEvent| {});

        // Case A: normal mode (salvage_corrupted = false) -> MUST fail
        {
            let mut req = WorkerRequest::default();
            req.salvage_corrupted = false;
            let archive = ZipArchive::new(Cursor::new(raw.clone())).unwrap();

            let res = extract_zip_with_sink(
                archive,
                &req,
                progress.clone(),
                cancel.clone(),
                |_name, is_dir| {
                    if is_dir {
                        Ok(None)
                    } else {
                        Ok(Some(Box::new(std::io::sink())))
                    }
                },
            );
            assert!(
                res.is_err(),
                "Non-salvage mode must return error on CRC corruption"
            );
        }

        // Case B: salvage mode (salvage_corrupted = true) -> MUST succeed and recover healthy file
        {
            let mut req = WorkerRequest::default();
            req.salvage_corrupted = true;
            let archive = ZipArchive::new(Cursor::new(raw.clone())).unwrap();

            let res = extract_zip_with_sink(
                archive,
                &req,
                progress.clone(),
                cancel.clone(),
                |_name, is_dir| {
                    if is_dir {
                        Ok(None)
                    } else {
                        Ok(Some(Box::new(std::io::sink())))
                    }
                },
            );
            assert!(res.is_ok(), "Salvage mode must succeed");
            let files = res.unwrap();
            assert_eq!(files.len(), 2);
            let healthy = files
                .iter()
                .find(|f| f.relative_path == "healthy.txt")
                .unwrap();
            assert!(!healthy.is_corrupted);

            let broken = files
                .iter()
                .find(|f| f.relative_path == "broken.txt")
                .unwrap();
            assert!(broken.is_corrupted);
        }
    }
}
