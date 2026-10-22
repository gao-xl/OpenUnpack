use crate::paths::{DirectoryGuard, LockedEntry, OWNER_FILE, check_path, relative};
use openunpack_protocol::{
    ConflictStrategy, RenamePatternRule, ToolboxAction, ToolboxExecuteRequest,
    ToolboxExecuteResult, ToolboxItemPlan, ToolboxKind, ToolboxOptions, ToolboxPreviewResult,
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

fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{stripped}"))
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
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

/// Splits a filename into `(stem, compound_extension)` preserving `.tar.gz`, `.tar.bz2`, etc.
pub fn split_stem_and_compound_ext(file_name: &str) -> (&str, &str) {
    let lower = file_name.to_ascii_lowercase();
    let compound_suffixes = [
        ".tar.gz.tar",
        ".tar.bz2.tar",
        ".tar.xz.tar",
        ".tar.zst.tar",
        ".tar.lz4.tar",
        ".tar.gz",
        ".tar.bz2",
        ".tar.xz",
        ".tar.zst",
        ".tar.lz4",
        ".sfx.exe",
    ];
    for suffix in compound_suffixes {
        if lower.ends_with(suffix) && file_name.len() > suffix.len() {
            let split_at = file_name.len() - suffix.len();
            return (&file_name[..split_at], &file_name[split_at..]);
        }
    }
    if let Some(dot) = file_name.rfind('.')
        && dot > 0
    {
        return (&file_name[..dot], &file_name[dot..]);
    }
    (file_name, "")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetectedArchiveKind {
    Zip,
    SevenZip,
    Rar,
    Gzip,
    Bzip2,
    Xz,
    Zstd,
    Lz4,
    Wim,
    Tar,
}

fn sniff_magic_kind(path: &Path) -> Option<DetectedArchiveKind> {
    let mut file = File::open(path).ok()?;
    let mut buf = [0u8; 264];
    let n = file.read(&mut buf).ok()?;
    let bytes = &buf[..n];

    if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04])
        || bytes.starts_with(&[0x50, 0x4B, 0x05, 0x06])
        || bytes.starts_with(&[0x50, 0x4B, 0x07, 0x08])
    {
        return Some(DetectedArchiveKind::Zip);
    }
    if bytes.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return Some(DetectedArchiveKind::SevenZip);
    }
    if bytes.starts_with(&[0x52, 0x61, 0x72, 0x21, 0x1A, 0x07]) {
        return Some(DetectedArchiveKind::Rar);
    }
    if bytes.starts_with(&[0x1F, 0x8B]) {
        return Some(DetectedArchiveKind::Gzip);
    }
    if bytes.starts_with(b"BZh") {
        return Some(DetectedArchiveKind::Bzip2);
    }
    if bytes.starts_with(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]) {
        return Some(DetectedArchiveKind::Xz);
    }
    if bytes.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        return Some(DetectedArchiveKind::Zstd);
    }
    if bytes.starts_with(&[0x04, 0x22, 0x4D, 0x18]) {
        return Some(DetectedArchiveKind::Lz4);
    }
    if bytes.starts_with(b"MSWIM\0\0\0") || bytes.starts_with(b"WLPWM\0\0\0") {
        return Some(DetectedArchiveKind::Wim);
    }
    if bytes.len() >= 262 && &bytes[257..262] == b"ustar" {
        return Some(DetectedArchiveKind::Tar);
    }
    None
}

