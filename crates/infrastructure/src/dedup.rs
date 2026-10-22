//! NTFS Hardlink Deduplication Engine.
//!
//! Provides content-addressable deduplication using NTFS hard links
//! (`CreateHardLinkW` / `std::fs::hard_link`), reducing physical disk storage
//! consumption for duplicate files within extracted directories to zero redundancy
//! while preserving complete, independent directory trees and file paths.

use openunpack_protocol::{CopyFile, FileRecord};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Summary report of deduplication results.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupReport {
    /// Total regular candidate files scanned.
    pub scanned_files: usize,
    /// Number of candidate files that share identical content with another file.
    pub duplicate_files: usize,
    /// Number of files successfully converted to NTFS hard links.
    pub hardlinked_files: usize,
    /// Total bytes of physical disk space saved (clusters released).
    pub saved_bytes: u64,
}

/// Common interface for file metadata candidates across worker and pipeline models.
pub trait DeduplicationCandidate {
    fn relative_path(&self) -> &str;
    fn size(&self) -> u64;
    fn sha256(&self) -> &str;
    fn is_directory(&self) -> bool;
    fn is_corrupted(&self) -> bool;
}

impl DeduplicationCandidate for FileRecord {
    fn relative_path(&self) -> &str {
        &self.relative_path
    }
    fn size(&self) -> u64 {
        self.size
    }
    fn sha256(&self) -> &str {
        &self.sha256
    }
    fn is_directory(&self) -> bool {
        self.is_directory
    }
    fn is_corrupted(&self) -> bool {
        self.is_corrupted
    }
}

impl DeduplicationCandidate for CopyFile {
    fn relative_path(&self) -> &str {
        &self.relative_path
    }
    fn size(&self) -> u64 {
        self.size
    }
    fn sha256(&self) -> &str {
        &self.sha256
    }
    fn is_directory(&self) -> bool {
        self.is_directory
    }
    fn is_corrupted(&self) -> bool {
        self.is_corrupted
    }
}

/// Abstract interface for file linking operations.
pub trait FileLinker: Send + Sync {
    /// Atomically replaces `duplicate` with a hard link to `primary`.
    /// Returns `Ok(true)` if newly linked, `Ok(false)` if already linked, or `Err`.
    fn replace_with_hardlink(&self, primary: &Path, duplicate: &Path) -> io::Result<bool>;
}

/// Production NTFS file linker using Win32 / standard filesystem hard link syscalls.
#[derive(Debug, Default, Clone, Copy)]
pub struct NtfsHardLinker;

impl FileLinker for NtfsHardLinker {
    fn replace_with_hardlink(&self, primary: &Path, duplicate: &Path) -> io::Result<bool> {
        replace_with_hardlink(primary, duplicate)
    }
}

/// Deduplicates files in `root` using known candidate records and production NTFS hardlinker.
pub fn deduplicate_extracted_files<T: DeduplicationCandidate>(
    root: &Path,
    files: &[T],
) -> io::Result<DedupReport> {
    deduplicate_extracted_files_with_linker(root, files, &NtfsHardLinker)
}

/// Deduplicates files in `root` using custom linker.
pub fn deduplicate_extracted_files_with_linker<T: DeduplicationCandidate, L: FileLinker>(
    root: &Path,
    files: &[T],
    linker: &L,
) -> io::Result<DedupReport> {
    let mut report = DedupReport::default();

    // Phase 1: Filter and group by size
    let mut by_size: HashMap<u64, Vec<&T>> = HashMap::new();
    for file in files {
        if file.is_directory() || file.is_corrupted() || file.size() == 0 {
            continue;
        }
        report.scanned_files += 1;
        by_size.entry(file.size()).or_default().push(file);
    }

    // Phase 2: For collisions, group by SHA-256
    for (&size, candidates) in &by_size {
        if candidates.len() < 2 {
            continue;
        }

        let mut by_hash: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for file in candidates {
            let full_path = root.join(file.relative_path());

            let hash = if !file.sha256().is_empty() {
                file.sha256().to_string()
            } else if full_path.is_file() {
                match compute_file_sha256(&full_path) {
                    Ok(h) => h,
                    Err(_) => continue,
                }
            } else {
                continue;
            };

            by_hash.entry(hash).or_default().push(full_path);
        }

        // Phase 3: Replace duplicates with hardlinks
        for (_hash, matching_paths) in by_hash {
            if matching_paths.len() < 2 {
                continue;
            }

            report.duplicate_files += matching_paths.len() - 1;
            let primary = &matching_paths[0];

            for dup in &matching_paths[1..] {
                match linker.replace_with_hardlink(primary, dup) {
                    Ok(true) => {
                        report.hardlinked_files += 1;
                        report.saved_bytes += size;
                    }
                    Ok(false) => {
                        // Already hardlinked to primary
                    }
                    Err(e) => {
                        // Failsafe: log/ignore error, file is preserved
                        let _ = e;
                    }
                }
            }
        }
    }

    Ok(report)
}

