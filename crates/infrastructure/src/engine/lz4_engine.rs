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

const LZ4_MAGIC: [u8; 4] = [0x04, 0x22, 0x4D, 0x18];

pub const MAX_LZ4_BLOCK_SIZE: usize = 8 * 1024 * 1024; // 8 MiB max per decompressed block

pub fn is_lz4_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "lz4" {
        return true;
    }
    let mut header = [0u8; 4];
    if let Ok(mut file) = File::open(path)
        && file.read_exact(&mut header).is_ok()
        && header == LZ4_MAGIC
    {
        return true;
    }
    false
}

pub fn get_extracted_file_name(source_path: &Path) -> String {
    let file_name = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extracted");
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".lz4") && file_name.len() > 4 {
        file_name[..file_name.len() - 4].to_string()
    } else {
        format!("{file_name}.extracted")
    }
}

pub fn inspect_lz4(
    archive_path: &Path,
    max_entries: u64,
    max_total_bytes: u64,
) -> Result<Vec<FileRecord>, String> {
    if max_entries > 0 && max_entries < 1 {
        return Err("ResourceLimit".into());
    }

    let mut file = File::open(archive_path).map_err(|_| "ArchiveNotFound".to_string())?;
    let mut header = [0u8; 7];
    file.read_exact(&mut header)
        .map_err(|_| "Corrupt".to_string())?;

    if header[0..4] != LZ4_MAGIC {
        return Err("Unsupported".into());
    }

    let flg = header[4];
    let version = (flg >> 6) & 0x03;
    if version != 1 {
        return Err("Unsupported".into());
    }

    let block_independence = (flg >> 5) & 1;
    if block_independence == 0 {
        return Err("Unsupported".into());
    }

    let content_size_present = (flg >> 3) & 1 != 0;
    let dict_id_present = (flg & 1) != 0;
    if dict_id_present {
        return Err("Unsupported".into());
    }

    let mut content_size: u64 = 0;
    let mut size_known = false;
    if content_size_present {
        let mut size_buf = [0u8; 8];
        file.read_exact(&mut size_buf)
            .map_err(|_| "Corrupt".to_string())?;
        content_size = u64::from_le_bytes(size_buf);
        size_known = true;
        if max_total_bytes > 0 && content_size > max_total_bytes {
            return Err("ResourceLimit".into());
        }
    }

    let out_file_name = get_extracted_file_name(archive_path);
    validate_relative_path(&out_file_name)?;

    Ok(vec![FileRecord {
        relative_path: out_file_name,
        size: if size_known { content_size } else { 0 },
        sha256: String::new(),
        is_directory: false,
        is_encrypted: false,
        is_corrupted: false,
        size_known,
    }])
}

