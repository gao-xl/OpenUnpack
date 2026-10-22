//! A Shell recycle operation cannot consume a DELETE-exclusive file handle.
//! Keep the original path and only permit this handoff when its ACLs exclude
//! other writers. Shared/unprovable namespaces retain the source instead.
use crate::paths::DirectoryGuard;
use std::{
    fs::File,
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, LocalFree},
    Security::{
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        *,
    },
    Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ, READ_CONTROL},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub struct RecyclableSource {
    file: File,
    path: PathBuf,
    _ancestors: DirectoryGuard,
}
impl RecyclableSource {
    pub fn open(path: &Path) -> io::Result<Self> {
        validate_shell_path(path)?;
        let parent = path.parent().ok_or_else(unsafe_namespace)?;
        let ancestors = DirectoryGuard::open(parent)?;
        let exclusive =
            ancestors.open_file_with_share(path, GENERIC_READ | READ_CONTROL, FILE_SHARE_READ)?;
        let identity = file_identity(&exclusive)?;
        if identity.nNumberOfLinks != 1 {
            return Err(unsafe_namespace());
        }
        let file = ancestors.open_file_with_share(
            path,
            GENERIC_READ | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
        )?;
        let metadata = file.metadata()?;
        use std::os::windows::fs::MetadataExt;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
            return Err(unsafe_namespace());
        }
        let user = CurrentUser::read()?;
        verify_acl(&file, &user)?;
        // Parent DELETE_CHILD can bypass a leaf DELETE restriction. Inherited
        // permissions can be rewritten from above: stop only at a verified
        // protected DACL, while every physical ancestor remains pinned.
        let mut protected = false;
        for directory in ancestors.ancestors() {
            let handle = directory.open_own_security()?;
            if verify_acl(&handle, &user)? {
                protected = true;
                break;
            }
        }
        if !protected {
            return Err(unsafe_namespace());
        }
        let handoff = file_identity(&file)?;
        if identity.dwVolumeSerialNumber != handoff.dwVolumeSerialNumber
            || identity.nFileIndexHigh != handoff.nFileIndexHigh
            || identity.nFileIndexLow != handoff.nFileIndexLow
        {
            return Err(unsafe_namespace());
        }
        drop(exclusive);
        Ok(Self {
            file,
            path: path.to_owned(),
            _ancestors: ancestors,
        })
    }
    pub fn file(&self) -> &File {
        &self.file
    }
    pub fn recycle(&self) -> Result<(), String> {
        crate::paths::shell_recycle(&self.path)
    }
}

fn file_identity(
    file: &File,
) -> io::Result<windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION> {
    let mut info = unsafe { std::mem::zeroed() };
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(
            file.as_raw_handle() as _,
            &mut info,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

fn unsafe_namespace() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "回收站清理不能固定共享目录中的源文件身份；源文件已保留。",
    )
}

pub(crate) fn validate_shell_path(path: &Path) -> io::Result<()> {
    // Shell parsing uses Win32 names after removing the extended prefix. NT
    // names with trailing dot/space, ADS or device aliases must never map to
    // another ordinary file during this handoff.
    use std::path::{Component, Prefix};
    if !path.is_absolute() {
        return Err(unsafe_namespace());
    }
    let validate_name = |name: &std::ffi::OsStr| {
        let name = name.to_str().ok_or_else(unsafe_namespace)?;
        // A slash in a verbatim component is a literal NT name, whereas Shell
        // parsing would treat it as a separator after removing the prefix.
        if name.contains(['/', '\\']) {
            return Err(unsafe_namespace());
        }
        crate::paths::relative(name)
            .map(|_| ())
            .map_err(|_| unsafe_namespace())
    };
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(_) | Prefix::VerbatimDisk(_) => (),
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    validate_name(server)?;
                    validate_name(share)?;
                }
                _ => return Err(unsafe_namespace()),
            },
            Component::RootDir | Component::CurDir => (),
            Component::Normal(name) => validate_name(name)?,
            Component::ParentDir => return Err(unsafe_namespace()),
        }
    }
    Ok(())
}

