//! 工具箱重命名规则编译、预览签名令牌校验与原子移动执行。

use super::*;

/// Pre-compiled and length-validated rename pattern rule.
pub struct CompiledRenameRule<'a> {
    pub rule: &'a RenamePatternRule,
    pub regex: Option<regex::Regex>,
}

impl<'a> CompiledRenameRule<'a> {
    pub fn new(rule: &'a RenamePatternRule) -> Result<Self, String> {
        if rule.search.len() > MAX_RULE_SEARCH_LEN
            || rule.replace.len() > MAX_RULE_REPLACE_LEN
            || rule.prefix.as_ref().map_or(0, |s| s.len()) > MAX_RULE_AFFIX_LEN
            || rule.suffix.as_ref().map_or(0, |s| s.len()) > MAX_RULE_AFFIX_LEN
        {
            return Err("重命名规则模式超出安全长度限制。".into());
        }

        let regex = if !rule.search.is_empty() {
            if rule.use_regex {
                let re = RegexBuilder::new(&rule.search)
                    .case_insensitive(!rule.case_sensitive)
                    .size_limit(1024 * 1024)
                    .build()
                    .map_err(|e| format!("正则表达式无效: {e}"))?;
                Some(re)
            } else if !rule.case_sensitive {
                let re = RegexBuilder::new(&regex::escape(&rule.search))
                    .case_insensitive(true)
                    .size_limit(1024 * 1024)
                    .build()
                    .map_err(|e| format!("搜索模式无效: {e}"))?;
                Some(re)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self { rule, regex })
    }

    pub fn apply(&self, file_name: &str, sequence_index: u32) -> Result<Option<String>, String> {
        let (stem, ext) = split_stem_and_compound_ext(file_name);
        let mut new_stem = stem.to_string();

        if !self.rule.search.is_empty() {
            if let Some(ref re) = self.regex {
                new_stem = re
                    .replace_all(&new_stem, self.rule.replace.as_str())
                    .into_owned();
            } else if self.rule.case_sensitive {
                new_stem = new_stem.replace(&self.rule.search, &self.rule.replace);
            }
        }

        if let Some(ref prefix) = self.rule.prefix
            && !prefix.is_empty()
        {
            new_stem = format!("{prefix}{new_stem}");
        }

        if let Some(ref suffix) = self.rule.suffix
            && !suffix.is_empty()
        {
            new_stem = format!("{new_stem}{suffix}");
        }

        if let Some(start) = self.rule.numbering_start {
            let digits = self.rule.numbering_digits.unwrap_or(3).clamp(1, 8) as usize;
            let num = start.saturating_add(sequence_index);
            if new_stem.is_empty() {
                new_stem = format!("{num:0digits$}");
            } else {
                new_stem = format!("{new_stem}_{num:0digits$}");
            }
        }

        let trimmed_stem = new_stem.trim();
        if trimmed_stem.is_empty() {
            return Err(format!("文件 '{file_name}' 重命名后主文件名为空"));
        }

        let candidate = format!("{trimmed_stem}{ext}");
        relative(&candidate)
            .map_err(|_| format!("重命名结果 '{candidate}' 含非法文件名字符或受限设备名"))?;

        if candidate == file_name {
            Ok(None)
        } else {
            Ok(Some(candidate))
        }
    }
}

/// Applies `RenamePatternRule` to a single file name.
pub fn apply_rename_rule(
    file_name: &str,
    rule: &RenamePatternRule,
    sequence_index: u32,
) -> Result<Option<String>, String> {
    CompiledRenameRule::new(rule)?.apply(file_name, sequence_index)
}

/// Computes a tamper-proof SHA-256 digest binding options, plans, and source file metadata versions.
pub fn compute_preview_digest(options: &ToolboxOptions, plans: &[ToolboxItemPlan]) -> String {
    let mut hasher = Sha256::new();
    let opts_bytes = serde_json::to_vec(options).unwrap_or_default();
    hasher.update(&opts_bytes);
    hasher.update((plans.len() as u64).to_le_bytes());
    for plan in plans {
        hasher.update(plan.source_path.as_bytes());
        hasher.update(b"\0");
        hasher.update(plan.proposed_path.as_bytes());
        hasher.update(b"\0");
        hasher.update(plan.size.to_le_bytes());
        hasher.update([plan.action as u8]);
        let identity = (|| -> std::io::Result<_> {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
                FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
            };
            let path = Path::new(&plan.source_path);
            let parent = DirectoryGuard::open(
                path.parent()
                    .ok_or_else(|| std::io::Error::other("missing parent"))?,
            )?;
            let file = parent.open_file_with_share(
                path,
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            )?;
            let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) }
                == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok((file.metadata()?, information))
        })();
        if let Ok((meta, identity)) = identity {
            hasher.update([1]);
            hasher.update(meta.last_write_time().to_le_bytes());
            hasher.update(meta.creation_time().to_le_bytes());
            hasher.update(identity.dwVolumeSerialNumber.to_le_bytes());
            hasher.update(identity.nFileIndexHigh.to_le_bytes());
            hasher.update(identity.nFileIndexLow.to_le_bytes());
        } else {
            hasher.update([0]);
        }
    }
    hex::encode(hasher.finalize())
}

