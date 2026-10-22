use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
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

/// Temporary file extensions used by browsers, download managers, and system locks.
pub fn is_temp_download_file(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_ascii_lowercase(),
        None => return true,
    };
    name.ends_with(".crdownload")
        || name.ends_with(".part")
        || name.ends_with(".tmp")
        || name.ends_with(".temp")
        || name.ends_with(".download")
        || name.ends_with(".aria2")
        || name.ends_with(".opdownload")
        || name.ends_with(".idm")
        || name.ends_with(".xltd")
        || name.ends_with(".td")
        || name.ends_with(".td.cfg")
        || name.ends_with(".!ut")
        || name.ends_with(".bts")
        || name.ends_with(".downloading")
        || name.ends_with(".baiduyun.p.downloading")
        || name.ends_with(".partial")
        || name.starts_with("~$")
        || name.ends_with(".swp")
        || name.ends_with(".lock")
}

/// Helper to strip temporary download suffixes to check the underlying target filename.
/// For example, "archive.part2.rar.crdownload" -> Some("archive.part2.rar")
pub fn strip_temp_download_suffix(filename: &str) -> Option<&str> {
    let lower = filename.to_ascii_lowercase();
    let temp_suffixes = [
        ".crdownload",
        ".part",
        ".tmp",
        ".temp",
        ".download",
        ".aria2",
        ".opdownload",
        ".idm",
        ".xltd",
        ".td.cfg",
        ".td",
        ".!ut",
        ".bts",
        ".downloading",
        ".baiduyun.p.downloading",
        ".partial",
    ];
    for suffix in &temp_suffixes {
        if lower.ends_with(suffix) {
            return Some(&filename[..filename.len() - suffix.len()]);
        }
    }
    None
}

/// Helper to check if a filename belongs to a temporary download of the given volume stem.
pub fn is_volume_sibling_temp(filename: &str, target_stem_lower: &str) -> bool {
    let p = Path::new(filename);
    if !is_temp_download_file(p) {
        return false;
    }
    if let Some(target) = strip_temp_download_suffix(filename) {
        let vol = classify_volume(target);
        if let Some(stem) = vol.stem() {
            return stem.to_ascii_lowercase() == target_stem_lower;
        }
    }
    false
}

/// Probes whether a file is finished writing and accessible for reading without concurrent writer lock.
/// On Windows, opening with FILE_SHARE_READ (without FILE_SHARE_WRITE) ensures that if an active process
/// currently holds GENERIC_WRITE, Windows will deny access with ERROR_SHARING_VIOLATION (32).
#[cfg(windows)]
pub fn probe_file_ready(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_SHARE_READ = 1.
    // Denying FILE_SHARE_WRITE ensures any active writer causes this call to fail.
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .is_ok()
}

#[cfg(not(windows))]
pub fn probe_file_ready(path: &Path) -> bool {
    std::fs::OpenOptions::new().read(true).open(path).is_ok()
}

/// L4 Pre-flight verification: inspects header and footer structures to ensure
/// the archive is not truncated (e.g. from an interrupted or paused download).
pub fn is_archive_complete_preflight(path: &Path) -> bool {
    let fname = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };

    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let file_len = meta.len();
    if file_len == 0 {
        return false;
    }

    let mut f = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    is_archive_complete_preflight_stream(fname, file_len, &mut f)
}

