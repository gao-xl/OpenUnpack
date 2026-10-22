use super::*;

pub fn validate_settings(s: &Settings) -> Result<(), String> {
    if !matches!(
        s.conflict_policy.as_str(),
        "rename" | "skip" | "timestamp" | "keepNewer" | "keepOlder" | "keepLarger" | "overwrite"
    ) || !matches!(s.cache_policy.as_str(), "auto" | "always")
        || !matches!(s.verification.as_str(), "standard" | "strict")
        || !matches!(s.theme.as_str(), "light" | "dark" | "system")
        || !matches!(s.source_cleanup.as_str(), "keep" | "recycle" | "delete")
    {
        return Err("设置选项无效。".into());
    }
    if s.max_depth == 0
        || s.max_depth > 8
        || s.max_entries == 0
        || s.max_entries > 100000
        || s.max_expanded_bytes == 0
        || s.cache_quota_bytes == 0
        || s.history_limit == 0
        || s.history_limit > 10000
        || s.concurrency == 0
        || s.concurrency > MAX_CONCURRENT_JOBS
    {
        return Err("资源限制超出支持范围。".into());
    }
    if s.cache_enabled && s.cache_root.trim().is_empty() {
        return Err("启用缓存时必须选择缓存目录。".into());
    }
    for p in [&s.cache_root, &s.output_root] {
        if !p.is_empty() {
            paths::check_path(Path::new(p))?;
        }
    }
    if s.watched_folders.len() > 50 {
        return Err("监听目录数量不能超过 50 个。".into());
    }
    for p in &s.watched_folders {
        if !p.is_empty() {
            paths::check_path(Path::new(p))?;
        }
    }
    if !s.output_root.is_empty() && !s.watched_folders.is_empty() {
        let watched_paths: Vec<PathBuf> = s.watched_folders.iter().map(PathBuf::from).collect();
        crate::intake_guard::check_loop_trap(&watched_paths, Path::new(&s.output_root))?;
    }
    if s.input_exclusions.len() > 100
        || s.output_exclusions.len() > 100
        || s.output_inclusions.len() > 10_000
        || s.input_exclusions
            .iter()
            .chain(&s.output_exclusions)
            .chain(&s.output_inclusions)
            .any(|p| p.len() > crate::scanner::MAX_EXCLUSION_PATTERN_BYTES)
    {
        return Err("排除或选择规则过多或过长。".into());
    }
    Ok(())
}
pub(super) fn witness_file(file: &fs::File) -> Result<SourceWitness, String> {
    use std::os::windows::io::AsRawHandle;
    let metadata = file.metadata().map_err(|_| "IoError")?;
    use std::os::windows::fs::MetadataExt;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
        return Err("UnsafePath".into());
    }
    let mut info: windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION =
        unsafe { std::mem::zeroed() };
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(
            file.as_raw_handle() as _,
            &mut info,
        )
    } == 0
    {
        return Err("IoError".into());
    }
    Ok(SourceWitness {
        size: metadata.len(),
        modified: metadata.modified().map_err(|_| "IoError")?,
        volume: info.dwVolumeSerialNumber,
        index: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    })
}
pub(super) fn source_witnesses(source: &SourceGroup) -> Result<Vec<SourceWitness>, String> {
    source
        .volumes
        .iter()
        .map(|volume| {
            let path = Path::new(volume);
            let directory = paths::DirectoryGuard::open(path.parent().ok_or("UnsafePath")?)
                .map_err(|_| "UnsafePath")?;
            let file = directory
                .open_file_with_share(
                    path,
                    windows_sys::Win32::Foundation::GENERIC_READ,
                    windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ,
                )
                .map_err(|_| "UnsafePath")?;
            witness_file(&file)
        })
        .collect()
}
pub(super) enum CleanupSource {
    Delete(paths::LockedEntry),
    Recycle(crate::saferecycle::RecyclableSource),
}
impl CleanupSource {
    pub(super) fn file(&self) -> &fs::File {
        match self {
            Self::Delete(entry) => entry.file(),
            Self::Recycle(entry) => entry.file(),
        }
    }
    pub(super) fn cleanup(&self) -> Result<(), String> {
        match self {
            Self::Delete(entry) => entry.delete().map_err(|_| "删除源文件失败".into()),
            Self::Recycle(entry) => entry.recycle(),
        }
    }
}
pub(super) fn lock_cleanup_sources(
    source: &SourceGroup,
    expected: &[SourceWitness],
    recycle: bool,
) -> Result<Vec<CleanupSource>, String> {
    if source.volumes.len() != expected.len() || expected.is_empty() {
        return Err("源分卷身份不完整，已保留源文件。".into());
    }
    let entries: Vec<CleanupSource> = source
        .volumes
        .iter()
        .map(|volume| {
            if recycle {
                crate::saferecycle::RecyclableSource::open(Path::new(volume))
                    .map(CleanupSource::Recycle)
                    .map_err(|e| e.to_string())
            } else {
                paths::LockedEntry::open(Path::new(volume), true)
                    .map(CleanupSource::Delete)
                    .map_err(|e| e.to_string())
            }
        })
        .collect::<Result<_, _>>()?;
    for (entry, witness) in entries.iter().zip(expected) {
        if witness_file(entry.file())? != *witness {
            return Err("源压缩包在运行期间发生变化，源文件已保留。".into());
        }
    }
    Ok(entries)
}
pub(super) fn verify_witnesses(
    source: &SourceGroup,
    expected: &[SourceWitness],
) -> Result<(), String> {
    if source_witnesses(source)? != expected {
        Err("源压缩包在运行期间发生变化，结果尚未发布。".into())
    } else {
        Ok(())
    }
}
pub(super) fn is_derivation_tree_closed(state: &SavedRun) -> bool {
    // 1. Pending queue must be completely empty (no unfinished nodes awaiting processing)
    if !state.pending.is_empty() {
        return false;
    }
    // 2. Failed nodes must be completely empty (no failed sub-archives)
    if !state.failed.is_empty() {
        return false;
    }
    // 3. All views must be in "completed" status
    if state.views.is_empty() || state.views.iter().any(|v| v.status != "completed") {
        return false;
    }
    // 4. Sealed nodes count must match views count
    if state.sealed.len() != state.views.len() {
        return false;
    }
    // 5. Must have at least one root node (depth 0, parent None)
    let has_root = state
        .sealed
        .iter()
        .any(|s| s.node.depth == 0 && s.node.parent.is_none());
    if !has_root {
        return false;
    }
    // 6. Every non-root node must point to a valid sealed parent node
    let sealed_ids: HashSet<&str> = state.sealed.iter().map(|s| s.node.id.as_str()).collect();
    for sealed in &state.sealed {
        if let Some(ref parent_id) = sealed.node.parent
            && !sealed_ids.contains(parent_id.as_str())
        {
            return false;
        }
    }
    true
}
pub(super) fn unique_prefix(base: PathBuf, files: &[CopyFile], reserved: &[PathBuf]) -> PathBuf {
    let mut result = base.clone();
    let mut n = 2;
    loop {
        let key = paths::path_string(&result)
            .replace('\\', "/")
            .to_lowercase();
        let collision = files.iter().any(|f| {
            let p = f.relative_path.replace('\\', "/").to_lowercase();
            p == key || p.starts_with(&(key.clone() + "/"))
        }) || reserved.iter().any(|r| {
            let p = paths::path_string(r).replace('\\', "/").to_lowercase();
            p == key || p.starts_with(&(key.clone() + "/")) || key.starts_with(&(p + "/"))
        });
        if !collision {
            return result;
        }
        let name = base.file_name().unwrap_or_default().to_string_lossy();
        result = base.with_file_name(format!("{name} ({n})"));
        n += 1;
    }
}
pub(super) fn unique_target(parent: &Path, name: &str) -> Result<PathBuf, String> {
    for n in 2..10000 {
        let p = parent.join(format!("{name} ({n})"));
        if !p.exists() {
            return Ok(p);
        }
    }
    Err("同名目标过多，请选择另一输出目录。".into())
}

