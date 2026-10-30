//! Unified archive extractor trait and registry.
//!
//! Provides a standardized interface across all extraction engines:
//! * Pure Rust native extractors (ZIP, TAR, LZ4)
//! * In-place shrinking extractor (HatchZip core)
//! * External fallback CLI extractor (7-Zip)

use super::*;
use openunpack_protocol::worker::{CandidateItem, FileRecord, WorkerEvent, WorkerRequest};
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

/// Unified extraction outcome.
#[derive(Debug, Clone)]
pub struct ExtractionOutcome {
    pub files: Vec<FileRecord>,
    pub total_bytes: u64,
    pub verification_scope: String,
    pub password_used: bool,
    pub is_salvaged: bool,
}

/// Core interface implemented by all extraction engines.
pub trait ArchiveExtractor: Send + Sync {
    /// Engine identifier (e.g., "zip", "tar", "lz4", "7z-cli", "inplace").
    fn id(&self) -> &'static str;

    /// Whether this extractor can handle the specified archive.
    fn can_handle(&self, path: &Path) -> bool;

    /// Inspect the archive header/entries without extracting payload.
    fn inspect(
        &self,
        archive: &Path,
        max_entries: u64,
        max_total_bytes: u64,
        password: Option<&str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<FileRecord>, String>> + Send + 'static>,
    >;

    /// Probe candidate passwords against the archive.
    fn probe(
        &self,
        archive: &Path,
        candidates: &[CandidateItem],
        cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ProbeOutcome, String>> + Send + 'static>,
    >;

    /// Perform full extraction of the archive.
    fn extract(
        &self,
        request: &WorkerRequest,
        destination: &Path,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
        cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ExtractionOutcome, String>> + Send + 'static>,
    >;
}

/// Extractor for ZIP archives with automatic 7z fallback for unsupported codecs.
pub struct ZipExtractor;

impl ArchiveExtractor for ZipExtractor {
    fn id(&self) -> &'static str {
        "zip"
    }

    fn can_handle(&self, path: &Path) -> bool {
        is_zip_file(path)
    }

    fn inspect(
        &self,
        archive: &Path,
        max_entries: u64,
        max_total_bytes: u64,
        password: Option<&str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<FileRecord>, String>> + Send + 'static>,
    > {
        let p = archive.to_path_buf();
        let pwd = password.map(str::to_string);
        Box::pin(async move {
            let p_zip = p.clone();
            let pwd_zip = pwd.clone();
            let zip_res = tokio::task::spawn_blocking(move || {
                zip_engine::inspect_zip(&p_zip, pwd_zip.as_deref(), max_entries, max_total_bytes)
            })
            .await
            .map_err(|e| format!("SpawnBlockingError: {e}"))?;

            match zip_res {
                Ok(records) => Ok(records),
                Err(e) if permits_zip_cli_fallback(&e) && cli_engine::has_7z_cli() => {
                    cli_engine::inspect_cli(&p, pwd.as_deref())
                        .await
                        .map_err(|_| e)
                }
                Err(e) => Err(e),
            }
        })
    }

    fn probe(
        &self,
        archive: &Path,
        candidates: &[CandidateItem],
        cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ProbeOutcome, String>> + Send + 'static>,
    > {
        let p = archive.to_path_buf();
        let mut cands = Vec::new();
        for c in candidates {
            cands.push((c.secret.clone(), Some(c.candidate_id.clone())));
        }
        Box::pin(async move {
            let zip_res = tokio::task::spawn_blocking({
                let cands = cands.clone();
                let c = cancel.clone();
                let p = p.clone();
                move || zip_engine::probe_zip(&p, &cands, &c)
            })
            .await
            .map_err(|e| format!("SpawnBlockingError: {e}"))?;

            match zip_res {
                Ok(ProbeOutcome::Corrupt | ProbeOutcome::Unsupported)
                    if cli_engine::has_7z_cli() =>
                {
                    cli_engine::probe_cli(&p, &cands, &cancel).await
                }
                Ok(res) => Ok(res),
                Err(e) if permits_zip_cli_fallback(&e) && cli_engine::has_7z_cli() => {
                    cli_engine::probe_cli(&p, &cands, &cancel)
                        .await
                        .map_err(|_| e)
                }
                Err(e) => Err(e),
            }
        })
    }

    fn extract(
        &self,
        request: &WorkerRequest,
        destination: &Path,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
        cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ExtractionOutcome, String>> + Send + 'static>,
    > {
        let req_clone = request.clone();
        let dest_clone = destination.to_path_buf();
        Box::pin(async move {
            let zip_res = tokio::task::spawn_blocking({
                let req = req_clone.clone();
                let dest = dest_clone.clone();
                let prog = progress.clone();
                let c = cancel.clone();
                move || zip_engine::extract_zip(&req, &dest, prog, c)
            })
            .await
            .map_err(|e| format!("SpawnBlockingError: {e}"))?;

            let (files, is_salvaged) = match zip_res {
                Ok(files) => {
                    let salvaged = files.iter().any(|f| f.is_corrupted);
                    (files, salvaged)
                }
                Err(e) if permits_zip_cli_fallback(&e) && cli_engine::has_7z_cli() => {
                    let files = cli_engine::extract_cli(&req_clone, &dest_clone, progress, cancel)
                        .await
                        .map_err(|_| e)?;
                    let salvaged = files.iter().any(|f| f.is_corrupted);
                    (files, salvaged)
                }
                Err(e) => return Err(e),
            };

            let total_bytes: u64 = files.iter().map(|f| f.size).sum();
            Ok(ExtractionOutcome {
                total_bytes,
                verification_scope: if is_salvaged {
                    "salvagedWithCrcWarning".into()
                } else {
                    "archiveCrcAndStreamSha256".into()
                },
                password_used: req_clone.password.is_some(),
                is_salvaged,
                files,
            })
        })
    }
}

/// Extractor for TAR archives.
pub struct TarExtractor;

impl ArchiveExtractor for TarExtractor {
    fn id(&self) -> &'static str {
        "tar"
    }

    fn can_handle(&self, path: &Path) -> bool {
        tar_engine::is_tar_file(path)
    }

    fn inspect(
        &self,
        archive: &Path,
        max_entries: u64,
        max_total_bytes: u64,
        _password: Option<&str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<FileRecord>, String>> + Send + 'static>,
    > {
        let p = archive.to_path_buf();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                tar_engine::inspect_tar(&p, max_entries, max_total_bytes)
            })
            .await
            .map_err(|e| format!("SpawnBlockingError: {e}"))?
        })
    }

    fn probe(
        &self,
        _archive: &Path,
        _candidates: &[CandidateItem],
        _cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ProbeOutcome, String>> + Send + 'static>,
    > {
        Box::pin(async { Ok(ProbeOutcome::NoPasswordNeeded) })
    }

    fn extract(
        &self,
        request: &WorkerRequest,
        destination: &Path,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
        cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ExtractionOutcome, String>> + Send + 'static>,
    > {
        let req_clone = request.clone();
        let dest_clone = destination.to_path_buf();
        Box::pin(async move {
            let (files, is_salvaged) = tokio::task::spawn_blocking(move || {
                tar_engine::extract_tar(&req_clone, &dest_clone, progress, cancel)
            })
            .await
            .map_err(|e| format!("SpawnBlockingError: {e}"))??;

            let total_bytes: u64 = files.iter().map(|f| f.size).sum();
            Ok(ExtractionOutcome {
                total_bytes,
                verification_scope: if is_salvaged {
                    "salvagedWithCrcWarning".into()
                } else {
                    "archiveCrcAndStreamSha256".into()
                },
                password_used: false,
                is_salvaged,
                files,
            })
        })
    }
}

/// Extractor for LZ4 single frames.
pub struct Lz4Extractor;

impl ArchiveExtractor for Lz4Extractor {
    fn id(&self) -> &'static str {
        "lz4"
    }

    fn can_handle(&self, path: &Path) -> bool {
        lz4_engine::is_lz4_file(path)
    }

    fn inspect(
        &self,
        archive: &Path,
        max_entries: u64,
        max_total_bytes: u64,
        _password: Option<&str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<FileRecord>, String>> + Send + 'static>,
    > {
        let p = archive.to_path_buf();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                lz4_engine::inspect_lz4(&p, max_entries, max_total_bytes)
            })
            .await
            .map_err(|e| format!("SpawnBlockingError: {e}"))?
        })
    }

    fn probe(
        &self,
        _archive: &Path,
        _candidates: &[CandidateItem],
        _cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ProbeOutcome, String>> + Send + 'static>,
    > {
        Box::pin(async { Ok(ProbeOutcome::NoPasswordNeeded) })
    }

    fn extract(
        &self,
        request: &WorkerRequest,
        destination: &Path,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
        cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ExtractionOutcome, String>> + Send + 'static>,
    > {
        let req_clone = request.clone();
        let dest_clone = destination.to_path_buf();
        Box::pin(async move {
            let files = tokio::task::spawn_blocking(move || {
                lz4_engine::extract_lz4(&req_clone, &dest_clone, progress, cancel)
            })
            .await
            .map_err(|e| format!("SpawnBlockingError: {e}"))??;
            let total_bytes: u64 = files.iter().map(|f| f.size).sum();
            Ok(ExtractionOutcome {
                total_bytes,
                verification_scope: "frameValidationAndStreamSha256".into(),
                password_used: false,
                is_salvaged: false,
                files,
            })
        })
    }
}

/// Extractor using 7-Zip CLI for general formats (7z, RAR, XZ, etc.).
pub struct CliExtractor;

impl ArchiveExtractor for CliExtractor {
    fn id(&self) -> &'static str {
        "7z-cli"
    }

    fn can_handle(&self, _path: &Path) -> bool {
        cli_engine::has_7z_cli()
    }

    fn inspect(
        &self,
        archive: &Path,
        _max_entries: u64,
        _max_total_bytes: u64,
        password: Option<&str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<FileRecord>, String>> + Send + 'static>,
    > {
        let p = archive.to_path_buf();
        let pwd = password.map(str::to_string);
        Box::pin(async move { cli_engine::inspect_cli(&p, pwd.as_deref()).await })
    }

    fn probe(
        &self,
        archive: &Path,
        candidates: &[CandidateItem],
        cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ProbeOutcome, String>> + Send + 'static>,
    > {
        let p = archive.to_path_buf();
        let mut cands = Vec::new();
        for c in candidates {
            cands.push((c.secret.clone(), Some(c.candidate_id.clone())));
        }
        Box::pin(async move { cli_engine::probe_cli(&p, &cands, &cancel).await })
    }

    fn extract(
        &self,
        request: &WorkerRequest,
        destination: &Path,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
        cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ExtractionOutcome, String>> + Send + 'static>,
    > {
        let req_clone = request.clone();
        let dest_clone = destination.to_path_buf();
        Box::pin(async move {
            let files = cli_engine::extract_cli(&req_clone, &dest_clone, progress, cancel).await?;
            let total_bytes: u64 = files.iter().map(|f| f.size).sum();
            Ok(ExtractionOutcome {
                total_bytes,
                verification_scope: "archiveCrcAndStreamSha256".into(),
                password_used: req_clone.password.is_some(),
                is_salvaged: false,
                files,
            })
        })
    }
}

/// Dispatches an archive to the appropriate extractor based on format.
pub fn select_extractor(path: &Path) -> Box<dyn ArchiveExtractor> {
    if is_zip_file(path) {
        Box::new(ZipExtractor)
    } else if tar_engine::is_tar_file(path) {
        Box::new(TarExtractor)
    } else if lz4_engine::is_lz4_file(path) {
        Box::new(Lz4Extractor)
    } else if cli_engine::has_7z_cli() {
        Box::new(CliExtractor)
    } else {
        // Default to ZIP extractor which returns UnsupportedFormat if unreadable
        Box::new(ZipExtractor)
    }
}

/// In-place shrinking extractor powered by HatchZip core.
pub struct InplaceExtractor;

impl ArchiveExtractor for InplaceExtractor {
    fn id(&self) -> &'static str {
        "inplace"
    }

    fn can_handle(&self, path: &Path) -> bool {
        is_zip_file(path) || tar_engine::is_tar_file(path)
    }

    fn inspect(
        &self,
        archive: &Path,
        max_entries: u64,
        max_total_bytes: u64,
        password: Option<&str>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<FileRecord>, String>> + Send + 'static>,
    > {
        let p = archive.to_path_buf();
        let pwd = password.map(str::to_string);
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                if is_zip_file(&p) {
                    zip_engine::inspect_zip(&p, pwd.as_deref(), max_entries, max_total_bytes)
                } else if tar_engine::is_tar_file(&p) {
                    tar_engine::inspect_tar(&p, max_entries, max_total_bytes)
                } else {
                    Err("UnsupportedFormat".to_string())
                }
            })
            .await
            .map_err(|e| format!("SpawnBlockingError: {e}"))?
        })
    }

    fn probe(
        &self,
        archive: &Path,
        candidates: &[CandidateItem],
        cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ProbeOutcome, String>> + Send + 'static>,
    > {
        let p = archive.to_path_buf();
        let mut cands = Vec::new();
        for c in candidates {
            cands.push((c.secret.clone(), Some(c.candidate_id.clone())));
        }
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                if is_zip_file(&p) {
                    zip_engine::probe_zip(&p, &cands, &cancel)
                } else {
                    Ok(ProbeOutcome::NoPasswordNeeded)
                }
            })
            .await
            .map_err(|e| format!("SpawnBlockingError: {e}"))?
        })
    }

    fn extract(
        &self,
        request: &WorkerRequest,
        destination: &Path,
        progress: Arc<dyn Fn(&WorkerEvent) + Send + Sync>,
        cancel: Arc<AtomicBool>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ExtractionOutcome, String>> + Send + 'static>,
    > {
        let req_clone = request.clone();
        let dest_clone = destination.to_path_buf();
        Box::pin(async move {
            let in_req = inplace::Request {
                archive: std::path::PathBuf::from(&req_clone.archive_path),
                output: dest_clone.clone(),
                passwords: req_clone.password.clone().into_iter().collect(),
                keep_free: req_clone.min_free_bytes,
                replace: false,
                verify_first: true,
                max_entries: req_clone.max_entries,
                max_expanded_bytes: req_clone.max_total_bytes,
            };

            let prog = Arc::new(inplace::Progress::default());
            let stop = cancel;

            let mut handle = tokio::task::spawn_blocking({
                let in_req = in_req.clone();
                let prog = prog.clone();
                let stop = stop.clone();
                let dest_clone = dest_clone.clone();
                move || -> Result<(Vec<FileRecord>, u64), String> {
                    let exec =
                        inplace::execute(&in_req, &prog, &stop).map_err(|e| e.to_string())?;
                    if !exec.is_finished() {
                        return Err("InplacePaused".to_string());
                    }
                    let copy_files = inplace::manifest(&in_req.archive, &dest_clone)
                        .map_err(|e| e.to_string())?;
                    let files: Vec<FileRecord> = copy_files
                        .into_iter()
                        .map(|cf| FileRecord {
                            relative_path: cf.relative_path,
                            size: cf.size,
                            size_known: cf.size_known,
                            sha256: cf.sha256,
                            is_directory: cf.is_directory,
                            is_encrypted: false,
                            is_corrupted: cf.is_corrupted,
                        })
                        .collect();
                    let total_bytes: u64 = files.iter().map(|f| f.size).sum();
                    Ok((files, total_bytes))
                }
            });

            let mut ticker = tokio::time::interval(std::time::Duration::from_millis(100));
            let result = loop {
                tokio::select! {
                    joined = &mut handle => break joined.map_err(|e| format!("SpawnBlockingError: {e}"))??,
                    _ = ticker.tick() => {
                        let mut ev = make_event(&req_clone, "progress");
                        ev.completed_bytes = prog.done();
                        ev.total_bytes = prog.total();
                        ev.total_bytes_known = prog.total() > 0;
                        progress(&ev);
                    }
                }
            };

            Ok(ExtractionOutcome {
                files: result.0,
                total_bytes: result.1,
                verification_scope: "inplaceChecksumAndManifestSha256".into(),
                password_used: req_clone.password.is_some(),
                is_salvaged: false,
            })
        })
    }
}

