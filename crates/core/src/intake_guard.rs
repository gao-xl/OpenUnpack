use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// G-01: Checks whether the output directory falls within the watched folder hierarchy (or vice versa),
/// which would cause an infinite recursive extraction loop (Loop Trap).
pub fn check_loop_trap(watched_dirs: &[PathBuf], output_dir: &Path) -> Result<(), String> {
    if output_dir.as_os_str().is_empty() {
        return Ok(());
    }

    let out_norm = normalized_components(output_dir);

    for watched in watched_dirs {
        if watched.as_os_str().is_empty() {
            continue;
        }
        let watched_norm = normalized_components(watched);

        // 1. Output is inside watched folder (e.g. Watched: C:\Down, Output: C:\Down\Extracted)
        if is_within(&out_norm, &watched_norm) {
            return Err(format!(
                "解压输出目录 '{}' 位于监听目录 '{}' 之下，将导致自激死循环解压！请指定独立的输出目录。",
                output_dir.display(),
                watched.display()
            ));
        }

        // 2. Watched folder is inside output directory (e.g. Output: C:\Data, Watched: C:\Data\In)
        if is_within(&watched_norm, &out_norm) {
            return Err(format!(
                "监听目录 '{}' 位于解压输出目录 '{}' 之下，将导致自激死循环解压！请指定独立的输出目录。",
                watched.display(),
                output_dir.display()
            ));
        }
    }

    Ok(())
}

/// Windows paths are case-insensitive and may contain `..` before they exist on
/// disk, so containment must be decided on normalized components rather than on
/// `Path::starts_with` (which is a case-sensitive, purely lexical comparison).
fn normalized_components(path: &Path) -> Vec<String> {
    if let Ok(resolved) = path.canonicalize() {
        return components_of(&resolved);
    }
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                parts.push(prefix.as_os_str().to_string_lossy().to_lowercase());
            }
            Component::RootDir => parts.push(String::new()),
            Component::Normal(segment) => {
                parts.push(segment.to_string_lossy().to_lowercase());
            }
            Component::ParentDir => {
                if parts.last().is_some_and(|last| !last.is_empty()) {
                    parts.pop();
                }
            }
            Component::CurDir => {}
        }
    }
    parts
}

fn components_of(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_lowercase()),
            Component::RootDir => Some(String::new()),
            Component::Normal(segment) => Some(segment.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect()
}

fn is_within(candidate: &[String], base: &[String]) -> bool {
    base.len() <= candidate.len() && candidate[..base.len()] == *base
}

/// G-02: Determines if a file is a cloud files-on-demand stub (OneDrive, iCloud, etc.)
/// without triggering hydration download.
#[cfg(windows)]
pub fn is_cloud_placeholder_file(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    if let Ok(meta) = path.metadata() {
        is_cloud_placeholder_attrs(meta.file_attributes())
    } else {
        false
    }
}

#[cfg(not(windows))]
pub fn is_cloud_placeholder_file(_path: &Path) -> bool {
    false
}

/// Pure attribute-based checker for cloud placeholder stubs.
pub fn is_cloud_placeholder_attrs(attrs: u32) -> bool {
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x00001000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x00000400;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x00400000;
    const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x00040000;

    let is_offline = (attrs & FILE_ATTRIBUTE_OFFLINE) != 0;
    let is_recall =
        (attrs & (FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_RECALL_ON_OPEN)) != 0;
    let is_reparse_point = (attrs & FILE_ATTRIBUTE_REPARSE_POINT) != 0;

    is_offline || is_recall || (is_reparse_point && (attrs & 0x00000010) == 0) // Reparse point that is a file
}

/// G-03: Checks whether an uncompressed continuation volume (e.g. *.part2.rar, *.7z.002)
/// is a P2P / torrent preallocated sparse or zero-filled file whose trailing blocks are not written yet.
pub fn is_trailing_zero_allocated<R: Read + Seek>(reader: &mut R, file_len: u64) -> bool {
    // If the file is smaller than 4KB, check the whole file; otherwise check trailing 4KB
    if file_len == 0 {
        return true;
    }
    let check_size = (file_len.min(4096)) as usize;
    let mut buf = vec![0u8; check_size];

    if reader.seek(SeekFrom::End(-(check_size as i64))).is_ok()
        && reader.read_exact(&mut buf).is_ok()
    {
        // If trailing 4KB are completely zeroed, it is almost certainly a preallocated uncompleted block
        buf.iter().all(|&b| b == 0)
    } else {
        false
    }
}