fn is_os_noise_entry(norm: &str) -> bool {
    let lower = norm.to_ascii_lowercase();
    lower == "__macosx"
        || lower.starts_with("__macosx/")
        || lower == ".ds_store"
        || lower.ends_with("/.ds_store")
        || lower == "thumbs.db"
        || lower.ends_with("/thumbs.db")
        || lower == "desktop.ini"
        || lower.ends_with("/desktop.ini")
        || lower.starts_with("._")
        || lower.contains("/._")
}

pub(super) fn strip_single_root_wrapper_once(files: &mut Vec<CopyFile>) -> bool {
    // Collect non-empty top-level directory components ignoring OS noise
    let mut top_dirs_map: HashMap<String, String> = HashMap::new();
    let mut has_root_files = false;

    for f in files.iter() {
        let norm = f.relative_path.replace('\\', "/");
        if is_os_noise_entry(&norm) {
            continue;
        }
        let mut comps = norm.split('/').filter(|s| !s.is_empty());
        if let Some(first) = comps.next() {
            if comps.next().is_none() && !f.is_directory {
                // There is a substantive file directly at root
                has_root_files = true;
                break;
            } else {
                let key = first.to_ascii_lowercase();
                top_dirs_map.entry(key).or_insert_with(|| first.to_string());
            }
        }
    }

    if !has_root_files && top_dirs_map.len() == 1 {
        let (_, canonical_root) = top_dirs_map.into_iter().next().unwrap();
        let root_lower = canonical_root.to_ascii_lowercase();
        let prefix_lower = format!("{}/", root_lower);

        let mut new_files = Vec::with_capacity(files.len());
        for f in files.iter() {
            let norm = f.relative_path.replace('\\', "/");
            let norm_lower = norm.to_ascii_lowercase();

            // Skip the top redundant container folder entry itself
            if norm_lower == root_lower && f.is_directory {
                continue;
            }

            if norm_lower.starts_with(&prefix_lower) {
                let stripped = &norm[canonical_root.len() + 1..];
                if !stripped.is_empty() {
                    let mut updated = f.clone();
                    updated.relative_path = stripped.replace('/', "\\");
                    new_files.push(updated);
                }
            } else if !is_os_noise_entry(&norm) {
                new_files.push(f.clone());
            }
        }

        if !new_files.is_empty() {
            *files = new_files;
            return true;
        }
    }
    false
}