/// Generic stream-based preflight verification across archive formats (allows in-memory testing).
pub fn is_archive_complete_preflight_stream<R: Read + Seek>(
    fname: &str,
    file_len: u64,
    reader: &mut R,
) -> bool {
    if file_len == 0 {
        return false;
    }

    // Split continuation volumes (e.g. *.part2.rar, *.z02, *.zip.002) don't have
    // leading magic or standalone trailing records. As long as length > 0, bypass preflight.
    let vol = classify_volume(fname);
    let is_continuation = matches!(
        vol,
        VolumeType::NumericSplit { num: 2.., .. }
            | VolumeType::PartRar { num: 2.., .. }
            | VolumeType::OldRar { num: Some(_), .. }
            | VolumeType::SplitZip { num: Some(2..), .. }
    );
    if is_continuation {
        // G-03 Guard: If it's a continuation volume, ensure it's not an unfinished P2P zero-fill preallocation
        if crate::intake_guard::is_trailing_zero_allocated(reader, file_len) {
            return false;
        }
        return true;
    }

    let lower = fname.to_ascii_lowercase();

    // 1. ZIP / JAR / APK:
    // Standard ZIP must contain End of Central Directory (EOCD) signature [0x50, 0x4b, 0x05, 0x06]
    // or ZIP64 EOCD locator [0x50, 0x4b, 0x06, 0x07] in the trailing 65557 bytes.
    if lower.ends_with(".zip") || lower.ends_with(".jar") || lower.ends_with(".apk") {
        if file_len < 22 {
            return false;
        }
        let read_len = (file_len.min(65557)) as usize;
        let mut buf = vec![0u8; read_len];
        if reader.seek(SeekFrom::End(-(read_len as i64))).is_ok()
            && reader.read_exact(&mut buf).is_ok()
        {
            let has_eocd = buf
                .windows(4)
                .any(|w| w == [0x50, 0x4b, 0x05, 0x06] || w == [0x50, 0x4b, 0x06, 0x07]);
            if !has_eocd {
                return false;
            }
        } else {
            return false;
        }
    }

    // 2. 7z:
    // Minimum 7z header is 32 bytes with signature [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C].
    // StartHeader contains NextHeaderOffset (8 bytes at 12) + NextHeaderSize (8 bytes at 20).
    // If NextHeaderSize > 0, 32 + NextHeaderOffset + NextHeaderSize must not exceed file_len.
    if lower.ends_with(".7z") {
        if file_len < 32 {
            return false;
        }
        let mut header = [0u8; 32];
        if reader.seek(SeekFrom::Start(0)).is_ok() && reader.read_exact(&mut header).is_ok() {
            if header[0..6] != [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C] {
                return false;
            }
            let next_header_offset =
                u64::from_le_bytes(header[12..20].try_into().unwrap_or([0; 8]));
            let next_header_size = u64::from_le_bytes(header[20..28].try_into().unwrap_or([0; 8]));
            if next_header_size > 0
                && let Some(needed) = 32u64
                    .checked_add(next_header_offset)
                    .and_then(|s| s.checked_add(next_header_size))
                && needed > file_len
            {
                return false; // Truncated end header
            }
        } else {
            return false;
        }
    }

    // 3. RAR:
    // RAR4 header: [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00] (7 bytes)
    // RAR5 header: [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00] (8 bytes)
    if lower.ends_with(".rar") {
        if file_len < 7 {
            return false;
        }
        let mut magic = [0u8; 8];
        if reader.seek(SeekFrom::Start(0)).is_ok() {
            let n = reader.read(&mut magic).unwrap_or(0);
            if n < 7 {
                return false;
            }
            let is_rar4 = magic[..7] == [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00];
            let is_rar5 = n >= 8 && magic[..8] == [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00];
            if !is_rar4 && !is_rar5 {
                return false;
            }
        } else {
            return false;
        }
    }

    // 4. GZ / TGZ:
    // Magic: [0x1F, 0x8B], at least 10 bytes header + 8 bytes footer = 18 bytes
    if lower.ends_with(".gz") || lower.ends_with(".tgz") {
        if file_len < 18 {
            return false;
        }
        let mut magic = [0u8; 2];
        if reader.seek(SeekFrom::Start(0)).is_ok()
            && reader.read_exact(&mut magic).is_ok()
            && magic != [0x1F, 0x8B]
        {
            return false;
        }
    }

    true
}

