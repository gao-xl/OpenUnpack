//! Structured domain errors and precise diagnosis metadata.
//!
//! Replaces ambiguous string collapses (such as plain "StorageError" or "ResourceLimit")
//! with actionable, typed errors carrying byte budgets, occupied PIDs, concrete paths,
//! and troubleshooting guidance.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Precise, structured domain unpack error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UnpackError {
    /// Database or metadata storage write/read failure with detailed root cause.
    #[serde(rename_all = "camelCase")]
    Storage {
        path: PathBuf,
        detail: String,
        is_busy_lock: bool,
    },
    /// Disk space exhausted or quota exceeded.
    #[serde(rename_all = "camelCase")]
    DiskFull {
        path: PathBuf,
        needed_bytes: u64,
        available_bytes: Option<u64>,
    },
    /// Target file/directory locked or in use by another process.
    #[serde(rename_all = "camelCase")]
    FileLocked {
        path: PathBuf,
        occupying_pid: Option<u32>,
        process_name: Option<String>,
    },
    /// Permission denied on specific directory or file.
    #[serde(rename_all = "camelCase")]
    PermissionDenied {
        path: PathBuf,
        required_access: String,
    },
    /// Archive is encrypted but no matching password was found.
    #[serde(rename_all = "camelCase")]
    NeedsPassword {
        archive_path: PathBuf,
        tried_count: usize,
    },
    /// Archive headers or payload corrupt/truncated.
    #[serde(rename_all = "camelCase")]
    CorruptedArchive {
        archive_path: PathBuf,
        detail: String,
    },
    /// Split multi-volume set is missing required segments.
    #[serde(rename_all = "camelCase")]
    MissingVolume {
        archive_path: PathBuf,
        missing_volume: String,
    },
    /// Extraction target conflict under current non-overwrite policy.
    #[serde(rename_all = "camelCase")]
    TargetConflict { existing_path: PathBuf },
    /// Background extraction worker or child process communication failure.
    #[serde(rename_all = "camelCase")]
    WorkerFailed { code: String, detail: String },
    /// General unhandled or platform I/O error.
    #[serde(rename_all = "camelCase")]
    Io {
        path: Option<PathBuf>,
        message: String,
    },
}

impl UnpackError {
    pub fn storage(path: impl Into<PathBuf>, detail: impl Into<String>) -> Self {
        let detail_str = detail.into();
        let is_busy = detail_str.contains("busy") || detail_str.contains("locked");
        Self::Storage {
            path: path.into(),
            detail: detail_str,
            is_busy_lock: is_busy,
        }
    }

    pub fn disk_full(path: impl Into<PathBuf>, needed: u64, available: Option<u64>) -> Self {
        Self::DiskFull {
            path: path.into(),
            needed_bytes: needed,
            available_bytes: available,
        }
    }

    pub fn file_locked(path: impl Into<PathBuf>, pid: Option<u32>, name: Option<String>) -> Self {
        Self::FileLocked {
            path: path.into(),
            occupying_pid: pid,
            process_name: name,
        }
    }

    pub fn permission_denied(path: impl Into<PathBuf>, access: impl Into<String>) -> Self {
        Self::PermissionDenied {
            path: path.into(),
            required_access: access.into(),
        }
    }

