//! 复合扩展名拆分、魔数后缀推断、规范化命名与后缀修复。

use super::*;

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
pub(super) enum DetectedArchiveKind {
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
    Cab,
}

pub(super) fn sniff_magic_kind(path: &Path) -> Option<DetectedArchiveKind> {
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
    if bytes.starts_with(b"MSCF") {
        return Some(DetectedArchiveKind::Cab);
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
            DetectedArchiveKind::Cab => ".cab",
        };

        // If stem already ends with desired_ext (e.g. file is `movie.zip.mp4` or `archive.7z.bak`),
        // strip the fake extension and keep the clean stem.
        let stem_lower = stem.to_ascii_lowercase();
        let candidate = if stem_lower.ends_with(desired_ext) {
            stem.to_string()
        } else {
            // Also check if stem ended with another archive extension (e.g. `file.rar.mp4` but magic is zip)
            // Longer/compound extensions must be checked first before single extensions
            let other_archive_exts = [
                ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.lz4", ".zip", ".7z", ".rar",
                ".tar", ".gz", ".bz2", ".xz", ".zst", ".lz4", ".wim", ".cab",
            ];
            let mut base_stem = stem;
            for arch_ext in other_archive_exts {
                if stem_lower.ends_with(arch_ext) && stem.len() > arch_ext.len() {
                    base_stem = &stem[..stem.len() - arch_ext.len()];
                    break;
                }
            }
            if desired_ext.starts_with(".tar.") && base_stem.to_ascii_lowercase().ends_with(".tar")
            {
                base_stem = &base_stem[..base_stem.len() - 4];
            }
            format!("{base_stem}{desired_ext}")
        };

        if candidate != file_name {
            return Some(candidate);
        }
    }

    // 3. Uppercase archive extension normalization even if empty or non-magic
    let archive_exts = [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.lz4", ".zip", ".7z", ".rar", ".tar",
        ".gz", ".bz2", ".xz", ".zst", ".lz4", ".wim", ".lzh", ".cab",
    ];
    if archive_exts.contains(&ext_lower.as_str()) && ext != ext_lower {
        return Some(format!("{stem}{ext_lower}"));
    }

    None
}

pub fn fix_archive_extensions(paths: &[String]) -> Result<Vec<ArchiveExtensionFixItem>, String> {
    let started = Instant::now();
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut candidate_files: Vec<PathBuf> = Vec::new();
    let mut visited_paths = HashSet::new();

    for p_str in paths {
        let p = PathBuf::from(p_str);
        if !p.exists() {
            continue;
        }
        if p.is_file() {
            if is_reparse_or_symlink(&p) {
                continue;
            }
            let norm_key = p.to_string_lossy().to_ascii_lowercase();
            if visited_paths.insert(norm_key) {
                candidate_files.push(p);
            }
        } else if p.is_dir() {
            validate_toolbox_directory(&p)?;
            for entry in WalkDir::new(&p)
                .min_depth(1)
                .max_depth(MAX_TOOLBOX_DEPTH)
                .follow_links(false)
            {
                if let Ok(e) = entry
                    && e.file_type().is_file()
                {
                    let ep = e.path();
                    if is_reparse_or_symlink(ep) {
                        continue;
                    }
                    if ep
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .eq_ignore_ascii_case(OWNER_FILE)
                    {
                        continue;
                    }
                    let norm_key = ep.to_string_lossy().to_ascii_lowercase();
                    if visited_paths.insert(norm_key) {
                        candidate_files.push(ep.to_path_buf());
                    }
                }
            }
        }
    }

    if candidate_files.len() > MAX_TOOLBOX_FILES {
        return Err(format!(
            "处理文件数量超出上限 ({MAX_TOOLBOX_FILES})，请分批处理。"
        ));
    }

    candidate_files.sort();

    let mut results = Vec::with_capacity(candidate_files.len());

    for file_path in candidate_files {
        check_toolbox_budget(started, results.len(), 0)?;
        let orig_path_str = file_path.to_string_lossy().into_owned();

        let file_name = match file_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => {
                results.push(ArchiveExtensionFixItem {
                    original_path: orig_path_str,
                    proposed_name: None,
                    new_path: None,
                    status: "error".to_string(),
                    message: "文件名无效".to_string(),
                });
                continue;
            }
        };

        let parent_dir = match file_path.parent() {
            Some(p) => p,
            None => {
                results.push(ArchiveExtensionFixItem {
                    original_path: orig_path_str,
                    proposed_name: None,
                    new_path: None,
                    status: "error".to_string(),
                    message: "无法获取文件所在目录".to_string(),
                });
                continue;
            }
        };

        if let Err(e) = validate_toolbox_directory(parent_dir) {
            results.push(ArchiveExtensionFixItem {
                original_path: orig_path_str,
                proposed_name: None,
                new_path: None,
                status: "error".to_string(),
                message: format!("安全限制拒绝操作: {e}"),
            });
            continue;
        }

        let normalized_opt = compute_normalized_name(&file_path, file_name);
        match normalized_opt {
            Some(new_name) => {
                let (stem, ext) = split_stem_and_compound_ext(&new_name);
                let mut candidate_name = new_name.clone();
                let mut candidate_path = parent_dir.join(&candidate_name);
                let mut counter = 1u32;

                while candidate_path.exists() {
                    let is_case_only = file_path
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&candidate_path.to_string_lossy());
                    if is_case_only {
                        break;
                    }
                    candidate_name = format!("{stem}_{counter}{ext}");
                    candidate_path = parent_dir.join(&candidate_name);
                    counter += 1;
                    if counter > 1000 {
                        break;
                    }
                }

                if counter > 1000 && candidate_path.exists() {
                    results.push(ArchiveExtensionFixItem {
                        original_path: orig_path_str,
                        proposed_name: Some(candidate_name),
                        new_path: None,
                        status: "error".to_string(),
                        message: "同名冲突过多，已达重试上限".to_string(),
                    });
                    continue;
                }

                let is_case_only = file_path
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&candidate_path.to_string_lossy());

                match safe_move_file(&file_path, &candidate_path, is_case_only) {
                    Ok(()) => {
                        let final_path_str = candidate_path.to_string_lossy().into_owned();
                        results.push(ArchiveExtensionFixItem {
                            original_path: orig_path_str,
                            proposed_name: Some(candidate_name.clone()),
                            new_path: Some(final_path_str),
                            status: "renamed".to_string(),
                            message: format!("已成功修改后缀为 {}", candidate_name),
                        });
                    }
                    Err(e) => {
                        results.push(ArchiveExtensionFixItem {
                            original_path: orig_path_str,
                            proposed_name: Some(candidate_name),
                            new_path: None,
                            status: "error".to_string(),
                            message: format!("修改后缀失败: {e}"),
                        });
                    }
                }
            }
            None => {
                if let Some(kind) = sniff_magic_kind(&file_path) {
                    results.push(ArchiveExtensionFixItem {
                        original_path: orig_path_str,
                        proposed_name: Some(file_name.to_string()),
                        new_path: Some(file_path.to_string_lossy().into_owned()),
                        status: "already_correct".to_string(),
                        message: format!("魔数检测有效（{kind:?}），后缀已正确无需修改"),
                    });
                } else {
                    results.push(ArchiveExtensionFixItem {
                        original_path: orig_path_str,
                        proposed_name: None,
                        new_path: None,
                        status: "unrecognized".to_string(),
                        message: "未检测到已知压缩包魔数特征，未作修改".to_string(),
                    });
                }
            }
        }
    }

    Ok(results)
}
