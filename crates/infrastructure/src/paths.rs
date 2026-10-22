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
    if path.split(['/', '\\']).count() > MAX_GUARDED_DEPTH {
        return Err("ResourceLimit".into());
    }
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
pub fn remove_owned(path: &Path, id: &str) -> Result<(), String> {
    remove_owned_with_hook(path, id, |_| {})
}

fn remove_owned_with_hook(
    path: &Path,
    id: &str,
    before_root_delete: impl FnOnce(&Path),
) -> Result<(), String> {
    let mut tree = LockedTree::open(path)
        .map_err(|_| "工作目录清理失败：目录或文件正被占用，或无法确认安全身份。可稍后重试。")?;
    tree.verify_owned(id, None)?;
    crate::saferecycle::verify_private_directory(
        &tree
            .directory_guard()
            .map_err(|_| "工作目录权限无法核对。")?,
    )
    .map_err(|_| "工作目录允许其他账号修改或无法确认权限，已停止清理。")?;
    if tree
        .entries
        .iter()
        .any(|entry| entry.path() == path.join(".openunpack-exit-unconfirmed.json"))
    {
        return Err("WorkerExitUnconfirmed".into());
    }
    let owner_path = path.join(OWNER_FILE);
    let index = tree
        .entries
        .iter()
        .position(|entry| entry.path() == owner_path)
        .ok_or("工作目录缺少独占标记。")?;
    let marker = tree.entries.remove(index);
    let saved_owner = read_owner_file(marker.file())?;
    let failure = || "工作目录清理失败，可稍后重试。".to_string();
    // All current descendants were locked and checked before the first delete.
    // Pop children first and close their handles before deleting parent objects.
    while let Some(entry) = tree.entries.pop() {
        entry.delete().map_err(|_| failure())?;
    }
    let remaining = tree
        .root
        .directory_guard()
        .map_err(|_| failure())?
        .entries()
        .map_err(|_| failure())?;
    if remaining.len() != 1 || remaining[0] != OWNER_FILE {
        return Err(failure());
    }
    marker.delete().map_err(|_| failure())?;
    drop(marker);
    before_root_delete(path);
    if tree.root.delete().is_err() {
        let root = tree.root.directory_guard().map_err(|_| failure())?;
        // If the final directory deletion fails (for example a child was
        // inserted concurrently), restore authority in the same pinned root.
        // Never overwrite a newly introduced marker owned by someone else.
        if atomic_json_guarded(&root, &owner_path, &saved_owner, false).is_err() {
            let marker = root
                .open_read_file(Path::new(OWNER_FILE))
                .map_err(|_| "清理失败且独占标记无法恢复；目录已保留。")?;
            verify_owner_file(&marker, &saved_owner.id, Some(&saved_owner.manifest))?;
        }
        return Err(failure());
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excessive_path_depth_is_rejected_before_creating_any_directory() {
        assert!(relative(&vec!["a"; 128].join("/")).is_ok());
        assert_eq!(
            relative(&vec!["a"; 129].join("/")).unwrap_err(),
            "ResourceLimit"
        );
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().join("not-created");
        let mut excessive = base.clone();
        for _ in 0..257 {
            excessive.push("a");
        }
        assert!(DirectoryGuard::create(&excessive).is_err());
        assert!(!base.exists());
    }

    #[test]
    fn native_handle_rename_and_tree_publication_keep_sources_locked() {
        let temp = tempfile::tempdir().unwrap();
        let file_path = temp.path().join("file");
        fs::write(&file_path, b"locked").unwrap();
        let parent = DirectoryGuard::open(temp.path()).unwrap();
        let mut file = parent.lock_entry(&file_path, true).unwrap();
        file.rename_no_replace(&parent, &temp.path().join("renamed"))
            .unwrap();
        drop(file);
        let source = temp.path().join("source");
        drop(parent.create_private_child(&source).unwrap());
        fs::create_dir_all(source.join("child")).unwrap();
        fs::write(source.join("child/data"), b"stable bytes").unwrap();
        let mut tree = LockedTree::open(&source).unwrap();
        assert_eq!(tree.entry_count(), 2);
        assert!(fs::write(source.join("child/data"), b"wrong").is_err());
        assert!(fs::rename(&source, temp.path().join("swapped")).is_err());
        assert!(
            tree.root_mut()
                .rename_no_replace(&parent, &temp.path().join("published"))
                .is_err(),
            "NTFS requires child leases to close before directory rename"
        );
        tree.rename_no_replace(&parent, &temp.path().join("published"))
            .unwrap();
        assert_eq!(
            fs::read(temp.path().join("published/child/data")).unwrap(),
            b"stable bytes"
        );
    }

    #[test]
    fn native_ancestor_leases_block_rename_then_release_it() {
        let temp = tempfile::tempdir().unwrap();
        let ancestor = temp.path().join("ancestor");
        let leaf = ancestor.join("child/grandchild");
        let guard = DirectoryGuard::create(&leaf).unwrap();
        assert!(fs::rename(&ancestor, temp.path().join("replacement")).is_err());
        assert!(fs::remove_dir(&leaf).is_err());
        drop(guard);
        fs::rename(&ancestor, temp.path().join("replacement")).unwrap();
    }

    #[test]
    fn cleanup_failure_after_marker_deletion_restores_same_owner_for_retry() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work");
        mark_owned(&work, "job", "manifest-kept").unwrap();
        fs::create_dir_all(work.join("child/grandchild")).unwrap();
        fs::write(work.join("child/grandchild/data"), b"old").unwrap();
        assert!(
            remove_owned_with_hook(&work, "job", |root| {
                fs::write(root.join("concurrent-new-child"), b"keep").unwrap();
            })
            .is_err()
        );
        assert_eq!(
            fs::read(work.join("concurrent-new-child")).unwrap(),
            b"keep"
        );
        owned(&work, "job", Some("manifest-kept")).unwrap();
        remove_owned(&work, "job").unwrap();
        assert!(!work.exists());
    }

    #[test]
    fn owned_children_cleanup_and_readonly_files_keep_authority() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work");
        mark_owned(&work, "job", "").unwrap();
        fs::create_dir_all(work.join("candidate/deep")).unwrap();
        let file = work.join("candidate/deep/data");
        fs::write(&file, b"remove").unwrap();
        let mut readonly = fs::metadata(&file).unwrap().permissions();
        readonly.set_readonly(true);
        fs::set_permissions(&file, readonly).unwrap();
        remove_file_inside_owned(&file, &work, "job").unwrap();
        remove_inside_owned(&work.join("candidate"), &work, "job").unwrap();
        assert!(!work.join("candidate").exists());
        owned(&work, "job", None).unwrap();
        remove_owned(&work, "job").unwrap();
    }

    #[test]
    fn repeated_owner_validation_does_not_consume_the_shared_file_cursor() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work");
        mark_owned(&work, "job", "manifest").unwrap();
        let tree = LockedTree::open(&work).unwrap();
        tree.verify_owned("job", Some("manifest")).unwrap();
        tree.verify_owned("job", Some("manifest")).unwrap();
    }

    #[test]
    fn locked_child_prevents_cleanup_before_any_entry_is_removed() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work");
        mark_owned(&work, "job", "").unwrap();
        fs::write(work.join("unlocked"), b"preserved").unwrap();
        fs::write(work.join("locked"), b"busy").unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(work.join("locked"))
            .unwrap();
        assert!(remove_owned(&work, "job").is_err());
        assert_eq!(fs::read(work.join("unlocked")).unwrap(), b"preserved");
        assert_eq!(fs::read(work.join("locked")).unwrap(), b"busy");
        owned(&work, "job", None).unwrap();
        drop(lock);
        remove_owned(&work, "job").unwrap();
    }

    #[test]
    fn existing_descendant_junction_blocks_cleanup_and_preserves_external_files() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join("work");
        let outside = temp.path().join("outside");
        mark_owned(&work, "job", "").unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside preserved").unwrap();
        let candidate = work.join("candidate");
        fs::create_dir(&candidate).unwrap();
        let writer = fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .share_mode(
                FILE_SHARE_READ
                    | FILE_SHARE_WRITE
                    | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE,
            )
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&candidate)
            .unwrap();
        set_test_junction(&writer, &outside).unwrap();
        assert!(remove_owned(&work, "job").is_err());
        assert!(remove_inside_owned(&candidate, &work, "job").is_err());
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"outside preserved"
        );
        owned(&work, "job", None).unwrap();
        clear_test_junction(&writer);
        drop(writer);
        remove_owned(&work, "job").unwrap();
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"outside preserved"
        );
    }

    #[test]
    fn native_json_commit_overwrites_only_named_config_and_ownership_tag_never_replaces() {
        let temp = tempfile::tempdir().unwrap();
        let settings = temp.path().join("nested/settings.json");
        atomic_json(&settings, &serde_json::json!({"value":1})).unwrap();
        atomic_json(&settings, &serde_json::json!({"value":2})).unwrap();
        assert_eq!(
            read_json::<serde_json::Value>(&settings).unwrap()["value"],
            2
        );
        assert_eq!(fs::read_dir(settings.parent().unwrap()).unwrap().count(), 1);
        let work = temp.path().join("work");
        mark_owned(&work, "job", "manifest").unwrap();
        assert!(tag_existing(&work, "other", "other-manifest").is_err());
        owned(&work, "job", Some("manifest")).unwrap();
        assert_eq!(fs::read_dir(&work).unwrap().count(), 1);
    }

    fn set_test_junction(file: &fs::File, target: &Path) -> io::Result<()> {
        let display: Vec<u16> = target.as_os_str().encode_wide().collect();
        let substitute: Vec<u16> = format!(r"\??\{}", target.display())
            .encode_utf16()
            .collect();
        let mut path_buffer = substitute.clone();
        path_buffer.push(0);
        path_buffer.extend(&display);
        path_buffer.push(0);
        let mut bytes = Vec::new();
        bytes.extend(0xA000_0003u32.to_le_bytes());
        bytes.extend(((path_buffer.len() * 2 + 8) as u16).to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        for value in [
            0,
            (substitute.len() * 2) as u16,
            ((substitute.len() + 1) * 2) as u16,
            (display.len() * 2) as u16,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        for character in path_buffer {
            bytes.extend(character.to_le_bytes());
        }
        let mut written = 0;
        if unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                0x0009_00A4,
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut written,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn clear_test_junction(file: &fs::File) {
        let mut bytes = 0xA000_0003u32.to_le_bytes().to_vec();
        bytes.extend([0u8; 4]);
        let mut written = 0;
        assert_ne!(
            unsafe {
                DeviceIoControl(
                    file.as_raw_handle(),
                    0x0009_00AC,
                    bytes.as_ptr().cast(),
                    bytes.len() as u32,
                    std::ptr::null_mut(),
                    0,
                    &mut written,
                    std::ptr::null_mut(),
                )
            },
            0,
            "{}",
            io::Error::last_os_error()
        );
    }

    #[test]
    fn attribute_only_junction_mutation_cannot_redirect_relative_native_operations() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("pinned");
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside preserved").unwrap();
        let guard = DirectoryGuard::create(&path).unwrap();
        let writer = fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .share_mode(
                FILE_SHARE_READ
                    | FILE_SHARE_WRITE
                    | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE,
            )
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&path)
            .unwrap();
        set_test_junction(&writer, &outside).unwrap();
        assert!(
            DirectoryGuard::open(&path).is_err(),
            "an existing junction is rejected"
        );
        assert!(
            !guard
                .entries()
                .unwrap()
                .iter()
                .any(|name| name == "sentinel")
        );
        assert!(
            guard.create_file(&path.join("created-by-object")).is_err(),
            "native child create rejects a reparse-mutated parent instead of redirecting it"
        );
        assert!(!outside.join("created-by-object").exists());
        clear_test_junction(&writer);
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"outside preserved"
        );
    }

    #[test]
    fn test_prune_empty_directories_cleans_multilevel_empty_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sandbox");
        fs::create_dir_all(root.join("empty_a/empty_b/empty_c")).unwrap();
        fs::create_dir_all(root.join("keep_a/keep_b")).unwrap();
        fs::write(root.join("keep_a/keep_b/file.txt"), b"keep me").unwrap();

        assert!(root.join("empty_a/empty_b/empty_c").exists());
        assert!(root.join("keep_a/keep_b/file.txt").exists());

        let pruned = prune_empty_directories(&root).unwrap();
        assert_eq!(pruned, 3); // empty_c, empty_b, empty_a

        assert!(!root.join("empty_a").exists());
        assert!(root.join("keep_a/keep_b/file.txt").exists());
        assert!(root.exists(), "Root must never be deleted");
    }

    #[test]
    fn native_sleep_requests_are_released_on_their_own_thread_after_guard_moves() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let guard = SleepBlockerGuard::with_execution_state(move |flags| {
            let result = unsafe { SetThreadExecutionState(flags) };
            recorded
                .lock()
                .unwrap()
                .push((std::thread::current().id(), flags, result));
            result
        });
        let second = SleepBlockerGuard::acquire();
        std::thread::spawn(move || drop(guard)).join().unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, calls[1].0);
        assert_ne!(calls[0].0, std::thread::current().id());
        assert_eq!(calls[0].1, ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        assert_eq!(calls[1].1, ES_CONTINUOUS);
        assert!(calls.iter().all(|call| call.2 != 0));
        drop(second);
    }
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

    #[test]
    fn test_drive_letter_of_supports_verbatim_and_standard_paths() {
        assert_eq!(
            drive_letter_of(Path::new(r"C:\Users\test\file.zip")),
            Some('C')
        );
        assert_eq!(
            drive_letter_of(Path::new(r"\\?\C:\Users\test\file.zip")),
            Some('C')
        );
        assert_eq!(
            drive_letter_of(Path::new(r"c:\archives\pack.7z")),
            Some('C')
        );
        assert_eq!(
            drive_letter_of(Path::new(r"\\?\d:\data\file.rar")),
            Some('D')
        );
        assert_eq!(drive_letter_of(Path::new(r"\\server\share\file.zip")), None);
        assert_eq!(
            drive_letter_of(Path::new(r"\\?\UNC\server\share\file.zip")),
            None
        );
    }
}
