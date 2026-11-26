//! 密码库原子写盘、Windows DPAPI 加解密及便携密钥加密存储。

use super::*;

pub(super) fn atomic_write_file(destination: &Path, content: &[u8]) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "Invalid destination path".to_string())?;
    fs::create_dir_all(parent).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;

    let tmp_name = format!(".tmp_{}_{}", std::process::id(), Uuid::new_v4().simple());
    let tmp_path = parent.join(tmp_name);

    let write_result: Result<(), String> = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
        f.write_all(content)
            .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
        f.sync_all().map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut wide_src: Vec<u16> = tmp_path.as_os_str().encode_wide().collect();
        wide_src.push(0);
        let mut wide_dst: Vec<u16> = destination.as_os_str().encode_wide().collect();
        wide_dst.push(0);

        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };

        let mut ret = 0;
        for attempt in 0..15 {
            ret = unsafe {
                MoveFileExW(
                    wide_src.as_ptr(),
                    wide_dst.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if ret != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5 * (attempt + 1)));
        }

        if ret == 0 {
            let _ = fs::remove_file(&tmp_path);
            return Err("Failed to atomic replace file".to_string());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        fs::rename(&tmp_path, destination).map_err(|_| {
            let _ = fs::remove_file(&tmp_path);
            ERR_VAULT_CORRUPT.to_string()
        })
    }
}

#[cfg(windows)]
pub(super) fn dpapi_encrypt(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi_transform(data, true)
}

#[cfg(windows)]
pub(super) fn dpapi_decrypt(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi_transform(data, false)
}

#[cfg(windows)]
pub(super) fn dpapi_transform(data: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    if data.len() > u32::MAX as usize {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let success = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if success == 0 || output.pbData.is_null() {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        LocalFree(output.pbData as *mut std::ffi::c_void);
    }
    Ok(result)
}

/// Windows portable mode is opt-in: an environment switch or an explicit key.
/// On other platforms portable storage is the only implementation.
#[cfg(windows)]
pub(super) fn portable_vault_opted_in() -> bool {
    std::env::var("OPENUNPACK_PORTABLE_VAULT").is_ok()
        || std::env::var("OPENUNPACK_VAULT_KEY").is_ok()
}
#[cfg(not(windows))]
pub(super) fn portable_vault_opted_in() -> bool {
    true
}

pub(super) fn resolve_vault_key(path: &Path) -> Result<Zeroizing<Vec<u8>>, String> {
    if let Ok(env_key) = std::env::var("OPENUNPACK_VAULT_KEY") {
        let trimmed = env_key.trim();
        if !trimmed.is_empty() {
            return Ok(Zeroizing::new(trimmed.as_bytes().to_vec()));
        }
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let key_file = parent.join(VAULT_KEY_FILE);

    // Generate first: `create_new` is the only race-free "first run" test, and
    // it never truncates a key another process already wrote.
    let mut key_bytes = Zeroizing::new([0u8; VAULT_KEY_LEN]);
    rand::thread_rng().fill_bytes(&mut *key_bytes);
    if !parent.is_dir() {
        fs::create_dir_all(parent).map_err(|_| ERR_VAULT_KEY_UNAVAILABLE.to_string())?;
    }
    match create_vault_key_file(&key_file, &key_bytes[..]) {
        Ok(()) => return Ok(Zeroizing::new(key_bytes.to_vec())),
        // A concurrent first run won the race: adopt the key it persisted.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(ERR_VAULT_KEY_UNAVAILABLE.to_string()),
    }
    read_vault_key_file(&key_file)?.ok_or_else(|| ERR_VAULT_CORRUPT.to_string())
}

/// Opens the existing key file without following a redirect, then validates the
/// handle itself so a swapped path cannot smuggle a link in after the check.
pub(super) fn read_vault_key_file(key_file: &Path) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: open the link itself, never its target.
        options.custom_flags(0x0020_0000);
    }
    let file = match options.open(key_file) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ERR_VAULT_CORRUPT.to_string()),
    };
    let metadata = file.metadata().map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
    if !portable_vault_opted_in() {
        return Err(ERR_PORTABLE_NOT_ENABLED.to_string());
    }
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Vault key must be a regular file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Vault key must not be a reparse point".into());
        }
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(VAULT_KEY_LEN));
    file.take(MAX_VAULT_KEY_FILE_SIZE)
        .read_to_end(&mut bytes)
        .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
    if bytes.len() != VAULT_KEY_LEN {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    Ok(Some(bytes))
}

