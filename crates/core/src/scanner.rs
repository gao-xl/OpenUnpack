//! 压缩包目录扫描、分卷归并及候选密码嗅探。

use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use walkdir::WalkDir;

use crate::types::{ScanNotice, ScanResult, SourceGroup};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

mod magic;
mod passwords;
mod volumes;

pub use magic::is_supported_archive_path;
use magic::*;
pub use passwords::{
    extract_password_from_filename, extract_passwords_from_filename, parse_passwords_from_text,
    sniff_accompanying_passwords, sniff_accompanying_passwords_authorized,
};
use volumes::*;
pub(crate) use volumes::{VolumeType, classify_volume};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x00000400;
// Wire rules are bounded by UTF-8 bytes, matching Domain, UI and Worker.
pub use openunpack_infrastructure::engine::path_safety::MAX_EXCLUSION_PATTERN_BYTES;
/// Hard ceiling on collected archive candidates. A user-selected tree can be
/// arbitrarily large, and an unbounded `Vec<PathBuf>` would exhaust memory long
/// before the UI could show a result.
pub const MAX_SCAN_FILES: usize = 200_000;
/// Ceiling on visited entries, bounding CPU time for trees that are mostly
/// non-archive files (where the candidate cap would never trigger).
pub const MAX_SCAN_ENTRIES: usize = 1_000_000;

static RE_NUMERIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.(?:7z|zip)\.\d+$").expect("valid RE_NUMERIC regex"));
static RE_PART: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.part\d+\.rar$").expect("valid RE_PART regex"));
static RE_OLD_RAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.r\d{2,}$").expect("valid RE_OLD_RAR regex"));
static RE_SPLIT_ZIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.z\d{2,}$").expect("valid RE_SPLIT_ZIP regex"));
static RE_DISGUISED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(?:zip|7z|rar)\.[a-zA-Z0-9_-]+$").expect("valid RE_DISGUISED regex")
});
static RE_MULTI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(?:(?:7z|zip)\.\d+|part\d+\.rar|r\d+|z\d+)$").expect("valid RE_MULTI regex")
});
static RE_CLASSIFY_NUM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?i)(.+)\.(7z|zip)\.(\d+)$").expect("valid RE_CLASSIFY_NUM regex")
});
static RE_CLASSIFY_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?i)(.+)\.part(\d+)\.rar$").expect("valid RE_CLASSIFY_PART regex")
});
static RE_CLASSIFY_OLD_R: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i)(.+)\.r(\d+)$").expect("valid RE_CLASSIFY_OLD_R regex"));
static RE_CLASSIFY_OLD_RAR_BASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i)(.+)\.rar$").expect("valid RE_CLASSIFY_OLD_RAR_BASE regex"));
static RE_CLASSIFY_Z_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i)(.+)\.z(\d+)$").expect("valid RE_CLASSIFY_Z_SPLIT regex"));
static RE_CLASSIFY_ZIP_BASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i)(.+)\.zip$").expect("valid RE_CLASSIFY_ZIP_BASE regex"));
static RE_PASSWORD_EXPLICIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:解压密码|解压码|提取码|提取|密码|(?:^|[^\w])(?:password|passwd|pwd|pass|code))[：:=_\s-]*([^\s\[\]【】()（）{}]+)")
        .expect("valid RE_PASSWORD_EXPLICIT regex")
});
static RE_PASSWORD_BRACKET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\[【({]([^\[\]【】(){}（）]+)[\]】)}]").expect("valid RE_PASSWORD_BRACKET regex")
});
static RE_DOMAIN_IN_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:https?://)?([a-zA-Z0-9-]+\.(?:com|cn|net|org|cc|me|vip|top|xyz|info|io|tv|club|online|site|icu))")
        .expect("valid RE_DOMAIN_IN_NAME regex")
});

pub fn is_archive_candidate_name(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    let single_exts = [
        ".zip", ".7z", ".rar", ".tar", ".gz", ".bz2", ".xz", ".cab", ".wim", ".swm", ".esd",
        ".lzh", ".lha", ".zst", ".zstd", ".tzst", ".lz4", ".sfx",
    ];
    if single_exts.iter().any(|ext| lower.ends_with(ext)) {
        return true;
    }
    RE_MULTI.is_match(&lower)
}

/// Exclusion previews and extraction must agree byte for byte, so the rule
/// engine is re-exported through infrastructure.
pub fn matches_exclusion(relative: &str, patterns: &[String]) -> bool {
    openunpack_infrastructure::engine::path_safety::matches_exclusion(relative, patterns)
}