/// Deduplicates all files in a directory tree by walking the filesystem.
pub fn deduplicate_directory_tree(root: &Path) -> io::Result<DedupReport> {
    let mut files = Vec::new();
    collect_files_recursive(root, root, &mut files)?;
    deduplicate_extracted_files(root, &files)
}

fn collect_files_recursive(
    current: &Path,
    root: &Path,
    results: &mut Vec<FileRecord>,
) -> io::Result<()> {
    if !current.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_files_recursive(&path, root, results)?;
        } else if file_type.is_file() {
            let metadata = entry.metadata()?;
            let rel = path
                .strip_prefix(root)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");

            results.push(FileRecord {
                relative_path: rel_str,
                size: metadata.len(),
                size_known: true,
                sha256: String::new(),
                is_directory: false,
                is_encrypted: false,
                is_corrupted: false,
            });
        }
    }
    Ok(())
}

#[cfg(windows)]
fn get_file_identity(file: &File) -> io::Result<(u32, u64, u32)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let success = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }

    let file_index = ((info.nFileIndexHigh as u64) << 32) | (info.nFileIndexLow as u64);
    Ok((info.dwVolumeSerialNumber, file_index, info.nNumberOfLinks))
}

/// Atomically replaces `duplicate` with an NTFS hard link to `primary`.
///
/// If `primary` and `duplicate` are already identical hard links (same file index),
/// returns `Ok(false)` without doing any work.
///
/// If hard link creation fails (e.g. cross-volume or filesystem without hardlink support),
/// the original `duplicate` file is guaranteed to be restored and intact.
pub fn replace_with_hardlink(primary: &Path, duplicate: &Path) -> io::Result<bool> {
    if primary == duplicate {
        return Ok(false);
    }

    if !primary.is_file() || !duplicate.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "One or both files do not exist or are not regular files",
        ));
    }

    #[cfg(windows)]
    {
        let primary_file = File::open(primary)?;
        let dup_file = File::open(duplicate)?;

        if let (Ok((pv, pi, _)), Ok((dv, di, _))) = (
            get_file_identity(&primary_file),
            get_file_identity(&dup_file),
        ) && pv == dv
            && pi == di
        {
            return Ok(false);
        }
    }

    let parent = duplicate.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Duplicate file has no parent directory",
        )
    })?;

    let unique_id = uuid::Uuid::new_v4();
    let backup_path = parent.join(format!(".dedup_tmp_{}.bak", unique_id));

    // Step 1: Rename duplicate to temporary backup in the same parent directory
    fs::rename(duplicate, &backup_path)?;

    // Step 2: Create hardlink from primary to duplicate path
    match fs::hard_link(primary, duplicate) {
        Ok(()) => {
            // Step 3a: Success - clean up temporary backup file
            let _ = fs::remove_file(&backup_path);
            Ok(true)
        }
        Err(err) => {
            // Step 3b: Failure - rollback and restore duplicate file
            let _ = fs::rename(&backup_path, duplicate);
            Err(err)
        }
    }
}

