use super::*;

pub(super) fn per_task_engine_threads(logical_cores: usize, concurrency: u32) -> u32 {
    (logical_cores / concurrency.clamp(1, MAX_CONCURRENT_JOBS) as usize).clamp(1, 64) as u32
}

pub(super) fn effective_engine_threads(requested: u32, concurrency: u32) -> u32 {
    if requested != 0 {
        return requested;
    }
    let logical_cores = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    per_task_engine_threads(logical_cores, concurrency)
}

pub(super) fn resolve_engine_threads_with_topology(
    requested: u32,
    concurrency: u32,
    is_hdd: bool,
) -> u32 {
    if requested != 0 {
        return requested;
    }
    if is_hdd {
        // HDD 寻道风暴避免：机械硬盘单主轴磁头顺序读写最佳，并发解压线程过多会导致严重寻道冲突（I/O Thrashing）。
        // 自动模式下对机械硬盘调度任务压制为最高 2 线程，确保流水线顺畅。
        return 2;
    }
    effective_engine_threads(0, concurrency)
}

pub(super) fn request(operation: &str, dest: &Path, s: &Settings) -> WorkerRequest {
    WorkerRequest {
        protocol_version: 2,
        request_id: uid(),
        operation: operation.into(),
        operation_id: Some(uid()),
        generation: Some(1),
        archive_path: String::new(),
        destination_directory: paths::path_string(dest),
        password: None,
        allowed_volumes: vec![],
        max_entries: s.max_entries,
        max_total_bytes: s.max_expanded_bytes,
        max_written_bytes: s.max_expanded_bytes,
        min_free_bytes: s.min_free_bytes,
        files: vec![],
        verification: s.verification.clone(),
        exclusions: if operation == "extract" {
            s.output_exclusions.clone()
        } else {
            vec![]
        },
        inclusions: if operation == "extract" {
            s.output_inclusions.clone()
        } else {
            vec![]
        },
        candidate_passwords: vec![],
        candidates: vec![],
        engine_threads: effective_engine_threads(s.engine_threads, s.concurrency),
        worker_priority: s.worker_priority.clone(),
        salvage_corrupted: s.salvage_corrupted,
    }
}
/// Split transfer lists by their exact JSON wire size, keeping the operation's
/// original aggregate limits. Response manifest spill does not enlarge the
/// opposite-direction named-pipe request budget.
pub(super) fn transfer_batches(mut request: WorkerRequest) -> Result<Vec<WorkerRequest>, String> {
    // Preserve the whole-list alias and file/parent preflight before writing
    // any batch. Checking only each slice would miss cross-batch conflicts.
    let mut names = std::collections::HashMap::new();
    for file in &request.files {
        let relative = paths::relative(&file.relative_path)?;
        let key = paths::path_string(&relative)
            .replace('\\', "/")
            .to_lowercase();
        if names.insert(key, file.is_directory).is_some() {
            return Err("UnsafePath".into());
        }
    }
    for name in names.keys() {
        for (index, byte) in name.bytes().enumerate() {
            if byte == b'/' && names.get(&name[..index]) == Some(&false) {
                return Err("UnsafePath".into());
            }
        }
    }
    let total = request.files.iter().try_fold(0u64, |sum, file| {
        sum.checked_add(file.size).ok_or("ResourceLimit")
    })?;
    if request.files.len() > 100_000
        || total > request.max_total_bytes
        || (request.operation != "readback" && total > request.max_written_bytes)
    {
        return Err("ResourceLimit".into());
    }
    let files = std::mem::take(&mut request.files);
    request.request_id = uid();
    request.operation_id = Some(uid());
    request.generation.get_or_insert(1);
    // Leave room for optional envelope additions while still measuring each
    // serialized record instead of guessing from path character counts.
    let wire_limit = MAX_MESSAGE_BYTES - 64 * 1024;
    let envelope = serde_json::to_vec(&request)
        .map_err(|_| "WorkerProtocolError")?
        .len();
    if envelope >= wire_limit {
        return Err("ResourceLimit".into());
    }
    let mut batches = Vec::new();
    let mut batch = request.clone();
    let mut bytes = envelope;
    let mut assigned = 0u64;
    for file in files {
        let record_bytes = serde_json::to_vec(&file)
            .map_err(|_| "WorkerProtocolError")?
            .len();
        if envelope + record_bytes > wire_limit {
            return Err("ResourceLimit".into());
        }
        let next_bytes = bytes + record_bytes + usize::from(!batch.files.is_empty());
        if next_bytes > wire_limit {
            assigned = assigned
                .checked_add(batch.files.iter().map(|f| f.size).sum::<u64>())
                .ok_or("ResourceLimit")?;
            batches.push(batch);
            batch = request.clone();
            batch.request_id = uid();
            batch.operation_id = Some(uid());
            if request.operation != "readback" {
                batch.max_written_bytes = request.max_written_bytes - assigned;
            }
            // A reduced numeric budget cannot enlarge the envelope.
            bytes = envelope;
        }
        bytes += record_bytes + usize::from(!batch.files.is_empty());
        batch.files.push(file);
    }
    if !batch.files.is_empty() {
        batches.push(batch);
    }
    Ok(batches)
}
pub(super) fn output_root(s: &Settings, source: &SourceGroup) -> Result<PathBuf, String> {
    if s.output_root.is_empty() {
        Path::new(&source.path)
            .parent()
            .map(Path::to_path_buf)
            .ok_or("UnsafePath".into())
    } else {
        Ok(PathBuf::from(&s.output_root))
    }
}
pub(super) fn read_run(work: &Path) -> Result<SavedRun, String> {
    paths::read_json_limited(&work.join(RUN_FILE), 512 * 1024 * 1024)
}
pub(super) fn save_run(work: &Path, state: &SavedRun) -> Result<(), String> {
    paths::atomic_json(&work.join(RUN_FILE), state)
}
pub(super) fn upgrade_legacy_run(state: &mut SavedRun) -> Result<(), String> {
    // v1 durably sealed each successful node before releasing its inputs, but
    // omitted retirement records. Completed sealed replacements are the only
    // evidence accepted for migrating that historical deletion window.
    for sealed in &state.sealed {
        if !state
            .views
            .iter()
            .any(|view| view.id == sealed.node.id && view.status == "completed")
        {
            return Err("旧工作区封存节点记录不完整，已停止迁移。".into());
        }
        if sealed.node.depth > 0 {
            state.retired_files.extend(
                sealed
                    .node
                    .group
                    .volumes
                    .iter()
                    .map(|path| path.to_lowercase()),
            );
        } else if !state.input_files.is_empty() {
            let volumes: Vec<_> = state
                .input_files
                .iter()
                .map(|file| file.source_path.clone())
                .collect();
            if sealed.node.group.volumes != volumes {
                return Err("旧工作区缓存与封存根节点不匹配，已停止迁移。".into());
            }
            state.input_retired = true;
        }
    }
    state.version = 2;
    Ok(())
}
pub(super) fn storage_directory_identity(path: &Path) -> Result<(u32, u64), String> {
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    // Compare held directory objects, not path spelling or a prefix that could
    // confuse cache/output aliases. The caller holds the workspace parent chain.
    let directory = paths::DirectoryGuard::open(path).map_err(|_| "UnsafePath")?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(directory.raw_handle(), &mut info) } == 0 {
        return Err("UnsafePath".into());
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    if index == 0 {
        return Err("UnsafePath".into());
    }
    Ok((info.dwVolumeSerialNumber, index))
}