pub(super) fn strip_single_root_wrapper(files: &mut Vec<CopyFile>) {
    for _ in 0..3 {
        if !strip_single_root_wrapper_once(files) {
            break;
        }
    }
}
pub(super) fn strip_empty_directories(files: &mut Vec<CopyFile>) {
    let mut non_empty_dirs = HashSet::new();
    for f in files.iter() {
        if !f.is_directory
            && let Ok(rel) = paths::relative(&f.relative_path)
        {
            for ancestor in rel.ancestors().filter(|p| !p.as_os_str().is_empty()) {
                non_empty_dirs.insert(
                    paths::path_string(ancestor)
                        .replace('\\', "/")
                        .to_lowercase(),
                );
            }
        }
    }
    files.retain(|f| {
        if !f.is_directory {
            return true;
        }
        if let Ok(rel) = paths::relative(&f.relative_path) {
            let key = paths::path_string(&rel).replace('\\', "/").to_lowercase();
            non_empty_dirs.contains(&key)
        } else {
            false
        }
    });
}
pub(super) fn validate_manifest(files: &[CopyFile]) -> Result<(), String> {
    let mut keys = std::collections::HashMap::with_capacity(files.len());
    for file in files {
        paths::relative(&file.relative_path)?;
        let key = file.relative_path.replace('\\', "/").to_lowercase();
        if keys.insert(key, file.is_directory).is_some() {
            return Err("内层合并出现重复路径，已停止发布。".into());
        }
        if !file.is_directory
            && (file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("解压摘要清单无效。".into());
        }
    }
    for key in keys.keys() {
        for (index, byte) in key.bytes().enumerate() {
            if byte == b'/' && keys.get(&key[..index]) == Some(&false) {
                return Err("文件与目录路径冲突，已停止发布。".into());
            }
        }
    }
    Ok(())
}
pub(super) fn lock_private_workspace(path: &Path) -> Result<paths::DirectoryGuard, String> {
    let lease =
        paths::DirectoryGuard::open(path).map_err(|_| "工作目录身份无法固定，已停止写入。")?;
    crate::saferecycle::verify_private_directory(&lease)
        .map_err(|_| "工作目录允许其他账号修改或无法确认权限，已停止写入并保留原包。")?;
    Ok(lease)
}

