//! 从文件名与随包文本文件提取候选解压密码。

use super::*;

pub fn extract_passwords_from_filename(filename: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    let mut add = |candidate: &str| {
        let mut trimmed = candidate
            .trim()
            .trim_matches(|c: char| {
                c == '.'
                    || c == ','
                    || c == '!'
                    || c == '?'
                    || c == ';'
                    || c == ':'
                    || c == '。'
                    || c == '！'
                    || c == '；'
                    || c == '：'
                    || c == '-'
                    || c == '_'
            })
            .trim();

        // Strip trailing archive extension if candidate captured the file extension at end
        let exts = [
            ".zip", ".rar", ".7z", ".tar.gz", ".tar.bz2", ".tar.xz", ".tar", ".gz", ".bz2", ".xz",
            ".sfx.exe", ".sfx",
        ];
        for ext in &exts {
            if trimmed.to_ascii_lowercase().ends_with(ext) {
                trimmed = trimmed[..trimmed.len() - ext.len()].trim();
                break;
            }
        }

        if trimmed.len() >= 2 && trimmed.len() <= 128 {
            let lower = trimmed.to_ascii_lowercase();
            if !lower.ends_with(".part")
                && !lower.starts_with("part")
                && seen.insert(trimmed.to_string())
            {
                results.push(trimmed.to_string());
            }
        }
    };

    // 1. Explicit matches on full filename: 密码123456, pwd=abc, 解压密码:xyz
    for caps in RE_PASSWORD_EXPLICIT.captures_iter(filename) {
        if let Some(m) = caps.get(1) {
            add(m.as_str());
        }
    }

    // 2. Bracket hints like [123456], 【密码: 123456】, (pwd=abc), [bbs.site.com]
    for caps in RE_PASSWORD_BRACKET.captures_iter(filename) {
        if let Some(m) = caps.get(1) {
            let inner = m.as_str().trim();
            if let Some(sub) = RE_PASSWORD_EXPLICIT.captures(inner)
                && let Some(sub_m) = sub.get(1)
            {
                add(sub_m.as_str());
            } else if inner.to_ascii_lowercase().starts_with("pw")
                || inner.to_ascii_lowercase().starts_with("pass")
            {
                let stripped = inner
                    .trim_start_matches(|c: char| {
                        c.is_alphabetic()
                            || c == ':'
                            || c == '：'
                            || c == '='
                            || c == '_'
                            || c == '-'
                    })
                    .trim();
                if !stripped.is_empty() {
                    add(stripped);
                }
            } else if inner.contains('.') && RE_DOMAIN_IN_NAME.is_match(inner) {
                add(inner);
            }
        }
    }

    // 3. Domain hints anywhere in filename (frequently used as passwords for forum releases)
    for caps in RE_DOMAIN_IN_NAME.captures_iter(filename) {
        if let Some(m) = caps.get(1) {
            add(m.as_str());
        }
    }

    results
}

pub fn extract_password_from_filename(filename: &str) -> Option<String> {
    extract_passwords_from_filename(filename).into_iter().next()
}

pub fn parse_passwords_from_text(content: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    let lines: Vec<&str> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    for line in &lines {
        for caps in RE_PASSWORD_EXPLICIT.captures_iter(line) {
            if let Some(m) = caps.get(1) {
                let s = m
                    .as_str()
                    .trim()
                    .trim_matches(|c: char| {
                        c == '.'
                            || c == ','
                            || c == '!'
                            || c == '?'
                            || c == ';'
                            || c == ':'
                            || c == '。'
                            || c == '！'
                            || c == '；'
                            || c == '：'
                    })
                    .trim();
                if s.len() >= 2 && s.len() <= 128 && seen.insert(s.to_string()) {
                    results.push(s.to_string());
                }
            }
        }
    }

    // If file is very concise (<= 3 lines, e.g. dedicated password.txt),
    // check each line as a direct candidate
    if lines.len() <= 3 {
        for line in lines {
            let s = line
                .trim()
                .trim_matches(|c: char| {
                    c == '.'
                        || c == ','
                        || c == '!'
                        || c == '?'
                        || c == ';'
                        || c == ':'
                        || c == '。'
                        || c == '！'
                        || c == '；'
                        || c == '：'
                })
                .trim();
            if s.len() >= 2 && s.len() <= 64 && !s.contains(' ') && seen.insert(s.to_string()) {
                results.push(s.to_string());
            }
        }
    }

    results
}

