use openunpack_protocol::worker::{CandidateItem, FileRecord, WorkerEvent, WorkerRequest};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub mod cli_engine;
pub mod extractor;
pub mod inplace;
pub mod lz4_engine;
pub mod oem_codepage;
pub mod path_safety;
mod process_job;
pub mod tar_engine;
pub mod zip_engine;

pub use cli_engine::*;
pub use extractor::*;
pub use lz4_engine::*;
pub use oem_codepage::*;
pub use path_safety::*;
pub use tar_engine::*;
pub use zip_engine::*;

#[derive(Debug, Clone)]
pub enum ProbeOutcome {
    NoPasswordNeeded,
    Success(String, Option<String>),
    WrongPassword,
    MissingVolume,
    Corrupt,
    Unsupported,
    Failed(String),
}

// Only decoding/format failures may try another ZIP implementation. Safety,
// cancellation, budget and filesystem failures must remain terminal, even if
// a locally installed 7-Zip would sanitize the entry name or ignore a limit.
fn permits_zip_cli_fallback(error: &str) -> bool {
    matches!(
        error.split(':').next(),
        Some(
            "CorruptArchive"
                | "ZipReadError"
                | "ZipError"
                | "ZipEntryError"
                | "NeedsPassword"
                | "PasswordMismatch"
                | "PasswordMismatchOrCorrupt"
        )
    )
}

pub(crate) fn make_event(request: &WorkerRequest, kind: &str) -> WorkerEvent {
    let mut ev = WorkerEvent::default();
    ev.protocol_version = request.protocol_version;
    ev.request_id = request.request_id.clone();
    ev.kind = kind.to_string();
    ev.operation_id = request.operation_id.clone();
    ev.generation = request.generation;
    ev
}