pub(super) fn resumed_storage_route(
    saved: &Settings,
    source: &SourceGroup,
    work: &Path,
    target_dir: PathBuf,
) -> Result<(ExtractionRoute, PathBuf), String> {
    let parent = work.parent().ok_or("UnsafePath")?.to_path_buf();
    let identity = storage_directory_identity(&parent)?;
    if saved.cache_enabled && !saved.cache_root.trim().is_empty() {
        let cache_parent = Path::new(saved.cache_root.trim()).join("OpenUnpack-cache");
        if storage_directory_identity(&cache_parent).ok() == Some(identity) {
            return Ok((
                ExtractionRoute::SsdCache {
                    cache_workspace: parent.clone(),
                    target_dir,
                    score: 0,
                    reason: "继续使用已验证的缓存工作区，按实际目录计算空间与缓存配额".into(),
                },
                parent,
            ));
        }
    }
    let direct_parent = output_root(saved, source)?.join(".openunpack-work");
    if storage_directory_identity(&direct_parent).ok() != Some(identity) {
        return Err("已保存工作区的存储位置无法核对；请保留现场并检查恢复记录。".into());
    }
    Ok((
        ExtractionRoute::Direct {
            target_dir,
            reason: "继续使用已验证的目标卷工作区，按实际目录计算空间预算".into(),
        },
        parent,
    ))
}

