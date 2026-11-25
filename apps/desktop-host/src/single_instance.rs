//! Single-instance wakeup pipe, Explorer CLI argument intake, and WebView2 runtime preflight.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    sync::Arc,
};

const WAKE_TOKEN_FILE: &str = ".wake.token";
const MAX_WAKE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WakeMessage {
    token: String,
    paths: Vec<String>,
}

/// Extracts existing file/folder positional paths from command-line arguments.
pub fn collect_cli_open_paths() -> Vec<String> {
    extract_cli_open_paths_from(std::env::args_os().skip(1))
}

pub(crate) fn extract_cli_open_paths_from<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString>,
{
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut skip_next = false;

    for raw_os in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        let os: std::ffi::OsString = raw_os.into();
        let text = os.to_string_lossy();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if matches!(
            trimmed,
            "--data-dir"
                | "--port"
                | "--on-complete"
                | "--on-extract-complete"
                | "--watch-dir"
                | "--watch"
                | "--read-root"
                | "--write-root"
        ) {
            skip_next = true;
            continue;
        }
        if trimmed.starts_with('-')
            || (trimmed.starts_with('/') && Path::new(trimmed).as_os_str() != Path::new("/"))
        {
            // Skip CLI flags and slash switches unless Windows path? On Windows paths start with C:\ or \\server
            #[cfg(windows)]
            if trimmed.starts_with('/') {
                continue;
            }
        }
        let candidate = Path::new(trimmed);
        if !candidate.exists() {
            continue;
        }
        let dedup_key = trimmed.to_ascii_lowercase();
        if seen.insert(dedup_key) {
            paths.push(trimmed.to_string());
        }
    }
    paths
}

/// Checks whether the Microsoft Edge WebView2 Evergreen Runtime is registered on Windows.
#[cfg(windows)]
pub fn is_webview2_installed() -> bool {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW,
        RegQueryValueExW,
    };

    const SUBKEYS: [&str; 2] = [
        r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    ];
    let value_name: Vec<u16> = "pv\0".encode_utf16().collect();

    let has_pv = |root: HKEY, subkey: &str| -> bool {
        let wide_key: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
        let mut hkey: HKEY = std::ptr::null_mut();
        let rc = unsafe { RegOpenKeyExW(root, wide_key.as_ptr(), 0, KEY_READ, &mut hkey) };
        if rc != 0 || hkey.is_null() {
            return false;
        }
        let mut buf = [0u16; 128];
        let mut size = (buf.len() * std::mem::size_of::<u16>()) as u32;
        let mut reg_type = 0u32;
        let qrc = unsafe {
            RegQueryValueExW(
                hkey,
                value_name.as_ptr(),
                std::ptr::null(),
                &mut reg_type,
                buf.as_mut_ptr() as *mut u8,
                &mut size,
            )
        };
        unsafe {
            RegCloseKey(hkey);
        }
        if qrc != 0 || size < 2 {
            return false;
        }
        let len = (size as usize / 2).min(buf.len());
        let ver = String::from_utf16_lossy(&buf[..len])
            .trim_end_matches('\0')
            .trim()
            .to_string();
        !ver.is_empty() && ver != "0.0.0.0"
    };

    for sub in SUBKEYS {
        if has_pv(HKEY_LOCAL_MACHINE, sub) {
            return true;
        }
    }
    has_pv(HKEY_CURRENT_USER, SUBKEYS[1])
}

#[cfg(not(windows))]
pub fn is_webview2_installed() -> bool {
    true
}

pub(crate) fn pipe_name_for_data_dir(data_dir: &Path) -> String {
    let canonical = fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    let key = canonical.to_string_lossy().to_ascii_lowercase();
    let digest = Sha256::digest(key.as_bytes());
    let encoded = hex::encode(digest);
    format!(r"\\.\pipe\openunpack-wake-{}", &encoded[..32])
}

pub fn prepare_wake_token(data_dir: &Path) -> Result<String, String> {
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let token_path = data_dir.join(WAKE_TOKEN_FILE);
    fs::write(&token_path, token.as_bytes())
        .map_err(|e| format!("Failed to persist single-instance wake token: {e}"))?;
    Ok(token)
}