/// Unified dispatcher for native archive extraction without .NET runtime dependency.
pub async fn execute_native_worker(
    request: WorkerRequest,
    progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<WorkerEvent, String> {
    if cancel.load(Ordering::Acquire) {
        return Err("Cancelled".into());
    }

    match request.operation.as_str() {
        "inspect" => inspect_archive(&request).await,
        "probe" => probe_archive(&request, cancel).await,
        "extract" => extract_archive(&request, progress, cancel).await,
        "copy" | "readback" => copy_files(&request, progress, cancel).await,
        _ => Err("WorkerProtocolError".into()),
    }
}

async fn inspect_archive(request: &WorkerRequest) -> Result<WorkerEvent, String> {
    let archive_path = Path::new(&request.archive_path);
    if !archive_path.is_file() {
        return Err("ArchiveNotFound".into());
    }

    let extractor = select_extractor(archive_path);
    let files = extractor
        .inspect(
            archive_path,
            request.max_entries,
            request.max_total_bytes,
            request.password.as_deref(),
        )
        .await?;

    let total_bytes: u64 = files.iter().fold(0u64, |acc, f| acc.saturating_add(f.size));
    let size_known = files.iter().all(|f| f.size_known);

    let mut event = make_event(request, "completed");
    event.completed_bytes = total_bytes;
    event.total_bytes = total_bytes;
    event.total_bytes_known = size_known;
    event.verification_scope = "metadataOnly".into();
    event.message = "目录扫描已完成。".into();
    event.files = files;
    Ok(event)
}

async fn probe_archive(
    request: &WorkerRequest,
    cancel: Arc<AtomicBool>,
) -> Result<WorkerEvent, String> {
    let archive_path = Path::new(&request.archive_path);
    if !archive_path.is_file() {
        return Err("ArchiveNotFound".into());
    }

    let mut candidates = Vec::new();
    for p in &request.candidate_passwords {
        candidates.push(CandidateItem {
            candidate_id: String::new(),
            source: "manual".into(),
            revision: 0,
            label: "manual".into(),
            secret: p.clone(),
        });
    }
    for c in &request.candidates {
        candidates.push(c.clone());
    }

    let extractor = select_extractor(archive_path);
    let outcome = extractor.probe(archive_path, &candidates, cancel).await?;

    match outcome {
        ProbeOutcome::NoPasswordNeeded => {
            let mut event = make_event(request, "completed");
            event.password_used = false;
            event.probe_status = Some("NoPasswordNeeded".into());
            event.message = "无需解压密码。".into();
            Ok(event)
        }
        ProbeOutcome::Success(password, candidate_id) => {
            let mut event = make_event(request, "completed");
            event.password_used = true;
            event.matched_password = Some(password);
            event.matched_candidate_id = candidate_id;
            event.probe_status = Some("Success".into());
            event.message = "密码探测成功。".into();
            Ok(event)
        }
        ProbeOutcome::WrongPassword => {
            let mut event = make_event(request, "completed");
            event.password_used = false;
            event.probe_status = Some("WrongPassword".into());
            event.message = "未能找到匹配的解压密码。".into();
            Ok(event)
        }
        ProbeOutcome::MissingVolume => {
            let mut event = make_event(request, "completed");
            event.password_used = false;
            event.probe_status = Some("MissingVolume".into());
            event.message = "分卷缺失。".into();
            Ok(event)
        }
        ProbeOutcome::Corrupt => {
            let mut event = make_event(request, "completed");
            event.password_used = false;
            event.probe_status = Some("Corrupt".into());
            event.message = "压缩文件已损坏。".into();
            Ok(event)
        }
        ProbeOutcome::Unsupported => {
            let mut event = make_event(request, "completed");
            event.password_used = false;
            event.probe_status = Some("Unsupported".into());
            event.message = "不支持的文件格式。".into();
            Ok(event)
        }
        ProbeOutcome::Failed(msg) => {
            let mut event = make_event(request, "completed");
            event.password_used = false;
            event.probe_status = Some("Failed".into());
            event.message = msg;
            Ok(event)
        }
    }
}

async fn extract_archive(
    request: &WorkerRequest,
    progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<WorkerEvent, String> {
    let archive_path = Path::new(&request.archive_path);
    if !archive_path.is_file() {
        return Err("ArchiveNotFound".into());
    }

    let destination = Path::new(&request.destination_directory);
    fs::create_dir_all(destination).map_err(|e| format!("DestinationCreationFailed: {e}"))?;

    let extractor = select_extractor(archive_path);
    let outcome = extractor
        .extract(request, destination, progress, cancel)
        .await?;

    let mut event = make_event(request, "completed");
    event.completed_bytes = outcome.total_bytes;
    event.total_bytes = outcome.total_bytes;
    event.total_bytes_known = true;
    event.message = "解压缩完成。".into();
    event.files = outcome.files;
    event.password_used = outcome.password_used;
    event.verification_scope = outcome.verification_scope;
    Ok(event)
}

async fn copy_files(
    request: &WorkerRequest,
    progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<WorkerEvent, String> {
    let req = request.clone();
    tokio::task::spawn_blocking(move || copy_files_blocking(&req, progress, cancel))
        .await
        .map_err(|e| format!("SpawnBlockingError: {e}"))?
}

fn copy_files_blocking(
    request: &WorkerRequest,
    progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<WorkerEvent, String> {
    let destination = Path::new(&request.destination_directory);
    fs::create_dir_all(destination).map_err(|e| format!("DestinationCreationFailed: {e}"))?;

    let mut copied_files = Vec::new();
    let total_bytes: u64 = request
        .files
        .iter()
        .fold(0u64, |acc, f| acc.saturating_add(f.size));
    let mut completed_bytes: u64 = 0;
    let mut last_progress = std::time::Instant::now();

    // 1 MB aligned buffer allocated once outside the loop to eliminate per-file heap thrashing
    const COPY_BUFFER_CAPACITY: usize = 1024 * 1024;
    let mut buffer = vec![0u8; COPY_BUFFER_CAPACITY];

    for file in &request.files {
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }

        let safe_rel = path_safety::validate_relative_path(&file.relative_path)?;
        let target_path = destination.join(safe_rel);

        if file.is_directory {
            fs::create_dir_all(&target_path).map_err(|e| format!("IoError: {e}"))?;
            copied_files.push(FileRecord {
                relative_path: file.relative_path.clone(),
                size: 0,
                size_known: true,
                sha256: String::new(),
                is_directory: true,
                is_encrypted: false,
                is_corrupted: false,
            });
            continue;
        }

        let is_readback = request.operation == "readback";
        if !is_readback && let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("IoError: {e}"))?;
        }

        let src = Path::new(&file.source_path);
        if !src.is_file() {
            return Err("SourceFileNotFound".into());
        }

        let mut reader = File::open(src).map_err(|e| format!("IoError: {e}"))?;
        let mut writer = if !is_readback {
            Some(std::io::BufWriter::with_capacity(
                COPY_BUFFER_CAPACITY,
                File::create(&target_path).map_err(|e| format!("IoError: {e}"))?,
            ))
        } else {
            None
        };
        let mut hasher = Sha256::new();

        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("Cancelled".into());
            }
            let read = reader
                .read(&mut buffer)
                .map_err(|e| format!("IoError: {e}"))?;
            if read == 0 {
                break;
            }
            if let Some(ref mut w) = writer {
                w.write_all(&buffer[..read])
                    .map_err(|e| format!("IoError: {e}"))?;
            }
            hasher.update(&buffer[..read]);
            completed_bytes = completed_bytes.saturating_add(read as u64);

            if last_progress.elapsed() >= std::time::Duration::from_millis(50) {
                let mut prog_ev = make_event(request, "progress");
                prog_ev.completed_bytes = completed_bytes;
                prog_ev.total_bytes = total_bytes;
                prog_ev.total_bytes_known = true;
                progress(&prog_ev);
                last_progress = std::time::Instant::now();
            }
        }

        if let Some(mut w) = writer {
            w.flush().map_err(|e| format!("IoError: {e}"))?;
        }

        let hash_hex = hex::encode(hasher.finalize());
        if request.operation == "readback" && !file.sha256.is_empty() && file.sha256 != hash_hex {
            let mut err_event = make_event(request, "error");
            err_event.error_code = "Corrupt".into();
            err_event.message = "文件回读校验哈希不匹配".into();
            return Ok(err_event);
        }
        copied_files.push(FileRecord {
            relative_path: file.relative_path.clone(),
            size: file.size,
            size_known: true,
            sha256: hash_hex,
            is_directory: false,
            is_encrypted: false,
            is_corrupted: false,
        });
    }

    let mut event = make_event(request, "completed");
    event.completed_bytes = total_bytes;
    event.total_bytes = total_bytes;
    event.total_bytes_known = true;
    event.message = "文件复制完成。".into();
    event.files = copied_files;
    event.verification_scope = if request.operation == "readback" {
        "targetReadback".into()
    } else {
        "sourceStreamSha256".into()
    };
    Ok(event)
}

