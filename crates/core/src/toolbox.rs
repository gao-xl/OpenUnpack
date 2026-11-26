//! 文件工具箱：批量重命名、归档清洗预览/执行与目录安全校验。

use crate::paths::{
    DirectoryGuard, LockedEntry, OWNER_FILE, check_path, relative, strip_verbatim_prefix,
};
use openunpack_protocol::{
    ArchiveExtensionFixItem, ConflictStrategy, RenamePatternRule, ToolboxAction,
    ToolboxExecuteRequest, ToolboxExecuteResult, ToolboxItemPlan, ToolboxKind, ToolboxOptions,
    ToolboxPreviewResult,
};
use regex::RegexBuilder;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use walkdir::WalkDir;
use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_NOT_SAME_DEVICE,
};

static ISSUED_TOKENS: OnceLock<Mutex<HashMap<String, (Instant, String)>>> = OnceLock::new();

fn clean_and_store_token(token: String, digest: String) {
    let map = ISSUED_TOKENS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut lock) = map.lock() {
        let now = Instant::now();
        lock.retain(|_, (issued, _)| now.duration_since(*issued) < Duration::from_secs(900));
        lock.insert(token, (now, digest));
    }
}

fn consume_token(token: &str) -> Result<String, String> {
    let map = ISSUED_TOKENS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut lock = map.lock().map_err(|_| "内部令牌锁损坏".to_string())?;
    let (issued, digest) = lock.remove(token).ok_or_else(|| {
        "操作已中止：预览安全令牌无效、已使用或已伪造。请重新预览后再执行。".to_string()
    })?;
    if issued.elapsed() > Duration::from_secs(900) {
        return Err("操作已中止：预览安全令牌已过期（超过 15 分钟）。请重新预览。".into());
    }
    Ok(digest)
}

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x00000400;
const MAX_TOOLBOX_FILES: usize = 50_000;
const MAX_TOOLBOX_DEPTH: usize = 64;
const MAX_RULE_SEARCH_LEN: usize = 1024;
const MAX_RULE_REPLACE_LEN: usize = 1024;
const MAX_RULE_AFFIX_LEN: usize = 256;
const MAX_TOOLBOX_ENTRIES: usize = 100_000;
const TOOLBOX_TIME_LIMIT: Duration = Duration::from_secs(30);

fn check_toolbox_budget(started: Instant, entries: usize, depth: usize) -> Result<(), String> {
    if entries > MAX_TOOLBOX_ENTRIES
        || depth > MAX_TOOLBOX_DEPTH
        || started.elapsed() > TOOLBOX_TIME_LIMIT
    {
        return Err("工具箱操作超出条目、目录深度或时间限制，请分批处理子目录。".into());
    }
    Ok(())
}

fn cleanup_directories(root: &Path, started: Instant) -> Result<Vec<LockedEntry>, String> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let root_guard = DirectoryGuard::open(root).map_err(|_| "无法固定清理目录。")?;
    let mut pending = vec![(root_guard, 0usize)];
    let mut directories = Vec::new();
    let mut entries = 0;
    while let Some((parent, depth)) = pending.pop() {
        for name in parent
            .entries()
            .map_err(|_| "无法完整检查清理目录，已保留空目录。")?
        {
            entries += 1;
            check_toolbox_budget(started, entries, depth + 1)?;
            let path = parent.path().join(name);
            let file = parent
                .open_file_with_share(
                    &path,
                    FILE_READ_ATTRIBUTES,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                )
                .map_err(|_| "无法检查清理目录项。")?;
            let is_directory = file
                .metadata()
                .map_err(|_| "无法检查清理目录项。")?
                .is_dir();
            drop(file);
            if is_directory {
                let directory = parent
                    .lock_entry(&path, false)
                    .map_err(|_| "无法固定清理目录项。")?;
                if directory
                    .metadata()
                    .map_err(|_| "无法检查清理目录项。")?
                    .is_dir()
                {
                    pending.push((
                        directory
                            .directory_guard()
                            .map_err(|_| "无法固定清理目录项。")?,
                        depth + 1,
                    ));
                    directories.push(directory);
                }
            }
        }
    }
    directories.sort_by_key(|entry| std::cmp::Reverse(entry.path().components().count()));
    Ok(directories)
}

