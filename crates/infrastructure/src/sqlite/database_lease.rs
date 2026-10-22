//! Process-level leases for the production task repository.
//! All consumers share this path and OS-lock implementation.
use std::fs::{File, OpenOptions};
use std::path::{Component, Path, PathBuf};

pub struct DatabaseLease {
    _file: File,
    path: PathBuf,
}

impl DatabaseLease {
    pub fn shared(path: &Path) -> Result<Self, String> {
        Self::acquire(path, false)
    }
    pub fn exclusive(path: &Path) -> Result<Self, String> {
        Self::acquire(path, true)
    }
    pub fn database_path(&self) -> &Path {
        &self.path
    }

    fn acquire(path: &Path, exclusive: bool) -> Result<Self, String> {
        let unsafe_path = || "StorageDatabasePathUnsafe".to_string();
        if path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(unsafe_path());
        }
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|_| unsafe_path())?
                .join(path)
        };
        for ancestor in absolute.ancestors() {
            match std::fs::symlink_metadata(ancestor) {
                Ok(metadata) if redirected(&metadata) => return Err(unsafe_path()),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(unsafe_path()),
            }
        }
        let parent = absolute.parent().ok_or_else(unsafe_path)?;
        std::fs::create_dir_all(parent).map_err(|_| unsafe_path())?;
        let parent = std::fs::canonicalize(parent).map_err(|_| unsafe_path())?;
        let name = absolute.file_name().ok_or_else(unsafe_path)?;
        let database = parent.join(name);
        if database.exists()
            && !regular_unaliased_file(&File::open(&database).map_err(|_| unsafe_path())?)?
        {
            return Err(unsafe_path());
        }
        // Resolve the existing file too: Windows 8.3 names otherwise derive a
        // different sidecar from the same database inode.
        let database = if database.exists() {
            std::fs::canonicalize(&database).map_err(|_| unsafe_path())?
        } else {
            database
        };
        let parent = database.parent().ok_or_else(unsafe_path)?;
        let name = database.file_name().ok_or_else(unsafe_path)?;
        let mut lock_name = name.to_os_string();
        lock_name.push(".openunpack-storage.lock");
        let lock_path = parent.join(lock_name);
        match std::fs::symlink_metadata(&lock_path) {
            Ok(metadata) if redirected(&metadata) || !metadata.is_file() => {
                return Err(unsafe_path());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(unsafe_path()),
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Keep the path stable while held, and open a reparse file without following it.
            options.share_mode(0x1 | 0x2).custom_flags(0x00200000);
        }
        let file = options
            .open(lock_path)
            .map_err(|_| "StorageLeaseUnavailable".to_string())?;
        if !regular_unaliased_file(&file)? {
            return Err(unsafe_path());
        }
        let locked = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        locked.map_err(|_| "StorageLeaseUnavailable".to_string())?;
        Ok(Self {
            _file: file,
            path: database,
        })
    }
}

fn redirected(metadata: &std::fs::Metadata) -> bool {
    let mut redirected = metadata.file_type().is_symlink();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        redirected |= metadata.file_attributes() & 0x400 != 0;
    }
    redirected
}

