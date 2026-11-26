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

/// The root-to-leaf chain excludes deletion and replacement. Relative native
/// operations below consume directory objects, so attribute-only reparse
/// mutation cannot redirect an absolute namespace lookup. Directory writes
/// must be shared for the kernel's own rename-target parent open to succeed.
#[derive(Clone)]
pub struct DirectoryGuard {
    path: PathBuf,
    handles: Vec<Arc<fs::File>>,
}

impl DirectoryGuard {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::open_chain(path, false)
    }

    /// Create and lock one component at a time. NtCreateFile returns the
    /// newly created directory handle atomically, without a create/open gap.
    pub fn create(path: &Path) -> io::Result<Self> {
        Self::open_chain(path, true)
    }

    fn open_chain(path: &Path, create: bool) -> io::Result<Self> {
        validate_guard_path(path)?;
        if path
            .components()
            .filter(|part| matches!(part, Component::Normal(_)))
            .count()
            > MAX_DIRECTORY_CHAIN_DEPTH
        {
            return Err(io::Error::other("目录层级超出安全限制。"));
        }
        let mut components = path.components();
        let mut root = PathBuf::new();
        root.push(components.next().ok_or_else(unsafe_path_error)?.as_os_str());
        root.push(components.next().ok_or_else(unsafe_path_error)?.as_os_str());
        let file = fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | SYNCHRONIZE_ACCESS)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&root)?;
        validate_metadata(&file.metadata()?, Some(true))?;
        let mut guard = Self {
            path: root,
            handles: vec![Arc::new(file)],
        };
        for component in components {
            match component {
                Component::Normal(name) => {
                    guard = guard.child_directory(
                        Path::new(name),
                        if create {
                            NATIVE_FILE_OPEN_IF
                        } else {
                            NATIVE_FILE_OPEN
                        },
                    )?;
                }
                Component::CurDir => (),
                _ => return Err(unsafe_path_error()),
            }
        }
        Ok(guard)
    }

    fn child_directory(&self, name: &Path, disposition: u32) -> io::Result<Self> {
        if self.handles.len() > MAX_DIRECTORY_CHAIN_DEPTH {
            return Err(io::Error::other("目录层级超出安全限制。"));
        }
        let file = open_relative_shared(
            self,
            name,
            FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | SYNCHRONIZE_ACCESS,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            disposition,
            true,
        )?;
        validate_metadata(&file.metadata()?, Some(true))?;
        let mut handles = self.handles.clone();
        handles.push(Arc::new(file));
        Ok(Self {
            path: self.path.join(name),
            handles,
        })
    }

    /// Application-owned workspace roots do not inherit another account's
    /// write/delete access from a shared cache or TEMP directory.
    pub fn create_private_child(&self, path: &Path) -> io::Result<Self> {
        let name = path
            .strip_prefix(self.path())
            .map_err(|_| unsafe_path_error())?;
        let descriptor = private_descriptor()?;
        let file = open_relative_secured(
            self,
            name,
            FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | SYNCHRONIZE_ACCESS,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            NATIVE_FILE_CREATE,
            true,
            descriptor.0,
        )?;
        validate_metadata(&file.metadata()?, Some(true))?;
        let mut handles = self.handles.clone();
        handles.push(Arc::new(file));
        Ok(Self {
            path: path.to_path_buf(),
            handles,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn create_file(&self, path: &Path) -> io::Result<LockedEntry> {
        let name = path
            .strip_prefix(self.path())
            .map_err(|_| unsafe_path_error())?;
        let file = open_relative(
            self,
            name,
            GENERIC_READ | GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
            NATIVE_FILE_CREATE,
            false,
        )?;
        validate_metadata(&file.metadata()?, Some(false))?;
        LockedEntry::from_created(path, file, self)
    }

    pub fn open_file_with_share(
        &self,
        path: &Path,
        access: u32,
        share: u32,
    ) -> io::Result<fs::File> {
        let name = path
            .strip_prefix(self.path())
            .map_err(|_| unsafe_path_error())?;
        let file = open_relative_shared(self, name, access, share, NATIVE_FILE_OPEN, false)?;
        validate_metadata(&file.metadata()?, None)?;
        Ok(file)
    }

    /// Each returned ancestor reuses existing handles, starting at the leaf.
    pub fn ancestors(&self) -> impl Iterator<Item = DirectoryGuard> + '_ {
        self.path
            .ancestors()
            .take(self.handles.len())
            .enumerate()
            .map(|(index, path)| Self {
                path: path.to_path_buf(),
                handles: self.handles[..self.handles.len() - index].to_vec(),
            })
    }

    pub fn open_own_security(&self) -> io::Result<fs::File> {
        const READ_CONTROL_ACCESS: u32 = 0x0002_0000;
        let file = if self.handles.len() == 1 {
            fs::OpenOptions::new()
                .access_mode(READ_CONTROL_ACCESS | FILE_READ_ATTRIBUTES)
                .share_mode(
                    FILE_SHARE_READ | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE,
                )
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(&self.path)?
        } else {
            let parent = Self {
                path: self
                    .path
                    .parent()
                    .ok_or_else(unsafe_path_error)?
                    .to_path_buf(),
                handles: self.handles[..self.handles.len() - 1].to_vec(),
            };
            open_relative_shared(
                &parent,
                Path::new(self.path.file_name().ok_or_else(unsafe_path_error)?),
                READ_CONTROL_ACCESS | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE,
                NATIVE_FILE_OPEN,
                true,
            )?
        };
        validate_metadata(&file.metadata()?, Some(true))?;
        Ok(file)
    }

    pub fn raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.handles
            .last()
            .expect("a directory lease has a root")
            .as_raw_handle()
    }

    /// Enumerate this directory object directly, without resolving a string
    /// path again. A concurrently introduced ancestor junction is never used.
    pub fn entries(&self) -> io::Result<Vec<std::ffi::OsString>> {
        let mut names = Vec::new();
        let mut restart = true;
        let mut buffer = vec![0u64; 8192];
        loop {
            let result = unsafe {
                GetFileInformationByHandleEx(
                    self.raw_handle(),
                    if restart {
                        FileIdBothDirectoryRestartInfo
                    } else {
                        FileIdBothDirectoryInfo
                    },
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 8) as u32,
                )
            };
            if result == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(18) {
                    break;
                } // ERROR_NO_MORE_FILES
                return Err(error);
            }
            restart = false;
            let mut offset = 0usize;
            loop {
                let name_offset = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
                if offset + std::mem::size_of::<FILE_ID_BOTH_DIR_INFO>() > buffer.len() * 8 {
                    return Err(io::Error::other("目录枚举数据无效。"));
                }
                let item = unsafe {
                    &*buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset)
                        .cast::<FILE_ID_BOTH_DIR_INFO>()
                };
                let name_bytes = item.FileNameLength as usize;
                if !name_bytes.is_multiple_of(2)
                    || offset + name_offset + name_bytes > buffer.len() * 8
                {
                    return Err(io::Error::other("目录枚举数据无效。"));
                }
                let name = unsafe {
                    std::slice::from_raw_parts(
                        std::ptr::addr_of!(item.FileName).cast::<u16>(),
                        name_bytes / 2,
                    )
                };
                if name != [46] && name != [46, 46] {
                    if names.len() >= MAX_GUARDED_ENTRIES {
                        return Err(io::Error::other("目录项目数超出安全限制。"));
                    }
                    names.push(std::ffi::OsString::from_wide(name));
                }
                if item.NextEntryOffset == 0 {
                    break;
                }
                let next = item.NextEntryOffset as usize;
                if !next.is_multiple_of(8)
                    || next < name_offset
                    || offset + next >= buffer.len() * 8
                {
                    return Err(io::Error::other("目录枚举数据无效。"));
                }
                offset += next;
            }
        }
        Ok(names)
    }

    /// Open only an immediate child relative to the already pinned directory.
    pub fn lock_entry(&self, path: &Path, readable: bool) -> io::Result<LockedEntry> {
        let name = path
            .strip_prefix(&self.path)
            .map_err(|_| unsafe_path_error())?;
        validate_leaf_name(name)?;
        let access = DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | FILE_LIST_DIRECTORY | SYNCHRONIZE_ACCESS | 0x0002_0000 // READ_CONTROL
            | if readable { GENERIC_READ } else { 0 };
        // Directory sharing must permit the kernel's own rename-target open;
        // files continue to exclude both writers and competing DELETE access.
        let file = match open_relative_shared(
            self,
            name,
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            NATIVE_FILE_OPEN,
            true,
        ) {
            Ok(directory) => directory,
            Err(error) if error.raw_os_error() == Some(267) => {
                open_relative(self, name, access, NATIVE_FILE_OPEN, false)?
            }
            Err(error) => return Err(error),
        };
        validate_metadata(&file.metadata()?, None)?;
        Ok(LockedEntry {
            parent: self.clone(),
            path: path.to_path_buf(),
            file: Arc::new(file),
        })
    }

    fn open_read_file(&self, name: &Path) -> io::Result<fs::File> {
        let file = open_relative(self, name, GENERIC_READ, NATIVE_FILE_OPEN, false)?;
        validate_metadata(&file.metadata()?, Some(false))?;
        Ok(file)
    }
}