/// Computes the normalized filename for `NormalizeExtensions`.
pub fn compute_normalized_name(path: &Path, file_name: &str) -> Option<String> {
    let lower = file_name.to_ascii_lowercase();

    // 1. Explicit broken compound extensions
    let explicit_rewrites: &[(&str, &str)] = &[
        (".tar.gz.tar", ".tar.gz"),
        (".tgz.tar", ".tar.gz"),
        (".tgz", ".tar.gz"),
        (".tar.bz2.tar", ".tar.bz2"),
        (".tbz2.tar", ".tar.bz2"),
        (".tbz2", ".tar.bz2"),
        (".tbz", ".tar.bz2"),
        (".tar.xz.tar", ".tar.xz"),
        (".txz.tar", ".tar.xz"),
        (".txz", ".tar.xz"),
        (".tar.zst.tar", ".tar.zst"),
        (".tzst.tar", ".tar.zst"),
        (".tzst", ".tar.zst"),
        (".tar.lz4.tar", ".tar.lz4"),
    ];
    for &(bad_ext, canonical_ext) in explicit_rewrites {
        if lower.ends_with(bad_ext) && file_name.len() > bad_ext.len() {
            let stem = &file_name[..file_name.len() - bad_ext.len()];
            let candidate = format!("{stem}{canonical_ext}");
            if candidate != file_name {
                return Some(candidate);
            }
        }
    }

    // 2. Magic-byte based normalization for misnamed or disguised archive files
    let (stem, ext) = split_stem_and_compound_ext(file_name);
    let ext_lower = ext.to_ascii_lowercase();

    // Do not touch Office/Java/Android packages that are intentionally ZIP containers
    let preserved_zip_containers = [
        ".docx", ".xlsx", ".pptx", ".odt", ".ods", ".epub", ".jar", ".apk", ".ipa", ".appx",
    ];
    if preserved_zip_containers.contains(&ext_lower.as_str()) {
        return None;
    }
    // Preserve multi-part volumes like .001, .r00, .z01
    if ext_lower.len() >= 2 && ext_lower[1..].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    if let Some(kind) = sniff_magic_kind(path) {
        let desired_ext = match kind {
            DetectedArchiveKind::Zip => ".zip",
            DetectedArchiveKind::SevenZip => ".7z",
            DetectedArchiveKind::Rar => ".rar",
            DetectedArchiveKind::Gzip => {
                if ext_lower == ".tar"
                    || ext_lower == ".tar.gz"
                    || stem.to_ascii_lowercase().ends_with(".tar")
                {
                    ".tar.gz"
                } else {
                    ".gz"
                }
            }
            DetectedArchiveKind::Bzip2 => {
                if ext_lower == ".tar"
                    || ext_lower == ".tar.bz2"
                    || stem.to_ascii_lowercase().ends_with(".tar")
                {
                    ".tar.bz2"
                } else {
                    ".bz2"
                }
            }
            DetectedArchiveKind::Xz => {
                if ext_lower == ".tar"
                    || ext_lower == ".tar.xz"
                    || stem.to_ascii_lowercase().ends_with(".tar")
                {
                    ".tar.xz"
                } else {
                    ".xz"
                }
            }
            DetectedArchiveKind::Zstd => {
                if ext_lower == ".tar"
                    || ext_lower == ".tar.zst"
                    || stem.to_ascii_lowercase().ends_with(".tar")
                {
                    ".tar.zst"
                } else {
                    ".zst"
                }
            }
            DetectedArchiveKind::Lz4 => {
                if ext_lower == ".tar"
                    || ext_lower == ".tar.lz4"
                    || stem.to_ascii_lowercase().ends_with(".tar")
                {
                    ".tar.lz4"
                } else {
                    ".lz4"
                }
            }
            DetectedArchiveKind::Wim => {
                if ext_lower == ".swm" || ext_lower == ".esd" {
                    ext_lower.as_str()
                } else {
                    ".wim"
                }
            }
            DetectedArchiveKind::Tar => ".tar",
        };

        // Handle case where stem itself ended with `.tar` and desired_ext is `.tar.gz` etc.
        let clean_stem =
            if desired_ext.starts_with(".tar.") && stem.to_ascii_lowercase().ends_with(".tar") {
                &stem[..stem.len() - 4]
            } else {
                stem
            };

        let candidate = format!("{clean_stem}{desired_ext}");
        if candidate != file_name {
            return Some(candidate);
        }
    }

    // 3. Uppercase archive extension normalization even if empty or non-magic
    let archive_exts = [
        ".zip", ".7z", ".rar", ".tar", ".gz", ".bz2", ".xz", ".zst", ".lz4", ".wim", ".lzh",
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.lz4",
    ];
    if archive_exts.contains(&ext_lower.as_str()) && ext != ext_lower {
        return Some(format!("{stem}{ext_lower}"));
    }

    None
}

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
#[cfg(test)]
fn safe_move_file(src: &Path, dst: &Path, is_case_only: bool) -> Result<(), String> {
    let source =
        LockedEntry::open(src, true).map_err(|e| format!("无法固定源文件及其目录: {e}"))?;
    safe_move_locked_file(src, dst, is_case_only, source)
}