/// G-04: Verifies target drive has sufficient free disk space to accommodate extraction.
/// Required = total_uncompressed_bytes * (1 + overhead_ratio) + safety_margin_bytes.
pub fn check_disk_space_sufficient(
    target_dir: &Path,
    total_uncompressed_bytes: u64,
    safety_margin_bytes: u64,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut target_norm = target_dir.to_path_buf();
        while !target_norm.exists() {
            if let Some(parent) = target_norm.parent() {
                target_norm = parent.to_path_buf();
            } else {
                break;
            }
        }

        let wide: Vec<u16> = target_norm
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut free_bytes_available: u64 = 0;
        let mut total_bytes: u64 = 0;
        let mut total_free_bytes: u64 = 0;

        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_bytes_available,
                &mut total_bytes,
                &mut total_free_bytes,
            )
        };

        if ok != 0 {
            // Require uncompressed size + 10% overhead + safety margin (default 500MB)
            let needed = total_uncompressed_bytes
                .saturating_add(total_uncompressed_bytes / 10)
                .saturating_add(safety_margin_bytes);

            if free_bytes_available < needed {
                return Err(format!(
                    "目标磁盘可用空间不足: 剩余可用 {:.2} GB，解压预估需要 {:.2} GB（含安全裕量）。",
                    (free_bytes_available as f64) / (1024.0 * 1024.0 * 1024.0),
                    (needed as f64) / (1024.0 * 1024.0 * 1024.0)
                ));
            }
        }
    }

    let _ = (target_dir, total_uncompressed_bytes, safety_margin_bytes);
    Ok(())
}

/// G-05: Stability state calculation with antivirus/EDR transient lock tolerance.
/// If file size and mtime are completely unchanged, but probe temporarily failed (e.g. Defender scanning),
/// we hold the ticks without immediately resetting to 0, resetting only if size/mtime change.
// Four independent in/out counters describe the probe state machine; bundling
// them would hide which fields the caller must persist between ticks.
#[allow(clippy::too_many_arguments)]
pub fn update_stability_with_backoff(
    stable_ticks: &mut usize,
    last_size: &mut u64,
    last_mtime: &mut SystemTime,
    consecutive_probe_failures: &mut usize,
    current_size: u64,
    current_mtime: SystemTime,
    probe_ok: bool,
    required_ticks: usize,
) -> bool {
    if current_size == 0 {
        *stable_ticks = 0;
        *consecutive_probe_failures = 0;
        return false;
    }

    let metadata_unchanged =
        current_size == *last_size && current_mtime == *last_mtime && *last_size > 0;

    if metadata_unchanged {
        if probe_ok {
            *stable_ticks += 1;
            *consecutive_probe_failures = 0;
        } else {
            // Single transient read lock (e.g. Defender scanning file in 1 tick)
            *consecutive_probe_failures += 1;
            if *consecutive_probe_failures >= 3 {
                // If it fails for 3 consecutive ticks, reset
                *stable_ticks = 0;
            }
        }
    } else {
        // File actively changing size or mtime -> download in progress
        *last_size = current_size;
        *last_mtime = current_mtime;
        *stable_ticks = 0;
        *consecutive_probe_failures = 0;
    }

    *stable_ticks >= required_ticks
}

