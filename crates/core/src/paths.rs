use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub const OWNER_FILE: &str = ".openunpack-owner.json";

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
    if path.is_empty() || path.len() > 30000 || path.starts_with(['/', '\\']) {
        return Err("UnsafePath".into());
    }
    let mut result = PathBuf::new();
    for part in path.split(['/', '\\']) {
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
    check_path(path)?;
    let parent = path.parent().ok_or("没有父目录。")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建数据目录。")?;
    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| "无法保存配置。")?;
        let mut writer = std::io::BufWriter::with_capacity(64 * 1024, f);
        serde_json::to_writer(&mut writer, value).map_err(|_| "无法序列化配置。")?;
        writer
            .flush()
            .and_then(|_| writer.get_ref().sync_all())
            .map_err(|_| "无法刷新配置。")?;
        drop(writer);
        let a: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            if windows_sys::Win32::Storage::FileSystem::MoveFileExW(a.as_ptr(), b.as_ptr(), 1 | 8)
                == 0
            {
                return Err("无法原子提交配置。".into());
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    read_json_limited(path, 16 * 1024 * 1024)
}
pub fn read_json_limited<T: DeserializeOwned>(path: &Path, max_bytes: u64) -> Result<T, String> {
    check_path(path)?;
    if fs::metadata(path).map_err(|_| "数据文件不可读。")?.len() > max_bytes {
        return Err("数据文件过大。".into());
    }
    let file = fs::File::open(path).map_err(|_| "数据文件不可读。")?;
    serde_json::from_reader(std::io::BufReader::with_capacity(64 * 1024, file))
        .map_err(|_| "数据文件损坏；原文件已保留。".into())
}
pub fn same_volume(a: &Path, b: &Path) -> Result<bool, String> {
    let volume = |path: &Path| -> Result<Vec<u16>, String> {
        check_path(path)?;
        let p: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut root = vec![0u16; 32768];
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW(
                p.as_ptr(),
                root.as_mut_ptr(),
                root.len() as u32,
            )
        } == 0
        {
            return Err("无法确认输出卷，已停止直接发布。".into());
        }
        root.truncate(root.iter().position(|u| *u == 0).unwrap_or(root.len()));
        Ok(root)
    };
    Ok(String::from_utf16_lossy(&volume(a)?)
        .eq_ignore_ascii_case(&String::from_utf16_lossy(&volume(b)?)))
}
/// Atomic target-volume commit, with replacement explicitly disabled. A concurrent
/// creator of the selected final name is never overwritten, even if it is empty.
pub fn publish_directory(source: &Path, target: &Path) -> Result<(), String> {
    check_path(source)?;
    check_path(target)?;
    let a: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { windows_sys::Win32::Storage::FileSystem::MoveFileExW(a.as_ptr(), b.as_ptr(), 8) }
        == 0
    {
        return Err("目标目录提交失败；临时结果已保留待核对。".into());
    }
    Ok(())
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Owner {
    application: String,
    id: String,
    manifest: String,
}
pub fn mark_owned(path: &Path, id: &str, manifest: &str) -> Result<(), String> {
    check_path(path)?;
    fs::create_dir(path).map_err(|_| "无法创建独占工作目录。")?;
    atomic_json(
        &path.join(OWNER_FILE),
        &Owner {
            application: "OpenUnpack/1".into(),
            id: id.into(),
            manifest: manifest.into(),
        },
    )
}
/// Attach publication ownership to an already validated extraction directory.
/// Archive entries are prohibited from using OWNER_FILE, so this never overwrites
/// a user-created marker or changes ownership of an existing output folder.
pub fn tag_existing(path: &Path, id: &str, manifest: &str) -> Result<(), String> {
    check_path(path)?;
    if !path.is_dir() || path.join(OWNER_FILE).exists() {
        return Err("发布目录不能取得独占标记。".into());
    }
    atomic_json(
        &path.join(OWNER_FILE),
        &Owner {
            application: "OpenUnpack/1".into(),
            id: id.into(),
            manifest: manifest.into(),
        },
    )
}
pub fn owned(path: &Path, id: &str, manifest: Option<&str>) -> Result<(), String> {
    check_path(path)?;
    let o: Owner = read_json(&path.join(OWNER_FILE))?;
    if o.application != "OpenUnpack/1" || o.id != id || manifest.is_some_and(|m| m != o.manifest) {
        return Err("工作目录标记不匹配，已停止清理。".into());
    }
    Ok(())
}
pub fn remove_owned(path: &Path, id: &str) -> Result<(), String> {
    owned(path, id, None)?;
    if path.join(".openunpack-exit-unconfirmed.json").exists() {
        return Err("WorkerExitUnconfirmed".into());
    }
    // Validate descendants before any recursive removal. Never follow links.
    for entry in walkdir::WalkDir::new(path).follow_links(false) {
        let entry = entry.map_err(|_| "工作目录不可检查。")?;
        check_path(entry.path())?;
    }
    fs::remove_dir_all(path).map_err(|_| "工作目录清理失败，可稍后重试。".into())
}
/// Candidate directories do not each carry an ownership marker. Their only valid
/// cleanup authority is the enclosing exclusive workspace, with no link traversal.
pub fn remove_inside_owned(path: &Path, workspace: &Path, id: &str) -> Result<(), String> {
    owned(workspace, id, None)?;
    if workspace.join(".openunpack-exit-unconfirmed.json").exists() {
        return Err("WorkerExitUnconfirmed".into());
    }
    let relative_path = path.strip_prefix(workspace).map_err(|_| "UnsafePath")?;
    if relative_path.as_os_str().is_empty() {
        return Err("UnsafePath".into());
    }
    relative(&path_string(relative_path))?;
    for entry in walkdir::WalkDir::new(path).follow_links(false) {
        check_path(entry.map_err(|_| "IoError")?.path())?;
    }
    fs::remove_dir_all(path).map_err(|_| "IoError".into())
}
pub fn remove_file_inside_owned(path: &Path, workspace: &Path, id: &str) -> Result<(), String> {
    owned(workspace, id, None)?;
    if workspace.join(".openunpack-exit-unconfirmed.json").exists() {
        return Err("WorkerExitUnconfirmed".into());
    }
    let r = path.strip_prefix(workspace).map_err(|_| "UnsafePath")?;
    relative(&path_string(r))?;
    check_path(path)?;
    if !path.is_file() {
        return Err("UnsafePath".into());
    }
    fs::remove_file(path).map_err(|_| "IoError".into())
}
pub fn tree_size(path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    check_path(path)?;
    let mut bytes = 0u64;
    for entry in walkdir::WalkDir::new(path).follow_links(false) {
        let entry = entry.map_err(|_| "缓存目录不可检查。")?;
        check_path(entry.path())?;
        if entry.file_type().is_file() {
            bytes = bytes
                .checked_add(entry.metadata().map_err(|_| "缓存大小不可读。")?.len())
                .ok_or("缓存大小溢出。")?;
        }
    }
    Ok(bytes)
}
pub fn free_bytes(path: &Path) -> Result<u64, String> {
    let ancestor = path
        .ancestors()
        .find(|p| p.exists())
        .ok_or("磁盘路径不可用。")?;
    check_path(ancestor)?;
    let p: Vec<u16> = ancestor.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0;
    let mut total = 0;
    let mut free = 0;
    unsafe {
        if windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            p.as_ptr(),
            &mut available,
            &mut total,
            &mut free,
        ) == 0
        {
            return Err("无法读取剩余磁盘空间。".into());
        }
    }
    Ok(available)
}