pub fn verify_private_directory(directory: &DirectoryGuard) -> io::Result<()> {
    let user = CurrentUser::read()?;
    for ancestor in directory.ancestors() {
        if verify_acl(&ancestor.open_own_security()?, &user)? {
            return Ok(());
        }
    }
    Err(unsafe_namespace())
}

pub(crate) fn verify_private_entries(entries: &[crate::paths::LockedEntry]) -> io::Result<()> {
    let user = CurrentUser::read()?;
    for entry in entries {
        verify_acl(entry.file(), &user)?;
    }
    Ok(())
}

struct CurrentUser(Vec<usize>);
impl CurrentUser {
    fn read() -> io::Result<Self> {
        let mut raw = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(raw as _) };
        let mut needed = 0;
        unsafe {
            GetTokenInformation(
                token.as_raw_handle() as _,
                TokenUser,
                std::ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        if needed == 0 || needed > 65536 {
            return Err(unsafe_namespace());
        }
        let mut bytes = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle() as _,
                TokenUser,
                bytes.as_mut_ptr() as _,
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(bytes))
    }
    fn trusted(&self, sid: PSID) -> bool {
        if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
            return false;
        }
        let user = unsafe { &*(self.0.as_ptr() as *const TOKEN_USER) };
        unsafe {
            EqualSid(sid, user.User.Sid) != 0
                || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
        }
    }
}

struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0 as _);
        }
    }
}

