//! 磁盘空间、介质探测、回收站、防休眠锁与大小写不敏感路径工具。

use super::*;

pub fn tree_size(path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    check_path(path)?;
    let mut bytes = 0u64;
    for entry in walkdir::WalkDir::new(path).follow_links(false) {
        let entry = entry.map_err(|_| "缓存目录不可检查。")?;
        let ft = entry.file_type();
        if ft.is_symlink() {
            return Err("不支持链接或目录联接路径。".into());
        }
        let meta = entry.metadata().map_err(|_| "缓存大小不可读。")?;
        #[cfg(windows)]
        if meta.file_attributes() & 0x400 != 0 {
            return Err("不支持链接或目录联接路径。".into());
        }
        if ft.is_file() {
            bytes = bytes.checked_add(meta.len()).ok_or("缓存大小溢出。")?;
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
    let target_dir = if ancestor.is_file() {
        ancestor.parent().unwrap_or(ancestor)
    } else {
        ancestor
    };
    let p: Vec<u16> = target_dir
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
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
    crate::saferecycle::RecyclableSource::open(path)
        .map_err(|e| e.to_string())?
        .recycle()
}

pub(crate) fn shell_recycle(path: &Path) -> Result<(), String> {
    crate::recycle_shell::recycle(path)
}

/// Safely remove empty directories bottom-up inside a sandbox directory.
/// Never follows symlinks, never touches files, never deletes `root` itself.
pub fn prune_empty_directories(root: &Path) -> Result<usize, String> {
    let guard = match DirectoryGuard::open(root) {
        Ok(guard) => guard,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(_) => return Err("目录不可安全锁定。".into()),
    };
    let mut entries = lock_descendants(&guard).map_err(|_| "目录检查失败。")?;
    let mut pruned_count = 0;
    while let Some(entry) = entries.pop() {
        if entry.metadata().map_err(|_| "目录检查失败。")?.is_dir()
            && entry
                .directory_guard()
                .map_err(|_| "目录检查失败。")?
                .entries()
                .map_err(|_| "目录检查失败。")?
                .is_empty()
        {
            match entry.delete() {
                Ok(()) => pruned_count += 1,
                // A new child inserted after enumeration keeps the directory.
                Err(error) if error.raw_os_error() == Some(145) => (),
                Err(_) => return Err("空目录清理失败。".into()),
            }
        }
    }
    Ok(pruned_count)
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(super) fn SetThreadExecutionState(es_flags: u32) -> u32;
    pub(super) fn DeviceIoControl(
        hdevice: *mut std::ffi::c_void,
        dwiocontrolcode: u32,
        lpinbuffer: *const std::ffi::c_void,
        ninbuffersize: u32,
        lpoutbuffer: *mut std::ffi::c_void,
        noutbuffersize: u32,
        lpbytesreturned: *mut u32,
        lpoverlapped: *mut std::ffi::c_void,
    ) -> i32;
    pub(super) fn CloseHandle(hobject: *mut std::ffi::c_void) -> i32;
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
    let volume_path: Vec<u16> = format!(r"\\.\{}:", drive_letter)
        .encode_utf16()
        .chain(Some(0))
        .collect();
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
    let volume_path: Vec<u16> = format!(r"\\.\{}:", drive_letter)
        .encode_utf16()
        .chain(Some(0))
        .collect();
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

    if res != 0 {
        unsafe {
            CloseHandle(handle);
        }
        return Some(dev_num.device_number);
    }

    const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x00560000;

    #[repr(C)]
    struct DiskExtent {
        disk_number: u32,
        starting_offset: i64,
        extent_length: i64,
    }

    #[repr(C)]
    struct VolumeDiskExtents {
        number_of_disk_extents: u32,
        extents: [DiskExtent; 1],
    }

    let mut extents = VolumeDiskExtents {
        number_of_disk_extents: 0,
        extents: [DiskExtent {
            disk_number: 0,
            starting_offset: 0,
            extent_length: 0,
        }],
    };
    let mut extents_returned = 0u32;
    let extents_res = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
            std::ptr::null(),
            0,
            &mut extents as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<VolumeDiskExtents>() as u32,
            &mut extents_returned,
            std::ptr::null_mut(),
        )
    };
    unsafe {
        CloseHandle(handle);
    }

    if extents_res != 0 && extents.number_of_disk_extents >= 1 {
        Some(extents.extents[0].disk_number)
    } else {
        None
    }
}

