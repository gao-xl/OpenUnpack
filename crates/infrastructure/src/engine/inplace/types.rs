//! 就地解压引擎请求参数、日志状态、预检与进度报告类型定义。

use super::*;

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

    pub(super) fn from_anyhow(error: anyhow::Error) -> Self {
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
    pub(super) done: AtomicU64,
    total: AtomicU64,
    pub(super) committed: AtomicU64,
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

    pub(super) fn begin(&self, phase: Phase, total: u64) {
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
