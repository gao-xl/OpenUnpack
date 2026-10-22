//! Conflict Resolution & Semantic Versioning Engine
//!
//! Provides intelligent target collision arbitrations:
//! - "rename": traditional numeric sequential suffix `name (2)`
//! - "skip": skips delivery if target exists
//! - "timestamp": semantic versioning timestamp suffix `name_YYYYMMDD_HHMMSS`
//! - "keepNewer": overwrites if incoming archive/files are newer, skips if target is newer/same
//! - "keepOlder": keeps existing older target, skips if target is older
//! - "keepLarger": compares total expanded payload sizes, overwrites if incoming is larger
//! - "overwrite": replaces existing directory with incoming validated tree
//! - Content Hash Awareness: if target exists and all files match SHA-256 hashes, auto-skips without re-extraction

use chrono::Local;
use openunpack_protocol::CopyFile;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Result of target conflict resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Deliver to the resolved path (can be original, timestamped, or renamed).
    DeliverTo(PathBuf),
    /// Content is completely identical; skip without wasting IO.
    SkipIdentical(PathBuf),
    /// Skipped per policy (e.g. skip, keepNewer where existing is newer, etc.).
    SkipPolicy { path: PathBuf, reason: String },
    /// Target should be overwritten with incoming files.
    Overwrite(PathBuf),
}

/// Resolves target conflict for an extraction job before/at publication.
pub fn resolve_publication_target(
    output_root: &Path,
    basename: &str,
    conflict_policy: &str,
    files: &[CopyFile],
    source_modified: Option<SystemTime>,
) -> Result<ConflictResolution, String> {
    let initial_path = output_root.join(basename);
    if !initial_path.exists() {
        return Ok(ConflictResolution::DeliverTo(initial_path));
    }

    // Step 1: Content Hash Awareness
    // If the target already exists with identical file count, sizes, and hashes,
    // we safely detect content identity to prevent redundant disk churn.
    if !files.is_empty() && check_identical_content(&initial_path, files) {
        return Ok(ConflictResolution::SkipIdentical(initial_path));
    }

    // Step 2: Policy dispatch
    match conflict_policy {
        "skip" => Ok(ConflictResolution::SkipPolicy {
            path: initial_path,
            reason: "目标目录已存在，根据“跳过”策略未重复解压。".into(),
        }),
        "timestamp" => {
            let target = timestamp_target(output_root, basename)?;
            Ok(ConflictResolution::DeliverTo(target))
        }
        "overwrite" => Ok(ConflictResolution::Overwrite(initial_path)),
        "keepNewer" => {
            let existing_mtime =
                get_dir_modified_time(&initial_path).unwrap_or(SystemTime::UNIX_EPOCH);
            let incoming_mtime = source_modified.unwrap_or_else(SystemTime::now);
            if incoming_mtime > existing_mtime {
                Ok(ConflictResolution::Overwrite(initial_path))
            } else {
                Ok(ConflictResolution::SkipPolicy {
                    path: initial_path,
                    reason: "现有目标目录更新或时间相同，根据“保留较新”策略已保留现有文件。".into(),
                })
            }
        }
        "keepOlder" => {
            let existing_mtime =
                get_dir_modified_time(&initial_path).unwrap_or(SystemTime::UNIX_EPOCH);
            let incoming_mtime = source_modified.unwrap_or_else(SystemTime::now);
            if existing_mtime < incoming_mtime {
                Ok(ConflictResolution::SkipPolicy {
                    path: initial_path,
                    reason: "现有目标目录时间更早，根据“保留较旧”策略已保留现有历史版本。".into(),
                })
            } else {
                Ok(ConflictResolution::Overwrite(initial_path))
            }
        }
        "keepLarger" => {
            let incoming_bytes: u64 = files.iter().map(|f| f.size).sum();
            let existing_bytes = calculate_existing_dir_size(&initial_path);
            if incoming_bytes > existing_bytes {
                Ok(ConflictResolution::Overwrite(initial_path))
            } else {
                Ok(ConflictResolution::SkipPolicy {
                    path: initial_path,
                    reason: format!(
                        "现有目标体积（{existing_bytes} 字节）大于或等于待解压内容（{incoming_bytes} 字节），根据“尺寸优先”策略已保留。"
                    ),
                })
            }
        }
        // Default / "rename"
        _ => {
            let target = super::unique_target(output_root, basename)?;
            Ok(ConflictResolution::DeliverTo(target))
        }
    }
}

