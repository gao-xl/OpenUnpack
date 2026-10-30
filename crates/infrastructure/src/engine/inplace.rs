//! In-place extraction engine (HatchZip core).
//!
//! Unlike the 7-Zip worker, this engine extracts an archive **and shrinks it
//! as it goes**: every committed batch is flushed to disk, then the archive
//! is truncated behind it. The volume therefore only has to hold the output,
//! which is the difference between needing 380 GB and 200 GB for a 180 GB
//! download.
//!
//! Two rules make that safe to expose next to the standard engine:
//!
//! * the whole archive is checksum-verified before the first byte is
//!   truncated ([`Request::verify_first`]), so a damaged download never
//!   loses data it could not have extracted;
//! * the space plan is computed before anything is touched, and a run that
//!   does not fit refuses instead of filling the disk halfway through.
//!
//! The engine keeps two side files next to the archive: HatchZip's own
//! `.hatchzip` journal plus `.hatchzip-tail` (the saved index, without which
//! a half-extracted archive cannot be resumed) and this crate's
//! `.openunpack.json` **entry manifest**, which is what lets a resumed run
//! still publish a complete file list. None of them may be deleted while an
//! extraction is unfinished.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use hatchzip_core::archive::storage::Reads;
use hatchzip_core::archive::{Damaged, Kind, PasswordNeeded, WrongPassword};
use hatchzip_core::engine::{self, Job, Outcome, Progress as HatchProgress, Run};
use hatchzip_core::journal::Paths;
use hatchzip_core::session::{self, State};
use openunpack_protocol::worker::CopyFile;
use sha2::{Digest, Sha256};

/// Identifier used by [`crate::engine`] callers and the coordinator.
pub const ENGINE_ID: &str = "inPlace";

/// Sidecar holding the archive's entry list, so a resumed run can publish a
/// complete manifest. Sits next to HatchZip's own `.hatchzip` journal.
const STATE_SUFFIX: &str = ".openunpack.json";
const STATE_VERSION: u32 = 1;