pub fn remove_wake_token(data_dir: &Path) {
    let _ = fs::remove_file(data_dir.join(WAKE_TOKEN_FILE));
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(windows)]
pub fn try_notify_running_instance(data_dir: &Path, paths: &[String]) -> bool {
    let Ok(raw_token) = fs::read_to_string(data_dir.join(WAKE_TOKEN_FILE)) else {
        return false;
    };
    let token = raw_token.trim().to_string();
    if token.is_empty() {
        return false;
    }
    let pipe_name = pipe_name_for_data_dir(data_dir);
    let payload = match serde_json::to_string(&WakeMessage {
        token,
        paths: paths.to_vec(),
    }) {
        Ok(json) if json.len() + 1 <= MAX_WAKE_BYTES => format!("{json}\n"),
        _ => return false,
    };

    for _ in 0..8 {
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pipe_name)
        {
            Ok(mut stream) => {
                if stream.write_all(payload.as_bytes()).is_err() || stream.flush().is_err() {
                    return false;
                }
                let mut reader = BufReader::new(stream);
                let mut ack = String::new();
                return reader.read_line(&mut ack).is_ok() && ack.trim() == "OK";
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(40)),
        }
    }
    false
}

#[cfg(not(windows))]
pub fn try_notify_running_instance(_data_dir: &Path, _paths: &[String]) -> bool {
    false
}

#[cfg(windows)]
pub fn spawn_wake_server(
    data_dir: &Path,
    expected_token: String,
    on_wake: Arc<dyn Fn(Vec<String>) + Send + Sync>,
) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe_name = pipe_name_for_data_dir(data_dir);
    tauri::async_runtime::spawn(async move {
        let mut first = true;
        loop {
            let mut opts = ServerOptions::new();
            opts.access_inbound(true)
                .access_outbound(true)
                .reject_remote_clients(true);
            if first {
                opts.first_pipe_instance(true);
                first = false;
            }
            let server = match opts.create(&pipe_name) {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!(error = %err, "wake_pipe_create_failed");
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    continue;
                }
            };
            if server.connect().await.is_err() {
                continue;
            }

            let expected = expected_token.clone();
            let cb = on_wake.clone();
            tauri::async_runtime::spawn(async move {
                let (reader_half, mut writer_half) = tokio::io::split(server);
                let mut limited = reader_half.take(MAX_WAKE_BYTES as u64);
                let mut buf_reader = AsyncBufReader::new(&mut limited);
                let mut line = String::new();
                let read_res = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    buf_reader.read_line(&mut line),
                )
                .await;
                if !matches!(read_res, Ok(Ok(n)) if n > 0) {
                    return;
                }
                let Ok(msg) = serde_json::from_str::<WakeMessage>(line.trim()) else {
                    return;
                };
                if !constant_time_eq(msg.token.trim(), expected.trim()) {
                    tracing::warn!("wake_pipe_rejected_invalid_token");
                    return;
                }
                let _ = writer_half.write_all(b"OK\n").await;
                let _ = writer_half.shutdown().await;
                cb(msg.paths);
            });
        }
    });
}

#[cfg(not(windows))]
pub fn spawn_wake_server(
    _data_dir: &Path,
    _expected_token: String,
    _on_wake: Arc<dyn Fn(Vec<String>) + Send + Sync>,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_path_extraction_filters_flags_missing_and_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("demo.zip");
        fs::write(&file, b"pk").unwrap();
        let missing = dir.path().join("missing.7z");
        let args = [
            "--headless".to_string(),
            "--port".to_string(),
            "47980".to_string(),
            file.to_string_lossy().to_string(),
            file.to_string_lossy().to_string(),
            missing.to_string_lossy().to_string(),
            "/S".to_string(),
        ];
        let parsed = extract_cli_open_paths_from(args);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0], file.to_string_lossy());
    }

    #[test]
    fn pipe_name_is_deterministic_per_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let a = pipe_name_for_data_dir(dir.path());
        let b = pipe_name_for_data_dir(dir.path());
        assert_eq!(a, b);
        assert!(a.starts_with(r"\\.\pipe\openunpack-wake-"));
    }
}