fn regular_unaliased_file(file: &File) -> Result<bool, String> {
    let metadata = file
        .metadata()
        .map_err(|_| "StorageDatabasePathUnsafe".to_string())?;
    if !metadata.is_file() || redirected(&metadata) {
        return Ok(false);
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        let success = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
        if success == 0 {
            return Err("StorageDatabasePathUnsafe".into());
        }
        Ok(info.nNumberOfLinks == 1 && info.dwFileAttributes & 0x400 == 0)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(metadata.nlink() == 1)
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err("StorageLeaseUnsupported".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_and_exclusive_ownership_are_mutually_exclusive_until_drop() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("jobs.db");
        let first = DatabaseLease::shared(&path).unwrap();
        let second = DatabaseLease::shared(&path).unwrap();
        assert!(DatabaseLease::exclusive(&path).is_err());
        drop(first);
        assert!(DatabaseLease::exclusive(&path).is_err());
        drop(second);
        let exclusive = DatabaseLease::exclusive(&path).unwrap();
        assert!(DatabaseLease::shared(&path).is_err());
        assert!(DatabaseLease::exclusive(&path).is_err());
        drop(exclusive);
        DatabaseLease::shared(&path).unwrap();
    }
    #[test]
    fn hard_link_database_aliases_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("jobs.db");
        std::fs::write(&path, b"isolated placeholder").unwrap();
        std::fs::create_dir(root.path().join("other-directory")).unwrap();
        let alias = root.path().join("other-directory").join("other.db");
        std::fs::hard_link(&path, &alias).unwrap();
        assert!(
            matches!(DatabaseLease::shared(&path), Err(error) if error == "StorageDatabasePathUnsafe")
        );
        assert!(
            matches!(DatabaseLease::exclusive(&alias), Err(error) if error == "StorageDatabasePathUnsafe")
        );
    }

    #[test]
    fn os_lease_is_enforced_between_processes() {
        if let Some(path) = std::env::var_os("OPENUNPACK_TEST_DB_LEASE_PATH") {
            let path = PathBuf::from(path);
            let exclusive = std::env::var("OPENUNPACK_TEST_DB_LEASE_MODE").unwrap() == "exclusive";
            let result = if exclusive {
                DatabaseLease::exclusive(&path)
            } else {
                DatabaseLease::shared(&path)
            };
            assert!(matches!(result, Err(error) if error == "StorageLeaseUnavailable"));
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("process-owned.db");
        let run_child = |mode: &str| {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "sqlite::database_lease::tests::os_lease_is_enforced_between_processes",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("OPENUNPACK_TEST_DB_LEASE_PATH", &path)
                .env("OPENUNPACK_TEST_DB_LEASE_MODE", mode)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "child lease check failed: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        };
        let shared = DatabaseLease::shared(&path).unwrap();
        run_child("exclusive");
        drop(shared);
        let exclusive = DatabaseLease::exclusive(&path).unwrap();
        run_child("shared");
        drop(exclusive);
        DatabaseLease::shared(&path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn junction_ancestor_cannot_bypass_database_format_ownership() {
        let root = tempfile::tempdir().unwrap();
        let actual = root.path().join("actual");
        std::fs::create_dir(&actual).unwrap();
        let path = actual.join("jobs.db");
        std::fs::write(&path, b"isolated placeholder").unwrap();
        let held = DatabaseLease::shared(&path).unwrap();
        let alias = root.path().join("redirect");
        let output = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&alias)
            .arg(&actual)
            .output()
            .unwrap();
        assert!(output.status.success());
        let result = DatabaseLease::exclusive(&alias.join("jobs.db"));
        std::fs::remove_dir(&alias).unwrap();
        assert!(matches!(result, Err(error) if error == "StorageDatabasePathUnsafe"));
        drop(held);
        assert!(path.is_file());
    }

    #[cfg(windows)]
    #[test]
    fn short_file_names_use_the_same_sidecar_as_the_long_database_name() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
        let root = tempfile::tempdir().unwrap();
        let path = root
            .path()
            .join("database-with-long-filename-for-lease-check.sqlite");
        std::fs::write(&path, b"isolated placeholder").unwrap();
        let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut buffer = vec![0u16; 32768];
        let written =
            unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) };
        assert!(
            written > 0 && (written as usize) < buffer.len(),
            "could not inspect the isolated 8.3 path"
        );
        let short = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..written as usize]));
        if short.file_name() == path.file_name() {
            eprintln!(
                "8.3 alias verification skipped: this temporary volume does not generate short filenames"
            );
            return;
        }
        let shared = DatabaseLease::shared(&path).unwrap();
        assert!(
            matches!(DatabaseLease::exclusive(&short), Err(error) if error == "StorageLeaseUnavailable")
        );
        let alias_shared = DatabaseLease::shared(&short).unwrap();
        drop(shared);
        drop(alias_shared);
        let exclusive = DatabaseLease::exclusive(&short).unwrap();
        assert!(
            matches!(DatabaseLease::shared(&path), Err(error) if error == "StorageLeaseUnavailable")
        );
        drop(exclusive);
        assert!(path.is_file());
    }
}