/// Durable transaction journal sidecar for crash recovery, resumption, and residue reconciliation.
pub const JOURNAL_SUFFIX: &str = ".openunpack-journal";
pub const JOURNAL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum JournalPhase {
    InProgress,
    Interrupted,
    Committed,
    Cancelled,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenUnpackJournal {
    pub version: u32,
    pub archive: String,
    pub output: String,
    pub phase: JournalPhase,
    pub verified: bool,
    pub total_bytes: u64,
    pub committed_bytes: u64,
    pub updated_at: u64,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub archive: PathBuf,
    pub output: PathBuf,
    /// Tried in order; the first one that opens the archive wins.
    pub passwords: Vec<String>,
    pub keep_free: u64,
    /// Replace existing files, moving them into `.hatchzip-replaced` until
    /// the extraction completes.
    pub replace: bool,
    /// Verify every entry's checksum before touching the archive.
    pub verify_first: bool,
    pub max_entries: u64,
    pub max_expanded_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct InplaceError {
    pub code: String,
    pub detail: String,
}

impl InplaceError {
    pub fn new(code: &str, detail: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            detail: detail.into(),
        }
    }

    fn from_anyhow(error: anyhow::Error) -> Self {
        let detail = format!("{error:#}");
        let code = if error.is::<WrongPassword>() || error.is::<PasswordNeeded>() {
            "NeedsPassword"
        } else if error.is::<Damaged>()
            || detail.to_lowercase().contains("damaged")
            || detail.to_lowercase().contains("corrupt")
            || detail.to_lowercase().contains("checksum")
            || detail.to_lowercase().contains("crc")
        {
            "Corrupt"
        } else if detail.contains("space") || detail.contains("Space") || detail.contains("no room")
        {
            "NoSpace"
        } else {
            "InplaceFailed"
        };
        Self::new(code, detail)
    }
}

impl std::fmt::Display for InplaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightState {
    /// Nothing extracted yet; the archive is whole.
    Fresh,
    /// A previous run stopped; continuing resumes where it stopped.
    Resume,
    /// The archive is gone but the run died before cleaning up: it finished.
    Orphan,
}

#[derive(Debug, Clone)]
pub struct Preflight {
    pub state: PreflightState,
    pub archive: PathBuf,
    pub output: PathBuf,
    pub default_output: PathBuf,
    pub entries: u64,
    pub bytes: u64,
    pub free: u64,
    /// Free space needed at the busiest moment.
    pub needed: u64,
    pub free_after: u64,
    pub fits: bool,
    /// The output is on the archive's filesystem, so truncating the archive
    /// actually gives the space back.
    pub reclaims: bool,
    pub encrypted: bool,
    /// Files already in the output folder at paths the archive writes to.
    pub existing: Vec<PathBuf>,
    /// Links can't be represented by the publication manifest yet.
    pub has_links: bool,
    /// Why this archive cannot be extracted in place at all, if it can't.
    pub refusal: Option<String>,
}

impl Preflight {
    pub fn fits_with_reserve(&self) -> bool {
        self.fits
    }

    /// The reason a caller must refuse this archive, if any.
    pub fn blocker(&self, max_entries: u64, max_expanded_bytes: u64) -> Option<String> {
        if let Some(reason) = &self.refusal {
            return Some(format!("该压缩包无法原地解压：{reason}。"));
        }
        if self.has_links {
            return Some(
                "原地模式暂不处理符号链接或硬链接；请改用标准解压，链接语义将无法写入发布清单。"
                    .into(),
            );
        }
        if !self.reclaims {
            return Some(
                "输出目录与压缩包不在同一磁盘：原地解压不会释放空间，反而会破坏源包。请把输出目录改到源包所在磁盘，或改用标准解压。"
                    .into(),
            );
        }
        if self.entries > max_entries {
            return Some("压缩包条目数超过当前上限，请调高条目上限后重试。".into());
        }
        if self.bytes > max_expanded_bytes {
            return Some("压缩包解压后体积超过当前上限，请调高展开上限后重试。".into());
        }
        if !self.fits {
            return Some(format!(
                "磁盘空间不足：原地解压最多需要 {}，当前可用 {}，还差 {}。",
                human(self.needed),
                human(self.free),
                human(self.needed.saturating_sub(self.free))
            ));
        }
        None
    }
}

/// Phases a caller can show while the blocking run is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Verifying,
    Extracting,
}

#[derive(Debug, Default)]
pub struct Progress {
    phase: AtomicU8,
    done: AtomicU64,
    total: AtomicU64,
    committed: AtomicU64,
}

impl Progress {
    pub fn phase(&self) -> Phase {
        match self.phase.load(Ordering::Relaxed) {
            1 => Phase::Verifying,
            2 => Phase::Extracting,
            _ => Phase::Idle,
        }
    }

    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Bytes of the archive already released.
    pub fn committed(&self) -> u64 {
        self.committed.load(Ordering::Relaxed)
    }

    /// 0..=1 for the current phase.
    pub fn fraction(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            0.
        } else {
            (self.done() as f64 / total as f64).clamp(0., 1.)
        }
    }

    fn begin(&self, phase: Phase, total: u64) {
        self.phase.store(
            match phase {
                Phase::Idle => 0,
                Phase::Verifying => 1,
                Phase::Extracting => 2,
            },
            Ordering::Relaxed,
        );
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Executed {
    /// The archive was consumed and every entry is in place.
    Finished { used_password: bool },
    /// Stopped by the caller; the journal holds the resume point.
    Paused,
    /// The previous run had already finished; only the side files were left.
    AlreadyFinished,
}

impl Executed {
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Finished { .. } | Self::AlreadyFinished)
    }
}

/// `game.zip` → `<parent>/game`, honouring split-volume and `.tar.gz` names.
pub fn default_output(archive: &Path) -> Result<PathBuf, InplaceError> {
    let (primary, _) = session::locate(archive).map_err(InplaceError::from_anyhow)?;
    Ok(session::default_output(&primary))
}

