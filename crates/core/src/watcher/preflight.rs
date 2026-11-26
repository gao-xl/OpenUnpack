//! 文件夹监控临时下载文件识别、分卷占用检测与归档完整性预检。

use super::*;

/// Temporary file extensions used by browsers, download managers, and system locks.
pub fn is_temp_download_file(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_ascii_lowercase(),
        None => return true,
    };
    name.ends_with(".crdownload")
        || name.ends_with(".part")
        || name.ends_with(".tmp")
        || name.ends_with(".temp")
        || name.ends_with(".download")
        || name.ends_with(".aria2")
        || name.ends_with(".opdownload")
        || name.ends_with(".idm")
        || name.ends_with(".xltd")
        || name.ends_with(".td")
        || name.ends_with(".td.cfg")
        || name.ends_with(".!ut")
        || name.ends_with(".bts")
        || name.ends_with(".downloading")
        || name.ends_with(".baiduyun.p.downloading")
        || name.ends_with(".partial")
        || name.starts_with("~$")
        || name.ends_with(".swp")
        || name.ends_with(".lock")
}

/// Helper to strip temporary download suffixes to check the underlying target filename.
/// For example, "archive.part2.rar.crdownload" -> Some("archive.part2.rar")
pub fn strip_temp_download_suffix(filename: &str) -> Option<&str> {
    let lower = filename.to_ascii_lowercase();
    let temp_suffixes = [
        ".crdownload",
        ".part",
        ".tmp",
        ".temp",
        ".download",
        ".aria2",
        ".opdownload",
        ".idm",
        ".xltd",
        ".td.cfg",
        ".td",
        ".!ut",
        ".bts",
        ".downloading",
        ".baiduyun.p.downloading",
        ".partial",
    ];
    for suffix in &temp_suffixes {
        if lower.ends_with(suffix) {
            return Some(&filename[..filename.len() - suffix.len()]);
        }
    }
    None
}

/// Helper to check if a filename belongs to a temporary download of the given volume stem.
pub fn is_volume_sibling_temp(filename: &str, target_stem_lower: &str) -> bool {
    let p = Path::new(filename);
    if !is_temp_download_file(p) {
        return false;
    }
    if let Some(target) = strip_temp_download_suffix(filename) {
        let vol = classify_volume(target);
        if let Some(stem) = vol.stem() {
            return stem.to_ascii_lowercase() == target_stem_lower;
        }
    }
    false
}

/// Probes whether a file is finished writing and accessible for reading without concurrent writer lock.
/// On Windows, opening with FILE_SHARE_READ (without FILE_SHARE_WRITE) ensures that if an active process
/// currently holds GENERIC_WRITE, Windows will deny access with ERROR_SHARING_VIOLATION (32).
#[cfg(windows)]
pub fn probe_file_ready(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_SHARE_READ = 1.
    // Denying FILE_SHARE_WRITE ensures any active writer causes this call to fail.
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .is_ok()
}

#[cfg(not(windows))]
pub fn probe_file_ready(path: &Path) -> bool {
    std::fs::OpenOptions::new().read(true).open(path).is_ok()
}

/// L4 Pre-flight verification: inspects header and footer structures to ensure
/// the archive is not truncated (e.g. from an interrupted or paused download).
pub fn is_archive_complete_preflight(path: &Path) -> bool {
    let fname = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };

    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let file_len = meta.len();
    if file_len == 0 {
        return false;
    }

    let mut f = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    is_archive_complete_preflight_stream(fname, file_len, &mut f)
}

