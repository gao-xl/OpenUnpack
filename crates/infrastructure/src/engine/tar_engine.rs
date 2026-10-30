use super::path_safety::{matches_exclusion, matches_inclusion, validate_relative_path};
use openunpack_protocol::worker::{FileRecord, WorkerEvent, WorkerRequest};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub fn is_tar_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "tar" {
        return true;
    }
    // Check for ustar at offset 257 in first 512 bytes
    if let Ok(mut file) = File::open(path) {
        let mut header = [0u8; 512];
        if file.read_exact(&mut header).is_ok()
            && (&header[257..262] == b"ustar" || has_valid_tar_checksum(&header))
        {
            return true;
        }
    }
    false
}

fn parse_octal(bytes: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(bytes).ok()?;
    let trimmed = s.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if trimmed.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(trimmed, 8).ok()
}

fn has_valid_tar_checksum(header: &[u8; 512]) -> bool {
    let recorded_cs = match parse_octal(&header[148..156]) {
        Some(cs) => cs as u32,
        None => return false,
    };
    if recorded_cs == 0 {
        return false;
    }
    let mut unsigned_sum: u32 = 0;
    for (i, &b) in header.iter().enumerate() {
        if (148..156).contains(&i) {
            unsigned_sum += u32::from(b' ');
        } else {
            unsigned_sum += u32::from(b);
        }
    }
    unsigned_sum == recorded_cs
}