pub fn output_name(path: &str) -> String {
    let p = Path::new(path);
    let file_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("archive");

    if let Some(mat) = RE_NUMERIC.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    if let Some(mat) = RE_PART.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    if let Some(mat) = RE_OLD_RAR.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    if let Some(mat) = RE_SPLIT_ZIP.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    let exts = [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.lz4", ".sfx.exe", ".zip", ".7z",
        ".rar", ".tar", ".gz", ".bz2", ".xz", ".cab", ".wim", ".swm", ".esd", ".lzh", ".lha",
        ".zst", ".zstd", ".tzst", ".lz4", ".sfx",
    ];
    let lower_name = file_name.to_ascii_lowercase();
    for ext in &exts {
        if lower_name.ends_with(ext) {
            let base = &file_name[..file_name.len() - ext.len()];
            if !base.is_empty() {
                return base.to_string();
            }
        }
    }

    // Secondary disguised extensions like foo.zip.mp4, bar.7z.png, etc.
    if let Some(mat) = RE_DISGUISED.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    if let Some(stem) = p.file_stem().and_then(|s| s.to_str())
        && !stem.is_empty()
    {
        return stem.to_string();
    }
    "archive".to_string()
}

fn push_candidate(files: &mut Vec<PathBuf>, path: PathBuf) -> Result<(), String> {
    if files.len() >= MAX_SCAN_FILES {
        return Err(format!(
            "扫描到的压缩包超过上限（{MAX_SCAN_FILES} 个），请缩小目录范围或增加排除规则。"
        ));
    }
    files.push(path);
    Ok(())
}