fn verify_acl(file: &File, user: &CurrentUser) -> io::Result<bool> {
    let mut owner = std::ptr::null_mut();
    let mut dacl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    let error = unsafe {
        GetSecurityInfo(
            file.as_raw_handle() as _,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if error != 0 {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    let descriptor = Descriptor(descriptor);
    if !user.trusted(owner) || dacl.is_null() || unsafe { IsValidAcl(dacl) } == 0 {
        return Err(unsafe_namespace());
    }
    const MODIFICATION: u32 =
        0x00000156 | 0x00010000 | 0x00040000 | 0x00080000 | 0x40000000 | 0x10000000;
    for index in 0..unsafe { (*dacl).AceCount } as u32 {
        let mut raw = std::ptr::null_mut();
        if unsafe { GetAce(dacl, index, &mut raw) } == 0 {
            return Err(unsafe_namespace());
        }
        let header = unsafe { &*(raw as *const ACE_HEADER) };
        match header.AceType {
            0 => {
                if (header.AceSize as usize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>() {
                    return Err(unsafe_namespace());
                }
                let allow = unsafe { &*(raw as *const ACCESS_ALLOWED_ACE) };
                let sid = (&allow.SidStart as *const u32).cast_mut().cast();
                if unsafe { IsValidSid(sid) } == 0
                    || (unsafe { GetLengthSid(sid) } as usize).saturating_add(8)
                        > header.AceSize as usize
                    || (allow.Mask & MODIFICATION != 0 && !user.trusted(sid))
                {
                    return Err(unsafe_namespace());
                }
            }
            1 => {} // A deny ACE cannot grant a competing modifier access.
            _ => return Err(unsafe_namespace()), // Unknown/object/conditional ACLs are unprovable.
        }
    }
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0 {
        return Err(unsafe_namespace());
    }
    Ok(control & SE_DACL_PROTECTED != 0)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_paths_reject_relative_and_verbatim_separator_aliases() {
        for path in [
            r"archive.zip",
            r"C:archive.zip",
            r"C:\private\..\other\archive.zip",
            r"\\?\C:\private\nested/archive.zip",
            r"\\?\UNC\server\share\nested/archive.zip",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\archive.zip",
        ] {
            assert!(validate_shell_path(Path::new(path)).is_err(), "{path}");
        }
        for path in [
            r"C:/private/nested/archive.zip",
            r"C:\private\nested/archive.zip",
            r"\\?\C:\private\nested\archive.zip",
            r"\\server\share\nested\archive.zip",
            r"\\?\UNC\server\share\nested\archive.zip",
        ] {
            assert!(validate_shell_path(Path::new(path)).is_ok(), "{path}");
        }
    }

    #[test]
    fn shell_ambiguous_nt_names_are_rejected_without_touching_other_files() {
        let (_temporary, private) = fixture();
        let private = private.canonicalize().unwrap();
        let ordinary = private.join("archive.zip");
        let ambiguous = private.join("archive.zip.");
        fs::write(&ordinary, b"ordinary sentinel").unwrap();
        fs::write(&ambiguous, b"NT trailing-dot source").unwrap();
        assert!(RecyclableSource::open(&ambiguous).is_err());
        assert_eq!(fs::read(&ordinary).unwrap(), b"ordinary sentinel");
        assert_eq!(fs::read(&ambiguous).unwrap(), b"NT trailing-dot source");
        for name in ["CON.zip", "file.zip ", "file.zip:stream"] {
            assert!(validate_shell_path(&private.join(name)).is_err());
        }
    }
    use std::{fs, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::{
        Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        },
        Security::{PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW},
    };
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let temporary = tempfile::tempdir().unwrap();
        let private = temporary.path().join("private");
        DirectoryGuard::open(temporary.path())
            .unwrap()
            .create_private_child(&private)
            .unwrap();
        fs::write(private.join("source.zip"), b"synthetic source").unwrap();
        (temporary, private)
    }
    fn set_test_dacl(path: &Path, sddl: &str) {
        let mut descriptor = std::ptr::null_mut();
        let mut size = 0;
        let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    &mut size,
                )
            },
            0
        );
        let descriptor = Descriptor(descriptor);
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        assert_ne!(
            unsafe {
                SetFileSecurityW(
                    path.as_ptr(),
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    descriptor.0,
                )
            },
            0
        );
    }
    #[test]
    fn private_single_link_source_supports_safe_shell_handoff_and_blocks_writes() {
        let (_temporary, private) = fixture();
        let source = RecyclableSource::open(&private.join("source.zip")).unwrap();
        assert!(fs::write(private.join("source.zip"), b"mutation").is_err());
        assert_eq!(source.file().metadata().unwrap().len(), 16);
        drop(source);
        fs::write(private.join("source.zip"), b"after release").unwrap();
    }
    #[test]
    fn writable_leaf_and_null_dacl_keep_the_source() {
        for sddl in ["D:P(A;;FA;;;WD)", "D:NO_ACCESS_CONTROL"] {
            let (_temporary, private) = fixture();
            let path = private.join("source.zip");
            set_test_dacl(&path, sddl);
            assert!(RecyclableSource::open(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), b"synthetic source");
        }
    }
    #[test]
    fn writable_parent_and_inherited_attributes_keep_the_source() {
        let (_temporary, private) = fixture();
        set_test_dacl(&private, "D:P(A;OICI;FA;;;WD)");
        assert!(RecyclableSource::open(&private.join("source.zip")).is_err());
        assert!(private.join("source.zip").exists());
    }
    #[test]
    fn hard_link_and_existing_delete_handles_keep_the_source() {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_WRITE,
        };
        let (_temporary, private) = fixture();
        let path = private.join("source.zip");
        fs::hard_link(&path, private.join("alias.zip")).unwrap();
        assert!(RecyclableSource::open(&path).is_err());
        fs::remove_file(private.join("alias.zip")).unwrap();
        let existing = OpenOptions::new()
            .access_mode(DELETE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
            .unwrap();
        assert!(RecyclableSource::open(&path).is_err());
        drop(existing);
        assert!(RecyclableSource::open(&path).is_ok());
    }
}