/// A source object and its ancestors remain pinned through readback, rename,
/// or deletion. None of these mutations reopen the source by pathname.
pub struct LockedEntry {
    parent: DirectoryGuard,
    path: PathBuf,
    file: Arc<fs::File>,
}

impl LockedEntry {
    /// Adopt the handle returned by a create_new operation performed while
    /// this parent lease was already held. Callers must never reopen the name.
    pub fn from_created(path: &Path, file: fs::File, parent: &DirectoryGuard) -> io::Result<Self> {
        validate_leaf_name(
            path.strip_prefix(parent.path())
                .map_err(|_| unsafe_path_error())?,
        )?;
        validate_metadata(&file.metadata()?, None)?;
        Ok(Self {
            parent: parent.clone(),
            path: path.to_path_buf(),
            file: Arc::new(file),
        })
    }

    pub fn open(path: &Path, readable: bool) -> io::Result<Self> {
        let parent = DirectoryGuard::open(path.parent().ok_or_else(unsafe_path_error)?)?;
        parent.lock_entry(path, readable)
    }

    pub fn open_directory(path: &Path) -> io::Result<Self> {
        let entry = Self::open(path, false)?;
        validate_metadata(&entry.metadata()?, Some(true))?;
        Ok(entry)
    }

    pub fn metadata(&self) -> io::Result<fs::Metadata> {
        self.file.metadata()
    }