pub fn sniff_accompanying_passwords(archive_path: &Path) -> Vec<String> {
    sniff_accompanying_passwords_authorized(archive_path, |_| true)
}

/// A companion is optional input, so denied or unreadable files are skipped.
/// The caller supplies its current task policy rather than granting access to
/// every file beside an otherwise authorized archive.
pub fn sniff_accompanying_passwords_authorized(
    archive_path: &Path,
    mut can_read: impl FnMut(&Path) -> bool,
) -> Vec<String> {
    let parent = match archive_path.parent() {
        Some(p) if p.is_dir() => p,
        _ => return Vec::new(),
    };
    // Pin the whole parent chain, then open each immediate child without
    // following reparses or permitting writers/replacement during inspection.
    let directory = match crate::paths::DirectoryGuard::open(parent) {
        Ok(directory) => directory,
        Err(_) => return Vec::new(),
    };

    let stem = archive_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    let mut candidate_files = Vec::new();

    // 1. Same stem companions: <stem>.txt, <stem>.nfo, <stem>.md
    if !stem.is_empty() {
        candidate_files.push(parent.join(format!("{stem}.txt")));
        candidate_files.push(parent.join(format!("{stem}.nfo")));
        candidate_files.push(parent.join(format!("{stem}.md")));
        if let Some(pos) = stem.to_ascii_lowercase().rfind(".part") {
            let base_stem = &stem[..pos];
            if !base_stem.is_empty() {
                candidate_files.push(parent.join(format!("{base_stem}.txt")));
                candidate_files.push(parent.join(format!("{base_stem}.nfo")));
            }
        }
    }

    // 2. Generic companion files
    let generic_names = [
        "password.txt",
        "passwords.txt",
        "pwd.txt",
        "pw.txt",
        "pass.txt",
        "code.txt",
        "密码.txt",
        "解压密码.txt",
        "提取码.txt",
        "说明.txt",
        "readme.txt",
        "README.txt",
        "read_me.txt",
        "info.txt",
    ];
    for name in &generic_names {
        candidate_files.push(parent.join(name));
    }

    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for file_path in candidate_files {
        if !can_read(&file_path) {
            continue;
        }
        let file = match directory.open_file_with_share(
            &file_path,
            windows_sys::Win32::Foundation::GENERIC_READ,
            windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ,
        ) {
            Ok(file) => file,
            Err(_) => continue,
        };
        let meta = match file.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        const MAX_COMPANION_BYTES: u64 = 64 * 1024;
        if !meta.is_file() || meta.len() > MAX_COMPANION_BYTES || meta.len() == 0 {
            continue;
        }
        // Recheck policy against the pinned namespace before reading content.
        // The bound is on the actual read, not just an earlier length sample.
        if !can_read(&file_path) {
            continue;
        }
        let mut raw_bytes = Vec::with_capacity(meta.len() as usize);
        if file
            .take(MAX_COMPANION_BYTES + 1)
            .read_to_end(&mut raw_bytes)
            .is_err()
            || raw_bytes.len() as u64 > MAX_COMPANION_BYTES
        {
            continue;
        }

        let content = match std::str::from_utf8(&raw_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                let (cow, _, _) = encoding_rs::GB18030.decode(&raw_bytes);
                cow.into_owned()
            }
        };

        for p in parse_passwords_from_text(&content) {
            if seen.insert(p.clone()) {
                results.push(p);
            }
        }
    }

    results
}
