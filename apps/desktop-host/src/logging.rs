//! Process logging and crash capture.
//!
//! The GUI build has no attached console, so `println!`/`eprintln!` output is
//! discarded and a failure leaves no trace. Every diagnostic is written to a
//! size-rotated file under `<data_dir>/logs` instead, with automatic sanitization
//! of passwords and sensitive tokens. Panics are captured via a global Panic Hook
//! that writes timestamped crash reports under `<data_dir>/crashes` with stack traces.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

/// One file per ~8 MiB with five generations kept: bounded disk use, while still
/// long enough to reconstruct a session that went wrong.
const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;
const LOG_GENERATIONS: u32 = 5;
const LOG_FILE_NAME: &str = "openunpack.log";
/// Console verbosity override, e.g. `OPENUNPACK_LOG=debug` or
/// `OPENUNPACK_LOG=info,openunpack_core=trace`.
const LOG_ENV: &str = "OPENUNPACK_LOG";

/// Redacts sensitive passwords, credentials, and tokens from log output.
pub fn sanitize_log_bytes(bytes: &[u8]) -> Vec<u8> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };

    let sanitized = sanitize_log_text(text);
    sanitized.into_bytes()
}

/// Redacts sensitive keywords, password assignments, and CLI arguments.
pub fn sanitize_log_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    // Fast-path: if text does not contain sensitive keywords, return as is
    let lower = text.to_lowercase();
    if !lower.contains("password")
        && !lower.contains("secret")
        && !lower.contains("pwd")
        && !lower.contains("-p")
    {
        return text.to_string();
    }

    for line in text.lines() {
        let mut line_str = line.to_string();

        // Redact -p<secret> CLI tokens
        if let Some(pos) = line_str.find("-p") {
            let rest = &line_str[pos + 2..];
            if !rest.starts_with('-') && !rest.starts_with(' ') && !rest.is_empty() {
                let end = rest
                    .find([' ', '"', '\'', ';', '\r', '\n'])
                    .unwrap_or(rest.len());
                let token_len = 2 + end;
                line_str.replace_range(pos..pos + token_len, "-p***REDACTED***");
            }
        }

        // Redact password = "..." or password: "..."
        for kw in &[
            "\"password\":",
            "password=",
            "password:",
            "secret=",
            "secret:",
            "pwd=",
        ] {
            let mut search_idx = 0;
            while let Some(rel_pos) = line_str[search_idx..].to_lowercase().find(kw) {
                let pos = search_idx + rel_pos + kw.len();
                let rest = &line_str[pos..];
                let trimmed = rest.trim_start();
                let trim_offset = rest.len() - trimmed.len();
                let val_start = pos + trim_offset;

                if let Some(stripped) = trimmed.strip_prefix('"') {
                    if let Some(close_quote) = stripped.find('"') {
                        line_str.replace_range(
                            val_start..val_start + close_quote + 2,
                            "\"***REDACTED***\"",
                        );
                        search_idx = val_start + "\"***REDACTED***\"".len();
                        continue;
                    }
                } else {
                    let end = trimmed
                        .find([' ', ',', ';', '}', ']', '\r', '\n'])
                        .unwrap_or(trimmed.len());
                    line_str.replace_range(val_start..val_start + end, "***REDACTED***");
                    search_idx = val_start + "***REDACTED***".len();
                    continue;
                }
                search_idx = pos;
            }
        }

        out.push_str(&line_str);
        out.push('\n');
    }

    if !text.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

struct RotatingFile {
    path: PathBuf,
    file: Mutex<File>,
    written: AtomicU64,
    mirror_stderr: bool,
}

impl RotatingFile {
    fn open(path: PathBuf, mirror_stderr: bool) -> io::Result<Self> {
        let file = open_append(&path)?;
        let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        Ok(Self {
            path,
            file: Mutex::new(file),
            written: AtomicU64::new(size),
            mirror_stderr,
        })
    }

    fn rotate(&self) -> io::Result<()> {
        let mut guard = self.file.lock().unwrap_or_else(|error| error.into_inner());
        for generation in (1..LOG_GENERATIONS).rev() {
            let from = self.generation_path(generation);
            let to = self.generation_path(generation + 1);
            if from.exists() {
                let _ = std::fs::rename(from, to);
            }
        }
        if self.path.exists() {
            let _ = std::fs::rename(&self.path, self.generation_path(1));
        }
        *guard = open_append(&self.path)?;
        self.written.store(0, Ordering::Relaxed);
        Ok(())
    }

    fn generation_path(&self, generation: u32) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(format!(".{generation}"));
        PathBuf::from(name)
    }
}

struct LockedFile<'a> {
    guard: MutexGuard<'a, File>,
    written: &'a AtomicU64,
    mirror_stderr: bool,
}