    pub fn file(&self) -> &fs::File {
        &self.file
    }

    pub fn raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.file.as_raw_handle()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn directory_guard(&self) -> io::Result<DirectoryGuard> {
        validate_metadata(&self.metadata()?, Some(true))?;
        let mut handles = self.parent.handles.clone();
        handles.push(self.file.clone());
        Ok(DirectoryGuard {
            path: self.path.clone(),
            handles,
        })
    }

    pub fn rename_no_replace(&mut self, parent: &DirectoryGuard, target: &Path) -> io::Result<()> {
        rename_handle(self.file(), parent, target, false)?;
        self.parent = parent.clone();
        self.path = target.to_path_buf();
        Ok(())
    }

    /// Mark this same object for deletion. It disappears after all leases to
    /// this handle are released; consumers must drop children before parents.
    pub fn delete(&self) -> io::Result<()> {
        let mut info = FILE_BASIC_INFO::default();
        if unsafe {
            GetFileInformationByHandleEx(
                self.raw_handle(),
                FileBasicInfo,
                (&mut info as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if info.FileAttributes & FILE_ATTRIBUTE_READONLY != 0 {
            info.FileAttributes &= !FILE_ATTRIBUTE_READONLY;
            if info.FileAttributes == 0 {
                info.FileAttributes = FILE_ATTRIBUTE_NORMAL;
            }
            if unsafe {
                SetFileInformationByHandle(
                    self.raw_handle(),
                    FileBasicInfo,
                    (&info as *const FILE_BASIC_INFO).cast(),
                    std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                self.raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Hold every existing entry stable through validation and final publication.
/// This prevents existing files from being replaced or rewritten. Sharing
/// leases do not prohibit new child names from being added to a directory.
pub struct LockedTree {
    root: LockedEntry,
    entries: Vec<LockedEntry>,
}

impl LockedTree {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::from_root(LockedEntry::open_directory(path)?)
    }

    pub fn from_root(root: LockedEntry) -> io::Result<Self> {
        let entries = lock_descendants(&root.directory_guard()?)?;
        Ok(Self { root, entries })
    }

    pub fn root(&self) -> &LockedEntry {
        &self.root
    }

    pub fn root_mut(&mut self) -> &mut LockedEntry {
        &mut self.root
    }

    pub fn directory_guard(&self) -> io::Result<DirectoryGuard> {
        self.root.directory_guard()
    }

    pub fn entries(&self) -> &[LockedEntry] {
        &self.entries
    }

    /// Resolve a relative manifest name against entries already pinned by this
    /// tree. It never resolves the filesystem namespace a second time.
    pub fn file_at(&self, relative_path: &Path) -> io::Result<&fs::File> {
        if relative_path.as_os_str().is_empty()
            || relative_path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(unsafe_path_error());
        }
        let path = self.root.path().join(relative_path);
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.path() == path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "清单文件不在已锁定树内。"))?;
        validate_metadata(&entry.metadata()?, Some(false))?;
        Ok(entry.file())
    }

    pub fn verify_owned(&self, id: &str, manifest: Option<&str>) -> Result<(), String> {
        let marker = self.root.path().join(OWNER_FILE);
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.path() == marker)
            .ok_or("工作目录缺少独占标记。")?;
        verify_owner_file(entry.file(), id, manifest)
    }

    pub fn rename_no_replace(&mut self, parent: &DirectoryGuard, target: &Path) -> io::Result<()> {
        crate::saferecycle::verify_private_directory(&self.root.directory_guard()?)?;
        crate::saferecycle::verify_private_entries(&self.entries)?;
        validate_metadata(&self.root.metadata()?, Some(true))?;
        // Full child readback completed while entries were locked. NTFS
        // requires their leases to close before root-directory rename. The
        // verified private root ACL excludes other-account writers here.
        self.entries.clear();
        self.root.rename_no_replace(parent, target)
    }

    /// NTFS cannot rename a directory while its descendants deny deletion.
    /// Release child leases after their final validation, while the root and
    /// ancestor objects remain pinned. Private workspace ACLs exclude other
    /// accounts; a process running as the same account is outside this fence.
    pub fn commit_no_replace(&mut self, parent: &DirectoryGuard, target: &Path) -> io::Result<()> {
        self.rename_no_replace(parent, target)
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

fn lock_descendants(root: &DirectoryGuard) -> io::Result<Vec<LockedEntry>> {
    let mut pending = vec![(root.clone(), 0usize)];
    let mut entries = Vec::new();
    while let Some((parent, depth)) = pending.pop() {
        if depth > MAX_GUARDED_DEPTH {
            return Err(io::Error::other("目录层级超出安全限制。"));
        }
        for name in parent.entries()? {
            if entries.len() >= MAX_GUARDED_ENTRIES {
                return Err(io::Error::other("目录项目数超出安全限制。"));
            }
            let entry = parent.lock_entry(&parent.path().join(name), true)?;
            if entry.metadata()?.is_dir() {
                pending.push((entry.directory_guard()?, depth + 1));
            }
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn unsafe_path_error() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "UnsafePath")
}

fn validate_guard_path(path: &Path) -> io::Result<()> {
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_) | Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _)))
        || !matches!(components.next(), Some(Component::RootDir))
        || components.any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(unsafe_path_error());
    }
    Ok(())
}

fn validate_leaf_name(name: &Path) -> io::Result<()> {
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

fn validate_metadata(metadata: &fs::Metadata, directory: Option<bool>) -> io::Result<()> {
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
struct NativeUnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct NativeObjectAttributes {
    length: u32,
    root: std::os::windows::io::RawHandle,
    name: *mut NativeUnicodeString,
    attributes: u32,
    security_descriptor: *mut std::ffi::c_void,
    security_qos: *mut std::ffi::c_void,
}
#[repr(C)]
struct NativeIoStatus {
    status: isize,
    information: usize,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
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
    fn RtlNtStatusToDosError(status: i32) -> u32;
    fn NtSetInformationFile(
        file: std::os::windows::io::RawHandle,
        status: *mut NativeIoStatus,
        information: *const std::ffi::c_void,
        length: u32,
        class: u32,
    ) -> i32;
}

fn open_relative(
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

fn open_relative_shared(
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

fn open_relative_secured(
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

struct LocalDescriptor(*mut std::ffi::c_void);
impl Drop for LocalDescriptor {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0);
        }
    }
}

fn private_descriptor() -> io::Result<LocalDescriptor> {
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

fn rename_handle(
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
#[derive(serde::Serialize, serde::Deserialize)]
struct Owner {
    application: String,
    id: String,
    manifest: String,
}
pub fn mark_owned(path: &Path, id: &str, manifest: &str) -> Result<(), String> {
    let parent = DirectoryGuard::open(path.parent().ok_or("没有父目录。")?)
        .map_err(|_| "无法锁定工作父目录。")?;
    let guard = parent
        .create_private_child(path)
        .map_err(|_| "无法创建独占工作目录。")?;
    atomic_json_guarded(
        &guard,
        &path.join(OWNER_FILE),
        &Owner {
            application: "OpenUnpack/1".into(),
            id: id.into(),
            manifest: manifest.into(),
        },
        false,
    )
}
/// Attach publication ownership to an already validated extraction directory.
/// Archive entries are prohibited from using OWNER_FILE, so this never overwrites
/// a user-created marker or changes ownership of an existing output folder.
pub fn tag_existing(path: &Path, id: &str, manifest: &str) -> Result<(), String> {
    let guard = DirectoryGuard::open(path).map_err(|_| "发布目录不能取得独占标记。")?;
    atomic_json_guarded(
        &guard,
        &path.join(OWNER_FILE),
        &Owner {
            application: "OpenUnpack/1".into(),
            id: id.into(),
            manifest: manifest.into(),
        },
        false,
    )
}
pub fn owned(path: &Path, id: &str, manifest: Option<&str>) -> Result<(), String> {
    let guard = DirectoryGuard::open(path).map_err(|_| "工作目录不可安全检查。")?;
    let file = guard
        .open_read_file(Path::new(OWNER_FILE))
        .map_err(|_| "工作目录缺少独占标记。")?;
    verify_owner_file(&file, id, manifest)
}

fn verify_owner_file(file: &fs::File, id: &str, manifest: Option<&str>) -> Result<(), String> {
    let o = read_owner_file(file)?;
    if o.application != "OpenUnpack/1" || o.id != id || manifest.is_some_and(|m| m != o.manifest) {
        return Err("工作目录标记不匹配，已停止清理。".into());
    }
    Ok(())
}

fn read_owner_file(file: &fs::File) -> Result<Owner, String> {
    if file.metadata().map_err(|_| "工作目录标记不可读。")?.len() > 16 * 1024 * 1024 {
        return Err("工作目录标记过大。".into());
    }
    let length = file.metadata().map_err(|_| "工作目录标记不可读。")?.len() as usize;
    let mut bytes = vec![0; length];
    let mut offset = 0;
    while offset < bytes.len() {
        let count = file
            .seek_read(&mut bytes[offset..], offset as u64)
            .map_err(|_| "工作目录标记不可读。")?;
        if count == 0 {
            return Err("工作目录标记损坏。".into());
        }
        offset += count;
    }
    serde_json::from_slice(&bytes).map_err(|_| "工作目录标记损坏。".into())
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetirementError {
    pub message: String,
    pub retryable: bool,
}

impl RetirementError {
    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }

    pub fn terminal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for RetirementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RetirementError {}

impl From<String> for RetirementError {
    fn from(s: String) -> Self {
        let retryable = transient_retirement(&s);
        Self {
            message: s,
            retryable,
        }
    }
}

impl From<&str> for RetirementError {
    fn from(s: &str) -> Self {
        let retryable = transient_retirement(s);
        Self {
            message: s.to_string(),
            retryable,
        }
    }
}

pub fn remove_owned(path: &Path, id: &str) -> Result<(), String> {
    remove_owned_with_hook(path, id, |_| {}).map_err(|err| err.message)
}

/// Post-delivery retirement runs after the cache lease is released, so the next
/// quota waiter is admitted while this workspace is still being torn down. That
/// leaves a short window where a sibling, the Worker or an on-access scanner can
/// hold a handle or a delete-pending child. Retry only those transient reasons;
/// ownership markers, private-ACL refusals, unsafe namespaces and unconfirmed
/// Worker exits remain terminal so a retry can never publish over a foreign tree.
pub fn remove_owned_retrying(path: &Path, id: &str) -> Result<(), String> {
    retry_retirement(|| remove_owned_with_hook(path, id, |_| {}))
}

pub fn retry_retirement(
    mut attempt: impl FnMut() -> Result<(), RetirementError>,
) -> Result<(), String> {
    // Three bounded retries (four attempts total) ride out a handle that is
    // released moments later: a sibling job's walker, the Worker exiting, or an
    // on-access scanner finishing a pass.
    const RETRY_DELAYS_MS: [u64; 3] = [50, 150, 400];
    for delay in RETRY_DELAYS_MS {
        match attempt() {
            Ok(()) => return Ok(()),
            Err(err) if err.retryable => {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            Err(err) => return Err(err.message),
        }
    }
    attempt().map_err(|err| err.message)
}

/// The two "try again later" messages raised by the delete path. Anything else
/// is a durable refusal and must not be looped on.
pub fn transient_retirement(reason: &str) -> bool {
    if reason.contains("并发变动")
        || reason.contains("已停止清理")
        || reason.contains("WorkerExitUnconfirmed")
        || reason.contains("缺少独占标记")
    {
        return false;
    }
    if reason.contains("os error 32")
        || reason.contains("os error 33")
        || reason.contains("os error 5")
        || reason.contains("os error 145")
    {
        return true;
    }
    reason.contains("正被占用") || reason.contains("可稍后重试")
}

fn remove_owned_with_hook(
    path: &Path,
    id: &str,
    before_root_delete: impl FnOnce(&Path),
) -> Result<(), RetirementError> {
    let mut tree = LockedTree::open(path).map_err(|err| {
        let os = err
            .raw_os_error()
            .map(|code| format!(" (os error {code})"))
            .unwrap_or_default();
        RetirementError::retryable(format!(
            "工作目录清理失败：目录或文件正被占用，或无法确认安全身份{os}。可稍后重试。"
        ))
    })?;
    tree.verify_owned(id, None)
        .map_err(RetirementError::terminal)?;
    {
        let guard = tree.directory_guard().map_err(|err| {
            let os = err
                .raw_os_error()
                .map(|code| format!(" (os error {code})"))
                .unwrap_or_default();
            RetirementError::retryable(format!("工作目录权限无法核对{os}。"))
        })?;
        crate::saferecycle::verify_private_directory(&guard).map_err(|_| {
            RetirementError::terminal("工作目录允许其他账号修改或无法确认权限，已停止清理。")
        })?;
    }
    if tree
        .entries
        .iter()
        .any(|entry| entry.path() == path.join(".openunpack-exit-unconfirmed.json"))
    {
        return Err(RetirementError::terminal("WorkerExitUnconfirmed"));
    }
    let owner_path = path.join(OWNER_FILE);
    let index = tree
        .entries
        .iter()
        .position(|entry| entry.path() == owner_path)
        .ok_or_else(|| RetirementError::terminal("工作目录缺少独占标记。"))?;
    let marker = tree.entries.remove(index);
    let saved_owner = read_owner_file(marker.file()).map_err(RetirementError::terminal)?;
    let retryable_failure = |err: std::io::Error| {
        let os = err
            .raw_os_error()
            .map(|code| format!(" (os error {code})"))
            .unwrap_or_default();
        RetirementError::retryable(format!("工作目录清理失败{os}，可稍后重试。"))
    };
    // All current descendants were locked and checked before the first delete.
    // Pop children first and close their handles before deleting parent objects.
    while let Some(entry) = tree.entries.pop() {
        entry.delete().map_err(retryable_failure)?;
    }
    let remaining = tree
        .root
        .directory_guard()
        .map_err(retryable_failure)?
        .entries()
        .map_err(retryable_failure)?;
    if remaining.len() != 1 || remaining[0] != OWNER_FILE {
        return Err(RetirementError::terminal(
            "工作目录在清理过程中发生并发变动，已停止清理。",
        ));
    }
    marker.delete().map_err(retryable_failure)?;
    drop(marker);
    before_root_delete(path);
    if tree.root.delete().is_err() {
        let root = tree.root.directory_guard().map_err(retryable_failure)?;
        // If the final directory deletion fails (for example a child was
        // inserted concurrently), restore authority in the same pinned root.
        // Never overwrite a newly introduced marker owned by someone else.
        if atomic_json_guarded(&root, &owner_path, &saved_owner, false).is_err() {
            let marker = root.open_read_file(Path::new(OWNER_FILE)).map_err(|_| {
                RetirementError::terminal("清理失败且独占标记无法恢复；目录已保留。")
            })?;
            verify_owner_file(&marker, &saved_owner.id, Some(&saved_owner.manifest))
                .map_err(RetirementError::terminal)?;
        }
        return Err(RetirementError::retryable("工作目录清理失败，可稍后重试。"));
    }
    // Windows only removes the delete-pending workspace after its last handle
    // closes. The tree also pins the parent without FILE_SHARE_DELETE, so it
    // must be released before attempting to prune the empty cache container.
    drop(tree);
    prune_empty_cache_container(path);
    Ok(())
}

/// Jobs share `<cache_root>/OpenUnpack-cache` as their parent, so the container
/// outlives every workspace and would linger as an empty directory. Removing it
/// only succeeds while it is empty, which keeps concurrent jobs unaffected.
fn prune_empty_cache_container(retired: &Path) {
    let Some(parent) = retired.parent() else {
        return;
    };
    if parent.file_name().and_then(|name| name.to_str()) != Some("OpenUnpack-cache") {
        return;
    }
    let _ = std::fs::remove_dir(parent);
}
/// Candidate directories do not each carry an ownership marker. Their only valid
/// cleanup authority is the enclosing exclusive workspace, with no link traversal.
pub fn remove_inside_owned(path: &Path, workspace: &Path, id: &str) -> Result<(), String> {
    let (parent, _owner) = owned_parent_for_entry(path, workspace, id)?;
    let root = parent.lock_entry(path, false).map_err(|_| "IoError")?;
    validate_metadata(&root.metadata().map_err(|_| "IoError")?, Some(true))
        .map_err(|_| "UnsafePath")?;
    let mut tree = LockedTree::from_root(root).map_err(|_| "IoError")?;
    while let Some(entry) = tree.entries.pop() {
        entry.delete().map_err(|_| "IoError")?;
    }
    tree.root.delete().map_err(|_| "IoError".into())
}
pub fn remove_file_inside_owned(path: &Path, workspace: &Path, id: &str) -> Result<(), String> {
    let (parent, _owner) = owned_parent_for_entry(path, workspace, id)?;
    let entry = parent.lock_entry(path, false).map_err(|_| "IoError")?;
    validate_metadata(&entry.metadata().map_err(|_| "IoError")?, Some(false))
        .map_err(|_| "UnsafePath")?;
    entry.delete().map_err(|_| "IoError".into())
}

fn owned_parent_for_entry(
    path: &Path,
    workspace: &Path,
    id: &str,
) -> Result<(DirectoryGuard, fs::File), String> {
    let r = path.strip_prefix(workspace).map_err(|_| "UnsafePath")?;
    relative(&path_string(r))?;
    let mut guard = DirectoryGuard::open(workspace).map_err(|_| "工作目录不可安全锁定。")?;
    crate::saferecycle::verify_private_directory(&guard)
        .map_err(|_| "工作目录允许其他账号修改或无法确认权限，已停止清理。")?;
    let owner = guard
        .open_read_file(Path::new(OWNER_FILE))
        .map_err(|_| "工作目录缺少独占标记。")?;
    verify_owner_file(&owner, id, None)?;
    if guard
        .entries()
        .map_err(|_| "工作目录不可检查。")?
        .iter()
        .any(|name| name == ".openunpack-exit-unconfirmed.json")
    {
        return Err("WorkerExitUnconfirmed".into());
    }
    if let Some(parent) = r.parent() {
        for component in parent.components() {
            if let Component::Normal(name) = component {
                guard = guard
                    .child_directory(Path::new(name), NATIVE_FILE_OPEN)
                    .map_err(|_| "UnsafePath")?;
            } else {
                return Err("UnsafePath".into());
            }
        }
    }
    Ok((guard, owner))
}
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

const ES_CONTINUOUS: u32 = 0x80000000;
const ES_SYSTEM_REQUIRED: u32 = 0x00000001;

// Execution state belongs to a Windows thread. An async job may migrate between
// Tokio threads, so both native calls must run on this guard's dedicated thread.
pub struct SleepBlockerGuard {
    release: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SleepBlockerGuard {
    pub fn acquire() -> Self {
        Self::with_execution_state(|flags| unsafe { SetThreadExecutionState(flags) })
    }

    fn with_execution_state(mut set: impl FnMut(u32) -> u32 + Send + 'static) -> Self {
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

fn component_eq_platform(left: Component<'_>, right: Component<'_>) -> bool {
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

#[cfg(test)]
mod tests;