pub(super) async fn lock_validated_publication(
    path: &Path,
    id: &str,
    manifest: &str,
    files: &[CopyFile],
    readback: bool,
    cancel: Arc<AtomicBool>,
) -> Result<paths::LockedTree, String> {
    let path = path.to_owned();
    let id = id.to_owned();
    let manifest = manifest.to_owned();
    let files = files.to_vec();
    tokio::task::spawn_blocking(move || {
        let tree = paths::LockedTree::open(&path).map_err(|_| "UnsafePath")?;
        tree.verify_owned(&id, Some(&manifest))?;
        validate_locked_target(&tree, &path, &files, readback, &cancel)?;
        Ok(tree)
    })
    .await
    .map_err(|_| "目标校验任务异常，临时结果已保留。")?
}

pub(super) fn remove_publication_marker(
    directory: &Path,
    id: &str,
    manifest: &str,
) -> Result<(), String> {
    let tree = paths::LockedTree::open(directory).map_err(|_| "UnsafePath")?;
    tree.verify_owned(id, Some(manifest))?;
    let marker = directory.join(paths::OWNER_FILE);
    tree.entries()
        .iter()
        .find(|entry| entry.path() == marker)
        .ok_or("工作目录缺少独占标记。")?
        .delete()
        .map_err(|_| "IoError")?;
    Ok(())
}

pub(super) fn validate_locked_target(
    tree: &paths::LockedTree,
    staging_root: &Path,
    files: &[CopyFile],
    readback: bool,
    cancel: &AtomicBool,
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek};
    use std::os::windows::fs::MetadataExt;
    validate_manifest(files)?;
    let mut expected = HashMap::new();
    let mut allowed = HashSet::new();
    for file in files {
        let path = paths::relative(&file.relative_path)?;
        for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
            allowed.insert(
                paths::path_string(ancestor)
                    .replace('\\', "/")
                    .to_lowercase(),
            );
        }
        expected.insert(
            paths::path_string(&path).replace('\\', "/").to_lowercase(),
            file,
        );
    }
    let mut found = HashSet::new();
    let mut buffer = vec![0u8; 128 * 1024];
    for entry in tree.entries() {
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }
        let relative = entry
            .path()
            .strip_prefix(staging_root)
            .map_err(|_| "UnsafePath")?;
        let key = paths::path_string(relative)
            .replace('\\', "/")
            .to_lowercase();
        let metadata = entry.metadata().map_err(|_| "IoError")?;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("UnsafePath".into());
        }
        if key == paths::OWNER_FILE {
            continue;
        }
        if !allowed.contains(&key) {
            return Err("目标目录出现清单之外的条目，已停止发布。".into());
        }
        let Some(expected_file) = expected.get(&key) else {
            if !metadata.is_dir() {
                return Err("目标目录类型与清单不符。".into());
            }
            continue;
        };
        if metadata.is_dir() != expected_file.is_directory
            || (!metadata.is_dir() && metadata.len() != expected_file.size)
        {
            return Err("目标文件类型或大小与清单不符，已停止发布。".into());
        }
        found.insert(key);
        if readback && !metadata.is_dir() {
            let mut source = entry.file();
            source.rewind().map_err(|_| "IoError")?;
            let mut digest = Sha256::new();
            let mut bytes = 0u64;
            loop {
                if cancel.load(Ordering::Acquire) {
                    return Err("Cancelled".into());
                }
                let count = source.read(&mut buffer).map_err(|_| "IoError")?;
                if count == 0 {
                    break;
                }
                bytes = bytes.checked_add(count as u64).ok_or("ResourceLimit")?;
                if bytes > expected_file.size {
                    return Err("HashMismatch".into());
                }
                digest.update(&buffer[..count]);
            }
            if bytes != expected_file.size
                || !hex::encode(digest.finalize()).eq_ignore_ascii_case(&expected_file.sha256)
            {
                return Err("HashMismatch".into());
            }
        }
    }
    if expected.keys().any(|path| !found.contains(path)) {
        return Err("目标清单中的文件缺失，已停止发布。".into());
    }
    Ok(())
}