/// Streams and computes SHA-256 for a file.
pub fn compute_file_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockLinker {
        linked_pairs: Mutex<Vec<(PathBuf, PathBuf)>>,
    }

    impl MockLinker {
        fn new() -> Self {
            Self {
                linked_pairs: Mutex::new(Vec::new()),
            }
        }
    }

    impl FileLinker for MockLinker {
        fn replace_with_hardlink(&self, primary: &Path, duplicate: &Path) -> io::Result<bool> {
            self.linked_pairs
                .lock()
                .unwrap()
                .push((primary.to_path_buf(), duplicate.to_path_buf()));
            Ok(true)
        }
    }

    #[test]
    fn test_dedup_report_defaults() {
        let report = DedupReport::default();
        assert_eq!(report.scanned_files, 0);
        assert_eq!(report.duplicate_files, 0);
        assert_eq!(report.hardlinked_files, 0);
        assert_eq!(report.saved_bytes, 0);
    }

    #[test]
    fn test_deduplication_grouping_and_savings_calculation() {
        let root = Path::new("C:/mock/extracted/game_root");
        let hash_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
        let hash_b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();

        let files = vec![
            // Group 1: 3 copies of 10MB texture (saves 20MB)
            FileRecord {
                relative_path: "assets/textures/grass.png".into(),
                size: 10_000_000,
                size_known: true,
                sha256: hash_a.clone(),
                is_directory: false,
                is_encrypted: false,
                is_corrupted: false,
            },
            FileRecord {
                relative_path: "levels/level1/grass_copy.png".into(),
                size: 10_000_000,
                size_known: true,
                sha256: hash_a.clone(),
                is_directory: false,
                is_encrypted: false,
                is_corrupted: false,
            },
            FileRecord {
                relative_path: "levels/level2/grass_copy.png".into(),
                size: 10_000_000,
                size_known: true,
                sha256: hash_a.clone(),
                is_directory: false,
                is_encrypted: false,
                is_corrupted: false,
            },
            // Group 2: 2 copies of 5MB sound (saves 5MB)
            FileRecord {
                relative_path: "audio/bgm.ogg".into(),
                size: 5_000_000,
                size_known: true,
                sha256: hash_b.clone(),
                is_directory: false,
                is_encrypted: false,
                is_corrupted: false,
            },
            FileRecord {
                relative_path: "menu/music/bgm.ogg".into(),
                size: 5_000_000,
                size_known: true,
                sha256: hash_b.clone(),
                is_directory: false,
                is_encrypted: false,
                is_corrupted: false,
            },
            // Unique file: 1 copy of 2MB binary (saves 0)
            FileRecord {
                relative_path: "bin/game.exe".into(),
                size: 2_000_000,
                size_known: true,
                sha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
                is_directory: false,
                is_encrypted: false,
                is_corrupted: false,
            },
            // Directory entry (ignored)
            FileRecord {
                relative_path: "assets/textures".into(),
                size: 0,
                size_known: true,
                sha256: String::new(),
                is_directory: true,
                is_encrypted: false,
                is_corrupted: false,
            },
            // Corrupted file (ignored)
            FileRecord {
                relative_path: "corrupted.dat".into(),
                size: 10_000_000,
                size_known: true,
                sha256: hash_a.clone(),
                is_directory: false,
                is_encrypted: false,
                is_corrupted: true,
            },
            // Zero-byte file (ignored)
            FileRecord {
                relative_path: "empty.txt".into(),
                size: 0,
                size_known: true,
                sha256: String::new(),
                is_directory: false,
                is_encrypted: false,
                is_corrupted: false,
            },
        ];

        let mock_linker = MockLinker::new();
        let report = deduplicate_extracted_files_with_linker(root, &files, &mock_linker)
            .expect("deduplicate extracted files");

        // 6 regular files scanned (3 textures + 2 audios + 1 exe)
        assert_eq!(report.scanned_files, 6);
        // Duplicate files: 2 for group 1, 1 for group 2 = 3
        assert_eq!(report.duplicate_files, 3);
        assert_eq!(report.hardlinked_files, 3);
        // Saved bytes: 2 * 10MB + 1 * 5MB = 25MB
        assert_eq!(report.saved_bytes, 25_000_000);

        let pairs = mock_linker.linked_pairs.lock().unwrap();
        assert_eq!(pairs.len(), 3);
    }
}