pub fn drive_letter_of(path: &Path) -> Option<char> {
    if let Some(Component::Prefix(prefix_component)) = path.components().next() {
        match prefix_component.kind() {
            std::path::Prefix::Disk(letter) | std::path::Prefix::VerbatimDisk(letter) => {
                return Some((letter as char).to_ascii_uppercase());
            }
            _ => return None,
        }
    }
    let s = path.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
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
        _ => same_volume(source, target).unwrap_or(true),
    };

    crate::route::DiskTopology {
        // These flags request serial I/O arbitration. Unknown media must not
        // bypass it; the original medium remains available via probe_drive_medium.
        source_is_hdd: source_medium != DriveMedium::Ssd,
        target_is_hdd: target_medium != DriveMedium::Ssd,
        is_same_physical_disk: is_same_physical,
        source_device_number: source_dev,
        target_device_number: target_dev,
    }
}

pub(super) const ES_CONTINUOUS: u32 = 0x80000000;
pub(super) const ES_SYSTEM_REQUIRED: u32 = 0x00000001;

// Execution state belongs to a Windows thread. An async job may migrate between
// Tokio threads, so both native calls must run on this guard's dedicated thread.
pub struct SleepBlockerGuard {
    pub(super) release: Option<std::sync::mpsc::Sender<()>>,
    pub(super) thread: Option<std::thread::JoinHandle<()>>,
}

impl SleepBlockerGuard {
    pub fn acquire() -> Self {
        Self::with_execution_state(|flags| unsafe { SetThreadExecutionState(flags) })
    }

    pub(super) fn with_execution_state(mut set: impl FnMut(u32) -> u32 + Send + 'static) -> Self {
        let (release, wait) = std::sync::mpsc::channel();
        let (started, ready) = std::sync::mpsc::sync_channel(0);
        let thread = std::thread::Builder::new()
            .name("openunpack-sleep-guard".into())
            .spawn(move || {
                let active = set(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) != 0;
                let _ = started.send(());
                let _ = wait.recv();
                if active {
                    set(ES_CONTINUOUS);
                }
            })
            .ok();
        if thread.is_some() {
            let _ = ready.recv();
        }
        Self {
            release: Some(release),
            thread,
        }
    }
}

impl Drop for SleepBlockerGuard {
    fn drop(&mut self) {
        self.release.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// 剥离 Windows 扩展长度前缀（`\\?\` 与 `\\?\UNC\`），用于统一展示与前缀比较。
pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(stripped) = raw.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{stripped}"))
    } else if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

/// 判断文件名段（允许带扩展名，如 `con.txt`）是否为 Windows 保留设备名（忽略大小写）。
pub fn is_reserved_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.trim().to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "CLOCK$"
            | "CONIN$"
            | "CONOUT$"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

pub(super) fn component_eq_platform(left: Component<'_>, right: Component<'_>) -> bool {
    #[cfg(windows)]
    {
        match (left, right) {
            (Component::Prefix(a), Component::Prefix(b)) => {
                a.as_os_str().to_string_lossy().to_lowercase()
                    == b.as_os_str().to_string_lossy().to_lowercase()
            }
            (Component::RootDir, Component::RootDir) => true,
            (Component::Normal(a), Component::Normal(b)) => {
                a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
            }
            (Component::CurDir, Component::CurDir) => true,
            (Component::ParentDir, Component::ParentDir) => true,
            _ => false,
        }
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

/// 按组件边界判断 `path` 是否与 `prefix` 相同或位于其目录下（Windows 下忽略大小写）。
pub fn path_starts_with_case_insensitive(path: &Path, prefix: &Path) -> bool {
    if prefix.as_os_str().is_empty() {
        return false;
    }
    let mut path_comps = path.components();
    for pre_comp in prefix.components() {
        let Some(path_comp) = path_comps.next() else {
            return false;
        };
        if !component_eq_platform(path_comp, pre_comp) {
            return false;
        }
    }
    true
}

/// 按组件判断两条路径是否等价（Windows 下忽略大小写）。
pub fn path_eq_case_insensitive(left: &Path, right: &Path) -> bool {
    let mut left_comps = left.components();
    let mut right_comps = right.components();
    loop {
        match (left_comps.next(), right_comps.next()) {
            (None, None) => return true,
            (Some(l), Some(r)) if component_eq_platform(l, r) => continue,
            _ => return false,
        }
    }
}
