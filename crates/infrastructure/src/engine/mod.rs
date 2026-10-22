use openunpack_protocol::worker::{FileRecord, WorkerEvent, WorkerRequest};
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
pub mod path_safety;
pub mod zip_engine;

pub use cli_engine::*;
pub use path_safety::*;
pub use zip_engine::*;

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

    let is_zip = is_zip_file(archive_path);

    let files = if is_zip {
        match zip_engine::inspect_zip(
            archive_path,
            request.password.as_deref(),
            request.max_entries,
            request.max_total_bytes,
        ) {
            Ok(records) => records,
            Err(e) if cli_engine::has_7z_cli() => {
                cli_engine::inspect_cli(archive_path, request.password.as_deref())
                    .await
                    .map_err(|_| e)?
            }
            Err(e) => return Err(e),
        }
    } else if cli_engine::has_7z_cli() {
        cli_engine::inspect_cli(archive_path, request.password.as_deref()).await?
    } else {
        zip_engine::inspect_zip(
            archive_path,
            request.password.as_deref(),
            request.max_entries,
            request.max_total_bytes,
        )
        .map_err(|_| "UnsupportedFormat".to_string())?
    };

    let total_bytes: u64 = files.iter().map(|f| f.size).sum();

    let mut event = make_event(request, "completed");
    event.completed_bytes = total_bytes;
    event.total_bytes = total_bytes;
    event.total_bytes_known = true;
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
        candidates.push((p.clone(), None));
    }
    for c in &request.candidates {
        candidates.push((c.secret.clone(), Some(c.candidate_id.clone())));
    }

    let is_zip = is_zip_file(archive_path);
    let matched = if is_zip {
        match zip_engine::probe_zip(archive_path, &candidates, &cancel) {
            Ok(res) => res,
            Err(e) if cli_engine::has_7z_cli() => {
                cli_engine::probe_cli(archive_path, &candidates, &cancel)
                    .await
                    .map_err(|_| e)?
            }
            Err(e) => return Err(e),
        }
    } else if cli_engine::has_7z_cli() {
        cli_engine::probe_cli(archive_path, &candidates, &cancel).await?
    } else {
        zip_engine::probe_zip(archive_path, &candidates, &cancel)
            .map_err(|_| "UnsupportedFormat".to_string())?
    };

    if let Some((password, candidate_id)) = matched {
        let mut event = make_event(request, "completed");
        event.password_used = true;
        event.matched_password = Some(password);
        event.matched_candidate_id = candidate_id;
        event.probe_status = Some("matched".into());
        event.message = "密码探测成功。".into();
        Ok(event)
    } else {
        let mut event = make_event(request, "completed");
        event.password_used = false;
        event.matched_password = None;
        event.matched_candidate_id = None;
        event.probe_status = Some("exhausted".into());
        event.message = "未能找到匹配的解压密码。".into();
        Ok(event)
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

    let is_zip = is_zip_file(archive_path);
    let extracted_files = if is_zip {
        match zip_engine::extract_zip(request, destination, progress.clone(), cancel.clone()) {
            Ok(files) => files,
            Err(e) if cli_engine::has_7z_cli() => {
                cli_engine::extract_cli(request, destination, progress.clone(), cancel)
                    .await
                    .map_err(|_| e)?
            }
            Err(e) => return Err(e),
        }
    } else if cli_engine::has_7z_cli() {
        cli_engine::extract_cli(request, destination, progress.clone(), cancel).await?
    } else {
        zip_engine::extract_zip(request, destination, progress.clone(), cancel)
            .map_err(|_| "UnsupportedFormat".to_string())?
    };

    let total_bytes: u64 = extracted_files.iter().map(|f| f.size).sum();

    let mut event = make_event(request, "completed");
    event.completed_bytes = total_bytes;
    event.total_bytes = total_bytes;
    event.total_bytes_known = true;
    event.message = "解压缩完成。".into();
    event.files = extracted_files;
    event.password_used = request.password.is_some();
    Ok(event)
}

async fn copy_files(
    request: &WorkerRequest,
    progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
    cancel: Arc<AtomicBool>,
) -> Result<WorkerEvent, String> {
    let destination = Path::new(&request.destination_directory);
    fs::create_dir_all(destination).map_err(|e| format!("DestinationCreationFailed: {e}"))?;

    let mut copied_files = Vec::new();
    let total_bytes: u64 = request.files.iter().map(|f| f.size).sum();
    let mut completed_bytes: u64 = 0;

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

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("IoError: {e}"))?;
        }

        let src = Path::new(&file.source_path);
        if !src.is_file() {
            return Err("SourceFileNotFound".into());
        }

        let mut reader = File::open(src).map_err(|e| format!("IoError: {e}"))?;
        let mut writer = File::create(&target_path).map_err(|e| format!("IoError: {e}"))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];

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
            writer
                .write_all(&buffer[..read])
                .map_err(|e| format!("IoError: {e}"))?;
            hasher.update(&buffer[..read]);
            completed_bytes += read as u64;

            let mut prog_ev = make_event(request, "progress");
            prog_ev.completed_bytes = completed_bytes;
            prog_ev.total_bytes = total_bytes;
            prog_ev.total_bytes_known = true;
            progress(&prog_ev);
        }

        let hash_hex = hex::encode(hasher.finalize());
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
