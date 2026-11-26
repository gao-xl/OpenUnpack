//! Windows NT 原生相对路径打开、句柄重命名与路径合法性校验。

use super::*;

pub(super) fn unsafe_path_error() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "UnsafePath")
}

pub(super) fn validate_guard_path(path: &Path) -> io::Result<()> {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_) | Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _)))
        || !matches!(components.next(), Some(Component::RootDir))
        || components.any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(unsafe_path_error());
    }
    Ok(())
}

pub(super) fn validate_leaf_name(name: &Path) -> io::Result<()> {
    let mut components = name.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(unsafe_path_error());
    }
    let wide: Vec<u16> = name.as_os_str().encode_wide().collect();
    if wide.is_empty()
        || wide.len() > 16_000
        || wide
            .iter()
            .any(|c| matches!(*c, 0 | 34 | 42 | 47 | 58 | 60 | 62 | 63 | 92 | 124))
        || wide.last().is_some_and(|last| matches!(*last, 32 | 46))
    {
        return Err(unsafe_path_error());
    }
    let display = path_string(name);
    if !display.eq_ignore_ascii_case(OWNER_FILE) && relative(&display).is_err() {
        return Err(unsafe_path_error());
    }
    Ok(())
}

pub(super) fn validate_metadata(
    metadata: &fs::Metadata,
    directory: Option<bool>,
) -> io::Result<()> {
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.file_type().is_symlink()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "不支持链接或目录联接路径。",
        ));
    }
    if directory.is_some_and(|expected| metadata.is_dir() != expected)
        || !(metadata.is_file() || metadata.is_dir())
    {
        return Err(unsafe_path_error());
    }
    Ok(())
}

#[repr(C)]
pub(super) struct NativeUnicodeString {
    pub(super) length: u16,
    pub(super) maximum_length: u16,
    pub(super) buffer: *mut u16,
}
#[repr(C)]
pub(super) struct NativeObjectAttributes {
    pub(super) length: u32,
    pub(super) root: std::os::windows::io::RawHandle,
    pub(super) name: *mut NativeUnicodeString,
    pub(super) attributes: u32,
    pub(super) security_descriptor: *mut std::ffi::c_void,
    pub(super) security_qos: *mut std::ffi::c_void,
}
#[repr(C)]
pub(super) struct NativeIoStatus {
    pub(super) status: isize,
    pub(super) information: usize,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    pub(super) fn NtCreateFile(
        file: *mut std::os::windows::io::RawHandle,
        access: u32,
        object: *mut NativeObjectAttributes,
        status: *mut NativeIoStatus,
        allocation: *const i64,
        attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *const std::ffi::c_void,
        ea_length: u32,
    ) -> i32;
    pub(super) fn RtlNtStatusToDosError(status: i32) -> u32;
    pub(super) fn NtSetInformationFile(
        file: std::os::windows::io::RawHandle,
        status: *mut NativeIoStatus,
        information: *const std::ffi::c_void,
        length: u32,
        class: u32,
    ) -> i32;
}

pub(super) fn open_relative(
    parent: &DirectoryGuard,
    name: &Path,
    access: u32,
    disposition: u32,
    directory_only: bool,
) -> io::Result<fs::File> {
    open_relative_shared(
        parent,
        name,
        access,
        FILE_SHARE_READ,
        disposition,
        directory_only,
    )
}

pub(super) fn open_relative_shared(
    parent: &DirectoryGuard,
    name: &Path,
    access: u32,
    share: u32,
    disposition: u32,
    directory_only: bool,
) -> io::Result<fs::File> {
    open_relative_secured(
        parent,
        name,
        access,
        share,
        disposition,
        directory_only,
        std::ptr::null_mut(),
    )
}