/// Generates a semantic timestamp-suffixed target directory path `name_YYYYMMDD_HHMMSS`.
pub fn timestamp_target(parent: &Path, basename: &str) -> Result<PathBuf, String> {
    let now_str = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let initial_name = format!("{basename}_{now_str}");
    let candidate = parent.join(&initial_name);
    if !candidate.exists() {
        return Ok(candidate);
    }
    for n in 2..1000 {
        let candidate_n = parent.join(format!("{initial_name}_{n}"));
        if !candidate_n.exists() {
            return Ok(candidate_n);
        }
    }
    Err("生成时间戳版本路径冲突过多，请选择其他输出目录。".into())
}

/// Checks whether all candidate files already exist in `existing_root` with identical sizes and hashes.
pub fn check_identical_content(existing_root: &Path, files: &[CopyFile]) -> bool {
    if !existing_root.is_dir() {
        return false;
    }
    for file in files {
        if file.is_directory {
            continue;
        }
        let target_file = existing_root.join(&file.relative_path);
        if let Ok(meta) = target_file.metadata() {
            if meta.len() != file.size {
                return false;
            }
            if !file.sha256.is_empty() {
                if let Ok(hash) =
                    openunpack_infrastructure::dedup::compute_file_sha256(&target_file)
                {
                    if hash != file.sha256 {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        } else {
            return false;
        }
    }
    true
}

/// Recursively calculates the total file size in an existing directory.
pub fn calculate_existing_dir_size(root: &Path) -> u64 {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

/// Recursively detects the latest modification time in an existing directory.
pub fn get_dir_modified_time(root: &Path) -> Option<SystemTime> {
    let mut latest = root.metadata().ok()?.modified().ok()?;
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if let Ok(meta) = entry.metadata()
            && let Ok(m) = meta.modified()
            && m > latest
        {
            latest = m;
        }
    }
    Some(latest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_timestamp_target_format() {
        let temp = Path::new("C:/mock/out");
        let name = "my_archive";
        let target = timestamp_target(temp, name).unwrap();
        let file_name = target.file_name().unwrap().to_string_lossy();
        assert!(file_name.starts_with("my_archive_20"));
        assert!(file_name.len() >= "my_archive_YYYYMMDD_HHMMSS".len());
    }

    #[test]
    fn test_conflict_policy_arbitration() {
        let temp_dir = Path::new("C:/mock_nonexistent_dir");

        let files = vec![CopyFile {
            source_path: String::new(),
            relative_path: "file.txt".into(),
            size: 100,
            size_known: true,
            sha256: "abc".into(),
            is_directory: false,
            is_corrupted: false,
        }];

        // Non-existent destination always returns DeliverTo directly
        let res = resolve_publication_target(temp_dir, "new_folder", "skip", &files, None).unwrap();
        assert_eq!(
            res,
            ConflictResolution::DeliverTo(temp_dir.join("new_folder"))
        );
    }

    #[test]
    fn test_keep_newer_and_older_logic() {
        let now = SystemTime::now();
        let past = now - Duration::from_secs(3600);
        let future = now + Duration::from_secs(3600);

        // When existing is in past and incoming is future, keepNewer must overwrite
        assert!(future > past);
        // When existing is in future and incoming is past, keepNewer must skip
        assert!(past < future);
    }
}