/// Backwards-compatible alias for computing preview tokens / digests.
pub fn compute_preview_token(options: &ToolboxOptions, plans: &[ToolboxItemPlan]) -> String {
    compute_preview_digest(options, plans)
}

/// Safely moves a file without replacing existing files, handling same-volume and cross-volume cases.
pub(super) fn safe_move_file(src: &Path, dst: &Path, is_case_only: bool) -> Result<(), String> {
    let source =
        LockedEntry::open(src, true).map_err(|e| format!("无法固定源文件及其目录: {e}"))?;
    safe_move_locked_file(src, dst, is_case_only, source)
}

pub(super) fn safe_move_locked_file(
    src: &Path,
    dst: &Path,
    is_case_only: bool,
    mut source: LockedEntry,
) -> Result<(), String> {
    let destination = DirectoryGuard::open(dst.parent().ok_or("目标父目录不存在")?)
        .map_err(|e| format!("无法固定目标目录: {e}"))?;
    if !source.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("安全限制：来源必须是普通文件。".into());
    }
    if is_case_only {
        let temporary =
            src.with_file_name(format!(".openunpack_tmp_{}", uuid::Uuid::new_v4().simple()));
        source
            .rename_no_replace(&destination, &temporary)
            .map_err(|e| format!("临时重命名失败: {e}"))?;
        if let Err(error) = source.rename_no_replace(&destination, dst) {
            let rollback = source.rename_no_replace(&destination, src);
            return Err(match rollback {
                Ok(()) => format!("大小写重命名失败，已恢复源名称: {error}"),
                Err(restore) => format!(
                    "大小写重命名失败: {error}；恢复失败: {restore}；文件保留于 {}",
                    temporary.display()
                ),
            });
        }
        return Ok(());
    }
    let error = match source.rename_no_replace(&destination, dst) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    if matches!(error.raw_os_error(), Some(code) if code == ERROR_FILE_EXISTS as i32 || code == ERROR_ALREADY_EXISTS as i32)
    {
        return Err("目标文件已存在，拒绝覆盖".into());
    }
    if error.raw_os_error() != Some(ERROR_NOT_SAME_DEVICE as i32) {
        return Err(format!("移动文件失败: {error}"));
    }
    // Both namespaces and the same source object remain fixed throughout copy.
    // The newly created destination object never passes through a close/reopen gap.
    let temporary = dst.with_file_name(format!(
        ".openunpack_tmp_xvol_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let mut target = destination
        .create_file(&temporary)
        .map_err(|e| format!("无法创建跨卷临时文件: {e}"))?;
    let transfer = std::io::copy(&mut source.file(), &mut target.file())
        .and_then(|_| target.file().sync_all());
    if let Err(error) = transfer {
        let cleanup = target.delete();
        return Err(format!(
            "跨卷复制或刷新失败: {error}；临时清理: {cleanup:?}"
        ));
    }
    if let Err(error) = target.rename_no_replace(&destination, dst) {
        let cleanup = target.delete();
        return Err(format!("跨卷提交失败: {error}；临时清理: {cleanup:?}"));
    }
    source
        .delete()
        .map_err(|e| format!("传输完成但清理源文件失败，目标已保留: {e}"))
}