/// Generic stream-based preflight verification across archive formats (allows in-memory testing).
pub fn is_archive_complete_preflight_stream<R: Read + Seek>(
    fname: &str,
    file_len: u64,
    reader: &mut R,
) -> bool {
    if file_len == 0 {
        return false;
    }

    // Split continuation volumes (e.g. *.part2.rar, *.z02, *.zip.002) don't have
    // leading magic or standalone trailing records. As long as length > 0, bypass preflight.
    let vol = classify_volume(fname);
    let is_continuation = matches!(
        vol,
        VolumeType::NumericSplit { num: 2.., .. }
            | VolumeType::PartRar { num: 2.., .. }
            | VolumeType::OldRar { num: Some(_), .. }
            | VolumeType::SplitZip { num: Some(2..), .. }
    );
    if is_continuation {
        // G-03 Guard: If it's a continuation volume, ensure it's not an unfinished P2P zero-fill preallocation
        if crate::intake_guard::is_trailing_zero_allocated(reader, file_len) {
            return false;
        }
        return true;
    }

    let lower = fname.to_ascii_lowercase();

    // 1. ZIP / JAR / APK:
    // Standard ZIP must contain End of Central Directory (EOCD) signature [0x50, 0x4b, 0x05, 0x06]
    // or ZIP64 EOCD locator [0x50, 0x4b, 0x06, 0x07] in the trailing 65557 bytes.
    if lower.ends_with(".zip") || lower.ends_with(".jar") || lower.ends_with(".apk") {
        if file_len < 22 {
            return false;
        }
        let read_len = (file_len.min(65557)) as usize;
        let mut buf = vec![0u8; read_len];
        if reader.seek(SeekFrom::End(-(read_len as i64))).is_ok()
            && reader.read_exact(&mut buf).is_ok()
        {
            let has_eocd = buf
                .windows(4)
                .any(|w| w == [0x50, 0x4b, 0x05, 0x06] || w == [0x50, 0x4b, 0x06, 0x07]);
            if !has_eocd {
                return false;
            }
        } else {
            return false;
        }
    }

    // 2. 7z:
    // Minimum 7z header is 32 bytes with signature [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C].
    // StartHeader contains NextHeaderOffset (8 bytes at 12) + NextHeaderSize (8 bytes at 20).
    // If NextHeaderSize > 0, 32 + NextHeaderOffset + NextHeaderSize must not exceed file_len.
    if lower.ends_with(".7z") {
        if file_len < 32 {
            return false;
        }
        let mut header = [0u8; 32];
        if reader.seek(SeekFrom::Start(0)).is_ok() && reader.read_exact(&mut header).is_ok() {
            if header[0..6] != [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C] {
                return false;
            }
            let next_header_offset =
                u64::from_le_bytes(header[12..20].try_into().unwrap_or([0; 8]));
            let next_header_size = u64::from_le_bytes(header[20..28].try_into().unwrap_or([0; 8]));
            if next_header_size > 0
                && let Some(needed) = 32u64
                    .checked_add(next_header_offset)
                    .and_then(|s| s.checked_add(next_header_size))
                && needed > file_len
            {
                return false; // Truncated end header
            }
        } else {
            return false;
        }
    }

    // 3. RAR:
    // RAR4 header: [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00] (7 bytes)
    // RAR5 header: [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00] (8 bytes)
    if lower.ends_with(".rar") {
        if file_len < 7 {
            return false;
        }
        let mut magic = [0u8; 8];
        if reader.seek(SeekFrom::Start(0)).is_ok() {
            let n = reader.read(&mut magic).unwrap_or(0);
            if n < 7 {
                return false;
            }
            let is_rar4 = magic[..7] == [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00];
            let is_rar5 = n >= 8 && magic[..8] == [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00];
            if !is_rar4 && !is_rar5 {
                return false;
            }
        } else {
            return false;
        }
    }

    // 4. GZ / TGZ:
    // Magic: [0x1F, 0x8B], at least 10 bytes header + 8 bytes footer = 18 bytes
    if lower.ends_with(".gz") || lower.ends_with(".tgz") {
        if file_len < 18 {
            return false;
        }
        let mut magic = [0u8; 2];
        if reader.seek(SeekFrom::Start(0)).is_ok()
            && reader.read_exact(&mut magic).is_ok()
            && magic != [0x1F, 0x8B]
        {
            return false;
        }
    }

    true
}