fn safe_move_locked_file(
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
mod tests {
    #[test]
    fn preview_digest_binds_file_identity_even_when_size_and_times_match() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_BASIC_INFO, FileBasicInfo, SetFileInformationByHandle,
        };
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source.txt");
        fs::write(&source, b"same bytes").unwrap();
        let options = ToolboxOptions {
            kind: ToolboxKind::BatchRename,
            source_directory: fixture.path().to_string_lossy().into_owned(),
            target_directory: None,
            recursive: false,
            remove_empty_folders: false,
            conflict_strategy: ConflictStrategy::Error,
            rename_rule: Some(RenamePatternRule {
                search: String::new(),
                replace: String::new(),
                case_sensitive: false,
                use_regex: false,
                prefix: Some("changed-".into()),
                suffix: None,
                numbering_start: None,
                numbering_digits: None,
            }),
        };
        let preview = preview_toolbox(&options).unwrap();
        assert_eq!(preview.plans.len(), 1);
        let before = compute_preview_digest(&options, &preview.plans);
        let metadata = fs::metadata(&source).unwrap();
        fs::rename(&source, fixture.path().join("preserved-original.txt")).unwrap();
        fs::write(&source, b"same bytes").unwrap();
        let replacement = fs::OpenOptions::new().write(true).open(&source).unwrap();
        let information = FILE_BASIC_INFO {
            CreationTime: metadata.creation_time() as i64,
            LastWriteTime: metadata.last_write_time() as i64,
            ..FILE_BASIC_INFO::default()
        };
        assert_ne!(
            unsafe {
                SetFileInformationByHandle(
                    replacement.as_raw_handle() as _,
                    FileBasicInfo,
                    &information as *const _ as _,
                    std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                )
            },
            0
        );
        drop(replacement);
        assert_ne!(before, compute_preview_digest(&options, &preview.plans));
    }
    #[test]
    fn case_only_move_uses_the_same_object_for_both_rename_steps() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("SOURCE.txt");
        let target = fixture.path().join("source.txt");
        fs::write(&source, b"case-preserving content").unwrap();
        safe_move_file(&source, &target, true).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"case-preserving content");
        let names: Vec<_> = fs::read_dir(fixture.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("source.txt")]);
    }
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    #[test]
    #[ignore = "requires an explicitly supplied second writable volume"]
    fn cross_volume_transfer_uses_locked_source_and_preserves_existing_target() {
        let other =
            std::env::var_os("OPENUNPACK_TOOLBOX_SECOND_VOLUME").expect("second volume required");
        let source_dir = tempfile::tempdir().unwrap();
        let target_dir = tempfile::Builder::new()
            .prefix("openunpack-xvol-regression-")
            .tempdir_in(other)
            .unwrap();
        let source = source_dir.path().join("source.bin");
        let target = target_dir.path().join("target.bin");
        std::fs::write(&source, b"owned source payload").unwrap();
        let a: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        assert_eq!(
            unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 0) },
            0,
            "volumes must be distinct"
        );
        assert_eq!(unsafe { GetLastError() }, ERROR_NOT_SAME_DEVICE);
        safe_move_file(&source, &target, false).unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"owned source payload");
        std::fs::write(&source, b"second owned source").unwrap();
        assert!(safe_move_file(&source, &target, false).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"second owned source");
        assert_eq!(std::fs::read(&target).unwrap(), b"owned source payload");
        assert_eq!(
            std::fs::read_dir(target_dir.path()).unwrap().count(),
            1,
            "no stale temporary copies"
        );
    }

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn cleanup_collects_before_mutating_and_rejects_excessive_depth() {
        let dir = tempdir().unwrap();
        let mut path = dir.path().to_path_buf();
        for _ in 0..=MAX_TOOLBOX_DEPTH {
            path.push("d");
            fs::create_dir(&path).unwrap();
        }
        assert!(cleanup_directories(dir.path(), Instant::now()).is_err());
        assert!(
            path.is_dir(),
            "an incomplete cleanup must preserve every directory"
        );
        assert!(check_toolbox_budget(Instant::now(), MAX_TOOLBOX_ENTRIES + 1, 1).is_err());
        assert!(
            check_toolbox_budget(
                Instant::now() - TOOLBOX_TIME_LIMIT - Duration::from_secs(1),
                0,
                1
            )
            .is_err()
        );
    }

    #[test]
    fn test_normalize_extensions_compound_and_magic() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // 1. Broken .tar.gz.tar -> .tar.gz
        let f1 = root.join("release.tar.gz.tar");
        fs::write(&f1, [0x1F, 0x8B, 0x08, 0x00, 0x00]).unwrap();

        // 2. .tgz -> .tar.gz
        let f2 = root.join("packages.tgz");
        fs::write(&f2, [0x1F, 0x8B, 0x08, 0x00, 0x00]).unwrap();

        // 3. Disguised 7z named .bin -> .7z
        let f3 = root.join("backup.bin");
        fs::write(&f3, [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0x00, 0x04]).unwrap();

        // 4. Normal .zip remains untouched
        let f4 = root.join("normal.zip");
        fs::write(&f4, [0x50, 0x4B, 0x03, 0x04, 0x14, 0x00]).unwrap();

        let opts = ToolboxOptions {
            kind: ToolboxKind::NormalizeExtensions,
            source_directory: root.to_string_lossy().into_owned(),
            target_directory: None,
            recursive: false,
            remove_empty_folders: false,
            conflict_strategy: ConflictStrategy::Rename,
            rename_rule: None,
        };

        let preview = preview_toolbox(&opts).unwrap();
        assert_eq!(preview.total_scanned, 4);
        assert_eq!(preview.total_planned, 3);

        let exec = execute_toolbox(&ToolboxExecuteRequest {
            options: opts,
            approved_paths: Some(
                preview
                    .plans
                    .iter()
                    .map(|p| p.source_path.clone())
                    .collect(),
            ),
            token: Some(preview.token),
        })
        .unwrap();
        assert_eq!(exec.success_count, 3);
        assert!(root.join("release.tar.gz").exists());
        assert!(root.join("packages.tar.gz").exists());
        assert!(root.join("backup.7z").exists());
        assert!(root.join("normal.zip").exists());
    }

    #[test]
    fn test_batch_rename_preserves_compound_ext_and_numbers() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join("[raw]_episode_a.tar.gz"), b"data1").unwrap();
        fs::write(root.join("[raw]_episode_b.tar.gz"), b"data2").unwrap();

        let opts = ToolboxOptions {
            kind: ToolboxKind::BatchRename,
            source_directory: root.to_string_lossy().into_owned(),
            target_directory: None,
            recursive: false,
            remove_empty_folders: false,
            conflict_strategy: ConflictStrategy::Rename,
            rename_rule: Some(RenamePatternRule {
                search: "[raw]_".into(),
                replace: "".into(),
                case_sensitive: false,
                use_regex: false,
                prefix: Some("clean_".into()),
                suffix: None,
                numbering_start: Some(1),
                numbering_digits: Some(2),
            }),
        };

        let preview = preview_toolbox(&opts).unwrap();
        assert_eq!(preview.total_planned, 2);
        assert_eq!(preview.plans[0].proposed_name, "clean_episode_a_01.tar.gz");
        assert_eq!(preview.plans[1].proposed_name, "clean_episode_b_02.tar.gz");

        let exec = execute_toolbox(&ToolboxExecuteRequest {
            options: opts,
            approved_paths: Some(
                preview
                    .plans
                    .iter()
                    .map(|p| p.source_path.clone())
                    .collect(),
            ),
            token: Some(preview.token),
        })
        .unwrap();
        assert_eq!(exec.success_count, 2);
        assert!(root.join("clean_episode_a_01.tar.gz").exists());
        assert!(root.join("clean_episode_b_02.tar.gz").exists());
    }

    #[test]
    fn test_flatten_directory_with_conflict_and_empty_folder_cleanup() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let sub1 = root.join("sub1").join("deep");
        let sub2 = root.join("sub2");
        fs::create_dir_all(&sub1).unwrap();
        fs::create_dir_all(&sub2).unwrap();

        fs::write(sub1.join("readme.txt"), b"from_sub1").unwrap();
        fs::write(sub2.join("readme.txt"), b"from_sub2").unwrap();
        fs::write(sub2.join("unique.zip"), b"zip_content").unwrap();

        let opts = ToolboxOptions {
            kind: ToolboxKind::FlattenDirectory,
            source_directory: root.to_string_lossy().into_owned(),
            target_directory: None,
            recursive: true,
            remove_empty_folders: true,
            conflict_strategy: ConflictStrategy::Rename,
            rename_rule: None,
        };

        let preview = preview_toolbox(&opts).unwrap();
        assert_eq!(preview.total_planned, 3);
        assert_eq!(preview.conflicts_count, 1);

        let exec = execute_toolbox(&ToolboxExecuteRequest {
            options: opts,
            approved_paths: Some(
                preview
                    .plans
                    .iter()
                    .map(|p| p.source_path.clone())
                    .collect(),
            ),
            token: Some(preview.token),
        })
        .unwrap();
        assert_eq!(exec.success_count, 3);
        assert!(exec.cleaned_folders_count >= 2, "{exec:?}");
        assert!(root.join("readme.txt").exists());
        assert!(root.join("readme_1.txt").exists());
        assert!(root.join("unique.zip").exists());
        assert!(!root.join("sub1").exists());
        assert!(!root.join("sub2").exists());
    }

    #[test]
    fn test_s04_system_directory_guards_verbatim_and_canonical() {
        let windows =
            PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()));
        assert!(
            validate_toolbox_directory(&windows).is_err(),
            "Windows dir must be rejected"
        );

        if let Ok(canon) = fs::canonicalize(&windows) {
            assert!(
                validate_toolbox_directory(&canon).is_err(),
                "Canonicalized \\\\?\\ Windows dir must be rejected"
            );
        }

        // Windows-backup or similar should NOT be falsely rejected
        let dir = tempdir().unwrap();
        let safe_dir = dir.path().join("Windows-backup");
        fs::create_dir(&safe_dir).unwrap();
        assert!(
            validate_toolbox_directory(&safe_dir).is_ok(),
            "Windows-backup must NOT be rejected"
        );
    }

    #[test]
    fn test_s05_preview_token_binding_and_rejection_on_mismatch() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let file_path = root.join("safe.txt");
        fs::write(&file_path, b"test content").unwrap();

        let mut opts = ToolboxOptions {
            kind: ToolboxKind::BatchRename,
            source_directory: root.to_string_lossy().into_owned(),
            target_directory: None,
            recursive: false,
            remove_empty_folders: false,
            conflict_strategy: ConflictStrategy::Error,
            rename_rule: Some(RenamePatternRule {
                search: String::new(),
                replace: String::new(),
                case_sensitive: false,
                use_regex: false,
                prefix: Some("preview-".into()),
                suffix: None,
                numbering_start: None,
                numbering_digits: None,
            }),
        };

        let preview = preview_toolbox(&opts).unwrap();
        let approved = preview
            .plans
            .iter()
            .map(|p| p.source_path.clone())
            .collect::<Vec<_>>();

        // 1. Missing token -> rejected
        let res_no_token = execute_toolbox(&ToolboxExecuteRequest {
            options: opts.clone(),
            approved_paths: Some(approved.clone()),
            token: None,
        });
        assert!(res_no_token.is_err());

        // 2. Modified options (e.g. prefix changed from preview- to unpreviewed-) -> token mismatch, rejected
        opts.rename_rule.as_mut().unwrap().prefix = Some("unpreviewed-".into());
        let res_mismatched = execute_toolbox(&ToolboxExecuteRequest {
            options: opts.clone(),
            approved_paths: Some(approved.clone()),
            token: Some(preview.token.clone()),
        });
        assert!(
            res_mismatched.is_err(),
            "Must reject execution with mismatched preview token"
        );
        assert!(!root.join("unpreviewed-safe.txt").exists());

        // 3. Re-preview with new options -> valid execution
        let new_preview = preview_toolbox(&opts).unwrap();
        let res_valid = execute_toolbox(&ToolboxExecuteRequest {
            options: opts.clone(),
            approved_paths: Some(approved.clone()),
            token: Some(new_preview.token.clone()),
        });
        assert!(res_valid.is_ok());
        assert!(root.join("unpreviewed-safe.txt").exists());

        // 4. Replay attack: reusing new_preview.token -> rejected because token is single-use
        let res_replay = execute_toolbox(&ToolboxExecuteRequest {
            options: opts.clone(),
            approved_paths: Some(approved.clone()),
            token: Some(new_preview.token.clone()),
        });
        assert!(
            res_replay.is_err(),
            "Replay of consumed token must be rejected"
        );

        // 5. Forged token not generated by preview -> rejected
        let res_forged = execute_toolbox(&ToolboxExecuteRequest {
            options: opts,
            approved_paths: Some(approved),
            token: Some("fake-forged-token-12345".into()),
        });
        assert!(res_forged.is_err(), "Forged token must be rejected");
    }

    #[test]
    fn test_s06_atomic_no_overwrite_rejection() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let f1 = root.join("file1.txt");
        let f2 = root.join("file2.txt");
        fs::write(&f1, b"content 1").unwrap();
        fs::write(&f2, b"content 2").unwrap();

        // Attempting to move file1 to existing file2 with safe_move_file must fail without replacing file2
        let err = safe_move_file(&f1, &f2, false);
        assert!(
            err.is_err(),
            "Must refuse to overwrite existing destination"
        );
        assert_eq!(
            fs::read(&f2).unwrap(),
            b"content 2",
            "Destination content must NOT be overwritten"
        );
        assert!(f1.exists(), "Source must still exist");
    }
}
