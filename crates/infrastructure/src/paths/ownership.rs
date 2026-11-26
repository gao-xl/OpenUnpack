//! 解压缓存/工作区所有权标记（Owner）与受控退场删除（Retirement）。

use super::*;

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Owner {
    pub(super) application: String,
    pub(super) id: String,
    pub(super) manifest: String,
}
pub fn mark_owned(path: &Path, id: &str, manifest: &str) -> Result<(), String> {
    let parent = DirectoryGuard::open(path.parent().ok_or("没有父目录。")?)
        .map_err(|_| "无法锁定工作父目录。")?;
    let guard = parent
        .create_private_child(path)
        .map_err(|_| "无法创建独占工作目录。")?;
    atomic_json_guarded(
        &guard,
        &path.join(OWNER_FILE),
        &Owner {
            application: "OpenUnpack/1".into(),
            id: id.into(),
            manifest: manifest.into(),
        },
        false,
    )
}
/// Attach publication ownership to an already validated extraction directory.
/// Archive entries are prohibited from using OWNER_FILE, so this never overwrites
/// a user-created marker or changes ownership of an existing output folder.
pub fn tag_existing(path: &Path, id: &str, manifest: &str) -> Result<(), String> {
    let guard = DirectoryGuard::open(path).map_err(|_| "发布目录不能取得独占标记。")?;
    atomic_json_guarded(
        &guard,
        &path.join(OWNER_FILE),
        &Owner {
            application: "OpenUnpack/1".into(),
            id: id.into(),
            manifest: manifest.into(),
        },
        false,
    )
}
pub fn owned(path: &Path, id: &str, manifest: Option<&str>) -> Result<(), String> {
    let guard = DirectoryGuard::open(path).map_err(|_| "工作目录不可安全检查。")?;
    let file = guard
        .open_read_file(Path::new(OWNER_FILE))
        .map_err(|_| "工作目录缺少独占标记。")?;
    verify_owner_file(&file, id, manifest)
}

pub(super) fn verify_owner_file(
    file: &fs::File,
    id: &str,
    manifest: Option<&str>,
) -> Result<(), String> {
    let o = read_owner_file(file)?;
    if o.application != "OpenUnpack/1" || o.id != id || manifest.is_some_and(|m| m != o.manifest) {
        return Err("工作目录标记不匹配，已停止清理。".into());
    }
    Ok(())
}