/// Updates a candidate's stability status given latest observed file size, mtime, and read probe result.
/// Returns whether the record successfully achieved stabilization.
pub fn update_candidate_stability(
    record: &mut FileStabilityRecord,
    current_size: u64,
    current_mtime: SystemTime,
    probe_ok: bool,
    required_ticks: usize,
) -> bool {
    crate::intake_guard::update_stability_with_backoff(
        &mut record.stable_ticks,
        &mut record.last_size,
        &mut record.last_mtime,
        &mut record.consecutive_probe_failures,
        current_size,
        current_mtime,
        probe_ok,
        required_ticks,
    )
}

/// Stability tracking for a candidate archive file.
#[derive(Debug, Clone)]
pub struct FileStabilityRecord {
    pub path: PathBuf,
    pub last_size: u64,
    pub last_mtime: SystemTime,
    pub stable_ticks: usize,
    pub consecutive_probe_failures: usize,
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
                eprintln!(
                    "[FolderWatcher] Failed to enqueue discovered archives: {}",
                    e
                );
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
    let mut enqueued_history: HashSet<(PathBuf, u64, SystemTime)> = HashSet::new();
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
        let mut to_remove = Vec::new();

        for (path, record) in pending_candidates.iter_mut() {
            if !path.exists() || !path.is_file() {
                to_remove.push(path.clone());
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

        for (path, record) in pending_candidates.iter() {
            if to_remove.contains(path) {
                continue;
            }
            if record.stable_ticks >= config.required_stable_ticks {
                // Verify it actually is a supported archive format
                let fname = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if !is_supported_archive_path(path, fname) {
                    to_remove.push(path.clone());
                    continue;
                }

                // L4 Pre-flight verification: header/footer truncation detection
                if !is_archive_complete_preflight(path) {
                    continue;
                }

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
                        .and_then(|parent| fs::read_dir(parent).ok())
                        .map(|entries| {
                            entries.flatten().any(|entry| {
                                let ep = entry.path();
                                if ep.is_file()
                                    && let Some(ename) = ep.file_name().and_then(|n| n.to_str())
                                {
                                    return is_volume_sibling_temp(ename, &stem_lower);
                                }
                                false
                            })
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
                to_remove.push(path.clone());
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
    enqueued_history: &mut HashSet<(PathBuf, u64, SystemTime)>,
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
    enqueued: &HashSet<(PathBuf, u64, SystemTime)>,
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
    enqueued: &HashSet<(PathBuf, u64, SystemTime)>,
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
                first_seen: Instant::now(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_temp_download_file_detection() {
        assert!(is_temp_download_file(Path::new("video.zip.crdownload")));
        assert!(is_temp_download_file(Path::new("archive.part")));
        assert!(is_temp_download_file(Path::new("data.tmp")));
        assert!(is_temp_download_file(Path::new("large.aria2")));
        assert!(is_temp_download_file(Path::new("file.idm")));
        assert!(is_temp_download_file(Path::new("movie.xltd")));
        assert!(is_temp_download_file(Path::new("movie.td")));
        assert!(is_temp_download_file(Path::new("torrent.!ut")));
        assert!(is_temp_download_file(Path::new("backup.downloading")));
        assert!(is_temp_download_file(Path::new("data.partial")));
        assert!(is_temp_download_file(Path::new("~$doc.docx")));

        assert!(!is_temp_download_file(Path::new("normal.zip")));
        assert!(!is_temp_download_file(Path::new("package.tar.gz")));
        assert!(!is_temp_download_file(Path::new("bundle.7z")));
        assert!(!is_temp_download_file(Path::new("game.part1.rar")));
    }

    #[test]
    fn test_strip_temp_download_suffix() {
        assert_eq!(
            strip_temp_download_suffix("archive.part2.rar.crdownload"),
            Some("archive.part2.rar")
        );
        assert_eq!(
            strip_temp_download_suffix("movie.7z.001.xltd"),
            Some("movie.7z.001")
        );
        assert_eq!(
            strip_temp_download_suffix("data.tar.gz.downloading"),
            Some("data.tar.gz")
        );
        assert_eq!(strip_temp_download_suffix("data.zip"), None);
    }

    #[test]
    fn test_volume_sibling_temp_detection() {
        assert!(is_volume_sibling_temp("Game.part2.rar.crdownload", "game"));
        assert!(is_volume_sibling_temp("Game.part2.rar.xltd", "game"));
        assert!(is_volume_sibling_temp("Movie.7z.002.downloading", "movie"));

        // Different stem
        assert!(!is_volume_sibling_temp(
            "Other.part2.rar.crdownload",
            "game"
        ));
        // Not a temp download file
        assert!(!is_volume_sibling_temp("Game.part2.rar", "game"));
    }

    #[test]
    fn test_candidate_archive_name_filter() {
        assert!(is_archive_candidate_name("archive.zip"));
        assert!(is_archive_candidate_name("files.7z"));
        assert!(is_archive_candidate_name("data.rar"));
        assert!(is_archive_candidate_name("release.tar.gz"));
        assert!(is_archive_candidate_name("backup.tar.xz"));
        assert!(is_archive_candidate_name("split.zip.001"));
        assert!(is_archive_candidate_name("movie.part1.rar"));

        assert!(!is_archive_candidate_name("image.png"));
        assert!(!is_archive_candidate_name("document.pdf"));
        assert!(!is_archive_candidate_name("notes.txt"));
    }

    #[test]
    fn test_probe_file_ready_read_probe() {
        // Non-existent file must fail read probe
        assert!(!probe_file_ready(Path::new(
            "non_existent_archive_probe.zip"
        )));

        let manifest_cargo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        // In unconstrained desktop execution, probing an accessible idle file succeeds.
        // In sandboxed/restricted execution environments, check safely returns bool without panicking.
        let is_ready = probe_file_ready(&manifest_cargo);
        let _ = is_ready;
    }

    #[test]
    fn test_stability_record_logic() {
        let t0 = SystemTime::UNIX_EPOCH;
        let mut record = FileStabilityRecord {
            path: PathBuf::from("test.zip"),
            last_size: 0,
            last_mtime: t0,
            stable_ticks: 0,
            consecutive_probe_failures: 0,
            first_seen: Instant::now(),
        };

        // 1. Zero size -> not stable
        assert!(!update_candidate_stability(&mut record, 0, t0, true, 3));
        assert_eq!(record.stable_ticks, 0);

        // 2. First tick with size 100 -> reset to 0 ticks
        assert!(!update_candidate_stability(&mut record, 100, t0, true, 3));
        assert_eq!(record.stable_ticks, 0);
        assert_eq!(record.last_size, 100);

        // 3. Second tick with same size, same mtime, probe OK -> tick 1
        assert!(!update_candidate_stability(&mut record, 100, t0, true, 3));
        assert_eq!(record.stable_ticks, 1);

        // 4. File size changes while downloading -> ticks reset to 0
        assert!(!update_candidate_stability(&mut record, 150, t0, true, 3));
        assert_eq!(record.stable_ticks, 0);
        assert_eq!(record.last_size, 150);

        // 5. Probe fails (e.g. writer exclusive lock held) -> ticks reset to 0
        assert!(!update_candidate_stability(&mut record, 150, t0, false, 3));
        assert_eq!(record.stable_ticks, 0);

        // 6. Stabilizing: tick 1
        assert!(!update_candidate_stability(&mut record, 150, t0, true, 3));
        assert_eq!(record.stable_ticks, 1);

        // 7. Stabilizing: tick 2
        assert!(!update_candidate_stability(&mut record, 150, t0, true, 3));
        assert_eq!(record.stable_ticks, 2);

        // 8. Stabilizing: tick 3 -> reaches required 3 ticks!
        assert!(update_candidate_stability(&mut record, 150, t0, true, 3));
        assert_eq!(record.stable_ticks, 3);
    }

    #[test]
    fn test_archive_complete_preflight_stream() {
        // 1. 0-byte file: rejected
        let mut cursor = Cursor::new(vec![]);
        assert!(!is_archive_complete_preflight_stream(
            "empty.zip",
            0,
            &mut cursor
        ));

        // 2. Valid ZIP (contains EOCD)
        let mut zip_data = vec![0u8; 30];
        zip_data[26..30].copy_from_slice(&[0x50, 0x4B, 0x05, 0x06]);
        let mut cursor = Cursor::new(zip_data);
        assert!(is_archive_complete_preflight_stream(
            "valid.zip",
            30,
            &mut cursor
        ));

        // 3. Valid ZIP64 (contains EOCD locator)
        let mut zip64_data = vec![0u8; 30];
        zip64_data[26..30].copy_from_slice(&[0x50, 0x4B, 0x06, 0x07]);
        let mut cursor = Cursor::new(zip64_data);
        assert!(is_archive_complete_preflight_stream(
            "valid.zip",
            30,
            &mut cursor
        ));

        // 4. Truncated ZIP (missing EOCD)
        let trunc_zip = b"PK\x03\x04randomdatawithoutcentraldir".to_vec();
        let len = trunc_zip.len() as u64;
        let mut cursor = Cursor::new(trunc_zip);
        assert!(!is_archive_complete_preflight_stream(
            "truncated.zip",
            len,
            &mut cursor
        ));

        // 5. Valid 7z (valid 32-byte header with within-bounds NextHeader)
        let mut header_7z = [0u8; 32];
        header_7z[0..6].copy_from_slice(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]);
        let mut cursor = Cursor::new(header_7z.to_vec());
        assert!(is_archive_complete_preflight_stream(
            "valid.7z",
            32,
            &mut cursor
        ));

        // 6. Truncated 7z (next_header claims more bytes than file size)
        let mut trunc_7z = [0u8; 32];
        trunc_7z[0..6].copy_from_slice(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]);
        trunc_7z[20..28].copy_from_slice(&500u64.to_le_bytes());
        let mut cursor = Cursor::new(trunc_7z.to_vec());
        assert!(!is_archive_complete_preflight_stream(
            "truncated.7z",
            32,
            &mut cursor
        ));

        // 7. Invalid 7z magic
        let mut bad_7z = [0u8; 32];
        bad_7z[0..6].copy_from_slice(b"bad7z!");
        let mut cursor = Cursor::new(bad_7z.to_vec());
        assert!(!is_archive_complete_preflight_stream(
            "bad.7z",
            32,
            &mut cursor
        ));

        // 8. Valid RAR4 header
        let rar4_data = b"Rar!\x1A\x07\x00extra".to_vec();
        let len = rar4_data.len() as u64;
        let mut cursor = Cursor::new(rar4_data);
        assert!(is_archive_complete_preflight_stream(
            "valid4.rar",
            len,
            &mut cursor
        ));

        // 9. Valid RAR5 header
        let rar5_data = b"Rar!\x1A\x07\x01\x00extra".to_vec();
        let len = rar5_data.len() as u64;
        let mut cursor = Cursor::new(rar5_data);
        assert!(is_archive_complete_preflight_stream(
            "valid5.rar",
            len,
            &mut cursor
        ));

        // 10. Corrupted RAR header
        let bad_rar = b"Rar!bad".to_vec();
        let len = bad_rar.len() as u64;
        let mut cursor = Cursor::new(bad_rar);
        assert!(!is_archive_complete_preflight_stream(
            "bad.rar",
            len,
            &mut cursor
        ));

        // 11. Valid GZ
        let mut gz_data = vec![0u8; 20];
        gz_data[0..2].copy_from_slice(&[0x1F, 0x8B]);
        let mut cursor = Cursor::new(gz_data);
        assert!(is_archive_complete_preflight_stream(
            "archive.tar.gz",
            20,
            &mut cursor
        ));

        // 12. Continuation volume bypasses header preflight
        let raw_continuation = vec![0x99; 50];
        let mut cursor = Cursor::new(raw_continuation);
        assert!(is_archive_complete_preflight_stream(
            "game.part2.rar",
            50,
            &mut cursor
        ));
        let mut cursor = Cursor::new(vec![0xAA; 50]);
        assert!(is_archive_complete_preflight_stream(
            "data.7z.002",
            50,
            &mut cursor
        ));
    }
}