/// Execution strategy for archive extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionStrategy {
    Standard,
    InPlace,
}

pub fn select_extractor_with_strategy(
    path: &Path,
    strategy: ExtractionStrategy,
) -> Box<dyn ArchiveExtractor> {
    match strategy {
        ExtractionStrategy::InPlace => {
            if is_zip_file(path) || tar_engine::is_tar_file(path) {
                Box::new(InplaceExtractor)
            } else {
                select_extractor(path)
            }
        }
        ExtractionStrategy::Standard => select_extractor(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_extractor_dispatch() {
        let zip_p = Path::new("test.zip");
        let ext = select_extractor(zip_p);
        assert_eq!(ext.id(), "zip");

        let tar_p = Path::new("test.tar");
        let ext = select_extractor(tar_p);
        assert_eq!(ext.id(), "tar");

        let lz4_p = Path::new("test.lz4");
        let ext = select_extractor(lz4_p);
        assert_eq!(ext.id(), "lz4");

        let inplace_ext = select_extractor_with_strategy(zip_p, ExtractionStrategy::InPlace);
        assert_eq!(inplace_ext.id(), "inplace");

        let standard_ext = select_extractor_with_strategy(zip_p, ExtractionStrategy::Standard);
        assert_eq!(standard_ext.id(), "zip");
    }
}