/// G-06: Evaluates if an incomplete multi-volume group has exceeded maximum waiting TTL.
pub fn is_volume_waiting_timed_out(first_seen: Instant, max_wait: Duration) -> bool {
    first_seen.elapsed() >= max_wait
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_check_loop_trap_detection() {
        let watched = vec![
            PathBuf::from(r"C:\Users\test\Downloads"),
            PathBuf::from(r"D:\Incoming"),
        ];

        // 1. Output inside watched folder -> ERROR
        let bad_out1 = PathBuf::from(r"C:\Users\test\Downloads\Unpacked");
        assert!(check_loop_trap(&watched, &bad_out1).is_err());

        // 2. Output exactly watched folder -> ERROR
        let bad_out2 = PathBuf::from(r"C:\Users\test\Downloads");
        assert!(check_loop_trap(&watched, &bad_out2).is_err());

        // 3. Watched folder inside output folder -> ERROR
        let bad_out3 = PathBuf::from(r"C:\Users\test");
        assert!(check_loop_trap(&watched, &bad_out3).is_err());

        // 4. Completely independent folders -> OK
        let good_out1 = PathBuf::from(r"C:\Users\test\Desktop\Extracted");
        assert!(check_loop_trap(&watched, &good_out1).is_ok());

        let good_out2 = PathBuf::from(r"E:\ArchiveOut");
        assert!(check_loop_trap(&watched, &good_out2).is_ok());

        // 5. Empty output -> OK
        assert!(check_loop_trap(&watched, Path::new("")).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn loop_trap_detection_is_case_insensitive_on_windows_paths() {
        let watched = vec![PathBuf::from(r"C:\Users\test\Downloads")];

        // Same folder typed in a different case must still be rejected.
        let lower_out = PathBuf::from(r"c:\users\test\downloads\unpacked");
        assert!(check_loop_trap(&watched, &lower_out).is_err());

        // Reverse direction with mixed case.
        let lower_watched = vec![PathBuf::from(r"c:\users\test\downloads")];
        let upper_out = PathBuf::from(r"C:\Users\Test\Downloads\out");
        assert!(check_loop_trap(&lower_watched, &upper_out).is_err());

        // A sibling sharing a string prefix is not nested.
        let sibling = PathBuf::from(r"C:\Users\test\Downloads2");
        assert!(check_loop_trap(&watched, &sibling).is_ok());
    }

    #[test]
    fn test_cloud_placeholder_attributes() {
        // Normal file
        assert!(!is_cloud_placeholder_attrs(0x00000020)); // FILE_ATTRIBUTE_ARCHIVE

        // Offline file (e.g. OneDrive placeholder)
        assert!(is_cloud_placeholder_attrs(0x00001000)); // FILE_ATTRIBUTE_OFFLINE
        assert!(is_cloud_placeholder_attrs(0x00001020)); // OFFLINE | ARCHIVE

        // Reparse point file (not directory)
        assert!(is_cloud_placeholder_attrs(0x00000420)); // REPARSE_POINT | ARCHIVE

        // Recall on access
        assert!(is_cloud_placeholder_attrs(0x00400020));
    }

    #[test]
    fn test_trailing_zero_allocation_detection() {
        // 1. Empty buffer -> treated as incomplete
        let mut cursor = Cursor::new(vec![]);
        assert!(is_trailing_zero_allocated(&mut cursor, 0));

        // 2. Trailing zero allocated (e.g. 8192 bytes with trailing 4096 zeroes)
        let mut data = vec![0xAB; 4096];
        data.extend(vec![0u8; 4096]);
        let mut cursor = Cursor::new(data);
        assert!(is_trailing_zero_allocated(&mut cursor, 8192));

        // 3. Fully populated payload with non-zero trailing data
        let mut data2 = vec![0xAB; 8192];
        data2[8190] = 0x55;
        let mut cursor = Cursor::new(data2);
        assert!(!is_trailing_zero_allocated(&mut cursor, 8192));
    }

    #[test]
    fn test_update_stability_with_backoff_transient_av_locks() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut stable_ticks = 0;
        let mut last_size = 0;
        let mut last_mtime = t0;
        let mut probe_failures = 0;

        // 1. First tick with 100 bytes -> tick 0
        assert!(!update_stability_with_backoff(
            &mut stable_ticks,
            &mut last_size,
            &mut last_mtime,
            &mut probe_failures,
            100,
            t0,
            true,
            3
        ));
        assert_eq!(stable_ticks, 0);

        // 2. Second tick unchanged -> tick 1
        assert!(!update_stability_with_backoff(
            &mut stable_ticks,
            &mut last_size,
            &mut last_mtime,
            &mut probe_failures,
            100,
            t0,
            true,
            3
        ));
        assert_eq!(stable_ticks, 1);

        // 3. Third tick: Transient AV scan (probe_ok = false, but size/mtime unchanged)
        // With backoff: ticks are NOT cleared to 0 immediately!
        assert!(!update_stability_with_backoff(
            &mut stable_ticks,
            &mut last_size,
            &mut last_mtime,
            &mut probe_failures,
            100,
            t0,
            false,
            3
        ));
        assert_eq!(stable_ticks, 1);
        assert_eq!(probe_failures, 1);

        // 4. Fourth tick: AV finishes, probe_ok = true -> tick 2
        assert!(!update_stability_with_backoff(
            &mut stable_ticks,
            &mut last_size,
            &mut last_mtime,
            &mut probe_failures,
            100,
            t0,
            true,
            3
        ));
        assert_eq!(stable_ticks, 2);
        assert_eq!(probe_failures, 0);

        // 5. Fifth tick: probe_ok = true -> reaches 3 ticks -> READY!
        assert!(update_stability_with_backoff(
            &mut stable_ticks,
            &mut last_size,
            &mut last_mtime,
            &mut probe_failures,
            100,
            t0,
            true,
            3
        ));
        assert_eq!(stable_ticks, 3);

        // 6. If size changes -> immediately reset to 0
        assert!(!update_stability_with_backoff(
            &mut stable_ticks,
            &mut last_size,
            &mut last_mtime,
            &mut probe_failures,
            200,
            t0,
            true,
            3
        ));
        assert_eq!(stable_ticks, 0);
    }
}