fn parse_pax_path(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    for line in text.lines() {
        if let Some(space_idx) = line.find(' ') {
            let kv = &line[space_idx + 1..];
            if let Some(eq_idx) = kv.find('=') {
                let key = &kv[..eq_idx];
                let val = &kv[eq_idx + 1..];
                if key == "path" {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

pub fn inspect_tar(
    archive_path: &Path,
    max_entries: u64,
    max_total_bytes: u64,
) -> Result<Vec<FileRecord>, String> {
    let mut file = File::open(archive_path).map_err(|e| format!("IoError: {e}"))?;
    let mut records = Vec::new();
    let mut total_bytes: u64 = 0;
    let mut header = [0u8; 512];
    let mut next_long_name: Option<String> = None;

    loop {
        let read = file
            .read(&mut header)
            .map_err(|e| format!("IoError: {e}"))?;
        if read == 0 {
            break;
        }
        if read < 512 {
            return Err("CorruptArchive: Truncated tar header".into());
        }
        if header.iter().all(|&b| b == 0) {
            // Potential end of archive block
            break;
        }
        if !has_valid_tar_checksum(&header) {
            return Err("CorruptArchive: Invalid tar header checksum".into());
        }

        let size = parse_octal(&header[124..136]).unwrap_or(0);
        let type_flag = header[156];
        let padded_size = size.div_ceil(512) * 512;
        let seek_offset = i64::try_from(padded_size)
            .map_err(|_| "CorruptArchive: Overflowing tar entry size".to_string())?;

        if type_flag == b'L' {
            if size > 64 * 1024 {
                return Err("ResourceLimit: Excessive long path in tar header".into());
            }
            let mut name_buf = vec![0u8; size as usize];
            file.read_exact(&mut name_buf)
                .map_err(|e| format!("IoError: {e}"))?;
            let trimmed = name_buf.strip_suffix(&[0]).unwrap_or(&name_buf);
            next_long_name = Some(String::from_utf8_lossy(trimmed).into_owned());
            let padding = (512 - (size % 512)) % 512;
            if padding > 0 {
                file.seek(SeekFrom::Current(padding as i64))
                    .map_err(|e| format!("IoError: {e}"))?;
            }
            continue;
        }

        if type_flag == b'x' {
            if size > 64 * 1024 {
                return Err("ResourceLimit: Excessive pax header in tar".into());
            }
            let mut pax_buf = vec![0u8; size as usize];
            file.read_exact(&mut pax_buf)
                .map_err(|e| format!("IoError: {e}"))?;
            if let Some(p) = parse_pax_path(&pax_buf) {
                next_long_name = Some(p);
            }
            let padding = (512 - (size % 512)) % 512;
            if padding > 0 {
                file.seek(SeekFrom::Current(padding as i64))
                    .map_err(|e| format!("IoError: {e}"))?;
            }
            continue;
        }

        if type_flag == b'g' {
            file.seek(SeekFrom::Current(seek_offset))
                .map_err(|e| format!("IoError: {e}"))?;
            continue;
        }

        if matches!(type_flag, b'1' | b'2' | b'3' | b'4' | b'6') {
            file.seek(SeekFrom::Current(seek_offset))
                .map_err(|e| format!("IoError: {e}"))?;
            continue;
        }

        let name = if let Some(long_name) = next_long_name.take() {
            long_name
        } else {
            let name_end = header[..100].iter().position(|&b| b == 0).unwrap_or(100);
            String::from_utf8_lossy(&header[..name_end]).into_owned()
        };
        let is_dir = type_flag == b'5' || name.ends_with('/');

        total_bytes = total_bytes.saturating_add(size);
        if max_total_bytes > 0 && total_bytes > max_total_bytes {
            return Err("ResourceLimit: Exceeds maxTotalBytes".into());
        }
        if max_entries > 0 && records.len() as u64 >= max_entries {
            return Err("ResourceLimit: Exceeds maxEntries".into());
        }

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
            is_encrypted: false,
            is_corrupted: false,
        });

        // Skip payload plus 512-byte block padding
        file.seek(SeekFrom::Current(seek_offset))
            .map_err(|e| format!("IoError: {e}"))?;
    }

    Ok(records)
}

pub fn extract_tar(
    request: &WorkerRequest,
    destination: &Path,
    progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<(Vec<FileRecord>, bool), String> {
    let archive_path = Path::new(&request.archive_path);
    let mut file = File::open(archive_path).map_err(|e| format!("IoError: {e}"))?;
    let mut extracted_records = Vec::new();
    let mut header = [0u8; 512];
    let mut completed_bytes: u64 = 0;
    let mut is_salvaged = false;
    let mut zero_block_count = 0usize;
    let mut next_long_name: Option<String> = None;

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

    loop {
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }

        let read = file
            .read(&mut header)
            .map_err(|e| format!("IoError: {e}"))?;
        if read == 0 {
            break;
        }
        if read < 512 {
            if request.salvage_corrupted {
                is_salvaged = true;
                break;
            } else {
                return Err("CorruptArchive: Truncated tar header".into());
            }
        }

        if header.iter().all(|&b| b == 0) {
            zero_block_count += 1;
            if zero_block_count >= 2 {
                break;
            }
            continue;
        }

        if !has_valid_tar_checksum(&header) {
            if request.salvage_corrupted {
                is_salvaged = true;
                break;
            } else {
                return Err("CorruptArchive: Invalid tar header checksum".into());
            }
        }

        let size = parse_octal(&header[124..136]).unwrap_or(0);
        let type_flag = header[156];
        let padded_size = size.div_ceil(512) * 512;
        let seek_offset = i64::try_from(padded_size)
            .map_err(|_| "CorruptArchive: Overflowing tar entry size".to_string())?;

        if type_flag == b'L' {
            if size > 64 * 1024 {
                return Err("ResourceLimit: Excessive long path in tar header".into());
            }
            let mut name_buf = vec![0u8; size as usize];
            file.read_exact(&mut name_buf)
                .map_err(|e| format!("IoError: {e}"))?;
            let trimmed = name_buf.strip_suffix(&[0]).unwrap_or(&name_buf);
            next_long_name = Some(String::from_utf8_lossy(trimmed).into_owned());
            let padding = (512 - (size % 512)) % 512;
            if padding > 0 {
                file.seek(SeekFrom::Current(padding as i64))
                    .map_err(|e| format!("IoError: {e}"))?;
            }
            continue;
        }

        if type_flag == b'x' {
            if size > 64 * 1024 {
                return Err("ResourceLimit: Excessive pax header in tar".into());
            }
            let mut pax_buf = vec![0u8; size as usize];
            file.read_exact(&mut pax_buf)
                .map_err(|e| format!("IoError: {e}"))?;
            if let Some(p) = parse_pax_path(&pax_buf) {
                next_long_name = Some(p);
            }
            let padding = (512 - (size % 512)) % 512;
            if padding > 0 {
                file.seek(SeekFrom::Current(padding as i64))
                    .map_err(|e| format!("IoError: {e}"))?;
            }
            continue;
        }

        if type_flag == b'g' {
            file.seek(SeekFrom::Current(seek_offset))
                .map_err(|e| format!("IoError: {e}"))?;
            continue;
        }

        if matches!(type_flag, b'1' | b'2' | b'3' | b'4' | b'6') {
            file.seek(SeekFrom::Current(seek_offset))
                .map_err(|e| format!("IoError: {e}"))?;
            continue;
        }

        let name = if let Some(long_name) = next_long_name.take() {
            long_name
        } else {
            let name_end = header[..100].iter().position(|&b| b == 0).unwrap_or(100);
            String::from_utf8_lossy(&header[..name_end]).into_owned()
        };
        let is_dir = type_flag == b'5' || name.ends_with('/');

        let relative_path = match validate_relative_path(&name) {
            Ok(p) => p,
            Err(_) if is_dir => continue,
            Err(e) => return Err(e),
        };

        if matches_exclusion(&relative_path, &request.exclusions) {
            file.seek(SeekFrom::Current(seek_offset))
                .map_err(|e| format!("IoError: {e}"))?;
            continue;
        }

        if !matches_inclusion(&relative_path, is_dir, &request.inclusions) {
            file.seek(SeekFrom::Current(seek_offset))
                .map_err(|e| format!("IoError: {e}"))?;
            continue;
        }

        if extracted_records.len() as u64 >= max_entries {
            return Err("ResourceLimit: Exceeds maxEntries".into());
        }

        let target_path = destination.join(&relative_path);
        if is_dir {
            fs::create_dir_all(&target_path).map_err(|e| format!("IoError: {e}"))?;
            extracted_records.push(FileRecord {
                relative_path,
                size: 0,
                size_known: true,
                sha256: String::new(),
                is_directory: true,
                is_encrypted: false,
                is_corrupted: false,
            });
            continue;
        }

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("IoError: {e}"))?;
        }

        let out_file_raw = File::create(&target_path).map_err(|e| format!("IoError: {e}"))?;
        let mut out_file = BufWriter::with_capacity(64 * 1024, out_file_raw);
        let mut hasher = Sha256::new();
        let mut remaining = size;
        let mut buf = vec![0u8; 64 * 1024];
        let mut entry_corrupted = false;

        while remaining > 0 {
            if cancel.load(Ordering::Acquire) {
                drop(out_file);
                let _ = fs::remove_file(&target_path);
                return Err("Cancelled".into());
            }
            let to_read = (remaining as usize).min(buf.len());
            let n = match file.read(&mut buf[..to_read]) {
                Ok(0) => {
                    entry_corrupted = true;
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    if request.salvage_corrupted {
                        entry_corrupted = true;
                        break;
                    } else {
                        drop(out_file);
                        let _ = fs::remove_file(&target_path);
                        return Err(format!("IoError: {e}"));
                    }
                }
            };

            if completed_bytes.saturating_add(n as u64) > max_written {
                drop(out_file);
                let _ = fs::remove_file(&target_path);
                return Err("ResourceLimit: Exceeds maxWrittenBytes".into());
            }

            out_file
                .write_all(&buf[..n])
                .map_err(|e| format!("IoError: {e}"))?;
            hasher.update(&buf[..n]);
            remaining -= n as u64;
            completed_bytes += n as u64;

            let mut ev = super::make_event(request, "progress");
            ev.completed_bytes = completed_bytes;
            progress(&ev);
        }

        out_file.flush().map_err(|e| format!("IoError: {e}"))?;

        if entry_corrupted {
            if request.salvage_corrupted {
                is_salvaged = true;
                extracted_records.push(FileRecord {
                    relative_path,
                    size: size - remaining,
                    size_known: true,
                    sha256: hex::encode(hasher.finalize()),
                    is_directory: false,
                    is_encrypted: false,
                    is_corrupted: true,
                });
                break;
            } else {
                return Err("CorruptArchive: Truncated tar member data".into());
            }
        }

        let padding = (512 - (size % 512)) % 512;
        if padding > 0 {
            let mut pad_buf = [0u8; 512];
            let pad_read = file.read(&mut pad_buf[..padding as usize]).unwrap_or(0);
            if pad_read < padding as usize {
                if request.salvage_corrupted {
                    is_salvaged = true;
                } else {
                    return Err("CorruptArchive: Truncated tar block padding".into());
                }
            }
        }

        extracted_records.push(FileRecord {
            relative_path,
            size,
            size_known: true,
            sha256: hex::encode(hasher.finalize()),
            is_directory: false,
            is_encrypted: false,
            is_corrupted: false,
        });
    }

    // A valid tar must end with 2 zero blocks (1024 zero bytes)
    if zero_block_count < 2 {
        if request.salvage_corrupted {
            is_salvaged = true;
        } else {
            return Err("CorruptArchive: Missing tar end of archive marker".into());
        }
    }

    Ok((extracted_records, is_salvaged))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_tar_header(name: &[u8], size: u64, type_flag: u8) -> [u8; 512] {
        let mut header = [0u8; 512];
        let name_len = name.len().min(100);
        header[..name_len].copy_from_slice(&name[..name_len]);

        // Mode: 0644 (octal)
        header[100..108].copy_from_slice(b"0000644\0");
        // UID / GID
        header[108..116].copy_from_slice(b"0001000\0");
        header[116..124].copy_from_slice(b"0001000\0");
        // Size: 11 octal digits + null/space
        let octal_size = format!("{size:011o} ");
        header[124..136].copy_from_slice(octal_size.as_bytes());
        // MTime
        header[136..148].copy_from_slice(b"14000000000\0");
        // Type flag
        header[156] = type_flag;
        // Magic & version: ustar\000
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");

        // Calculate and write checksum
        header[148..156].copy_from_slice(b"        ");
        let mut sum: u32 = 0;
        for &b in &header {
            sum += u32::from(b);
        }
        let octal_cs = format!("{sum:06o}\0 ");
        header[148..156].copy_from_slice(octal_cs.as_bytes());

        header
    }

    #[test]
    fn test_tar_gnu_long_link_and_pax_handling() {
        let scratch =
            std::env::temp_dir().join(format!("test_tar_{}", uuid::Uuid::new_v4().simple()));
        let _ = fs::create_dir_all(&scratch);
        let archive_path = scratch.join("sample_long.tar");

        let mut tar_bytes = Vec::new();

        // 1. GNU LongLink entry (type 'L') with 120-char path
        let long_path = "sub/very_deeply_nested_directory_structure_exceeding_one_hundred_characters_limit_by_a_wide_margin_test/file.txt";
        let long_path_bytes = long_path.as_bytes();
        let long_header = create_tar_header(b"././@LongLink", long_path_bytes.len() as u64, b'L');
        tar_bytes.extend_from_slice(&long_header);
        tar_bytes.extend_from_slice(long_path_bytes);
        let long_pad = (512 - (long_path_bytes.len() % 512)) % 512;
        tar_bytes.extend(std::iter::repeat_n(0, long_pad));

        // 2. Normal file entry with dummy short name (will be overridden by long_path)
        let file_payload = b"Payload in long path file!";
        let file_header =
            create_tar_header(b"sub/truncated_name.txt", file_payload.len() as u64, b'0');
        tar_bytes.extend_from_slice(&file_header);
        tar_bytes.extend_from_slice(file_payload);
        let file_pad = (512 - (file_payload.len() % 512)) % 512;
        tar_bytes.extend(std::iter::repeat_n(0, file_pad));

        // 3. Symlink entry (type '2') which should be safely skipped
        let symlink_header = create_tar_header(b"sub/malicious_symlink.txt", 0, b'2');
        tar_bytes.extend_from_slice(&symlink_header);

        // 4. Two end-of-archive zero blocks (1024 bytes)
        tar_bytes.extend(std::iter::repeat_n(0, 1024));

        fs::write(&archive_path, &tar_bytes).unwrap();

        // Test inspect
        let inspected = inspect_tar(&archive_path, 100, 1024 * 1024).unwrap();
        assert_eq!(inspected.len(), 1);
        assert_eq!(inspected[0].relative_path, long_path);
        assert_eq!(inspected[0].size, file_payload.len() as u64);

        // Test extract
        let out_dir = scratch.join("out");
        let mut req = WorkerRequest::default();
        req.archive_path = archive_path.to_string_lossy().to_string();
        req.destination_directory = out_dir.to_string_lossy().to_string();

        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(|_: &WorkerEvent| {});

        let (extracted, is_salvaged) = extract_tar(&req, &out_dir, progress, cancel).unwrap();
        assert!(!is_salvaged);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].relative_path, long_path);

        let target_file = out_dir.join(long_path);
        assert!(target_file.exists());
        assert_eq!(fs::read(&target_file).unwrap(), file_payload);
        assert!(!out_dir.join("sub/malicious_symlink.txt").exists());

        // Test ResourceLimit on max_written_bytes
        req.max_written_bytes = 5;
        let limit_err = extract_tar(
            &req,
            &out_dir,
            Arc::new(|_: &WorkerEvent| {}),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(limit_err.is_err());
        assert_eq!(
            limit_err.unwrap_err(),
            "ResourceLimit: Exceeds maxWrittenBytes"
        );

        let _ = fs::remove_dir_all(&scratch);
    }
}
