//! 文件系统路径安全边界、句柄锁定、所有权管理与 Windows 磁盘/编码辅助。

use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io::{self, Write};
use std::os::windows::{
    ffi::{OsStrExt, OsStringExt},
    fs::{FileExt, MetadataExt, OpenOptionsExt},
    io::{AsRawHandle, FromRawHandle},
};
use std::{
    fs,
    path::{Component, Path, PathBuf, Prefix},
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE},
    Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_BASIC_INFO, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FILE_WRITE_ATTRIBUTES, FileBasicInfo, FileDispositionInfo, FileIdBothDirectoryInfo,
        FileIdBothDirectoryRestartInfo, GetFileInformationByHandleEx, SetFileInformationByHandle,
    },
};

pub const OWNER_FILE: &str = ".openunpack-owner.json";

const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;
const NATIVE_FILE_OPEN: u32 = 1;
const NATIVE_FILE_CREATE: u32 = 2;
const NATIVE_FILE_OPEN_IF: u32 = 3;
// Include application metadata and implicit parent directories in addition to
// the validated 100,000 archive-entry ceiling.
const MAX_GUARDED_ENTRIES: usize = 200_000;
const MAX_GUARDED_DEPTH: usize = 128;
const MAX_DIRECTORY_CHAIN_DEPTH: usize = 256;

mod guard;
mod nt_io;
mod ownership;
mod system_info;

use guard::*;
pub use guard::{DirectoryGuard, LockedEntry, LockedTree};
use nt_io::*;
use ownership::*;
pub use ownership::{
    RetirementError, mark_owned, owned, remove_file_inside_owned, remove_inside_owned,
    remove_owned, remove_owned_retrying, retry_retirement, tag_existing, transient_retirement,
};
pub(crate) use system_info::shell_recycle;
pub use system_info::{
    DriveMedium, SleepBlockerGuard, drive_letter_of, free_bytes, is_reserved_windows_device_name,
    move_to_recycle_bin, path_eq_case_insensitive, path_starts_with_case_insensitive,
    probe_disk_topology, probe_drive_medium, probe_physical_device_number, prune_empty_directories,
    strip_verbatim_prefix, tree_size,
};

/// Reject junctions as well as symlinks in every existing ancestor.
pub fn check_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("路径必须是绝对路径。".into());
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("路径不能包含父目录跳转。".into());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(m) if m.file_attributes() & 0x400 != 0 || m.file_type().is_symlink() => {
                return Err("不支持链接或目录联接路径。".into());
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err("无法检查路径权限。".into()),
        }
    }
    Ok(())
}
pub fn relative(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.split(['/', '\\']).count() > MAX_GUARDED_DEPTH {
        return Err("ResourceLimit".into());
    }
    if trimmed.is_empty() || trimmed.len() > 30000 || trimmed.starts_with(['/', '\\']) {
        return Err("UnsafePath".into());
    }
    let mut result = PathBuf::new();
    for part in trimmed.split(['/', '\\']) {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.contains([':', '\0', '<', '>', '"', '|', '?', '*'])
            || part.ends_with(['.', ' '])
        {
            return Err("UnsafePath".into());
        }
        let stem = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches([' ', '.'])
            .to_ascii_uppercase();
        let device_digit = stem
            .chars()
            .nth(3)
            .is_some_and(|c| matches!(c, '1'..='9' | '¹' | '²' | '³'));
        if matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
        ) || (stem.chars().count() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && device_digit)
        {
            return Err("UnsafePath".into());
        }
        if part.eq_ignore_ascii_case(OWNER_FILE) {
            return Err("归档包含保留的应用标记名称。".into());
        }
        result.push(part);
    }
    Ok(result)
}
pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
pub fn digest<T: Serialize>(value: &T) -> Result<String, String> {
    struct HashWriter(Sha256);
    impl Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, value).map_err(|_| "无法生成清单摘要。")?;
    Ok(hex::encode(writer.0.finalize()))
}
pub fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path.parent().ok_or("没有父目录。")?;
    let guard = DirectoryGuard::create(parent).map_err(|_| "无法创建安全的数据目录。")?;
    atomic_json_guarded(&guard, path, value, true)
}

