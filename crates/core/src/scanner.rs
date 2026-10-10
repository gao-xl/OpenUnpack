use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use walkdir::WalkDir;

use crate::types::{ScanNotice, ScanResult, SourceGroup};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x00000400;

pub fn matches_exclusion(relative: &str, patterns: &[String]) -> bool {
    if relative.len() > 32768 {
        return false;
    }
    let norm_path = relative.replace('\\', "/");
    let norm_path_lower = norm_path.to_lowercase();

    for pat in patterns {
        if pat.is_empty() || pat.len() > 256 {
            continue;
        }
        let norm_pat = pat.replace('\\', "/").to_lowercase();
        if wildcard_match(&norm_pat, &norm_path_lower) {
            return true;
        }
        // Also allow matching solely against file/directory base name
        if let Some(filename) = norm_path_lower.rsplit('/').next() {
            if wildcard_match(&norm_pat, filename) {
                return true;
            }
        }
    }
    false
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    // '?' denotes one Unicode scalar value, not one byte of UTF-8.
    let p_bytes: Vec<char> = pattern.chars().collect();
    let t_bytes: Vec<char> = text.chars().collect();
    let mut p_idx = 0;
    let mut t_idx = 0;
    let mut star_idx = None;
    let mut match_idx = 0;

    while t_idx < t_bytes.len() {
        if p_idx < p_bytes.len() && (p_bytes[p_idx] == '?' || p_bytes[p_idx] == t_bytes[t_idx]) {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p_bytes.len() && p_bytes[p_idx] == '*' {
            star_idx = Some(p_idx);
            p_idx += 1;
            match_idx = t_idx;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            t_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < p_bytes.len() && p_bytes[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == p_bytes.len()
}

pub fn output_name(path: &str) -> String {
    let p = Path::new(path);
    let file_name = match p.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => "archive",
    };

    let re_numeric = Regex::new(r"(?i)\.(?:7z|zip)\.\d+$").unwrap();
    if let Some(mat) = re_numeric.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    let re_part = Regex::new(r"(?i)\.part\d+\.rar$").unwrap();
    if let Some(mat) = re_part.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    let re_old_rar = Regex::new(r"(?i)\.r\d{2,}$").unwrap();
    if let Some(mat) = re_old_rar.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    let re_split_zip = Regex::new(r"(?i)\.z\d{2,}$").unwrap();
    if let Some(mat) = re_split_zip.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    let exts = [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".zip", ".7z", ".rar", ".tar", ".gz", ".bz2", ".xz",
        ".cab",
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
    let re_disguised = Regex::new(r"(?i)\.(?:zip|7z|rar)\.[a-zA-Z0-9_-]+$").unwrap();
    if let Some(mat) = re_disguised.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
        if !stem.is_empty() {
            return stem.to_string();
        }
    }
    "archive".to_string()
}

fn is_reparse_point_or_symlink(path: &Path) -> bool {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return true;
        }
        #[cfg(windows)]
        {
            if (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
                return true;
            }
        }
    }
    false
}

fn is_container_extension(lower: &str) -> bool {
    let containers = [
        ".docx", ".xlsx", ".pptx", ".jar", ".war", ".ear", ".apk", ".aab", ".ipa", ".epub", ".xpi",
        ".vsix", ".nupkg", ".whl", ".odt", ".ods", ".odp", ".crx",
    ];
    containers.iter().any(|ext| lower.ends_with(ext))
}

fn has_archive_magic_bytes(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 512];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    if n < 2 {
        return false;
    }
    // GZIP: 1F 8B
    if buf[0] == 0x1F && buf[1] == 0x8B {
        return true;
    }
    // BZIP2: 42 5A 68 ('B', 'Z', 'h')
    if n >= 3 && buf[0] == 0x42 && buf[1] == 0x5A && buf[2] == 0x68 {
        return true;
    }
    if n < 4 {
        return false;
    }
    // ZIP: PK\x03\x04, PK\x05\x06 (empty), PK\x07\x08 (spanned)
    if buf[0] == 0x50
        && buf[1] == 0x4B
        && ((buf[2] == 0x03 && buf[3] == 0x04)
            || (buf[2] == 0x05 && buf[3] == 0x06)
            || (buf[2] == 0x07 && buf[3] == 0x08))
    {
        return true;
    }
    // CAB: 'M', 'S', 'C', 'F'
    if buf[0] == 0x4D && buf[1] == 0x53 && buf[2] == 0x43 && buf[3] == 0x46 {
        return true;
    }
    // Zstandard: 28 B5 2F FD
    if buf[0] == 0x28 && buf[1] == 0xB5 && buf[2] == 0x2F && buf[3] == 0xFD {
        return true;
    }
    // 7z: 37 7A BC AF 27 1C
    if n >= 6 && buf[..6] == [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C] {
        return true;
    }
    // XZ: FD 37 7A 58 5A 00
    if n >= 6 && buf[..6] == [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00] {
        return true;
    }
    // RAR4 / RAR5: 52 61 72 21 1A 07 00 or 52 61 72 21 1A 07 01 00
    if n >= 7
        && buf[..6] == [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07]
        && (buf[6] == 0x00 || (n >= 8 && buf[6] == 0x01 && buf[7] == 0x00))
    {
        return true;
    }
    // TAR (POSIX ustar header at offset 257)
    if n >= 262 && &buf[257..262] == b"ustar" {
        return true;
    }
    false
}