impl Write for LockedFile<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let sanitized = sanitize_log_bytes(buffer);
        let written = self.guard.write(&sanitized)?;
        self.written.fetch_add(written as u64, Ordering::Relaxed);
        if self.mirror_stderr {
            let _ = io::stderr().write_all(&sanitized);
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.guard.flush()?;
        if self.mirror_stderr {
            let _ = io::stderr().flush();
        }
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for RotatingFile {
    type Writer = LockedFile<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        if self.written.load(Ordering::Relaxed) >= MAX_LOG_BYTES {
            let _ = self.rotate();
        }
        LockedFile {
            guard: self.file.lock().unwrap_or_else(|error| error.into_inner()),
            written: &self.written,
            mirror_stderr: self.mirror_stderr,
        }
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

/// Installs the global subscriber and the panic hook.
///
/// Returns the active log path, or `None` when a subscriber was already
/// installed or the log directory is unusable. Logging is best effort: a
/// read-only data directory must never stop the application from starting.
pub fn init(data_dir: &Path, mirror_stderr: bool) -> Option<PathBuf> {
    let log_dir = data_dir.join("logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        eprintln!(
            "[Warning] Log directory is not writable: {}",
            log_dir.display()
        );
        return None;
    }
    let path = log_dir.join(LOG_FILE_NAME);
    let writer = match RotatingFile::open(path.clone(), mirror_stderr) {
        Ok(writer) => writer,
        Err(error) => {
            eprintln!(
                "[Warning] Log file is not writable ({error}): {}",
                path.display()
            );
            return None;
        }
    };

    let filter = EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(true)
        .with_thread_names(false)
        .with_writer(writer)
        .finish();
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        return None;
    }
    install_panic_hook(data_dir);
    Some(path)
}

/// Writes a crash report with timestamp, error location, message and backtrace.
pub fn write_crash_report(
    crash_dir: &Path,
    payload: &str,
    location: &str,
    backtrace: &str,
) -> Option<PathBuf> {
    if std::fs::create_dir_all(crash_dir).is_err() {
        return None;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let filename = format!("crash-{}.txt", now);
    let path = crash_dir.join(filename);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .ok()?;
    writeln!(file, "==================================================").ok()?;
    writeln!(file, "OpenUnpack Fatal Crash Report").ok()?;
    writeln!(file, "Timestamp (Epoch): {}", now).ok()?;
    writeln!(file, "Location: {}", location).ok()?;
    writeln!(file, "Panic Message: {}", sanitize_log_text(payload)).ok()?;
    writeln!(file, "==================================================").ok()?;
    writeln!(file, "Stack Backtrace:").ok()?;
    writeln!(file, "{}", backtrace).ok()?;
    Some(path)
}

/// Records the panic before the default hook prints it and persists a crash report.
fn install_panic_hook(data_dir: &Path) {
    let crash_dir = data_dir.join("crashes");
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        let location = match info.location() {
            Some(location) => format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            ),
            None => "unknown".to_string(),
        };
        let backtrace = format!("{}", std::backtrace::Backtrace::force_capture());
        let sanitized_msg = sanitize_log_text(message);
        tracing::error!(
            location = %location,
            backtrace = %backtrace,
            "panic: {sanitized_msg}"
        );
        let _ = write_crash_report(&crash_dir, message, &location, &backtrace);
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_keeps_a_bounded_number_of_generations() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let directory = std::env::temp_dir().join(format!("openunpack-log-{stamp}"));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(LOG_FILE_NAME);
        let writer = RotatingFile::open(path.clone(), false).unwrap();

        for _ in 0..(LOG_GENERATIONS + 2) {
            writer.written.store(MAX_LOG_BYTES, Ordering::Relaxed);
            writer.make_writer().write_all(b"rotated\n").unwrap();
        }

        let generations = (1..=LOG_GENERATIONS + 1)
            .filter(|generation| writer.generation_path(*generation).exists())
            .count();
        assert!(
            generations <= LOG_GENERATIONS as usize,
            "kept {generations} generations, expected at most {LOG_GENERATIONS}"
        );
        assert!(path.exists(), "the active log must always exist");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn test_sensitive_password_and_token_sanitization() {
        let raw = r#"Executing: 7z.exe x -pMySecretPassword archive.7z "password": "123456" password=TopSecret"#;
        let sanitized = sanitize_log_text(raw);
        assert!(!sanitized.contains("MySecretPassword"));
        assert!(!sanitized.contains("123456"));
        assert!(!sanitized.contains("TopSecret"));
        assert!(sanitized.contains("-p***REDACTED***"));
        assert!(sanitized.contains(r#""***REDACTED***""#));
    }

    #[test]
    fn test_write_crash_report_persistence() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let crash_dir = std::env::temp_dir().join(format!("openunpack-crashes-{stamp}"));
        let report_path = write_crash_report(
            &crash_dir,
            "unexpected null pointer with password=SecretPwd",
            "crates/core/src/worker.rs:42:10",
            "fake stack frame 0\nfake stack frame 1",
        );
        assert!(report_path.is_some());
        let path = report_path.unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("OpenUnpack Fatal Crash Report"));
        assert!(content.contains("crates/core/src/worker.rs:42:10"));
        assert!(!content.contains("SecretPwd"));
        assert!(content.contains("***REDACTED***"));
        let _ = std::fs::remove_dir_all(&crash_dir);
    }
}
