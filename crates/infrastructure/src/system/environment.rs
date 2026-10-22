use openunpack_application::{ApplicationError, SystemEnvironment};
use openunpack_domain::DriveMedium;
use std::path::Path;

pub struct WindowsSystemEnvironment;

impl Default for WindowsSystemEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsSystemEnvironment {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
fn drive_letter_of(path: &Path) -> Option<char> {
    if let Some(std::path::Component::Prefix(prefix_component)) = path.components().next() {
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

#[cfg(windows)]
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

const ES_CONTINUOUS: u32 = 0x80000000;
const ES_SYSTEM_REQUIRED: u32 = 0x00000001;

pub struct SleepLockGuard {
    #[cfg(windows)]
    release: Option<std::sync::mpsc::Sender<()>>,
    #[cfg(windows)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for SleepLockGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            self.release.take();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

impl SystemEnvironment for WindowsSystemEnvironment {
    fn workspace_size(&self, path: &str) -> Result<u64, ApplicationError> {
        let started = std::time::Instant::now();
        let mut pending = vec![std::path::PathBuf::from(path)];
        let mut visited = 0usize;
        let mut bytes = 0u64;
        while let Some(current) = pending.pop() {
            visited += 1;
            if visited > 100_000 || started.elapsed() > std::time::Duration::from_secs(5) {
                return Err(ApplicationError::EnvironmentError(
                    "工作区统计超过扫描预算".into(),
                ));
            }
            let metadata = match std::fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    return Err(ApplicationError::EnvironmentError(
                        "无法读取工作区占用".into(),
                    ));
                }
            };
            let mut redirected = metadata.file_type().is_symlink();
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                redirected |= metadata.file_attributes() & 0x400 != 0;
            }
            if redirected {
                return Err(ApplicationError::EnvironmentError(
                    "工作区包含链接，统计已停止".into(),
                ));
            }
            if metadata.is_file() {
                bytes = bytes.checked_add(metadata.len()).ok_or_else(|| {
                    ApplicationError::EnvironmentError("工作区占用超过计数范围".into())
                })?;
            } else if metadata.is_dir() {
                for entry in std::fs::read_dir(current)
                    .map_err(|_| ApplicationError::EnvironmentError("无法枚举工作区占用".into()))?
                {
                    pending.push(
                        entry
                            .map_err(|_| {
                                ApplicationError::EnvironmentError("无法枚举工作区占用".into())
                            })?
                            .path(),
                    );
                    if pending.len() > 100_000 {
                        return Err(ApplicationError::EnvironmentError(
                            "工作区统计超过扫描预算".into(),
                        ));
                    }
                }
            }
        }
        Ok(bytes)
    }

    fn probe_drive_medium(&self, path: &str) -> DriveMedium {
        #[cfg(windows)]
        {
            let p = Path::new(path);
            let drive_letter = match drive_letter_of(p) {
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
        #[cfg(not(windows))]
        {
            let _ = path;
            DriveMedium::Unknown
        }
    }

    fn available_disk_space(&self, path: &str) -> Result<u64, ApplicationError> {
        let p = Path::new(path);
        if !p.exists() {
            return Err(ApplicationError::EnvironmentError(format!(
                "指定路径不存在: {}",
                path
            )));
        }

        #[cfg(windows)]
        {
            use std::ffi::OsStr;
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

            let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
            let mut free_bytes_available = 0u64;
            let mut total_number_of_bytes = 0u64;
            let mut total_number_of_free_bytes = 0u64;

            unsafe {
                let success = GetDiskFreeSpaceExW(
                    wide.as_ptr(),
                    &mut free_bytes_available,
                    &mut total_number_of_bytes,
                    &mut total_number_of_free_bytes,
                );
                if success != 0 {
                    Ok(free_bytes_available)
                } else {
                    Err(ApplicationError::EnvironmentError(format!(
                        "获取磁盘剩余空间失败: {}",
                        std::io::Error::last_os_error()
                    )))
                }
            }
        }
        #[cfg(not(windows))]
        {
            Ok(100 * 1024 * 1024 * 1024)
        }
    }

    fn acquire_sleep_lock(&self, _reason: &str) -> Box<dyn Send> {
        #[cfg(windows)]
        {
            let (release, wait) = std::sync::mpsc::channel();
            let (started, ready) = std::sync::mpsc::sync_channel(0);
            let thread = std::thread::Builder::new()
                .name("openunpack-sleep-guard".into())
                .spawn(move || {
                    let active =
                        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) != 0 };
                    let _ = started.send(());
                    let _ = wait.recv();
                    if active {
                        unsafe {
                            SetThreadExecutionState(ES_CONTINUOUS);
                        }
                    }
                })
                .ok();
            if thread.is_some() {
                let _ = ready.recv();
            }
            Box::new(SleepLockGuard {
                release: Some(release),
                thread,
            })
        }
        #[cfg(not(windows))]
        {
            Box::new(SleepLockGuard {})
        }
    }
}