    /// Formats a user-friendly and diagnostic-rich Chinese description.
    pub fn display_message(&self) -> String {
        match self {
            Self::Storage {
                path,
                detail,
                is_busy_lock,
            } => {
                if *is_busy_lock {
                    format!(
                        "任务数据库正被其他进程或事务占用（{}），已安全挂起并等待释放：{}",
                        path.display(),
                        detail
                    )
                } else {
                    format!(
                        "任务数据库读写失败（{}），请检查磁盘健康状况：{}",
                        path.display(),
                        detail
                    )
                }
            }
            Self::DiskFull {
                path,
                needed_bytes,
                available_bytes,
            } => {
                let mb = |b: u64| format!("{:.1} MB", (b as f64) / 1024.0 / 1024.0);
                match available_bytes {
                    Some(avail) => format!(
                        "磁盘空间不足：{} 剩余 {}，本次解压需要至少 {}；请清理磁盘后重试。",
                        path.display(),
                        mb(*avail),
                        mb(*needed_bytes)
                    ),
                    None => format!(
                        "磁盘空间不足：{} 需要至少 {} 可用空间；请清理磁盘后重试。",
                        path.display(),
                        mb(*needed_bytes)
                    ),
                }
            }
            Self::FileLocked {
                path,
                occupying_pid,
                process_name,
            } => {
                let proc_info = match (occupying_pid, process_name) {
                    (Some(pid), Some(name)) => format!("（占用进程: {} [PID: {}]）", name, pid),
                    (Some(pid), None) => format!("（占用进程 PID: {}）", pid),
                    _ => String::new(),
                };
                format!(
                    "目标文件已被其他程序打开占用{}，无法写入或移动：{}；请关闭占用程序后重试。",
                    proc_info,
                    path.display()
                )
            }
            Self::PermissionDenied {
                path,
                required_access,
            } => {
                format!(
                    "没有足够的系统权限访问路径（缺少 {} 权限）：{}；请以管理员权限运行或调整目录权限。",
                    required_access,
                    path.display()
                )
            }
            Self::NeedsPassword {
                archive_path,
                tried_count,
            } => {
                format!(
                    "已尝试 {} 个候选密码均未命中，压缩包需要口令解密：{}",
                    tried_count,
                    archive_path.display()
                )
            }
            Self::CorruptedArchive {
                archive_path,
                detail,
            } => {
                format!(
                    "压缩包文件损坏或 CRC 校验不符（{}）：{}",
                    archive_path.display(),
                    detail
                )
            }
            Self::MissingVolume {
                archive_path,
                missing_volume,
            } => {
                format!(
                    "多分卷压缩包缺少分卷文件：{} 缺少 {}，请补齐所有分卷。",
                    archive_path.display(),
                    missing_volume
                )
            }
            Self::TargetConflict { existing_path } => {
                format!(
                    "输出目录已存在同名内容且冲突策略为跳过：{}；原压缩包已安全保留。",
                    existing_path.display()
                )
            }
            Self::WorkerFailed { code, detail } => {
                format!("后台解压引擎执行失败 [{}]：{}", code, detail)
            }
            Self::Io { path, message } => {
                if let Some(p) = path {
                    format!("文件读写异常（{}）：{}", p.display(), message)
                } else {
                    format!("系统 I/O 异常：{}", message)
                }
            }
        }
    }

    /// Serializes this error into a structured protocol string that includes structured JSON payload
    /// while remaining recognizable by legacy message string matchers.
    pub fn to_diagnostic_string(&self) -> String {
        let json_part = serde_json::to_string(self).unwrap_or_default();
        format!("{} [UnpackError:{}]", self.display_message(), json_part)
    }

    pub fn parse_envelope(input: &str) -> Option<Self> {
        let tag_start = input.find("[UnpackError:")?;
        let rest = &input[tag_start + "[UnpackError:".len()..];
        let json_str = rest.strip_suffix(']')?;
        serde_json::from_str(json_str).ok()
    }
}

impl std::fmt::Display for UnpackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_message())
    }
}

impl std::error::Error for UnpackError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unpack_error_storage_formatting() {
        let err = UnpackError::storage(r"C:dbopenunpack.db", "database is locked");
        assert!(err.display_message().contains("正被其他进程或事务占用"));
        let diag = err.to_diagnostic_string();
        assert!(diag.contains("[UnpackError:"));
        assert!(diag.contains(r#""isBusyLock":true"#));
    }

    #[test]
    fn test_unpack_error_disk_full_formatting() {
        let err = UnpackError::disk_full(r"D:output", 1024 * 1024 * 500, Some(1024 * 1024 * 50));
        let msg = err.display_message();
        assert!(msg.contains("磁盘空间不足"));
        assert!(msg.contains("剩余 50.0 MB"));
        assert!(msg.contains("需要至少 500.0 MB"));
    }

    #[test]
    fn test_unpack_error_file_locked_formatting() {
        let err =
            UnpackError::file_locked(r"C:	empile.txt", Some(1234), Some("explorer.exe".into()));
        let msg = err.display_message();
        assert!(msg.contains("占用进程: explorer.exe [PID: 1234]"));
    }

    #[test]
    fn test_unpack_error_permission_denied_formatting() {
        let err = UnpackError::permission_denied(r"C:Program Filesapp", "Write");
        let msg = err.display_message();
        assert!(msg.contains("没有足够的系统权限"));
        assert!(msg.contains("缺少 Write 权限"));
    }
}