pub(super) fn read_owner_file(file: &fs::File) -> Result<Owner, String> {
    if file.metadata().map_err(|_| "工作目录标记不可读。")?.len() > 16 * 1024 * 1024 {
        return Err("工作目录标记过大。".into());
    }
    let length = file.metadata().map_err(|_| "工作目录标记不可读。")?.len() as usize;
    let mut bytes = vec![0; length];
    let mut offset = 0;
    while offset < bytes.len() {
        let count = file
            .seek_read(&mut bytes[offset..], offset as u64)
            .map_err(|_| "工作目录标记不可读。")?;
        if count == 0 {
            return Err("工作目录标记损坏。".into());
        }
        offset += count;
    }
    serde_json::from_slice(&bytes).map_err(|_| "工作目录标记损坏。".into())
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetirementError {
    pub message: String,
    pub retryable: bool,
}

impl RetirementError {
    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }

    pub fn terminal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for RetirementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RetirementError {}

impl From<String> for RetirementError {
    fn from(s: String) -> Self {
        let retryable = transient_retirement(&s);
        Self {
            message: s,
            retryable,
        }
    }
}

impl From<&str> for RetirementError {
    fn from(s: &str) -> Self {
        let retryable = transient_retirement(s);
        Self {
            message: s.to_string(),
            retryable,
        }
    }
}

pub fn remove_owned(path: &Path, id: &str) -> Result<(), String> {
    remove_owned_with_hook(path, id, |_| {}).map_err(|err| err.message)
}

/// Post-delivery retirement runs after the cache lease is released, so the next
/// quota waiter is admitted while this workspace is still being torn down. That
/// leaves a short window where a sibling, the Worker or an on-access scanner can
/// hold a handle or a delete-pending child. Retry only those transient reasons;
/// ownership markers, private-ACL refusals, unsafe namespaces and unconfirmed
/// Worker exits remain terminal so a retry can never publish over a foreign tree.
pub fn remove_owned_retrying(path: &Path, id: &str) -> Result<(), String> {
    retry_retirement(|| remove_owned_with_hook(path, id, |_| {}))
}

pub fn retry_retirement(
    mut attempt: impl FnMut() -> Result<(), RetirementError>,
) -> Result<(), String> {
    // Three bounded retries (four attempts total) ride out a handle that is
    // released moments later: a sibling job's walker, the Worker exiting, or an
    // on-access scanner finishing a pass.
    const RETRY_DELAYS_MS: [u64; 3] = [50, 150, 400];
    for delay in RETRY_DELAYS_MS {
        match attempt() {
            Ok(()) => return Ok(()),
            Err(err) if err.retryable => {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            Err(err) => return Err(err.message),
        }
    }
    attempt().map_err(|err| err.message)
}

/// The two "try again later" messages raised by the delete path. Anything else
/// is a durable refusal and must not be looped on.
pub fn transient_retirement(reason: &str) -> bool {
    if reason.contains("并发变动")
        || reason.contains("已停止清理")
        || reason.contains("WorkerExitUnconfirmed")
        || reason.contains("缺少独占标记")
    {
        return false;
    }
    if reason.contains("os error 32")
        || reason.contains("os error 33")
        || reason.contains("os error 5")
        || reason.contains("os error 145")
    {
        return true;
    }
    reason.contains("正被占用") || reason.contains("可稍后重试")
}

pub(super) fn remove_owned_with_hook(
    path: &Path,
    id: &str,
    before_root_delete: impl FnOnce(&Path),
) -> Result<(), RetirementError> {
    let mut tree = LockedTree::open(path).map_err(|err| {
        let os = err
            .raw_os_error()
            .map(|code| format!(" (os error {code})"))
            .unwrap_or_default();
        RetirementError::retryable(format!(
            "工作目录清理失败：目录或文件正被占用，或无法确认安全身份{os}。可稍后重试。"
        ))
    })?;
    tree.verify_owned(id, None)
        .map_err(RetirementError::terminal)?;
    {
        let guard = tree.directory_guard().map_err(|err| {
            let os = err
                .raw_os_error()
                .map(|code| format!(" (os error {code})"))
                .unwrap_or_default();
            RetirementError::retryable(format!("工作目录权限无法核对{os}。"))
        })?;
        crate::saferecycle::verify_private_directory(&guard).map_err(|_| {
            RetirementError::terminal("工作目录允许其他账号修改或无法确认权限，已停止清理。")
        })?;
    }
    if tree
        .entries
        .iter()
        .any(|entry| entry.path() == path.join(".openunpack-exit-unconfirmed.json"))
    {
        return Err(RetirementError::terminal("WorkerExitUnconfirmed"));
    }
    let owner_path = path.join(OWNER_FILE);
    let index = tree
        .entries
        .iter()
        .position(|entry| entry.path() == owner_path)
        .ok_or_else(|| RetirementError::terminal("工作目录缺少独占标记。"))?;
    let marker = tree.entries.remove(index);
    let saved_owner = read_owner_file(marker.file()).map_err(RetirementError::terminal)?;
    let retryable_failure = |err: std::io::Error| {
        let os = err
            .raw_os_error()
            .map(|code| format!(" (os error {code})"))
            .unwrap_or_default();
        RetirementError::retryable(format!("工作目录清理失败{os}，可稍后重试。"))
    };
    // All current descendants were locked and checked before the first delete.
    // Pop children first and close their handles before deleting parent objects.
    while let Some(entry) = tree.entries.pop() {
        entry.delete().map_err(retryable_failure)?;
    }
    let remaining = tree
        .root
        .directory_guard()
        .map_err(retryable_failure)?
        .entries()
        .map_err(retryable_failure)?;
    if remaining.len() != 1 || remaining[0] != OWNER_FILE {
        return Err(RetirementError::terminal(
            "工作目录在清理过程中发生并发变动，已停止清理。",
        ));
    }
    marker.delete().map_err(retryable_failure)?;
    drop(marker);
    before_root_delete(path);
    if tree.root.delete().is_err() {
        let root = tree.root.directory_guard().map_err(retryable_failure)?;
        // If the final directory deletion fails (for example a child was
        // inserted concurrently), restore authority in the same pinned root.
        // Never overwrite a newly introduced marker owned by someone else.
        if atomic_json_guarded(&root, &owner_path, &saved_owner, false).is_err() {
            let marker = root.open_read_file(Path::new(OWNER_FILE)).map_err(|_| {
                RetirementError::terminal("清理失败且独占标记无法恢复；目录已保留。")
            })?;
            verify_owner_file(&marker, &saved_owner.id, Some(&saved_owner.manifest))
                .map_err(RetirementError::terminal)?;
        }
        return Err(RetirementError::retryable("工作目录清理失败，可稍后重试。"));
    }
    // Windows only removes the delete-pending workspace after its last handle
    // closes. The tree also pins the parent without FILE_SHARE_DELETE, so it
    // must be released before attempting to prune the empty cache container.
    drop(tree);
    prune_empty_cache_container(path);
    Ok(())
}

/// Jobs share `<cache_root>/OpenUnpack-cache` as their parent, so the container
/// outlives every workspace and would linger as an empty directory. Removing it
/// only succeeds while it is empty, which keeps concurrent jobs unaffected.
pub(super) fn prune_empty_cache_container(retired: &Path) {
    let Some(parent) = retired.parent() else {
        return;
    };
    if parent.file_name().and_then(|name| name.to_str()) != Some("OpenUnpack-cache") {
        return;
    }
    let _ = std::fs::remove_dir(parent);
}
/// Candidate directories do not each carry an ownership marker. Their only valid
/// cleanup authority is the enclosing exclusive workspace, with no link traversal.
pub fn remove_inside_owned(path: &Path, workspace: &Path, id: &str) -> Result<(), String> {
    let (parent, _owner) = owned_parent_for_entry(path, workspace, id)?;
    let root = parent.lock_entry(path, false).map_err(|_| "IoError")?;
    validate_metadata(&root.metadata().map_err(|_| "IoError")?, Some(true))
        .map_err(|_| "UnsafePath")?;
    let mut tree = LockedTree::from_root(root).map_err(|_| "IoError")?;
    while let Some(entry) = tree.entries.pop() {
        entry.delete().map_err(|_| "IoError")?;
    }
    tree.root.delete().map_err(|_| "IoError".into())
}
pub fn remove_file_inside_owned(path: &Path, workspace: &Path, id: &str) -> Result<(), String> {
    let (parent, _owner) = owned_parent_for_entry(path, workspace, id)?;
    let entry = parent.lock_entry(path, false).map_err(|_| "IoError")?;
    validate_metadata(&entry.metadata().map_err(|_| "IoError")?, Some(false))
        .map_err(|_| "UnsafePath")?;
    entry.delete().map_err(|_| "IoError".into())
}

pub(super) fn owned_parent_for_entry(
    path: &Path,
    workspace: &Path,
    id: &str,
) -> Result<(DirectoryGuard, fs::File), String> {
    let r = path.strip_prefix(workspace).map_err(|_| "UnsafePath")?;
    relative(&path_string(r))?;
    let mut guard = DirectoryGuard::open(workspace).map_err(|_| "工作目录不可安全锁定。")?;
    crate::saferecycle::verify_private_directory(&guard)
        .map_err(|_| "工作目录允许其他账号修改或无法确认权限，已停止清理。")?;
    let owner = guard
        .open_read_file(Path::new(OWNER_FILE))
        .map_err(|_| "工作目录缺少独占标记。")?;
    verify_owner_file(&owner, id, None)?;
    if guard
        .entries()
        .map_err(|_| "工作目录不可检查。")?
        .iter()
        .any(|name| name == ".openunpack-exit-unconfirmed.json")
    {
        return Err("WorkerExitUnconfirmed".into());
    }
    if let Some(parent) = r.parent() {
        for component in parent.components() {
            if let Component::Normal(name) = component {
                guard = guard
                    .child_directory(Path::new(name), NATIVE_FILE_OPEN)
                    .map_err(|_| "UnsafePath")?;
            } else {
                return Err("UnsafePath".into());
            }
        }
    }
    Ok((guard, owner))
}