/// Sidecar holding this engine's entry manifest for `archive`.
pub fn state_path(archive: &Path) -> PathBuf {
    let primary = session::locate(archive)
        .map(|(primary, _)| primary)
        .unwrap_or_else(|_| archive.to_path_buf());
    let mut name = primary.into_os_string();
    name.push(STATE_SUFFIX);
    PathBuf::from(name)
}

/// Durable transaction journal sidecar path for `archive`.
pub fn journal_path(archive: &Path) -> PathBuf {
    let primary = session::locate(archive)
        .map(|(primary, _)| primary)
        .unwrap_or_else(|_| archive.to_path_buf());
    let mut name = primary.into_os_string();
    name.push(JOURNAL_SUFFIX);
    PathBuf::from(name)
}

/// Reads the entry manifest a previous run left, if any.
pub fn read_state(archive: &Path) -> Option<StateSidecar> {
    let bytes = fs::read(state_path(archive)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Reads the transaction journal a previous run left, if any.
pub fn read_journal(archive: &Path) -> Option<OpenUnpackJournal> {
    let bytes = fs::read(journal_path(archive)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Writes or atomically updates the transaction journal for `archive`.
pub fn write_journal(archive: &Path, journal: &OpenUnpackJournal) -> Result<(), InplaceError> {
    crate::paths::atomic_json(&journal_path(archive), journal)
        .map_err(|detail| InplaceError::new("IoError", detail))
}

/// Removes the transaction journal. Only call this when extraction is committed or discarded.
pub fn forget_journal(archive: &Path) -> Result<(), InplaceError> {
    match fs::remove_file(journal_path(archive)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(InplaceError::new(
            "IoError",
            format!("无法删除原地解压事务日志：{e}"),
        )),
    }
}

/// Reconciles the in-place journal after an interruption or crash.
/// Returns whether the session is valid for resumption.
pub fn reconcile_journal(archive: &Path) -> Result<bool, InplaceError> {
    if let Some(mut journal) = read_journal(archive) {
        if journal.phase == JournalPhase::InProgress {
            journal.phase = JournalPhase::Interrupted;
            journal.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            write_journal(archive, &journal)?;
        }
        return Ok(journal.phase == JournalPhase::Interrupted);
    }
    if read_state(archive).is_some() {
        return Ok(true);
    }
    Ok(false)
}

/// Marks session as cancelled and protects partially extracted residues.
pub fn mark_journal_cancelled(archive: &Path) -> Result<(), InplaceError> {
    if let Some(mut journal) = read_journal(archive) {
        journal.phase = JournalPhase::Cancelled;
        journal.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        write_journal(archive, &journal)?;
    }
    Ok(())
}

/// Removes both the entry manifest and transaction journal. Only call when no extraction is pending.
pub fn forget_state(archive: &Path) -> Result<(), InplaceError> {
    let _ = forget_journal(archive);
    match fs::remove_file(state_path(archive)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(InplaceError::new(
            "IoError",
            format!("无法删除原地解压记录：{e}"),
        )),
    }
}

/// Check if an in-place session exists (journal, entry manifest, or HatchZip journal).
pub fn has_session(archive: &Path) -> bool {
    read_journal(archive).is_some()
        || read_state(archive).is_some()
        || session::locate(archive)
            .map(|(_, p)| p.journal.exists())
            .unwrap_or(false)
}

/// Finishes an orphaned in-place run by cleaning up HatchZip's journal files.
pub fn finish_orphaned(archive: &Path) -> Result<(), InplaceError> {
    let (primary, paths) = session::locate(archive).map_err(InplaceError::from_anyhow)?;
    let reads = Arc::new(Reads::default());
    let (state, _) = open_with_passwords(
        &primary,
        &paths,
        &Request {
            archive: archive.to_path_buf(),
            output: PathBuf::new(),
            passwords: Vec::new(),
            keep_free: 0,
            replace: false,
            verify_first: false,
            max_entries: u64::MAX,
            max_expanded_bytes: u64::MAX,
        },
        false,
        &[],
        &reads,
    )?;
    if let State::Orphan(journal) = state {
        engine::finish_orphaned(&journal, &paths).map_err(InplaceError::from_anyhow)?;
    }
    Ok(())
}

/// Cleans up empty / 0-byte archive residue left after complete in-place extraction.
pub fn cleanup_archive(archive: &Path) {
    if let Ok((primary, paths)) = session::locate(archive) {
        if let Ok(meta) = fs::metadata(&paths.archive)
            && meta.len() == 0
        {
            let _ = fs::remove_file(&paths.archive);
        }
        if let Ok(meta) = fs::metadata(&primary)
            && meta.len() == 0
        {
            let _ = fs::remove_file(&primary);
        }
    }
    if let Ok(meta) = fs::metadata(archive)
        && meta.len() == 0
    {
        let _ = fs::remove_file(archive);
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StateSidecar {
    pub version: u32,
    pub archive: String,
    pub output: String,
    pub verified: bool,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManifestEntry {
    /// Relative path with `/` separators.
    pub path: String,
    pub size: u64,
    pub dir: bool,
}

/// Opens the archive read-only and reports what an in-place run would cost.
pub fn preflight(request: &Request) -> Result<Preflight, InplaceError> {
    let (primary, paths) = session::locate(&request.archive).map_err(InplaceError::from_anyhow)?;
    let default_output = session::default_output(&primary);
    let reads = Arc::new(Reads::default());
    let (state, _used) =
        open_with_passwords(&primary, &paths, request, false, &request.passwords, &reads)?;
    match state {
        State::Orphan(journal) => Ok(Preflight {
            state: PreflightState::Orphan,
            archive: primary,
            output: journal.output.clone(),
            default_output,
            entries: 0,
            bytes: 0,
            free: 0,
            needed: 0,
            free_after: 0,
            fits: true,
            reclaims: true,
            encrypted: false,
            existing: Vec::new(),
            has_links: false,
            refusal: None,
        }),
        State::Fresh(job, out) => summarize(job, out, default_output, primary, false, request),
        State::Resume(job, journal) => summarize(
            job,
            journal.output.clone(),
            default_output,
            primary,
            true,
            request,
        ),
    }
}

fn summarize(
    job: Job,
    out: PathBuf,
    default_output: PathBuf,
    archive: PathBuf,
    resuming: bool,
    request: &Request,
) -> Result<Preflight, InplaceError> {
    let plan = session::plan(&job, &out, resuming, request.keep_free, request.replace)
        .map_err(InplaceError::from_anyhow)?;
    let existing = if resuming {
        Vec::new()
    } else {
        job.units
            .iter()
            .flat_map(|unit| &unit.items)
            .map(|item| out.join(&item.path))
            .filter(|p| p.exists())
            .collect()
    };
    let has_links = job
        .units
        .iter()
        .flat_map(|unit| &unit.items)
        .any(|item| matches!(item.kind, Kind::Symlink | Kind::HardLink(_)));
    Ok(Preflight {
        state: if resuming {
            PreflightState::Resume
        } else {
            PreflightState::Fresh
        },
        archive,
        output: out,
        default_output,
        entries: job.item_count() as u64,
        bytes: job.remaining_size(),
        free: plan.free,
        needed: plan.needed,
        free_after: plan.free_after,
        fits: plan.fits,
        reclaims: plan.reclaims,
        encrypted: job.encrypted(),
        existing,
        has_links,
        refusal: job.in_place_refusal(),
    })
}

/// Runs the extraction. Blocking: call it from `spawn_blocking`.
///
/// `stop` is checked between batches; setting it pauses the run and leaves
/// the archive in a resumable state.
pub fn execute(
    request: &Request,
    progress: &Arc<Progress>,
    stop: &Arc<AtomicBool>,
) -> Result<Executed, InplaceError> {
    let (primary, paths) = session::locate(&request.archive).map_err(InplaceError::from_anyhow)?;
    let reads = Arc::new(Reads::default());

    // Read-only pass first: verification must never run after a truncation.
    let (state, used) =
        open_with_passwords(&primary, &paths, request, false, &request.passwords, &reads)?;
    match state {
        State::Orphan(journal) => {
            engine::finish_orphaned(&journal, &paths).map_err(InplaceError::from_anyhow)?;
            Ok(Executed::AlreadyFinished)
        }
        State::Resume(job, _journal) => {
            // The interrupted run already truncated part of the archive, so a
            // checksum pass now would only prove the remainder is readable.
            drop(job);
            let (job, journal) = reopen_writable(&primary, &paths, request, &used, &reads)?;
            session::clear_uncommitted(&job, &journal.output).map_err(InplaceError::from_anyhow)?;
            progress.begin(Phase::Extracting, journal.total_size);
            progress.done.store(
                journal.total_size.saturating_sub(job.remaining_size()),
                Ordering::Relaxed,
            );
            progress
                .committed
                .store(journal.committed_len, Ordering::Relaxed);
            run(job, journal, paths, request, progress, stop)
        }
        State::Fresh(job, out) => {
            if request.verify_first {
                // Verification reads the archive's bytes, not the entries', so
                // the bar counts what is actually read.
                let compressed = job.storage.parts().iter().map(|part| part.len).sum::<u64>();
                progress.begin(Phase::Verifying, compressed);
                let counter = progress.clone();
                job.verify(&move |bytes| {
                    counter.done.fetch_add(bytes, Ordering::Relaxed);
                })
                .map_err(InplaceError::from_anyhow)?;
            }
            drop(job);
            let (job, out) = reopen_fresh(&primary, &paths, request, &used, &reads, out)?;
            let journal = session::begin(&job, &paths, out, request.replace)
                .map_err(InplaceError::from_anyhow)?;
            write_state(&job, &journal, &primary, request.verify_first)?;
            progress.begin(Phase::Extracting, journal.total_size);
            progress
                .committed
                .store(journal.committed_len, Ordering::Relaxed);
            run(job, journal, paths, request, progress, stop)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    job: Job,
    journal: hatchzip_core::journal::Journal,
    paths: Paths,
    request: &Request,
    progress: &Arc<Progress>,
    stop: &Arc<AtomicBool>,
) -> Result<Executed, InplaceError> {
    let bytes = progress.clone();
    let committed = progress.clone();
    let run = Run {
        job,
        journal,
        paths,
        stop: stop.as_ref(),
        progress: HatchProgress {
            on_bytes: Box::new(move |n| {
                bytes.done.fetch_add(n, Ordering::Relaxed);
            }),
            on_commit: Box::new(move |len| {
                committed.committed.store(len, Ordering::Relaxed);
            }),
        },
        keep_free: request.keep_free,
    };
    let outcome = run.execute().map_err(InplaceError::from_anyhow)?;
    if let Some(mut j) = read_journal(&request.archive) {
        j.committed_bytes = progress.committed();
        j.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match outcome {
            Outcome::Finished => j.phase = JournalPhase::Committed,
            Outcome::Paused => j.phase = JournalPhase::Interrupted,
        }
        if let Err(e) = write_journal(&request.archive, &j) {
            eprintln!(
                "inplace_journal_update_failed archive={} reason={e}",
                request.archive.display()
            );
        }
    }
    match outcome {
        Outcome::Finished => Ok(Executed::Finished {
            used_password: !request.passwords.is_empty(),
        }),
        Outcome::Paused => Ok(Executed::Paused),
    }
}

fn reopen_fresh(
    primary: &Path,
    paths: &Paths,
    request: &Request,
    password: &Option<String>,
    reads: &Arc<Reads>,
    expected_output: PathBuf,
) -> Result<(Job, PathBuf), InplaceError> {
    let writable = Request {
        archive: request.archive.clone(),
        output: expected_output.clone(),
        passwords: password.iter().cloned().collect(),
        keep_free: request.keep_free,
        replace: request.replace,
        verify_first: request.verify_first,
        max_entries: request.max_entries,
        max_expanded_bytes: request.max_expanded_bytes,
    };
    let (state, _) =
        open_with_passwords(primary, paths, &writable, true, &writable.passwords, reads)?;
    match state {
        State::Fresh(job, out) => Ok((job, out)),
        State::Resume(job, journal) => Ok((job, journal.output)),
        State::Orphan(_) => Err(InplaceError::new(
            "InplaceFailed",
            "压缩包在预检后被移动或删除，无法继续原地解压。",
        )),
    }
}

fn reopen_writable(
    primary: &Path,
    paths: &Paths,
    request: &Request,
    password: &Option<String>,
    reads: &Arc<Reads>,
) -> Result<(Job, hatchzip_core::journal::Journal), InplaceError> {
    let passwords: Vec<String> = password.iter().cloned().collect();
    let (state, _) = open_with_passwords(primary, paths, request, true, &passwords, reads)?;
    match state {
        State::Resume(job, journal) => Ok((job, journal)),
        State::Fresh(..) => Err(InplaceError::new(
            "InplaceFailed",
            "原地解压进度在继续前丢失，源包可能已被移动。",
        )),
        State::Orphan(_) => Err(InplaceError::new(
            "InplaceFailed",
            "压缩包已不存在，无法继续原地解压。",
        )),
    }
}

/// Tries each password in order. A missing one surfaces as `NeedsPassword`
/// only after every candidate failed, so a vault entry further down still
/// gets its turn.
fn open_with_passwords(
    primary: &Path,
    paths: &Paths,
    request: &Request,
    writable: bool,
    passwords: &[String],
    reads: &Arc<Reads>,
) -> Result<(State, Option<String>), InplaceError> {
    if passwords.is_empty() {
        let state = session::open(
            primary,
            paths,
            Some(request.output.clone()),
            writable,
            None,
            reads,
        )
        .map_err(InplaceError::from_anyhow)?;
        return Ok((state, None));
    }
    let mut last: Option<InplaceError> = None;
    for password in passwords {
        match session::open(
            primary,
            paths,
            Some(request.output.clone()),
            writable,
            Some(password.as_str()),
            reads,
        ) {
            Ok(state) => return Ok((state, Some(password.clone()))),
            Err(error) if error.is::<WrongPassword>() || error.is::<PasswordNeeded>() => {
                last = Some(InplaceError::from_anyhow(error));
            }
            Err(error) => return Err(InplaceError::from_anyhow(error)),
        }
    }
    Err(last.unwrap_or_else(|| InplaceError::new("NeedsPassword", "需要密码才能打开该压缩包。")))
}

fn write_state(
    job: &Job,
    journal: &hatchzip_core::journal::Journal,
    archive: &Path,
    verified: bool,
) -> Result<(), InplaceError> {
    let mut entries = Vec::new();
    for item in job.units.iter().flat_map(|unit| &unit.items) {
        entries.push(ManifestEntry {
            path: item.path.to_string_lossy().replace('\\', "/"),
            size: item.size,
            dir: item.kind == Kind::Dir,
        });
    }
    let state = StateSidecar {
        version: STATE_VERSION,
        archive: archive.to_string_lossy().into_owned(),
        output: journal.output.to_string_lossy().into_owned(),
        verified,
        entries: entries.clone(),
    };
    crate::paths::atomic_json(&state_path(archive), &state)
        .map_err(|detail| InplaceError::new("IoError", detail))?;

    let ou_journal = OpenUnpackJournal {
        version: JOURNAL_VERSION,
        archive: archive.to_string_lossy().into_owned(),
        output: journal.output.to_string_lossy().into_owned(),
        phase: JournalPhase::InProgress,
        verified,
        total_bytes: journal.total_size,
        committed_bytes: journal.committed_len,
        updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        entries,
    };
    write_journal(archive, &ou_journal)?;
    Ok(())
}

/// Builds the publication manifest for the finished output folder.
///
/// Paths come from the run's own entry list, so files the person already had
/// in the output folder are never claimed as extracted.
pub fn manifest(archive: &Path, output: &Path) -> Result<Vec<CopyFile>, InplaceError> {
    let entries = match read_state(archive) {
        Some(state) if state.version == STATE_VERSION => state.entries,
        // No sidecar: the run finished before it could be written, or it was
        // removed. Fall back to walking the output folder rather than
        // publishing an empty manifest.
        _ => walk_entries(output)?,
    };
    let mut files = Vec::with_capacity(entries.len());
    for entry in entries {
        let full = output.join(entry.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if entry.dir {
            files.push(CopyFile {
                source_path: crate::paths::path_string(&full),
                relative_path: entry.path,
                size: 0,
                size_known: true,
                sha256: String::new(),
                is_directory: true,
                is_corrupted: false,
            });
            continue;
        }
        let (size, digest) = digest_file(&full)?;
        files.push(CopyFile {
            source_path: crate::paths::path_string(&full),
            relative_path: entry.path,
            size,
            size_known: true,
            sha256: digest,
            is_directory: false,
            is_corrupted: false,
        });
    }
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

fn walk_entries(output: &Path) -> Result<Vec<ManifestEntry>, InplaceError> {
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(output).follow_links(false) {
        let entry = entry.map_err(|e| InplaceError::new("IoError", e.to_string()))?;
        if entry.path() == output {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(output)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let meta = entry
            .metadata()
            .map_err(|e| InplaceError::new("IoError", e.to_string()))?;
        entries.push(ManifestEntry {
            path: relative,
            size: meta.len(),
            dir: meta.is_dir(),
        });
    }
    Ok(entries)
}

fn digest_file(path: &Path) -> Result<(u64, String), InplaceError> {
    let mut file = fs::File::open(path)
        .map_err(|e| InplaceError::new("IoError", format!("无法读取 {}：{e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| InplaceError::new("IoError", format!("读取失败：{e}")))?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok((size, hex::encode(hasher.finalize())))
}

fn human(bytes: u64) -> String {
    engine::human(bytes)
}

/// Byte counts in the engine's own units, for progress messages.
pub fn human_bytes(bytes: u64) -> String {
    engine::human(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    fn sample_zip(dir: &Path) -> PathBuf {
        let path = dir.join("game.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for index in 0..6u8 {
            writer
                .start_file(format!("data/{index}.bin"), options)
                .unwrap();
            writer
                .write_all(&vec![index; 120_000 + usize::from(index)])
                .unwrap();
        }
        writer.finish().unwrap();
        path
    }

    fn request(archive: &Path, output: &Path) -> Request {
        Request {
            archive: archive.to_path_buf(),
            output: output.to_path_buf(),
            passwords: Vec::new(),
            keep_free: 0,
            replace: false,
            verify_first: true,
            max_entries: 10_000,
            max_expanded_bytes: 1 << 30,
        }
    }

    #[test]
    fn preflight_reports_an_in_place_run_fits_where_a_copy_would_not() {
        let dir = tempfile::tempdir().unwrap();
        let archive = sample_zip(dir.path());
        let output = dir.path().join("game");
        let plan = preflight(&request(&archive, &output)).unwrap();
        assert_eq!(plan.state, PreflightState::Fresh);
        assert_eq!(plan.entries, 6);
        assert!(plan.reclaims);
        assert!(plan.needed < plan.bytes);
        assert!(plan.blocker(10_000, 1 << 30).is_none());
    }

    #[test]
    fn execute_frees_the_archive_and_writes_every_entry() {
        let dir = tempfile::tempdir().unwrap();
        let archive = sample_zip(dir.path());
        let output = dir.path().join("game");
        let progress = Arc::new(Progress::default());
        let stop = Arc::new(AtomicBool::new(false));
        let result = execute(&request(&archive, &output), &progress, &stop).unwrap();
        assert!(result.is_finished());
        assert!(
            fs::metadata(&archive).map(|m| m.len() == 0).unwrap_or(true),
            "archive is consumed"
        );
        assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
        let manifest = manifest(&archive, &output).unwrap();
        assert_eq!(manifest.iter().filter(|f| !f.is_directory).count(), 6);
        assert!(
            manifest
                .iter()
                .filter(|f| !f.is_directory)
                .all(|f| !f.sha256.is_empty())
        );
        assert!(
            read_state(&archive).is_some(),
            "state sidecar is retained until publication commit"
        );
        forget_state(&archive).unwrap();
        assert!(read_state(&archive).is_none());
    }

    #[test]
    fn verify_first_refuses_a_damaged_archive_without_touching_it() {
        let dir = tempfile::tempdir().unwrap();
        let archive = sample_zip(dir.path());
        let mut bytes = fs::read(&archive).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xff;
        fs::write(&archive, &bytes).unwrap();
        let output = dir.path().join("game");
        let progress = Arc::new(Progress::default());
        let stop = Arc::new(AtomicBool::new(false));
        let error = execute(&request(&archive, &output), &progress, &stop).unwrap_err();
        assert_eq!(error.code, "Corrupt");
        assert_eq!(fs::metadata(&archive).unwrap().len(), bytes.len() as u64);
        assert!(!output.exists());
    }

    #[test]
    fn a_paused_run_resumes_where_it_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let archive = sample_zip(dir.path());
        let output = dir.path().join("game");
        let stop = Arc::new(AtomicBool::new(false));
        stop.store(true, Ordering::Relaxed);
        let progress = Arc::new(Progress::default());
        let paused = execute(&request(&archive, &output), &progress, &stop).unwrap();
        assert_eq!(paused, Executed::Paused);
        stop.store(false, Ordering::Relaxed);
        let resumed = execute(&request(&archive, &output), &progress, &stop).unwrap();
        assert!(resumed.is_finished());
        assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
    }

    #[test]
    fn test_journal_transaction_lifecycle_and_reconcile() {
        let dir = tempfile::tempdir().unwrap();
        let archive = sample_zip(dir.path());
        let output = dir.path().join("game");
        let stop = Arc::new(AtomicBool::new(false));
        stop.store(true, Ordering::Relaxed);
        let progress = Arc::new(Progress::default());

        // 1. 中途暂停模拟断电/崩溃
        let paused = execute(&request(&archive, &output), &progress, &stop).unwrap();
        assert_eq!(paused, Executed::Paused);

        // 2. 验证 .openunpack-journal 事务文件生成且包含状态
        assert!(journal_path(&archive).exists(), "journal file must exist");
        let journal = read_journal(&archive).expect("must parse journal");
        assert_eq!(journal.phase, JournalPhase::Interrupted);
        assert_eq!(journal.entries.len(), 6);

        // 3. 自愈检测
        assert!(
            reconcile_journal(&archive).unwrap(),
            "reconcile must confirm resumable session"
        );

        // 4. 接续解压至完成
        stop.store(false, Ordering::Relaxed);
        let resumed = execute(&request(&archive, &output), &progress, &stop).unwrap();
        assert!(resumed.is_finished());

        let final_journal = read_journal(&archive).expect("must read finished journal");
        assert_eq!(final_journal.phase, JournalPhase::Committed);

        // 5. 提交后清理验证
        forget_state(&archive).unwrap();
        assert!(
            !journal_path(&archive).exists(),
            "journal must be deleted on commit"
        );
        assert!(
            !state_path(&archive).exists(),
            "state sidecar must be deleted on commit"
        );
    }

    #[test]
    fn test_journal_cancellation_marks_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let archive = sample_zip(dir.path());
        let output = dir.path().join("game");
        let stop = Arc::new(AtomicBool::new(false));
        stop.store(true, Ordering::Relaxed);
        let progress = Arc::new(Progress::default());

        let _ = execute(&request(&archive, &output), &progress, &stop).unwrap();
        assert!(journal_path(&archive).exists());

        mark_journal_cancelled(&archive).unwrap();
        let j = read_journal(&archive).unwrap();
        assert_eq!(j.phase, JournalPhase::Cancelled);
    }
}
