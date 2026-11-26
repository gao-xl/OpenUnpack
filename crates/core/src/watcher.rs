//! 实时目录监控与稳定期防抖入队调度。

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::coordinator::Coordinator;
use crate::scanner::{
    VolumeType, classify_volume, is_archive_candidate_name, is_supported_archive_path,
};

mod preflight;

pub use preflight::{
    is_archive_complete_preflight, is_archive_complete_preflight_stream, is_temp_download_file,
    is_volume_sibling_temp, probe_file_ready, strip_temp_download_suffix,
};

/// Updates a candidate's stability status given latest observed file size, mtime, and read probe result.
/// Returns whether the record successfully achieved stabilization.
pub fn update_candidate_stability(
    record: &mut FileStabilityRecord,
    current_size: u64,
    current_mtime: SystemTime,
    probe_ok: bool,
    required_ticks: usize,
) -> bool {
    let mut counters = crate::intake_guard::StabilityCounters {
        stable_ticks: record.stable_ticks,
        last_size: record.last_size,
        last_mtime: record.last_mtime,
        consecutive_probe_failures: record.consecutive_probe_failures,
    };
    let ready = crate::intake_guard::update_stability_with_backoff(
        &mut counters,
        crate::intake_guard::StabilitySample {
            current_size,
            current_mtime,
            probe_ok,
            required_ticks,
        },
    );
    record.stable_ticks = counters.stable_ticks;
    record.last_size = counters.last_size;
    record.last_mtime = counters.last_mtime;
    record.consecutive_probe_failures = counters.consecutive_probe_failures;
    ready
}

/// Maximum remembered archive fingerprints kept in memory by the folder watcher.
const MAX_ENQUEUED_HISTORY: usize = 4096;

/// Bounded FIFO fingerprint set preventing unbounded watcher RAM growth over
/// long desktop sessions while deduplicating already-seen archives.
#[derive(Debug, Default)]
pub struct BoundedEnqueueHistory {
    entries: HashSet<(PathBuf, u64, SystemTime)>,
    order: VecDeque<(PathBuf, u64, SystemTime)>,
    capacity: usize,
}

impl BoundedEnqueueHistory {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: HashSet::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn contains(&self, key: &(PathBuf, u64, SystemTime)) -> bool {
        self.entries.contains(key)
    }

    pub fn insert(&mut self, key: (PathBuf, u64, SystemTime)) -> bool {
        if !self.entries.insert(key.clone()) {
            return false;
        }
        self.order.push_back(key);
        while self.order.len() > self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.entries.remove(&evicted);
            }
        }
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Computes exponential backoff duration after consecutive preflight failures,
/// capping at 32 seconds so incomplete downloads aren't probed every 500ms tick.
pub fn preflight_backoff_duration(failures: u32) -> Duration {
    if failures == 0 {
        return Duration::ZERO;
    }
    Duration::from_secs(1u64 << failures.saturating_sub(1).min(5))
}

/// Stability tracking for a candidate archive file.
#[derive(Debug, Clone)]
pub struct FileStabilityRecord {
    pub path: PathBuf,
    pub last_size: u64,
    pub last_mtime: SystemTime,
    pub stable_ticks: usize,
    pub consecutive_probe_failures: usize,
    pub preflight_failures: u32,
    pub next_preflight_at: Option<Instant>,
    pub first_seen: Instant,
}

#[derive(Debug, Clone)]
pub struct WatcherConfig {
    pub tick_interval: Duration,
    pub required_stable_ticks: usize,
    pub recursive: bool,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_millis(500),
            // 6 ticks * 500ms = 3.0 seconds quiet stabilization window
            required_stable_ticks: 6,
            recursive: false,
        }
    }
}

pub enum WatcherCommand {
    SyncFolders(Vec<PathBuf>),
    Stop,
}

