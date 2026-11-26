//! 基于句柄链的目录防劫持锁（DirectoryGuard / LockedEntry / LockedTree）。

use super::*;

/// The root-to-leaf chain excludes deletion and replacement. Relative native
/// operations below consume directory objects, so attribute-only reparse
/// mutation cannot redirect an absolute namespace lookup. Directory writes
/// must be shared for the kernel's own rename-target parent open to succeed.
#[derive(Clone)]
pub struct DirectoryGuard {
    pub(super) path: PathBuf,
    pub(super) handles: Vec<Arc<fs::File>>,
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

    pub(super) fn open_chain(path: &Path, create: bool) -> io::Result<Self> {
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

    pub(super) fn child_directory(&self, name: &Path, disposition: u32) -> io::Result<Self> {
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

    pub(super) fn open_read_file(&self, name: &Path) -> io::Result<fs::File> {
        let file = open_relative(self, name, GENERIC_READ, NATIVE_FILE_OPEN, false)?;
        validate_metadata(&file.metadata()?, Some(false))?;
        Ok(file)
    }
}

/// A source object and its ancestors remain pinned through readback, rename,
/// or deletion. None of these mutations reopen the source by pathname.
pub struct LockedEntry {
    pub(super) parent: DirectoryGuard,
    pub(super) path: PathBuf,
    pub(super) file: Arc<fs::File>,
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
    pub(super) root: LockedEntry,
    pub(super) entries: Vec<LockedEntry>,
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

pub(super) fn lock_descendants(root: &DirectoryGuard) -> io::Result<Vec<LockedEntry>> {
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