/// Send file or directory to Windows Recycle Bin via SHFileOperationW
pub fn move_to_recycle_bin(path: &Path) -> Result<(), String> {
    check_path(path)?;
    if !path.exists() {
        return Ok(());
    }
    // SHFileOperation does not support \\?\ extended-length syntax (returns error 124).
    let path_str = path.to_string_lossy();
    let clean_path = if let Some(stripped) = path_str.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", stripped)
    } else if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        path_str.into_owned()
    };

    let mut wide: Vec<u16> = std::ffi::OsStr::new(&clean_path).encode_wide().collect();
    wide.push(0);
    wide.push(0);

    let mut file_op = windows_sys::Win32::UI::Shell::SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: windows_sys::Win32::UI::Shell::FO_DELETE,
        pFrom: wide.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (windows_sys::Win32::UI::Shell::FOF_ALLOWUNDO
            | windows_sys::Win32::UI::Shell::FOF_NOCONFIRMATION
            | windows_sys::Win32::UI::Shell::FOF_NOERRORUI
            | windows_sys::Win32::UI::Shell::FOF_SILENT) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };

    let ret = unsafe { windows_sys::Win32::UI::Shell::SHFileOperationW(&mut file_op) };
    if ret != 0 || file_op.fAnyOperationsAborted != 0 {
        return Err(format!("移入回收站失败 (代码: {})", ret));
    }
    Ok(())
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetThreadExecutionState(es_flags: u32) -> u32;
    fn DeviceIoControl(
        hdevice: *mut std::ffi::c_void,
        dwiocontrolcode: u32,
        lpinbuffer: *const std::ffi::c_void,
        ninbuffersize: u32,
        lpoutbuffer: *mut std::ffi::c_void,
        noutbuffersize: u32,
        lpbytesreturned: *mut u32,
        lpoverlapped: *mut std::ffi::c_void,
    ) -> i32;
    fn CloseHandle(hobject: *mut std::ffi::c_void) -> i32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveMedium {
    Ssd,
    Hdd,
    Unknown,
}