pub struct FolderWatcher {
    cmd_tx: Sender<WatcherCommand>,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl FolderWatcher {
    /// Spawns a background folder watcher hooked to the Coordinator.
    pub fn start_with_coordinator(
        coordinator: Weak<Coordinator>,
        initial_folders: Vec<PathBuf>,
        config: WatcherConfig,
    ) -> Result<Self, String> {
        let callback = Arc::new(move |paths: Vec<PathBuf>| {
            if let Some(coord) = coordinator.upgrade()
                && let Err(e) = coord.enqueue_discovered_files(&paths)
            {
                tracing::warn!(error = %e, "folder watcher failed to enqueue discovered archives");
            }
        });
        Self::start(callback, initial_folders, config)
    }

    /// Spawns a background folder watcher with a custom callback (used for tests or standalone operation).
    pub fn start(
        on_stable_archives: Arc<dyn Fn(Vec<PathBuf>) + Send + Sync + 'static>,
        initial_folders: Vec<PathBuf>,
        config: WatcherConfig,
    ) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = channel::<WatcherCommand>();
        let (event_tx, event_rx) = channel::<notify::Result<Event>>();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let watcher_res = RecommendedWatcher::new(
            move |res| {
                let _ = event_tx.send(res);
            },
            Config::default(),
        );

        let mut watcher = watcher_res.map_err(|e| format!("无法创建目录监听器: {}", e))?;

        let handle = std::thread::Builder::new()
            .name("openunpack-folder-watcher".into())
            .spawn(move || {
                run_watcher_loop(
                    &mut watcher,
                    cmd_rx,
                    event_rx,
                    on_stable_archives,
                    initial_folders,
                    config,
                    running_clone,
                );
            })
            .map_err(|e| format!("无法启动目录监听线程: {}", e))?;

        Ok(Self {
            cmd_tx,
            running,
            handle: Some(handle),
        })
    }

    /// Synchronizes the set of actively watched folders.
    pub fn sync_folders(&self, folders: Vec<PathBuf>) -> Result<(), String> {
        self.cmd_tx
            .send(WatcherCommand::SyncFolders(folders))
            .map_err(|e| format!("发送同步目录指令失败: {}", e))
    }