fn is_supported_archive_path(path: &Path, filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    if is_container_extension(&lower) {
        return false;
    }
    let re_multi = Regex::new(r"(?i)\.(?:(?:7z|zip)\.\d+|part\d+\.rar|r\d+|z\d+)$").unwrap();
    if re_multi.is_match(&lower) {
        return true;
    }

    let single_exts = [".zip", ".7z", ".rar", ".tar", ".gz", ".bz2", ".xz", ".cab"];
    if single_exts.iter().any(|ext| lower.ends_with(ext)) {
        return true;
    }

    // Disguised archive detection via Magic Bytes (e.g. .zip.mp4, .rar.png, .7z.bin)
    has_archive_magic_bytes(path)
}

#[derive(Debug)]
enum VolumeType {
    NumericSplit {
        stem: String,
        ext: String,
        num: u32,
        width: usize,
    },
    PartRar {
        stem: String,
        num: u32,
        width: usize,
    },
    OldRar {
        stem: String,
        num: Option<u32>,
    },
    SplitZip {
        stem: String,
        num: Option<u32>,
    },
    Single,
}

fn classify_volume(filename: &str) -> VolumeType {
    let re_num = Regex::new(r"^(?i)(.+)\.(7z|zip)\.(\d+)$").unwrap();
    if let Some(caps) = re_num.captures(filename) {
        let stem = caps.get(1).unwrap().as_str().to_string();
        let ext = caps.get(2).unwrap().as_str().to_ascii_lowercase();
        let num_str = caps.get(3).unwrap().as_str();
        if let Ok(num) = num_str.parse::<u32>() {
            return VolumeType::NumericSplit {
                stem,
                ext,
                num,
                width: num_str.len(),
            };
        }
    }

    let re_part = Regex::new(r"^(?i)(.+)\.part(\d+)\.rar$").unwrap();
    if let Some(caps) = re_part.captures(filename) {
        let stem = caps.get(1).unwrap().as_str().to_string();
        let num_str = caps.get(2).unwrap().as_str();
        if let Ok(num) = num_str.parse::<u32>() {
            return VolumeType::PartRar {
                stem,
                num,
                width: num_str.len(),
            };
        }
    }

    let re_old_r = Regex::new(r"^(?i)(.+)\.r(\d+)$").unwrap();
    if let Some(caps) = re_old_r.captures(filename) {
        let stem = caps.get(1).unwrap().as_str().to_string();
        if let Ok(num) = caps.get(2).unwrap().as_str().parse::<u32>() {
            return VolumeType::OldRar {
                stem,
                num: Some(num),
            };
        }
    }

    let re_old_rar_base = Regex::new(r"^(?i)(.+)\.rar$").unwrap();
    if let Some(caps) = re_old_rar_base.captures(filename) {
        let stem = caps.get(1).unwrap().as_str().to_string();
        return VolumeType::OldRar { stem, num: None };
    }

    let re_z_split = Regex::new(r"^(?i)(.+)\.z(\d+)$").unwrap();
    if let Some(caps) = re_z_split.captures(filename) {
        let stem = caps.get(1).unwrap().as_str().to_string();
        if let Ok(num) = caps.get(2).unwrap().as_str().parse::<u32>() {
            return VolumeType::SplitZip {
                stem,
                num: Some(num),
            };
        }
    }

    let re_zip_base = Regex::new(r"^(?i)(.+)\.zip$").unwrap();
    if let Some(caps) = re_zip_base.captures(filename) {
        let stem = caps.get(1).unwrap().as_str().to_string();
        return VolumeType::SplitZip { stem, num: None };
    }

    VolumeType::Single
}