pub(super) fn open_relative_secured(
    parent: &DirectoryGuard,
    name: &Path,
    access: u32,
    share: u32,
    disposition: u32,
    directory_only: bool,
    security: *mut std::ffi::c_void,
) -> io::Result<fs::File> {
    validate_leaf_name(name)?;
    let mut wide: Vec<u16> = name.as_os_str().encode_wide().collect();
    let mut name = NativeUnicodeString {
        length: (wide.len() * 2) as u16,
        maximum_length: (wide.len() * 2) as u16,
        buffer: wide.as_mut_ptr(),
    };
    let mut object = NativeObjectAttributes {
        length: std::mem::size_of::<NativeObjectAttributes>() as u32,
        root: parent.raw_handle(),
        name: &mut name,
        attributes: 0x40, // OBJ_CASE_INSENSITIVE
        security_descriptor: security,
        security_qos: std::ptr::null_mut(),
    };
    let mut status = NativeIoStatus {
        status: 0,
        information: 0,
    };
    let mut handle = std::ptr::null_mut();
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            access | SYNCHRONIZE_ACCESS,
            &mut object,
            &mut status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            share,
            disposition,
            0x0020_0000 | 0x20 | if directory_only { 1 } else { 0 }, // OPEN_REPARSE_POINT | SYNCHRONOUS_IO_NONALERT
            std::ptr::null(),
            0,
        )
    };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    // NtCreateFile with SYNCHRONOUS_IO_NONALERT completed before returning;
    // ownership transfers exactly once to File, including metadata failures.
    Ok(unsafe { fs::File::from_raw_handle(handle) })
}

pub(super) struct LocalDescriptor(pub(super) *mut std::ffi::c_void);
impl Drop for LocalDescriptor {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0);
        }
    }
}

pub(super) fn private_descriptor() -> io::Result<LocalDescriptor> {
    use std::os::windows::io::OwnedHandle;
    use windows_sys::Win32::{
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            },
            GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut raw = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut required = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut required,
        );
    }
    if required == 0 || required > 65536 {
        return Err(io::Error::other("无法取得当前用户身份。"));
    }
    let mut bytes = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            bytes.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*bytes.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid_memory = LocalDescriptor(sid.cast());
    let mut length = 0;
    while length < 256 && unsafe { *sid.add(length) } != 0 {
        length += 1;
    }
    if length == 256 {
        return Err(io::Error::other("当前用户身份无效。"));
    }
    let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(sid, length) })
        .map_err(|_| io::Error::other("当前用户身份无效。"))?;
    drop(sid_memory);
    let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(LocalDescriptor(descriptor))
}

pub(super) fn rename_handle(
    file: &fs::File,
    parent: &DirectoryGuard,
    target: &Path,
    replace: bool,
) -> io::Result<()> {
    let name = target
        .strip_prefix(parent.path())
        .map_err(|_| unsafe_path_error())?;
    validate_leaf_name(name)?;
    let wide: Vec<u16> = name.as_os_str().encode_wide().chain(Some(0)).collect();
    let size = std::mem::offset_of!(FILE_RENAME_INFO, FileName) + wide.len() * 2;
    // u64 storage provides the alignment required by FILE_RENAME_INFO on x64.
    let mut buffer = vec![0u64; size.div_ceil(std::mem::size_of::<u64>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = replace;
        (*info).RootDirectory = parent.raw_handle();
        (*info).FileNameLength = ((wide.len() - 1) * 2) as u32;
        std::ptr::copy_nonoverlapping(
            wide.as_ptr(),
            std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            wide.len(),
        );
        let mut status = NativeIoStatus {
            status: 0,
            information: 0,
        };
        // The Win32 wrapper rejects this relative RootDirectory form on the
        // supported native path. NtSetInformationFile resolves only this leaf
        // against the pinned parent object, never an absolute namespace path.
        let result = NtSetInformationFile(
            file.as_raw_handle(),
            &mut status,
            info.cast(),
            size as u32,
            10,
        ); // FileRenameInformation
        if result < 0 {
            return Err(io::Error::from_raw_os_error(
                RtlNtStatusToDosError(result) as i32
            ));
        }
    }
    Ok(())
}