pub fn scan(
    paths: &[String],
    recursive: bool,
    exclusions: &[String],
) -> Result<ScanResult, String> {
    scan_with_cancel(paths, recursive, exclusions, &AtomicBool::new(false))
}
pub fn scan_with_cancel(
    paths: &[String],
    recursive: bool,
    exclusions: &[String],
    cancel: &AtomicBool,
) -> Result<ScanResult, String> {
    if exclusions.len() > 100
        || exclusions
            .iter()
            .any(|pattern| pattern.len() > MAX_EXCLUSION_PATTERN_BYTES)
    {
        return Err("排除规则过多或过长：最多 100 条，单条不超过 512 UTF-8 字节。".into());
    }
    let mut notices = Vec::new();
    let mut collected_files = Vec::new();
    let mut seen_canonical = BTreeSet::new();
    let mut scanned = 0usize;

    for p in paths {
        if cancel.load(Ordering::Acquire) {
            return Err("扫描已取消。".into());
        }
        let path = Path::new(p);
        if !path.exists() {
            notices.push(ScanNotice {
                path: p.clone(),
                message: "指定路径不存在".to_string(),
            });
            continue;
        }

        if path.is_file() {
            if is_reparse_point_or_symlink(path) {
                continue;
            }
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if matches_exclusion(file_name, exclusions) {
                continue;
            }
            if !is_supported_archive_path(path, file_name) {
                continue;
            }
            match path.canonicalize() {
                Ok(cpath) => {
                    let key = file_name_key(&cpath.to_string_lossy());
                    if seen_canonical.insert(key) {
                        push_candidate(&mut collected_files, cpath)?;
                    }
                }
                Err(_) => {
                    notices.push(ScanNotice {
                        path: p.clone(),
                        message: "无法访问文件".to_string(),
                    });
                }
            }
            // Selecting any volume authorizes only its sibling family, not unrelated archives.
            if let (Some(family), Some(parent)) = (volume_family(file_name), path.parent())
                && let Ok(siblings) = fs::read_dir(parent)
            {
                for sibling in siblings.flatten() {
                    if cancel.load(Ordering::Acquire) {
                        return Err("扫描已取消。".into());
                    }
                    let sibling_path = sibling.path();
                    if is_reparse_point_or_symlink(&sibling_path) || !sibling_path.is_file() {
                        continue;
                    }
                    let name = sibling.file_name().to_string_lossy().into_owned();
                    if volume_family(&name).as_ref() != Some(&family)
                        || matches_exclusion(&name, exclusions)
                    {
                        continue;
                    }
                    if let Ok(cpath) = sibling_path.canonicalize()
                        && seen_canonical.insert(file_name_key(&cpath.to_string_lossy()))
                    {
                        push_candidate(&mut collected_files, cpath)?;
                    }
                }
            }
            continue;
        }

        if path.is_dir() {
            let max_depth = if recursive { usize::MAX } else { 1 };
            let walker = WalkDir::new(path)
                .follow_links(false)
                .max_depth(max_depth)
                .into_iter()
                .filter_entry(|entry| {
                    !cancel.load(Ordering::Acquire)
                        && !is_reparse_point_or_symlink(entry.path())
                        && (!entry.file_type().is_dir()
                            || entry.depth() == 0
                            || (!entry.path().join(crate::paths::OWNER_FILE).is_file()
                                && !matches_exclusion(
                                    &entry
                                        .path()
                                        .strip_prefix(path)
                                        .unwrap_or(entry.path())
                                        .to_string_lossy(),
                                    exclusions,
                                )))
                });

            for entry_res in walker {
                scanned += 1;
                if scanned > MAX_SCAN_ENTRIES {
                    return Err(format!(
                        "扫描遍历的条目超过上限（{MAX_SCAN_ENTRIES}），请缩小目录范围或增加排除规则。"
                    ));
                }
                if cancel.load(Ordering::Acquire) {
                    return Err("扫描已取消。".into());
                }
                let entry = match entry_res {
                    Ok(e) => e,
                    Err(_) => {
                        notices.push(ScanNotice {
                            path: p.clone(),
                            message: "遍历目录时发生读取错误".to_string(),
                        });
                        continue;
                    }
                };

                let file_type = entry.file_type();
                if file_type.is_symlink() || !file_type.is_file() {
                    continue;
                }
                let entry_path = entry.path();
                #[cfg(windows)]
                {
                    if entry.metadata().is_ok_and(|meta| {
                        (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0
                    }) {
                        continue;
                    }
                }
                {
                    let rel = match entry_path.strip_prefix(path) {
                        Ok(r) => r.to_string_lossy().to_string(),
                        Err(_) => entry_path.to_string_lossy().to_string(),
                    };

                    if matches_exclusion(&rel, exclusions) {
                        continue;
                    }

                    let file_name = match entry_path.file_name().and_then(|n| n.to_str()) {
                        Some(n) => n,
                        None => continue,
                    };

                    if !is_supported_archive_path(entry_path, file_name) {
                        continue;
                    }

                    match entry_path.canonicalize() {
                        Ok(cpath) => {
                            let key = file_name_key(&cpath.to_string_lossy());
                            if seen_canonical.insert(key) {
                                push_candidate(&mut collected_files, cpath)?;
                            }
                        }
                        Err(_) => {
                            notices.push(ScanNotice {
                                path: entry_path.to_string_lossy().to_string(),
                                message: "无法读取文件信息".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    let mut dir_map: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for f in collected_files {
        if cancel.load(Ordering::Acquire) {
            return Err("扫描已取消。".into());
        }
        if let Some(parent) = f.parent() {
            dir_map.entry(parent.to_path_buf()).or_default().push(f);
        }
    }

    let mut sources = Vec::new();

    for (dir, files) in dir_map {
        if cancel.load(Ordering::Acquire) {
            return Err("扫描已取消。".into());
        }
        let mut numeric_groups: BTreeMap<(FileNameKey, String), BTreeMap<u32, (PathBuf, usize)>> =
            BTreeMap::new();
        let mut part_groups: BTreeMap<FileNameKey, BTreeMap<u32, (PathBuf, usize)>> =
            BTreeMap::new();
        let mut old_rar_groups: BTreeMap<FileNameKey, (Option<PathBuf>, BTreeMap<u32, PathBuf>)> =
            BTreeMap::new();
        let mut split_zip_groups: BTreeMap<FileNameKey, (Option<PathBuf>, BTreeMap<u32, PathBuf>)> =
            BTreeMap::new();
        let mut standalone_files: Vec<PathBuf> = Vec::new();

        for file in files {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            let fname = match file.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            match classify_volume(fname) {
                VolumeType::NumericSplit {
                    stem,
                    ext,
                    num,
                    width,
                } => {
                    numeric_groups
                        .entry((file_name_key(&stem), ext))
                        .or_default()
                        .insert(num, (file, width));
                }
                VolumeType::PartRar { stem, num, width } => {
                    part_groups
                        .entry(file_name_key(&stem))
                        .or_default()
                        .insert(num, (file, width));
                }
                VolumeType::OldRar { stem, num } => {
                    let entry = old_rar_groups.entry(file_name_key(&stem)).or_default();
                    if let Some(n) = num {
                        entry.1.insert(n, file);
                    } else {
                        entry.0 = Some(file);
                    }
                }
                VolumeType::SplitZip { stem, num } => {
                    let entry = split_zip_groups.entry(file_name_key(&stem)).or_default();
                    if let Some(n) = num {
                        entry.1.insert(n, file);
                    } else {
                        entry.0 = Some(file);
                    }
                }
                VolumeType::Single => {
                    standalone_files.push(file);
                }
            }
        }

        for (_, map) in numeric_groups {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            let Some(&min_k) = map.keys().next() else {
                continue;
            };
            let mut missing = min_k != 1;
            let mut expected = min_k;
            for &k in map.keys() {
                if k != expected {
                    missing = true;
                }
                expected = k.saturating_add(1);
            }
            // Check disk for missing volumes
            if let Some((first_file, width)) = map.values().next() {
                let first_stem = output_name(&first_file.to_string_lossy());
                let full_stem = first_file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&first_stem);
                if min_k > 1 {
                    let candidate = dir.join(format!("{}.{:0width$}", full_stem, 1, width = width));
                    if !candidate.exists() {
                        missing = true;
                    }
                }
            }

            let mut vols = Vec::new();
            let mut total_size = 0u64;
            for (p, _) in map.values() {
                vols.push(p.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(p) {
                    total_size += meta.len();
                }
            }
            let first_path = vols[0].clone();
            let disp = output_name(&first_path);
            sources.push(SourceGroup {
                path: first_path,
                display_name: disp,
                volumes: vols,
                size: total_size,
                missing_volume: missing,
            });
        }

        for (_, map) in part_groups {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            let Some(&min_k) = map.keys().next() else {
                continue;
            };
            let mut missing = min_k != 1;
            let mut expected = min_k;
            for &k in map.keys() {
                if k != expected {
                    missing = true;
                }
                expected = k.saturating_add(1);
            }

            let mut vols = Vec::new();
            let mut total_size = 0u64;
            for (p, _) in map.values() {
                vols.push(p.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(p) {
                    total_size += meta.len();
                }
            }
            let first_path = vols[0].clone();
            let disp = output_name(&first_path);
            sources.push(SourceGroup {
                path: first_path,
                display_name: disp,
                volumes: vols,
                size: total_size,
                missing_volume: missing,
            });
        }

        for (_, (rar_base, r_map)) in old_rar_groups {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            if rar_base.is_none() && r_map.is_empty() {
                continue;
            }
            if let (Some(file), true) = (rar_base.as_ref(), r_map.is_empty()) {
                let size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                let p_str = file.to_string_lossy().to_string();
                let disp = output_name(&p_str);
                sources.push(SourceGroup {
                    path: p_str.clone(),
                    display_name: disp,
                    volumes: vec![p_str],
                    size,
                    missing_volume: false,
                });
                continue;
            }

            let mut missing = rar_base.is_none();
            let mut vols = Vec::new();
            let mut total_size = 0u64;

            if let Some(ref base) = rar_base {
                vols.push(base.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(base) {
                    total_size += meta.len();
                }
            }

            let mut expected = 0;
            for (&k, p) in &r_map {
                if k != expected {
                    missing = true;
                }
                expected = k.saturating_add(1);
                vols.push(p.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(p) {
                    total_size += meta.len();
                }
            }

            let first_path = vols[0].clone();
            let disp = output_name(&first_path);
            sources.push(SourceGroup {
                path: first_path,
                display_name: disp,
                volumes: vols,
                size: total_size,
                missing_volume: missing,
            });
        }

        for (_, (zip_base, z_map)) in split_zip_groups {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            if zip_base.is_none() && z_map.is_empty() {
                continue;
            }
            if let (Some(file), true) = (zip_base.as_ref(), z_map.is_empty()) {
                let size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                let p_str = file.to_string_lossy().to_string();
                let disp = output_name(&p_str);
                sources.push(SourceGroup {
                    path: p_str.clone(),
                    display_name: disp,
                    volumes: vec![p_str],
                    size,
                    missing_volume: false,
                });
                continue;
            }

            let mut missing = zip_base.is_none();
            let mut vols = Vec::new();
            let mut total_size = 0u64;

            let mut expected = 1;
            for (&k, p) in &z_map {
                if k != expected {
                    missing = true;
                }
                expected = k.saturating_add(1);
                vols.push(p.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(p) {
                    total_size += meta.len();
                }
            }

            if let Some(ref base) = zip_base {
                vols.push(base.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(base) {
                    total_size += meta.len();
                }
            }

            let first_path = zip_base
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| vols[0].clone());
            let disp = output_name(&first_path);
            sources.push(SourceGroup {
                path: first_path,
                display_name: disp,
                volumes: vols,
                size: total_size,
                missing_volume: missing,
            });
        }

        for file in standalone_files {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
            let p_str = file.to_string_lossy().to_string();
            let disp = output_name(&p_str);
            sources.push(SourceGroup {
                path: p_str.clone(),
                display_name: disp,
                volumes: vec![p_str],
                size,
                missing_volume: false,
            });
        }
    }

    if cancel.load(Ordering::Acquire) {
        return Err("扫描已取消。".into());
    }
    sources.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ScanResult { sources, notices })
}

#[cfg(test)]
#[cfg(test)]
mod tests;