fn volume_family(filename: &str) -> Option<String> {
    match classify_volume(filename) {
        VolumeType::NumericSplit { stem, ext, .. } => {
            Some(format!("numeric:{}:{ext}", stem.to_lowercase()))
        }
        VolumeType::PartRar { stem, .. } => Some(format!("part:{}", stem.to_lowercase())),
        VolumeType::OldRar { stem, .. } => Some(format!("rar:{}", stem.to_lowercase())),
        VolumeType::SplitZip { stem, .. } => Some(format!("zip:{}", stem.to_lowercase())),
        VolumeType::Single => None,
    }
}

pub fn extract_password_from_filename(filename: &str) -> Option<String> {
    // Patterns like: 密码123456, 密码:123456, 密码：123456, pwd123456, pw:123456, [pw_1234]
    let re_explicit =
        Regex::new(r"(?i)(?:密码|pwd|password|解压码|提取码)[：:\s_-]*([^\s\[\]【】()（）]+)")
            .unwrap();
    if let Some(caps) = re_explicit.captures(filename) {
        if let Some(m) = caps.get(1) {
            let s = m.as_str().trim();
            if !s.is_empty() && s.len() <= 128 {
                return Some(s.to_string());
            }
        }
    }

    // Bracket hints like [123456] or 【123456】 if they contain 'pw' or 'pwd' or start with '密码'
    let re_bracket = Regex::new(r"[\[【(]([^\[\]【】()（）]+)[\]】)]").unwrap();
    for caps in re_bracket.captures_iter(filename) {
        if let Some(m) = caps.get(1) {
            let inner = m.as_str().trim();
            if let Some(sub) = re_explicit.captures(inner) {
                if let Some(sub_m) = sub.get(1) {
                    let s = sub_m.as_str().trim();
                    if !s.is_empty() && s.len() <= 128 {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }
    None
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
    let mut notices = Vec::new();
    let mut collected_files = Vec::new();
    let mut seen_canonical = HashSet::new();

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
                    let key = cpath.to_string_lossy().to_ascii_lowercase();
                    if seen_canonical.insert(key) {
                        collected_files.push(cpath);
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
            if let (Some(family), Some(parent)) = (volume_family(file_name), path.parent()) {
                if let Ok(siblings) = fs::read_dir(parent) {
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
                        if let Ok(cpath) = sibling_path.canonicalize() {
                            if seen_canonical.insert(cpath.to_string_lossy().to_lowercase()) {
                                collected_files.push(cpath);
                            }
                        }
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

                let entry_path = entry.path();
                if is_reparse_point_or_symlink(entry_path) {
                    continue;
                }

                if entry_path.is_file() {
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
                            let key = cpath.to_string_lossy().to_ascii_lowercase();
                            if seen_canonical.insert(key) {
                                collected_files.push(cpath);
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
        let mut numeric_groups: HashMap<(String, String), BTreeMap<u32, (PathBuf, usize)>> =
            HashMap::new();
        let mut part_groups: HashMap<String, BTreeMap<u32, (PathBuf, usize)>> = HashMap::new();
        let mut old_rar_groups: HashMap<String, (Option<PathBuf>, BTreeMap<u32, PathBuf>)> =
            HashMap::new();
        let mut split_zip_groups: HashMap<String, (Option<PathBuf>, BTreeMap<u32, PathBuf>)> =
            HashMap::new();
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
                        .entry((stem.to_ascii_lowercase(), ext))
                        .or_default()
                        .insert(num, (file, width));
                }
                VolumeType::PartRar { stem, num, width } => {
                    part_groups
                        .entry(stem.to_ascii_lowercase())
                        .or_default()
                        .insert(num, (file, width));
                }
                VolumeType::OldRar { stem, num } => {
                    let entry = old_rar_groups.entry(stem.to_ascii_lowercase()).or_default();
                    if let Some(n) = num {
                        entry.1.insert(n, file);
                    } else {
                        entry.0 = Some(file);
                    }
                }
                VolumeType::SplitZip { stem, num } => {
                    let entry = split_zip_groups
                        .entry(stem.to_ascii_lowercase())
                        .or_default();
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
            if map.is_empty() {
                continue;
            }
            let min_k = *map.keys().next().unwrap();
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
            for (_, (p, _)) in &map {
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
            if map.is_empty() {
                continue;
            }
            let min_k = *map.keys().next().unwrap();
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
            for (_, (p, _)) in &map {
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
            if rar_base.is_some() && r_map.is_empty() {
                let file = rar_base.unwrap();
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
            if zip_base.is_some() && z_map.is_empty() {
                let file = zip_base.unwrap();
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
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_extract_password_from_filename() {
        assert_eq!(
            extract_password_from_filename("资源包[密码:abc1234].zip"),
            Some("abc1234".to_string())
        );
        assert_eq!(
            extract_password_from_filename("【密码123456】我的文件.7z"),
            Some("123456".to_string())
        );
        assert_eq!(
            extract_password_from_filename("video[pwd_secret].rar"),
            Some("secret".to_string())
        );
        assert_eq!(extract_password_from_filename("normal_archive.zip"), None);
    }

    #[test]
    fn test_wildcard_no_prefix_match() {
        let patterns = vec!["*.tmp".to_string(), "backup*".to_string()];
        assert!(matches_exclusion("test.tmp", &patterns));
        assert!(matches_exclusion("backup_file.zip", &patterns));
        assert!(!matches_exclusion("my_backup_file.zip", &patterns));
        assert!(!matches_exclusion("tmp_file.zip", &patterns));
    }

    #[test]
    fn test_output_name_stems() {
        assert_eq!(output_name("C:/data/photo.part01.rar"), "photo");
        assert_eq!(output_name("/home/user/archive.7z.001"), "archive");
        assert_eq!(output_name("D:\\backup\\docs.tar.gz"), "docs");
        assert_eq!(output_name("file.zip"), "file");
        assert_eq!(output_name("disguised.zip.mp4"), "disguised");
        assert_eq!(output_name("video.rar.png"), "video");
        assert_eq!(output_name(""), "archive");
    }

    #[test]
    fn test_magic_bytes_disguised_and_container_skip() {
        use std::io::Write;
        let dir = tempdir().unwrap();

        // 1. Disguised ZIP (.zip.mp4) with valid PK\x03\x04 magic bytes
        let disguised_zip = dir.path().join("movie.zip.mp4");
        let mut f1 = File::create(&disguised_zip).unwrap();
        f1.write_all(b"PK\x03\x04fakezipdata").unwrap();

        // 2. Disguised 7z (.bin) with valid 7z magic bytes
        let disguised_7z = dir.path().join("archive.bin");
        let mut f2 = File::create(&disguised_7z).unwrap();
        f2.write_all(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0x00, 0x04])
            .unwrap();

        // 3. Container file (.docx) with PK\x03\x04 magic bytes -> MUST BE IGNORED
        let docx_file = dir.path().join("report.docx");
        let mut f3 = File::create(&docx_file).unwrap();
        f3.write_all(b"PK\x03\x04worddocument").unwrap();

        // 4. Plain mp4 video -> MUST BE IGNORED
        let real_mp4 = dir.path().join("real_video.mp4");
        let mut f4 = File::create(&real_mp4).unwrap();
        f4.write_all(b"\x00\x00\x00\x18ftypmp42").unwrap();

        let dir_str = dir.path().to_string_lossy().to_string();
        let res = scan(&[dir_str], false, &[]).unwrap();
        assert_eq!(res.sources.len(), 2);
        let names: Vec<_> = res
            .sources
            .iter()
            .map(|s| s.display_name.as_str())
            .collect();
        assert!(names.contains(&"movie"));
        assert!(names.contains(&"archive"));
    }

    #[test]
    fn test_temp_grouped_numeric_volumes() {
        let dir = tempdir().unwrap();
        let p1 = dir.path().join("data.7z.001");
        let p2 = dir.path().join("data.7z.002");
        File::create(&p1).unwrap();
        File::create(&p2).unwrap();

        let dir_str = dir.path().to_string_lossy().to_string();
        let res = scan(&[dir_str], false, &[]).unwrap();
        assert_eq!(res.sources.len(), 1);
        let src = &res.sources[0];
        assert_eq!(src.display_name, "data");
        assert_eq!(src.volumes.len(), 2);
        assert!(!src.missing_volume);
    }
    #[test]
    fn selected_volume_collects_siblings_but_not_unrelated() {
        let d = tempdir().unwrap();
        for n in ["a.7z.001", "a.7z.002", "b.zip"] {
            File::create(d.path().join(n)).unwrap();
        }
        let r = scan(
            &[d.path().join("a.7z.002").to_string_lossy().into_owned()],
            false,
            &[],
        )
        .unwrap();
        assert_eq!(r.sources.len(), 1);
        assert_eq!(r.sources[0].volumes.len(), 2);
        assert!(r.sources[0].path.ends_with("a.7z.001"));
        assert!(!r.sources[0].missing_volume);
    }
    #[test]
    fn zip_uses_directory_volume_as_start() {
        let d = tempdir().unwrap();
        for n in ["a.z01", "a.zip"] {
            File::create(d.path().join(n)).unwrap();
        }
        let r = scan(
            &[d.path().join("a.z01").to_string_lossy().into_owned()],
            false,
            &[],
        )
        .unwrap();
        assert_eq!(r.sources.len(), 1);
        assert!(r.sources[0].path.ends_with("a.zip"));
        assert_eq!(r.sources[0].volumes.len(), 2);
    }

    #[test]
    fn test_expanded_magic_bytes_detection() {
        use std::io::Write;
        let d = tempdir().unwrap();

        // GZ magic bytes (1F 8B) disguised as .dat
        let gz_file = d.path().join("hidden_gzip.dat");
        let mut f_gz = File::create(&gz_file).unwrap();
        f_gz.write_all(&[0x1F, 0x8B, 0x08, 0x00]).unwrap();

        // BZ2 magic bytes (BZh / 42 5A 68) disguised as .bin
        let bz2_file = d.path().join("hidden_bzip2.bin");
        let mut f_bz2 = File::create(&bz2_file).unwrap();
        f_bz2.write_all(b"BZh91AY&SY").unwrap();

        // XZ magic bytes disguised as .mp4
        let xz_file = d.path().join("hidden_xz.mp4");
        let mut f_xz = File::create(&xz_file).unwrap();
        f_xz.write_all(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00, 0x00]).unwrap();

        let dir_str = d.path().to_string_lossy().to_string();
        let res = scan(&[dir_str], false, &[]).unwrap();
        assert_eq!(res.sources.len(), 3);
        let names: Vec<_> = res.sources.iter().map(|s| s.display_name.as_str()).collect();
        assert!(names.contains(&"hidden_gzip"));
        assert!(names.contains(&"hidden_bzip2"));
        assert!(names.contains(&"hidden_xz"));
    }

    #[test]
    fn test_missing_volume_detection() {
        let d = tempdir().unwrap();
        // Only volume 002 exists, 001 is missing
        let p2 = d.path().join("broken_split.7z.002");
        File::create(&p2).unwrap();

        let dir_str = d.path().to_string_lossy().to_string();
        let res = scan(&[dir_str], false, &[]).unwrap();
        assert_eq!(res.sources.len(), 1);
        assert!(res.sources[0].missing_volume, "缺少首卷必须被标记为 missing_volume");
    }
}