pub fn probe_drive_medium(path: &Path) -> DriveMedium {
    let drive_letter = match drive_letter_of(path) {
        Some(letter) => letter,
        None => return DriveMedium::Unknown,
    };
    let volume_path: Vec<u16> = format!(r"\\.\{}:", drive_letter).encode_utf16().chain(Some(0)).collect();
    let handle = unsafe {
        windows_sys::Win32::Storage::FileSystem::CreateFileW(
            volume_path.as_ptr(),
            0,
            windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
                | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
            std::ptr::null(),
            windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return DriveMedium::Unknown;
    }

    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D1400;
    const STORAGE_DEVICE_SEEK_PENALTY_PROPERTY: u32 = 7;
    const PROPERTY_STANDARD_QUERY: u32 = 0;

    #[repr(C)]
    struct StoragePropertyQuery {
        property_id: u32,
        query_type: u32,
        additional_parameters: [u8; 1],
    }

    #[repr(C)]
    struct DeviceSeekPenaltyDescriptor {
        version: u32,
        size: u32,
        incurs_seek_penalty: u8,
    }

    let query = StoragePropertyQuery {
        property_id: STORAGE_DEVICE_SEEK_PENALTY_PROPERTY,
        query_type: PROPERTY_STANDARD_QUERY,
        additional_parameters: [0],
    };
    let mut descriptor = DeviceSeekPenaltyDescriptor {
        version: 0,
        size: 0,
        incurs_seek_penalty: 0,
    };
    let mut returned = 0u32;
    let res = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            &query as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<StoragePropertyQuery>() as u32,
            &mut descriptor as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<DeviceSeekPenaltyDescriptor>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    unsafe {
        CloseHandle(handle);
    }

    if res != 0 {
        if descriptor.incurs_seek_penalty != 0 {
            DriveMedium::Hdd
        } else {
            DriveMedium::Ssd
        }
    } else {
        DriveMedium::Unknown
    }
}

pub fn probe_physical_device_number(path: &Path) -> Option<u32> {
    let drive_letter = drive_letter_of(path)?;
    let volume_path: Vec<u16> = format!(r"\\.\{}:", drive_letter).encode_utf16().chain(Some(0)).collect();
    let handle = unsafe {
        windows_sys::Win32::Storage::FileSystem::CreateFileW(
            volume_path.as_ptr(),
            0,
            windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
                | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
            std::ptr::null(),
            windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return None;
    }

    const IOCTL_STORAGE_GET_DEVICE_NUMBER: u32 = 0x002D1080;

    #[repr(C)]
    struct StorageDeviceNumber {
        device_type: u32,
        device_number: u32,
        partition_number: u32,
    }

    let mut dev_num = StorageDeviceNumber {
        device_type: 0,
        device_number: 0,
        partition_number: 0,
    };
    let mut returned = 0u32;
    let res = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_GET_DEVICE_NUMBER,
            std::ptr::null(),
            0,
            &mut dev_num as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<StorageDeviceNumber>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    unsafe {
        CloseHandle(handle);
    }

    if res != 0 {
        Some(dev_num.device_number)
    } else {
        None
    }
}

fn drive_letter_of(path: &Path) -> Option<char> {
    let s = path.to_string_lossy();
    let mut chars = s.chars();
    let first = chars.next()?;
    let second = chars.next()?;
    if first.is_ascii_alphabetic() && second == ':' {
        Some(first.to_ascii_uppercase())
    } else {
        None
    }
}

pub fn probe_disk_topology(source: &Path, target: &Path) -> crate::route::DiskTopology {
    let source_medium = probe_drive_medium(source);
    let target_medium = probe_drive_medium(target);
    let source_dev = probe_physical_device_number(source);
    let target_dev = probe_physical_device_number(target);

    let is_same_physical = match (source_dev, target_dev) {
        (Some(s), Some(t)) => s == t,
        _ => same_volume(source, target).unwrap_or(false),
    };

    crate::route::DiskTopology {
        source_is_hdd: source_medium == DriveMedium::Hdd,
        target_is_hdd: target_medium == DriveMedium::Hdd,
        is_same_physical_disk: is_same_physical,
        source_device_number: source_dev,
        target_device_number: target_dev,
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBeep(u_type: u32) -> i32;
}

const ES_CONTINUOUS: u32 = 0x80000000;
const ES_SYSTEM_REQUIRED: u32 = 0x00000001;
const MB_ICONASTERISK: u32 = 0x00000040;

pub struct SleepBlockerGuard;

impl SleepBlockerGuard {
    pub fn acquire() -> Self {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        }
        Self
    }
}

impl Drop for SleepBlockerGuard {
    fn drop(&mut self) {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

pub fn play_completion_sound() {
    unsafe {
        MessageBeep(MB_ICONASTERISK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_windows_aliases_and_traversal() {
        for p in [
            "../a",
            "C:/x",
            "a:ads",
            "a/../b",
            "NUL.txt",
            "folder/COM1",
            "folder/COM¹.txt",
            "LPT²",
            "CON .txt",
            "CONIN$",
            "x.",
            OWNER_FILE,
            "/abs",
        ] {
            assert!(relative(p).is_err(), "{p}")
        }
        assert!(relative("中文/file.txt").is_ok());
    }
    #[test]
    fn owned_cleanup_cannot_remove_unmarked() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("work");
        fs::create_dir(&p).unwrap();
        assert!(remove_owned(&p, "id").is_err());
        assert!(p.exists());
        fs::remove_dir(&p).unwrap();
        mark_owned(&p, "id", "").unwrap();
        assert!(remove_owned(&p, "wrong").is_err());
        remove_owned(&p, "id").unwrap();
        assert!(!p.exists());
    }
    #[test]
    fn publication_never_replaces_an_existing_empty_directory() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("source");
        let target = d.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("data"), b"synthetic committed file").unwrap();
        assert!(publish_directory(&source, &target).is_err());
        assert!(source.join("data").is_file());
        assert!(target.is_dir());
        assert!(!target.join("data").exists());
    }
    #[test]
    fn unresolved_worker_marker_blocks_all_workspace_cleanup() {
        let d = tempfile::tempdir().unwrap();
        let work = d.path().join("work");
        mark_owned(&work, "job", "").unwrap();
        fs::create_dir(work.join("candidate")).unwrap();
        fs::write(work.join(".openunpack-exit-unconfirmed.json"), b"{}").unwrap();
        assert!(remove_owned(&work, "job").is_err());
        assert!(remove_inside_owned(&work.join("candidate"), &work, "job").is_err());
        assert!(work.join("candidate").is_dir());
    }
    #[test]
    fn test_unc_path_stripping() {
        let p_unc = r"\\?\UNC\server\share\file.zip";
        let clean = if let Some(stripped) = p_unc.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{}", stripped)
        } else if let Some(stripped) = p_unc.strip_prefix(r"\\?\") {
            stripped.to_string()
        } else {
            p_unc.to_string()
        };
        assert_eq!(clean, r"\\server\share\file.zip");

        let p_drive = r"\\?\C:\Users\test\file.zip";
        let clean_drive = if let Some(stripped) = p_drive.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{}", stripped)
        } else if let Some(stripped) = p_drive.strip_prefix(r"\\?\") {
            stripped.to_string()
        } else {
            p_drive.to_string()
        };
        assert_eq!(clean_drive, r"C:\Users\test\file.zip");
    }
}