pub(super) fn validate_target_tree(target: &Path, files: &[CopyFile]) -> Result<(), String> {
    paths::check_path(target)?;
    validate_manifest(files)?;
    let mut expected = HashSet::new();
    for file in files {
        let relative = paths::relative(&file.relative_path)?;
        let path = target.join(&relative);
        paths::check_path(&path)?;
        let meta = fs::metadata(&path).map_err(|_| "目标清单中的文件缺失，已停止恢复。")?;
        if meta.is_dir() != file.is_directory || (!file.is_directory && meta.len() != file.size) {
            return Err("目标文件类型或大小与清单不符，已停止恢复。".into());
        }
        for ancestor in relative.ancestors().filter(|p| !p.as_os_str().is_empty()) {
            expected.insert(
                paths::path_string(ancestor)
                    .replace('\\', "/")
                    .to_lowercase(),
            );
        }
    }
    for entry in walkdir::WalkDir::new(target).follow_links(false) {
        let entry = entry.map_err(|_| "IoError")?;
        paths::check_path(entry.path())?;
        let relative = entry
            .path()
            .strip_prefix(target)
            .map_err(|_| "UnsafePath")?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let key = paths::path_string(relative)
            .replace('\\', "/")
            .to_lowercase();
        if key != paths::OWNER_FILE && !expected.contains(&key) {
            return Err("目标目录出现清单之外的条目，已停止恢复。".into());
        }
    }
    Ok(())
}
pub(super) fn message(code: &str) -> String {
    match code {
        "NeedsPassword" => "已保存密码未能打开压缩包，请补充密码后重试；损坏也可能导致此状态。",
        "Cancelled" => "任务已取消；原包和工作目录已保留。",
        "ResourceLimit" => "磁盘空间、缓存配额或展开限制不足，请调整后重试。",
        "CacheQuotaContention" => "当前其他并发任务正在占用缓存配额，等待资源释放后自动调度。",
        "SharedResourceUnavailable" => {
            "工作磁盘不可用或低于保留空间，队列已暂停；请恢复磁盘后重试。"
        }
        "MissingVolume" => "压缩包缺少分卷，请补齐后重试。",
        "UnsafePath" => "压缩包含不支持的路径或链接，已停止写入。",
        "Corrupt" | "PasswordMismatchOrCorrupt" => "密码不匹配或压缩包损坏，解压失败。",
        "WorkerExitUnconfirmed" => "无法确认解压进程退出；已停止后续任务并保留工作目录。",
        "StorageError" => "任务数据库写入失败，已停止后续操作；请检查恢复记录。",
        "ProtocolError" | "WorkerProtocolError" => "后台解压进程通信异常，解压失败。",
        "WorkerManifestCorrupted" => "任务清单数据损坏或分块索引异常，解压失败。",
        "WorkerStartFailed" | "WorkerUnavailable" => "后台解压进程启动失败，请检查运行环境。",
        "WorkerConnectTimeout" | "WorkerCancellationTimedOut" => {
            "后台解压进程响应超时，操作未完成。"
        }
        "WorkerDisconnected" => "后台解压进程意外断开连接，解压未完成。",
        "WorkerPipeUnavailable" | "WorkerPipeIdentityMismatch" => {
            "后台进程通信管道异常或身份不匹配。"
        }
        "WorkerJobObjectUnavailable" => "系统资源沙箱配置失败。",
        "WorkerResourceProbeUnavailable" => {
            "无法读取系统可用内存，已停止启动后台进程；请检查系统后重试。"
        }
        "WorkerControlFailed" | "WorkerUnresponsive" => "后台解压进程无响应或控制指令执行失败。",
        "WorkerProcessIdentityUnavailable" => "无法校验后台解压进程标识。",
        "WorkerUnsupportedPlatform" => "当前操作系统平台不支持此解压组件。",
        "IoError" => "文件读写失败；原包和工作目录已保留。",
        "Unsupported" => "此压缩格式暂不支持。",
        _ => code,
    }
    .into()
}