    /// Stops the watcher thread cleanly.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        let _ = self.cmd_tx.send(WatcherCommand::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for FolderWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_watcher_loop(
    watcher: &mut RecommendedWatcher,
    cmd_rx: Receiver<WatcherCommand>,
    event_rx: Receiver<notify::Result<Event>>,
    on_stable_archives: Arc<dyn Fn(Vec<PathBuf>) + Send + Sync + 'static>,
    initial_folders: Vec<PathBuf>,
    config: WatcherConfig,
    running: Arc<AtomicBool>,
) {
    let mut watched_dirs: HashSet<PathBuf> = HashSet::new();
    let mut pending_candidates: HashMap<PathBuf, FileStabilityRecord> = HashMap::new();
    let mut enqueued_history = BoundedEnqueueHistory::with_capacity(MAX_ENQUEUED_HISTORY);
    let recursive_mode = if config.recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };

    // Apply initial folders
    sync_watched_dirs(
        watcher,
        &mut watched_dirs,
        initial_folders,
        recursive_mode,
        &mut enqueued_history,
    );

    while running.load(Ordering::Acquire) {
        // 1. Process control commands (non-blocking)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                WatcherCommand::SyncFolders(new_folders) => {
                    sync_watched_dirs(
                        watcher,
                        &mut watched_dirs,
                        new_folders,
                        recursive_mode,
                        &mut enqueued_history,
                    );
                }
                WatcherCommand::Stop => {
                    running.store(false, Ordering::Release);
                    return;
                }
            }
        }

        // 2. Drain any notify events received
        while let Ok(event_res) = event_rx.try_recv() {
            if let Ok(event) = event_res {
                handle_notify_event(event, &mut pending_candidates, &enqueued_history);
            }
        }

        // 3. Stabilization tick
        let mut stable_batch = Vec::new();
        let mut to_remove: HashSet<PathBuf> = HashSet::new();

        for (path, record) in pending_candidates.iter_mut() {
            if !path.exists() || !path.is_file() {
                to_remove.insert(path.clone());
                continue;
            }

            // Exclude temporary download files that haven't been renamed yet
            if is_temp_download_file(path) {
                continue;
            }

            let meta = match fs::metadata(path) {
                Ok(m) => m,
                Err(_) => {
                    record.stable_ticks = 0;
                    continue;
                }
            };

            let current_size = meta.len();
            let current_mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

            // Zero-byte files cannot be valid archives; still being created
            if current_size == 0 {
                record.stable_ticks = 0;
                continue;
            }

            if current_size != record.last_size || current_mtime != record.last_mtime {
                record.preflight_failures = 0;
                record.next_preflight_at = None;
            }

            // Non-blocking exclusive read probe (Layer 2) and stability update (Layer 3)
            let probe_ok = probe_file_ready(path);
            update_candidate_stability(
                record,
                current_size,
                current_mtime,
                probe_ok,
                config.required_stable_ticks,
            );
        }

        // 3.2 Pre-calculate which volume stems have parts that are still unstable
        let mut unstable_volume_stems: HashSet<(PathBuf, String)> = HashSet::new();
        for (p, r) in pending_candidates.iter() {
            if r.stable_ticks < config.required_stable_ticks
                && let (Some(parent), Some(fname)) =
                    (p.parent(), p.file_name().and_then(|n| n.to_str()))
            {
                let vol = classify_volume(fname);
                if let Some(stem) = vol.stem() {
                    unstable_volume_stems.insert((parent.to_path_buf(), stem.to_ascii_lowercase()));
                }
            }
        }

        // 3.3 Cache directory listings for this tick so sibling volumes sharing a
        // parent directory trigger at most one read_dir each, instead of one per
        // stable candidate.
        let mut dir_listing_cache: HashMap<PathBuf, Option<Vec<String>>> = HashMap::new();
        let now = Instant::now();

        for (path, record) in pending_candidates.iter_mut() {
            if to_remove.contains(path) {
                continue;
            }
            if record.stable_ticks >= config.required_stable_ticks {
                if record
                    .next_preflight_at
                    .is_some_and(|next_at| now < next_at)
                {
                    continue;
                }
                // Verify it actually is a supported archive format
                let fname = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if !is_supported_archive_path(path, fname) {
                    to_remove.insert(path.clone());
                    continue;
                }

                // L4 Pre-flight verification: header/footer truncation detection
                if !is_archive_complete_preflight(path) {
                    record.preflight_failures = record.preflight_failures.saturating_add(1);
                    record.next_preflight_at =
                        Some(now + preflight_backoff_duration(record.preflight_failures));
                    continue;
                }
                record.preflight_failures = 0;
                record.next_preflight_at = None;

                // L5 Volume Co-readiness verification:
                // If this is a multi-part volume archive, check whether any sibling volume
                // in the same directory is still incomplete or actively downloading.
                let vol = classify_volume(fname);
                let has_sibling_unready = if let Some(stem) = vol.stem() {
                    let stem_lower = stem.to_ascii_lowercase();
                    let parent_buf = path.parent().map(|p| p.to_path_buf());

                    // Check if parent directory contains temporary download files for the same volume stem
                    let parent_has_temp = path
                        .parent()
                        .map(|parent| {
                            let names = dir_listing_cache
                                .entry(parent.to_path_buf())
                                .or_insert_with(|| {
                                    fs::read_dir(parent).ok().map(|entries| {
                                        entries
                                            .flatten()
                                            .filter_map(|entry| {
                                                if entry.path().is_file() {
                                                    entry.file_name().to_str().map(String::from)
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect::<Vec<String>>()
                                    })
                                });
                            names
                                .as_ref()
                                .map(|names| {
                                    names
                                        .iter()
                                        .any(|ename| is_volume_sibling_temp(ename, &stem_lower))
                                })
                                .unwrap_or(false)
                        })
                        .unwrap_or(false);

                    let pending_has_unstable = if let Some(parent) = parent_buf {
                        unstable_volume_stems.contains(&(parent, stem_lower))
                    } else {
                        false
                    };

                    parent_has_temp || pending_has_unstable
                } else {
                    false
                };

                if has_sibling_unready {
                    // Sibling volumes are still in flight; wait for all parts to stabilize
                    continue;
                }

                stable_batch.push((path.clone(), record.last_size, record.last_mtime));
                to_remove.insert(path.clone());
            }
        }

        for p in to_remove {
            pending_candidates.remove(&p);
        }

        // 4. Dispatch stable archives
        if !stable_batch.is_empty() {
            let mut paths_to_enqueue = Vec::new();
            for (path, size, mtime) in stable_batch {
                if enqueued_history.insert((path.clone(), size, mtime)) {
                    paths_to_enqueue.push(path);
                }
            }
            if !paths_to_enqueue.is_empty() {
                on_stable_archives(paths_to_enqueue);
            }
        }

        // Sleep for the configured tick interval
        std::thread::sleep(config.tick_interval);
    }
}

fn sync_watched_dirs(
    watcher: &mut RecommendedWatcher,
    current_dirs: &mut HashSet<PathBuf>,
    new_dirs: Vec<PathBuf>,
    recursive_mode: RecursiveMode,
    enqueued_history: &mut BoundedEnqueueHistory,
) {
    let target_set: HashSet<PathBuf> = new_dirs
        .into_iter()
        .filter(|p| p.is_dir())
        .map(|p| p.canonicalize().unwrap_or(p))
        .collect();

    // Unwatch removed directories
    let to_unwatch: Vec<PathBuf> = current_dirs.difference(&target_set).cloned().collect();
    for dir in to_unwatch {
        let _ = watcher.unwatch(&dir);
        current_dirs.remove(&dir);
    }

    // Watch new directories
    let to_watch: Vec<PathBuf> = target_set.difference(current_dirs).cloned().collect();
    for dir in to_watch {
        if watcher.watch(&dir, recursive_mode).is_ok() {
            // Snapshot baseline existing files so we don't accidentally dump old archives into queue
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let ep = entry.path();
                    if ep.is_file()
                        && let Ok(meta) = ep.metadata()
                    {
                        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                        enqueued_history.insert((ep, meta.len(), mtime));
                    }
                }
            }
            current_dirs.insert(dir);
        }
    }
}