fn normalize_components(path: &Path) -> Vec<String> {
    let clean = strip_verbatim_prefix(path);
    clean
        .components()
        .map(|c| {
            c.as_os_str()
                .to_string_lossy()
                .trim_end_matches(['/', '\\'])
                .to_ascii_lowercase()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn path_is_same_or_descendant(path: &Path, protected_base: &Path) -> bool {
    let p_comps = normalize_components(path);
    let b_comps = normalize_components(protected_base);
    if b_comps.is_empty() || p_comps.len() < b_comps.len() {
        return false;
    }
    p_comps[..b_comps.len()] == b_comps[..]
}

/// Validates that a directory is safe for Toolbox operations.
pub fn validate_toolbox_directory(dir: &Path) -> Result<(), String> {
    check_path(dir)?;
    if !dir.exists() || !dir.is_dir() {
        return Err(format!("目录不存在或不是有效文件夹: {}", dir.display()));
    }

    let clean = strip_verbatim_prefix(dir);
    if clean.parent().is_none() || clean.components().count() <= 1 {
        return Err("安全限制：禁止直接对磁盘根目录执行批量整理操作。".into());
    }

    let comps = normalize_components(dir);
    // 1. Check volume-level sensitive directories on any drive:
    for comp in &comps {
        if comp == "system volume information" || comp == "$recycle.bin" {
            return Err("安全限制：禁止对系统卷或回收站目录执行批量整理操作。".into());
        }
    }

    // 2. Dynamic known system & program directories from OS environment / Known Folders:
    let mut protected_dirs: Vec<PathBuf> = Vec::new();

    if let Some(val) = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("windir")) {
        protected_dirs.push(PathBuf::from(val));
    } else {
        protected_dirs.push(PathBuf::from(r"C:\Windows"));
    }

    if let Some(val) = std::env::var_os("ProgramFiles") {
        protected_dirs.push(PathBuf::from(val));
    }
    if let Some(val) = std::env::var_os("ProgramFiles(x86)") {
        protected_dirs.push(PathBuf::from(val));
    }
    if let Some(val) = std::env::var_os("ProgramW6432") {
        protected_dirs.push(PathBuf::from(val));
    }
    if let Some(val) = std::env::var_os("ProgramData") {
        protected_dirs.push(PathBuf::from(val));
    }

    // App data and runtime directories:
    if let Some(val) = std::env::var_os("LOCALAPPDATA") {
        let p = PathBuf::from(val);
        protected_dirs.push(p.join("OpenUnpack"));
        protected_dirs.push(p.join("Programs").join("OpenUnpack"));
    }
    if let Some(val) = std::env::var_os("APPDATA") {
        let p = PathBuf::from(val);
        protected_dirs.push(p.join("OpenUnpack"));
    }

    // Direct user profile root prohibition (e.g. C:\Users\Alice directly):
    if let Some(val) = std::env::var_os("USERPROFILE") {
        let up = PathBuf::from(val);
        if comps == normalize_components(&up) {
            return Err("安全限制：禁止直接对用户主目录根路径执行批量整理操作。".into());
        }
    }

    for prot in &protected_dirs {
        if path_is_same_or_descendant(dir, prot) {
            return Err("安全限制：禁止对系统关键目录或应用数据目录执行批量整理操作。".into());
        }
        if let Ok(canon_prot) = fs::canonicalize(prot)
            && path_is_same_or_descendant(dir, &canon_prot)
        {
            return Err("安全限制：禁止对系统关键目录或应用数据目录执行批量整理操作。".into());
        }
    }

    Ok(())
}

fn is_reparse_or_symlink(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            (m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 || m.file_type().is_symlink()
        }
        Err(_) => true,
    }
}

mod naming;
mod rules;

pub use naming::{compute_normalized_name, fix_archive_extensions, split_stem_and_compound_ext};
use rules::*;
pub use rules::{
    CompiledRenameRule, apply_rename_rule, compute_preview_digest, compute_preview_token,
};

/// Generates a dry-run preview of the requested Toolbox operation.
pub fn preview_toolbox(options: &ToolboxOptions) -> Result<ToolboxPreviewResult, String> {
    let started = Instant::now();
    let source_dir = PathBuf::from(&options.source_directory);
    validate_toolbox_directory(&source_dir)?;

    let target_dir = match options.target_directory.as_deref() {
        Some(t) if !t.trim().is_empty() => {
            let td = PathBuf::from(t);
            validate_toolbox_directory(&td)?;
            td
        }
        _ => source_dir.clone(),
    };

    let compiled_rule = options
        .rename_rule
        .as_ref()
        .map(CompiledRenameRule::new)
        .transpose()?;

    let max_depth = match options.kind {
        ToolboxKind::FlattenDirectory => MAX_TOOLBOX_DEPTH,
        _ => {
            if options.recursive {
                MAX_TOOLBOX_DEPTH
            } else {
                1
            }
        }
    };

    let mut files: Vec<(PathBuf, u64)> = Vec::new();
    let mut scanned_entries = 0usize;
    for entry in WalkDir::new(&source_dir)
        .min_depth(1)
        .max_depth(if max_depth == MAX_TOOLBOX_DEPTH {
            max_depth + 1
        } else {
            max_depth
        })
        .follow_links(false)
    {
        scanned_entries += 1;
        check_toolbox_budget(started, scanned_entries, 0)?;
        if files.len() > MAX_TOOLBOX_FILES {
            return Err(format!(
                "目录包含的文件或条目数量超出单次工具箱处理上限 ({MAX_TOOLBOX_FILES})，请分批选择子目录进行处理。"
            ));
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        check_toolbox_budget(started, scanned_entries, entry.depth())?;
        if is_reparse_or_symlink(path) {
            continue;
        }
        if entry.file_type().is_file() {
            if files.len() == MAX_TOOLBOX_FILES {
                return Err("目录文件数量超出单次工具箱处理上限，请分批选择子目录。".into());
            }
            let name = entry.file_name().to_string_lossy();
            if name.eq_ignore_ascii_case(OWNER_FILE) {
                continue;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            files.push((path.to_path_buf(), size));
        }
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));

    let total_scanned = files.len();
    let mut plans: Vec<ToolboxItemPlan> = Vec::new();
    let mut planned_targets: HashSet<String> = HashSet::new();
    let mut conflicts_count = 0usize;

    let mut seq_index = 0u32;
    for (file_path, size) in files {
        check_toolbox_budget(started, plans.len(), 0)?;
        let file_name = match file_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        let parent_dir = match file_path.parent() {
            Some(p) => p,
            None => continue,
        };
        let rel_path = file_path
            .strip_prefix(&source_dir)
            .unwrap_or(&file_path)
            .to_string_lossy()
            .into_owned();

        let (base_dest_dir, desired_name, base_action) = match options.kind {
            ToolboxKind::NormalizeExtensions => {
                match compute_normalized_name(&file_path, file_name) {
                    Some(new_name) => (parent_dir.to_path_buf(), new_name, ToolboxAction::Rename),
                    None => continue,
                }
            }
            ToolboxKind::BatchRename => {
                let rule = compiled_rule
                    .as_ref()
                    .ok_or_else(|| "批量重命名需要提供重命名规则 (renameRule)。".to_string())?;
                match rule.apply(file_name, seq_index)? {
                    Some(new_name) => {
                        seq_index += 1;
                        (parent_dir.to_path_buf(), new_name, ToolboxAction::Rename)
                    }
                    None => {
                        seq_index += 1;
                        continue;
                    }
                }
            }
            ToolboxKind::FlattenDirectory => {
                if parent_dir == target_dir {
                    continue;
                }
                (
                    target_dir.clone(),
                    file_name.to_string(),
                    ToolboxAction::Move,
                )
            }
        };

        let mut candidate_name = desired_name.clone();
        let mut candidate_path = base_dest_dir.join(&candidate_name);
        let mut has_conflict = false;
        let mut conflict_reason = None;
        let mut action = base_action;

        let is_case_only_same_file = file_path
            .to_string_lossy()
            .eq_ignore_ascii_case(&candidate_path.to_string_lossy());

        let target_exists = !is_case_only_same_file && candidate_path.exists();
        let target_in_planned =
            planned_targets.contains(&candidate_path.to_string_lossy().to_ascii_lowercase());

        if target_exists || target_in_planned {
            has_conflict = true;
            conflicts_count += 1;
            match options.conflict_strategy {
                ConflictStrategy::Rename => {
                    let (stem, ext) = split_stem_and_compound_ext(&desired_name);
                    let mut counter = 1u32;
                    loop {
                        check_toolbox_budget(started, counter as usize, 0)?;
                        let next_name = format!("{stem}_{counter}{ext}");
                        let next_path = base_dest_dir.join(&next_name);
                        let next_key = next_path.to_string_lossy().to_ascii_lowercase();
                        if !next_path.exists() && !planned_targets.contains(&next_key) {
                            candidate_name = next_name;
                            candidate_path = next_path;
                            conflict_reason = Some(format!(
                                "目标存在同名文件 '{}'，将自动重命名为 '{}'",
                                desired_name, candidate_name
                            ));
                            break;
                        }
                        counter += 1;
                    }
                }
                ConflictStrategy::Skip => {
                    action = ToolboxAction::Skip;
                    conflict_reason = Some(format!("目标已存在 '{}'，按策略跳过", desired_name));
                }
                ConflictStrategy::Error => {
                    conflict_reason = Some(format!("目标已存在同名文件冲突: '{}'", desired_name));
                }
            }
        }

        if action != ToolboxAction::Skip {
            planned_targets.insert(candidate_path.to_string_lossy().to_ascii_lowercase());
        }

        plans.push(ToolboxItemPlan {
            source_path: file_path.to_string_lossy().into_owned(),
            relative_path: rel_path,
            proposed_name: candidate_name,
            proposed_path: candidate_path.to_string_lossy().into_owned(),
            size,
            action,
            has_conflict,
            conflict_reason,
        });
    }

    let digest = compute_preview_digest(options, &plans);
    let token = format!("{}-{}", uuid::Uuid::new_v4().simple(), &digest[..16]);
    clean_and_store_token(token.clone(), digest);

    Ok(ToolboxPreviewResult {
        total_scanned,
        total_planned: plans.len(),
        conflicts_count,
        plans,
        token,
    })
}

/// Executes a confirmed Toolbox operation after re-validating the plan.
pub fn execute_toolbox(request: &ToolboxExecuteRequest) -> Result<ToolboxExecuteResult, String> {
    let started = Instant::now();
    let token = match request.token.as_deref() {
        Some(t) if !t.trim().is_empty() => t.trim(),
        _ => {
            return Err(
                "操作已中止：执行请求未提供有效的预览安全令牌。请重新预览后再执行。".into(),
            );
        }
    };

    let expected_digest = consume_token(token)?;

    let approved_paths = match request.approved_paths.as_ref() {
        Some(paths) if !paths.is_empty() => paths,
        _ => return Err("操作已中止：未选择任何已批准的执行文件。".into()),
    };
    if approved_paths.len() > MAX_TOOLBOX_FILES {
        return Err("所选文件超出单次工具箱处理上限。".into());
    }
    let approved_set: HashSet<String> = approved_paths.iter().cloned().collect();

    let preview = preview_toolbox(&request.options)?;
    // Pin the selected objects before revalidating the preview. The digest
    // includes their file IDs, and the same handles perform every later move.
    let mut locked_sources = HashMap::new();
    for plan in &preview.plans {
        if approved_set.contains(&plan.source_path) && plan.action != ToolboxAction::Skip {
            check_toolbox_budget(started, locked_sources.len(), 0)?;
            let source = LockedEntry::open(Path::new(&plan.source_path), true)
                .map_err(|e| format!("无法固定已选择的源文件，操作未开始: {e}"))?;
            locked_sources.insert(plan.source_path.clone(), source);
        }
    }
    let current_digest = compute_preview_digest(&request.options, &preview.plans);
    if current_digest != expected_digest {
        return Err(
            "操作已中止：操作选项、文件版本或目标文件夹内容已改变，原预览已失效。请重新预览。"
                .into(),
        );
    }

    let selected_plans: Vec<&ToolboxItemPlan> = preview
        .plans
        .iter()
        .filter(|p| approved_set.contains(&p.source_path))
        .collect();

    if selected_plans.is_empty() {
        return Err("操作已中止：所选文件均未匹配当前预览计划。".into());
    }

    if request.options.conflict_strategy == ConflictStrategy::Error
        && selected_plans.iter().any(|p| p.has_conflict)
    {
        return Err(
            "存在未解决的同名文件冲突，操作已中止。请切换冲突策略为“自动编号”或“跳过”。".into(),
        );
    }

    let mut success_count = 0usize;
    let mut skipped_count = 0usize;
    let mut failed_count = 0usize;
    let mut errors = Vec::new();

    for plan in selected_plans {
        if let Err(error) =
            check_toolbox_budget(started, success_count + skipped_count + failed_count, 0)
        {
            failed_count += 1;
            errors.push(error);
            break;
        }
        if plan.action == ToolboxAction::Skip {
            skipped_count += 1;
            continue;
        }
        let src = Path::new(&plan.source_path);
        let dst = Path::new(&plan.proposed_path);

        if let Err(e) = check_path(src).and_then(|_| check_path(dst)) {
            failed_count += 1;
            errors.push(format!("{}: {}", plan.relative_path, e));
            continue;
        }

        let is_case_only = src
            .to_string_lossy()
            .eq_ignore_ascii_case(&dst.to_string_lossy());

        let source = locked_sources
            .remove(&plan.source_path)
            .ok_or("所选文件身份已失效，请重新预览。")?;
        match safe_move_locked_file(src, dst, is_case_only, source) {
            Ok(()) => success_count += 1,
            Err(e) => {
                failed_count += 1;
                errors.push(format!("{}: {}", plan.relative_path, e));
            }
        }
    }

    drop(locked_sources);
    let mut cleaned_folders_count = 0usize;
    if request.options.remove_empty_folders {
        let source_dir = PathBuf::from(&request.options.source_directory);
        let target_dir = request
            .options
            .target_directory
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| source_dir.clone());

        let directories = match cleanup_directories(&source_dir, started) {
            Ok(directories) => directories,
            Err(error) => {
                failed_count += 1;
                errors.push(error);
                Vec::new()
            }
        };
        for directory in directories {
            if let Err(error) = check_toolbox_budget(started, cleaned_folders_count, 0) {
                failed_count += 1;
                errors.push(error);
                break;
            }
            if directory.path() == source_dir || directory.path() == target_dir {
                continue;
            }
            let guard = directory
                .directory_guard()
                .map_err(|_| "无法固定待清理目录。")?;
            if guard
                .entries()
                .map_err(|_| "无法检查待清理目录。")?
                .is_empty()
                && directory.delete().is_ok()
            {
                cleaned_folders_count += 1;
            }
        }
    }

    Ok(ToolboxExecuteResult {
        success_count,
        skipped_count,
        failed_count,
        cleaned_folders_count,
        errors,
    })
}

#[cfg(test)]
mod tests;