fn atomic_json_guarded<T: Serialize>(
    parent: &DirectoryGuard,
    path: &Path,
    value: &T,
    replace: bool,
) -> Result<(), String> {
    let temp = parent.path().join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let name = temp.file_name().ok_or("没有文件名。")?;
    let file = open_relative(
        parent,
        Path::new(name),
        GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
        NATIVE_FILE_CREATE,
        false,
    )
    .map_err(|_| "无法保存配置。")?;
    let entry = LockedEntry::from_created(&temp, file, parent).map_err(|_| "无法锁定配置。")?;
    let result = (|| {
        let mut writer = std::io::BufWriter::with_capacity(64 * 1024, entry.file());
        serde_json::to_writer(&mut writer, value).map_err(|_| "无法序列化配置。")?;
        writer
            .flush()
            .and_then(|_| writer.get_ref().sync_all())
            .map_err(|_| "无法刷新配置。")?;
        drop(writer);
        rename_handle(entry.file(), parent, path, replace).map_err(|_| "无法原子提交配置。")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = entry.delete();
    }
    result
}
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    read_json_limited(path, 16 * 1024 * 1024)
}

pub fn open_manifest_file(path: &Path) -> io::Result<LockedEntry> {
    LockedEntry::open(path, true)
}
pub fn read_json_limited<T: DeserializeOwned>(path: &Path, max_bytes: u64) -> Result<T, String> {
    let parent = DirectoryGuard::open(path.parent().ok_or("没有父目录。")?)
        .map_err(|_| "数据目录不可读。")?;
    let file = parent
        .open_read_file(Path::new(path.file_name().ok_or("没有文件名。")?))
        .map_err(|_| "数据文件不可读。")?;
    if file.metadata().map_err(|_| "数据文件不可读。")?.len() > max_bytes {
        return Err("数据文件过大。".into());
    }
    serde_json::from_reader(std::io::BufReader::with_capacity(64 * 1024, file))
        .map_err(|_| "数据文件损坏；原文件已保留。".into())
}
pub fn volume_root(path: &Path) -> Result<PathBuf, String> {
    let ancestor = path.ancestors().find(|p| p.exists()).unwrap_or(path);
    check_path(ancestor)?;
    let p: Vec<u16> = ancestor.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = vec![0u16; 32768];
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW(
            p.as_ptr(),
            root.as_mut_ptr(),
            root.len() as u32,
        )
    } == 0
    {
        return Err("无法确认磁盘卷根路径。".into());
    }
    root.truncate(root.iter().position(|u| *u == 0).unwrap_or(root.len()));
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&root)))
}
pub fn same_volume(a: &Path, b: &Path) -> Result<bool, String> {
    let root_a = volume_root(a)?;
    let root_b = volume_root(b)?;
    Ok(root_a
        .to_string_lossy()
        .eq_ignore_ascii_case(&root_b.to_string_lossy()))
}
/// Atomic target-volume commit, with replacement explicitly disabled. A concurrent
/// creator of the selected final name is never overwritten, even if it is empty.
pub fn publish_directory(source: &Path, target: &Path) -> Result<(), String> {
    let mut tree = LockedTree::open(source).map_err(|_| "无法锁定待发布目录。")?;
    let parent = DirectoryGuard::open(target.parent().ok_or("没有父目录。")?)
        .map_err(|_| "无法锁定目标父目录。")?;
    tree.commit_no_replace(&parent, target)
        .map_err(|_| "目标目录提交失败；临时结果已保留待核对。".into())
}

#[cfg(test)]
use ownership::prune_empty_cache_container;
#[cfg(test)]
use system_info::{DeviceIoControl, ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState};

#[cfg(test)]
mod tests;