pub fn extract_lz4(
    request: &WorkerRequest,
    destination: &Path,
    progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<FileRecord>, String> {
    let archive_path = Path::new(&request.archive_path);
    let mut file = File::open(archive_path).map_err(|_| "ArchiveNotFound".to_string())?;

    let mut header = [0u8; 7];
    file.read_exact(&mut header)
        .map_err(|_| "Corrupt".to_string())?;

    if header[0..4] != LZ4_MAGIC {
        return Err("Unsupported".into());
    }

    let flg = header[4];
    let version = (flg >> 6) & 0x03;
    if version != 1 {
        return Err("Unsupported".into());
    }

    let block_independence = (flg >> 5) & 1;
    if block_independence == 0 {
        return Err("Unsupported".into());
    }

    let block_checksum_present = (flg >> 4) & 1 != 0;
    let content_size_present = (flg >> 3) & 1 != 0;
    let content_checksum_present = (flg >> 2) & 1 != 0;
    let reserved = (flg >> 1) & 1;
    let dict_id_present = (flg & 1) != 0;

    if reserved != 0 || dict_id_present {
        return Err("Unsupported".into());
    }

    let mut declared_content_size = None;
    if content_size_present {
        let mut size_buf = [0u8; 8];
        file.read_exact(&mut size_buf)
            .map_err(|_| "Corrupt".to_string())?;
        declared_content_size = Some(u64::from_le_bytes(size_buf));
    }

    // Skip 1 byte header checksum (already read in header if no content size,
    // otherwise 1 byte follows the size)
    if content_size_present {
        let mut hc = [0u8; 1];
        file.read_exact(&mut hc)
            .map_err(|_| "Corrupt".to_string())?;
    }

    let max_bytes = match (request.max_total_bytes > 0, request.max_written_bytes > 0) {
        (true, true) => request.max_total_bytes.min(request.max_written_bytes),
        (true, false) => request.max_total_bytes,
        (false, true) => request.max_written_bytes,
        (false, false) => u64::MAX,
    };
    if let Some(declared) = declared_content_size
        && declared > max_bytes
    {
        return Err("ResourceLimit".into());
    }

    let out_file_name = get_extracted_file_name(archive_path);
    validate_relative_path(&out_file_name)?;

    let is_excluded = matches_exclusion(&out_file_name, &request.exclusions);
    let is_included = matches_inclusion(&out_file_name, false, &request.inclusions);
    let should_extract = !is_excluded && is_included;

    let out_path = destination.join(&out_file_name);
    let mut out_writer: Option<BufWriter<File>> = if should_extract {
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("IoError: {e}"))?;
        }
        let out_file = File::create(&out_path).map_err(|e| format!("IoError: {e}"))?;
        Some(BufWriter::with_capacity(64 * 1024, out_file))
    } else {
        None
    };

    let mut block_size_buf = [0u8; 4];
    let mut block_decompressed = Vec::with_capacity(64 * 1024);
    let mut total_decompressed: u64 = 0;
    let mut hasher = Sha256::new();
    let mut last_progress = Instant::now();

    let result = (|| -> Result<(), String> {
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("Cancelled".into());
            }

            if file.read_exact(&mut block_size_buf).is_err() {
                return Err("Corrupt".into());
            }

            let raw_block_size = u32::from_le_bytes(block_size_buf);
            if raw_block_size == 0 {
                // EndMark
                break;
            }

            let is_uncompressed = (raw_block_size & 0x80000000) != 0;
            let block_len = (raw_block_size & 0x7FFFFFFF) as usize;

            if block_len > 4 * 1024 * 1024 + 64 {
                return Err("Corrupt".into());
            }

            let mut block_data = vec![0u8; block_len];
            file.read_exact(&mut block_data)
                .map_err(|_| "Corrupt".to_string())?;

            let remaining_budget = max_bytes.saturating_sub(total_decompressed);
            if remaining_budget == 0 {
                return Err("ResourceLimit".into());
            }

            block_decompressed.clear();
            if is_uncompressed {
                if (block_len as u64) > remaining_budget || block_len > MAX_LZ4_BLOCK_SIZE {
                    return Err("ResourceLimit".into());
                }
                block_decompressed.extend_from_slice(&block_data);
            } else {
                let block_budget = (remaining_budget as usize).min(MAX_LZ4_BLOCK_SIZE);
                decompress_lz4_block_bounded(&block_data, &mut block_decompressed, block_budget)?;
            }

            if block_checksum_present {
                let mut bc = [0u8; 4];
                file.read_exact(&mut bc)
                    .map_err(|_| "Corrupt".to_string())?;
            }

            let chunk_len = block_decompressed.len() as u64;
            total_decompressed = total_decompressed.saturating_add(chunk_len);
            if total_decompressed > max_bytes {
                return Err("ResourceLimit".into());
            }

            hasher.update(&block_decompressed);

            if let Some(ref mut writer) = out_writer {
                writer
                    .write_all(&block_decompressed)
                    .map_err(|e| format!("IoError: {e}"))?;
            }

            if last_progress.elapsed() >= Duration::from_millis(50) {
                let mut prog_event = super::make_event(request, "progress");
                prog_event.completed_bytes = total_decompressed;
                prog_event.total_bytes = declared_content_size.unwrap_or(0);
                prog_event.total_bytes_known = declared_content_size.is_some();
                progress(&prog_event);
                last_progress = Instant::now();
            }
        }

        if content_checksum_present {
            let mut cc = [0u8; 4];
            file.read_exact(&mut cc)
                .map_err(|_| "Corrupt".to_string())?;
        }

        // Trailing data or concatenated frames are unsupported
        let mut trailing = [0u8; 1];
        if file.read(&mut trailing).unwrap_or(0) > 0 {
            return Err("Unsupported".into());
        }

        if let Some(declared) = declared_content_size
            && total_decompressed != declared
        {
            return Err("Corrupt".into());
        }

        if let Some(ref mut writer) = out_writer {
            writer.flush().map_err(|e| format!("IoError: {e}"))?;
        }

        Ok(())
    })();

    if let Err(err) = result {
        drop(out_writer);
        if should_extract && out_path.exists() {
            let _ = fs::remove_file(&out_path);
        }
        return Err(err);
    }

    if !should_extract {
        return Ok(Vec::new());
    }

    let hash_hex = hex::encode(hasher.finalize());

    Ok(vec![FileRecord {
        relative_path: out_file_name,
        size: total_decompressed,
        sha256: hash_hex,
        is_directory: false,
        is_encrypted: false,
        is_corrupted: false,
        size_known: true,
    }])
}

pub fn decompress_lz4_block(input: &[u8], output: &mut Vec<u8>) -> Result<(), String> {
    decompress_lz4_block_bounded(input, output, MAX_LZ4_BLOCK_SIZE)
}