pub(super) fn extraction_compatible(old: &Settings, new: &Settings) -> bool {
    old.cache_enabled == new.cache_enabled
        && old.cache_policy == new.cache_policy
        && old.cache_root == new.cache_root
        && old.cache_input == new.cache_input
        && old.expand_nested == new.expand_nested
        && old.max_depth == new.max_depth
        && old.output_exclusions == new.output_exclusions
}
pub(super) fn node_view(node: &NodeWork, status: &str, message: &str) -> NodeView {
    NodeView {
        id: node.id.clone(),
        parent_id: node.parent.clone(),
        name: node.group.display_name.clone(),
        depth: node.depth,
        status: status.into(),
        stage: "extracting".into(),
        progress: 0.,
        attempts: 0,
        message: message.into(),
        password_label: None,
    }
}
pub(super) fn content_fingerprint(hashes: &[(u64, String)]) -> Result<String, String> {
    if hashes.is_empty()
        || hashes
            .iter()
            .any(|(_, h)| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("ProtocolError".into());
    }
    paths::digest(&("OpenUnpack/content-group/v1", hashes))
}
pub(super) fn fingerprint_for_group(
    group: &SourceGroup,
    files: &[CopyFile],
) -> Result<Option<String>, String> {
    let hashes: Option<Vec<_>> = group
        .volumes
        .iter()
        .map(|p| {
            files
                .iter()
                .find(|f| f.source_path.eq_ignore_ascii_case(p) && !f.is_directory)
                .map(|f| (f.size, f.sha256.clone()))
        })
        .collect();
    hashes
        .map(|hashes| content_fingerprint(&hashes))
        .transpose()
}
pub(super) fn aggregate_verification_scopes<'a>(
    scopes: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut components = Vec::new();
    for scope in scopes {
        for component in scope.split('+').filter(|component| !component.is_empty()) {
            if !components.contains(&component) {
                components.push(component);
            }
        }
    }
    // Legacy checkpoints did not record source validation. Preserve only the
    // stream digest that every sealed file has; do not infer an archive CRC.
    if components.is_empty() {
        "streamSha256".into()
    } else {
        components.join("+")
    }
}
pub(super) fn unique_candidates(candidates: Vec<Candidate>) -> Vec<Candidate> {
    use sha2::{Digest, Sha256};
    let salt = zeroize::Zeroizing::new(rand::random::<[u8; 32]>());
    let mut indexes: std::collections::HashMap<[u8; 32], Vec<usize>> =
        std::collections::HashMap::new();
    let mut result: Vec<Candidate> = vec![];
    for candidate in candidates {
        let mut hash = Sha256::new();
        hash.update(salt.as_ref());
        hash.update(candidate.secret.as_bytes());
        let key: [u8; 32] = hash.finalize().into();
        let bucket = indexes.entry(key).or_default();
        if !bucket
            .iter()
            .any(|index| result[*index].secret == candidate.secret)
        {
            bucket.push(result.len());
            result.push(candidate);
        }
    }
    result
}
pub(super) fn prioritize_snapshot(
    snapshot: &[Candidate],
    priorities: Vec<Candidate>,
) -> Vec<Candidate> {
    let priority: std::collections::HashMap<_, _> = priorities
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.id.as_str(), index))
        .collect();
    let mut result = snapshot.to_vec();
    result.sort_by_key(|c| priority.get(c.id.as_str()).copied().unwrap_or(usize::MAX));
    result
}
pub(super) fn sealed_copy_files(sealed: &SealedNode) -> Result<Vec<CopyFile>, String> {
    sealed
        .files
        .iter()
        .map(|f| {
            Ok(CopyFile {
                size_known: true,
                source_path: paths::path_string(
                    &sealed.directory.join(paths::relative(&f.relative_path)?),
                ),
                relative_path: f.relative_path.clone(),
                size: f.size,
                sha256: f.sha256.clone(),
                is_directory: f.is_directory,
                is_corrupted: f.is_corrupted,
            })
        })
        .collect()
}
pub(super) fn retained_sealed_files(
    sealed: &SealedNode,
    retired: &HashSet<String>,
) -> Result<Vec<CopyFile>, String> {
    let original = sealed_copy_files(sealed)?;
    let mut files: Vec<_> = original
        .iter()
        .filter(|file| {
            !retired.contains(&file.source_path.to_lowercase())
                || Path::new(&file.source_path).exists()
        })
        .cloned()
        .collect();
    let mut existing: HashSet<_> = files
        .iter()
        .map(|file| file.relative_path.replace('\\', "/").to_lowercase())
        .collect();
    // Some ZIPs omit explicit parent-directory entries. Empty implicit directories
    // left after retiring an inner archive still belong to the immutable parent tree.
    for file in &original {
        for parent in paths::relative(&file.relative_path)?
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            let path = sealed.directory.join(parent);
            let relative = paths::path_string(parent);
            let key = relative.replace('\\', "/").to_lowercase();
            if path.is_dir() && existing.insert(key) {
                files.push(CopyFile {
                    size_known: true,
                    source_path: paths::path_string(&path),
                    relative_path: relative,
                    size: 0,
                    sha256: String::new(),
                    is_directory: true,
                    is_corrupted: false,
                });
            }
        }
    }
    Ok(files)
}
pub(super) fn direct_manifest(sealed: &SealedNode, files: &[CopyFile]) -> bool {
    sealed_copy_files(sealed)
        .ok()
        .is_some_and(|original| paths::digest(&original).ok() == paths::digest(&files).ok())
}
pub(super) fn validate_saved_paths(work: &Path, state: &SavedRun) -> Result<(), String> {
    let inside = |path: &Path| -> Result<(), String> {
        paths::check_path(path)?;
        let r = path.strip_prefix(work).map_err(|_| "UnsafePath")?;
        paths::relative(&paths::path_string(r))?;
        Ok(())
    };
    for sealed in &state.sealed {
        inside(&sealed.directory)?;
        validate_manifest(&sealed_copy_files(sealed)?)?;
    }
    for file in state.final_files.iter().chain(&state.input_files) {
        inside(Path::new(&file.source_path))?;
        paths::relative(&file.relative_path)?;
    }
    if !state.input_files.is_empty() {
        validate_manifest(&state.input_files)?;
        let input = work.join("input");
        for file in &state.input_files {
            if file.is_directory
                || Path::new(&file.source_path) != input.join(paths::relative(&file.relative_path)?)
            {
                return Err("工作区输入缓存记录不匹配，已停止。".into());
            }
        }
    }
    if state.input_retired {
        let cached_volumes: Vec<_> = state
            .input_files
            .iter()
            .map(|file| file.source_path.clone())
            .collect();
        if cached_volumes.is_empty()
            || !state
                .sealed
                .iter()
                .any(|sealed| sealed.node.depth == 0 && sealed.node.group.volumes == cached_volumes)
            || state
                .pending
                .iter()
                .chain(state.failed.iter().map(|(node, _)| node))
                .any(|node| node.depth == 0)
        {
            return Err("工作区输入缓存释放记录不匹配，已停止。".into());
        }
    }
    validate_manifest(&state.final_files)?;
    for retired in &state.retired_files {
        if !state.sealed.iter().any(|sealed| {
            sealed.node.depth > 0
                && sealed
                    .node
                    .group
                    .volumes
                    .iter()
                    .any(|volume| volume.to_lowercase() == *retired)
                && state.sealed.iter().any(|parent| {
                    sealed.node.parent.as_ref() == Some(&parent.node.id)
                        && sealed_copy_files(parent).is_ok_and(|files| {
                            files.iter().any(|file| {
                                !file.is_directory && file.source_path.to_lowercase() == *retired
                            })
                        })
                })
        }) || state
            .final_files
            .iter()
            .any(|file| file.source_path.to_lowercase() == *retired)
        {
            return Err("工作区中间文件释放记录不匹配，已停止。".into());
        }
    }
    for node in state
        .pending
        .iter()
        .chain(state.failed.iter().map(|(node, _)| node))
    {
        if !node.prefix.as_os_str().is_empty() {
            paths::relative(&paths::path_string(&node.prefix))?;
        }
        if node.depth > 0 {
            for volume in &node.group.volumes {
                inside(Path::new(volume))?;
            }
        } else if node.group.volumes != state.source.volumes
            && node.group.volumes
                != state
                    .input_files
                    .iter()
                    .map(|f| f.source_path.clone())
                    .collect::<Vec<_>>()
        {
            return Err("UnsafePath".into());
        }
    }
    Ok(())
}