pub(super) fn create_vault_key_file(key_file: &Path, key: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(key_file)?;
    // A half-written key would be read back as if it were the real one and
    // would silently produce an undecryptable vault, so remove it on failure.
    if let Err(error) = file.write_all(key).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(key_file);
        return Err(error);
    }
    drop(file);
    // Portable mode keeps the key next to the vault by design. That is only
    // acceptable when no other principal can read it, so the DACL is reduced
    // to the owner. Failing to harden the key is fatal: writing a key that
    // every local user can read would be worse than refusing to start.
    #[cfg(windows)]
    restrict_key_file_to_owner(key_file)?;
    Ok(())
}

/// Replaces the key file's DACL with "protected, full control for the owner".
#[cfg(windows)]
pub(super) fn restrict_key_file_to_owner(key_file: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
    };

    // OW is the "owner rights" SID, so the rule follows the file's owner rather
    // than hard-coding a user SID that a copied profile would invalidate.
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)\0".encode_utf16().collect();
    let mut descriptor = std::ptr::null_mut();
    let mut size = 0u32;
    // SAFETY: `sddl` is NUL-terminated and the out-params are valid.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            &mut size,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let path: Vec<u16> = key_file.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: the descriptor is owned by us and released below.
    let applied = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    // SAFETY: `descriptor` came from the conversion API above.
    unsafe {
        LocalFree(descriptor);
    }
    if applied == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub fn portable_encrypt(data: &[u8], path: &Path) -> Result<Vec<u8>, String> {
    let master_key = resolve_vault_key(path)?;
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let mut derived_key = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2_hmac::<Sha256>(
        &master_key,
        &salt,
        VAULT_PORTABLE_DEFAULT_ITER,
        &mut *derived_key,
    );

    let cipher =
        Aes256Gcm::new_from_slice(&*derived_key).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;

    let mut output = Vec::with_capacity(8 + 4 + 16 + 12 + ciphertext.len());
    output.extend_from_slice(VAULT_MAGIC);
    output.extend_from_slice(&VAULT_PORTABLE_DEFAULT_ITER.to_be_bytes());
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

pub fn portable_decrypt(data: &[u8], path: &Path) -> Result<Vec<u8>, String> {
    if data.len() < 8 + 4 + 16 + 12 + 16 {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    let iter = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    if !(BACKUP_MIN_ITER..=BACKUP_MAX_ITER).contains(&iter) {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    let salt = &data[12..28];
    let nonce_bytes = &data[28..40];
    let ciphertext = &data[40..];

    let master_key = resolve_vault_key(path)?;
    let mut derived_key = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2_hmac::<Sha256>(&master_key, salt, iter, &mut *derived_key);

    let cipher =
        Aes256Gcm::new_from_slice(&*derived_key).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
    Ok(plaintext)
}

pub(super) fn vault_encrypt(data: &[u8], path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        let use_portable = portable_vault_opted_in();
        if use_portable {
            portable_encrypt(data, path)
        } else {
            dpapi_encrypt(data)
        }
    }
    #[cfg(not(windows))]
    {
        portable_encrypt(data, path)
    }
}

pub(super) fn vault_decrypt(data: &[u8], path: &Path) -> Result<Vec<u8>, String> {
    if data.starts_with(VAULT_MAGIC) {
        #[cfg(windows)]
        if !portable_vault_opted_in() {
            return Err(ERR_PORTABLE_NOT_ENABLED.to_string());
        }
        portable_decrypt(data, path)
    } else {
        #[cfg(windows)]
        {
            dpapi_decrypt(data)
        }
        #[cfg(not(windows))]
        {
            Err(ERR_VAULT_CORRUPT.to_string())
        }
    }
}