pub fn decompress_lz4_block_bounded(
    input: &[u8],
    output: &mut Vec<u8>,
    max_decompressed_bytes: usize,
) -> Result<(), String> {
    let initial_len = output.len();
    let mut ip = 0;
    while ip < input.len() {
        let token = input[ip];
        ip += 1;
        let mut literal_len = (token >> 4) as usize;
        if literal_len == 15 {
            loop {
                if ip >= input.len() {
                    return Err("Corrupt".into());
                }
                let b = input[ip] as usize;
                ip += 1;
                literal_len += b;
                if b != 255 {
                    break;
                }
            }
        }
        if ip + literal_len > input.len() {
            return Err("Corrupt".into());
        }
        let current_decompressed = output.len() - initial_len;
        if current_decompressed.saturating_add(literal_len) > max_decompressed_bytes {
            return Err("ResourceLimit".into());
        }
        output.extend_from_slice(&input[ip..ip + literal_len]);
        ip += literal_len;
        if ip >= input.len() {
            break;
        }
        if ip + 2 > input.len() {
            return Err("Corrupt".into());
        }
        let offset = u16::from_le_bytes([input[ip], input[ip + 1]]) as usize;
        ip += 2;
        if offset == 0 || offset > output.len() {
            return Err("Corrupt".into());
        }
        let mut match_len = (token & 0x0F) as usize + 4;
        if match_len == 19 {
            loop {
                if ip >= input.len() {
                    return Err("Corrupt".into());
                }
                let b = input[ip] as usize;
                ip += 1;
                match_len += b;
                if b != 255 {
                    break;
                }
            }
        }
        let current_decompressed = output.len() - initial_len;
        if current_decompressed.saturating_add(match_len) > max_decompressed_bytes {
            return Err("ResourceLimit".into());
        }
        let start = output.len() - offset;
        for i in 0..match_len {
            let b = output[start + i];
            output.push(b);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lz4_decompress_block_literal() {
        // Token 0x50: 5 literals, 0 match
        let input = [0x50, b'h', b'e', b'l', b'l', b'o'];
        let mut output = Vec::new();
        decompress_lz4_block(&input, &mut output).unwrap();
        assert_eq!(&output, b"hello");
    }

    #[test]
    fn test_lz4_stream_extraction_and_cleanup() {
        let scratch =
            std::env::temp_dir().join(format!("test_lz4_{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&scratch).expect("failed to create scratch dir");
        let archive_path = scratch.join("sample.txt.lz4");

        // Construct valid LZ4 frame with 1 uncompressed block
        let mut frame_bytes = Vec::new();
        frame_bytes.extend_from_slice(&LZ4_MAGIC);
        let flg = 0x60u8; // Version 01, BlockIndep 1
        let bd = 0x40u8; // 64KB
        frame_bytes.push(flg);
        frame_bytes.push(bd);
        frame_bytes.push(0x55); // Header checksum (1 byte)

        // Uncompressed block with "Hello, LZ4 streaming world!"
        let payload = b"Hello, LZ4 streaming world!";
        let raw_block_size = 0x80000000u32 | (payload.len() as u32);
        frame_bytes.extend_from_slice(&raw_block_size.to_le_bytes());
        frame_bytes.extend_from_slice(payload);

        // EndMark
        frame_bytes.extend_from_slice(&0u32.to_le_bytes());
        fs::write(&archive_path, &frame_bytes).unwrap();

        let out_dir = scratch.join("out");
        let mut req = WorkerRequest::default();
        req.archive_path = archive_path.to_string_lossy().to_string();
        req.destination_directory = out_dir.to_string_lossy().to_string();
        req.max_total_bytes = 1024 * 1024;
        req.max_written_bytes = 1024 * 1024;

        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(|_: &WorkerEvent| {});

        let records = extract_lz4(&req, &out_dir, progress.clone(), cancel.clone()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].relative_path, "sample.txt");
        assert_eq!(records[0].size, payload.len() as u64);

        let extracted_file = out_dir.join("sample.txt");
        assert_eq!(fs::read(&extracted_file).unwrap(), payload);

        // Test ResourceLimit cleanup
        req.max_written_bytes = 5;
        let limit_err = extract_lz4(&req, &out_dir, progress.clone(), cancel.clone());
        assert!(limit_err.is_err());
        assert_eq!(limit_err.unwrap_err(), "ResourceLimit");

        // Test inclusion filtering
        req.max_written_bytes = 1024 * 1024;
        req.inclusions = vec!["*.other".to_string()];
        let filtered_records = extract_lz4(&req, &out_dir, progress, cancel).unwrap();
        assert!(filtered_records.is_empty());

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn test_lz4_decompress_block_bounded_limit() {
        let input = [0x50, b'h', b'e', b'l', b'l', b'o'];
        let mut output = Vec::new();
        let res = decompress_lz4_block_bounded(&input, &mut output, 3);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "ResourceLimit");
    }
}