pub fn is_zip_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "zip"
        || ext == "cbz"
        || ext == "epub"
        || ext == "docx"
        || ext == "xlsx"
        || ext == "pptx"
        || ext == "jar"
    {
        return true;
    }
    // Check magic bytes PK\x03\x04
    if let Ok(mut file) = File::open(path) {
        let mut magic = [0u8; 4];
        if file.read_exact(&mut magic).is_ok() && &magic == b"PK\x03\x04" {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod fallback_tests {
    use super::permits_zip_cli_fallback;

    #[test]
    fn zip_cli_fallback_preserves_safety_cancellation_and_write_failures() {
        for error in [
            "UnsafePath: Path traversal detected",
            "ResourceLimit: Exceeds maxEntries",
            "ResourceLimit: Exceeds maxWrittenBytes",
            "Cancelled",
            "IoError: access denied",
            "SinkError: access denied",
            "SinkDidNotProvideWriter",
            "MkdirError: access denied",
            "WriteError: disk full",
            "FutureUnknownFailure",
        ] {
            assert!(!permits_zip_cli_fallback(error), "{error}");
        }
        assert!(permits_zip_cli_fallback("ZipEntryError: unsupported codec"));
        assert!(permits_zip_cli_fallback("NeedsPassword"));
    }
}