fn handle_notify_event(
    event: Event,
    pending: &mut HashMap<PathBuf, FileStabilityRecord>,
    enqueued: &BoundedEnqueueHistory,
) {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Any => {
            for path in event.paths {
                if !path.is_file() && path.exists() {
                    // If a directory was created/modified, scan top-level children
                    if let Ok(entries) = fs::read_dir(&path) {
                        for entry in entries.flatten() {
                            let ep = entry.path();
                            if ep.is_file() {
                                consider_candidate(ep, pending, enqueued);
                            }
                        }
                    }
                    continue;
                }
                consider_candidate(path, pending, enqueued);
            }
        }
        _ => {}
    }
}

fn consider_candidate(
    path: PathBuf,
    pending: &mut HashMap<PathBuf, FileStabilityRecord>,
    enqueued: &BoundedEnqueueHistory,
) {
    let fname = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return,
    };

    if is_temp_download_file(&path) {
        return;
    }

    // G-02 Guard: Skip cloud files-on-demand placeholders (e.g. OneDrive offline stub)
    // to avoid triggering forced background network hydration.
    if crate::intake_guard::is_cloud_placeholder_file(&path) {
        return;
    }

    if !is_archive_candidate_name(fname) {
        return;
    }

    if let Ok(meta) = path.metadata() {
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if enqueued.contains(&(path.clone(), meta.len(), mtime)) {
            return;
        }
    }

    if !pending.contains_key(&path) {
        let (size, mtime) = path
            .metadata()
            .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
            .unwrap_or((0, SystemTime::UNIX_EPOCH));

        pending.insert(
            path.clone(),
            FileStabilityRecord {
                path,
                last_size: size,
                last_mtime: mtime,
                stable_ticks: 0,
                consecutive_probe_failures: 0,
                preflight_failures: 0,
                next_preflight_at: None,
                first_seen: Instant::now(),
            },
        );
    }
}

#[cfg(test)]
mod tests;
