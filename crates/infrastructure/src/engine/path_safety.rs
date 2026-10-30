use std::path::{Component, Path};

/// Validates and sanitizes a relative archive entry path to prevent Zip Slip / path traversal attacks.
pub fn validate_relative_path(name: &str) -> Result<String, String> {
    if name.starts_with('/') || name.starts_with('\\') {
        return Err("UnsafePath: Absolute paths forbidden".into());
    }
    let normalized = name.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("UnsafePath: Empty entry path".into());
    }

    // Strip leading "./" prefixes
    let mut path_str = trimmed;
    while path_str.starts_with("./") {
        path_str = &path_str[2..];
    }
    if path_str.is_empty() {
        return Err("UnsafePath: Entry resolves to root".into());
    }

    if path_str.contains('\0') || path_str.contains(':') {
        return Err("UnsafePath: Contains illegal characters".into());
    }

    let p = Path::new(path_str);
    let mut depth = 0;
    for component in p.components() {
        match component {
            Component::Normal(seg) => {
                let seg_str = seg.to_string_lossy();
                if seg_str == "." || seg_str == ".." {
                    return Err("UnsafePath: Path traversal detected".into());
                }
                if seg_str.ends_with(' ') || seg_str.ends_with('.') {
                    return Err("UnsafePath: Trailing dot or space in segment".into());
                }
                let base = seg_str.split('.').next().unwrap_or("").trim_end();
                if is_reserved_windows_name(base) {
                    return Err("UnsafePath: Reserved system device name".into());
                }
                depth += 1;
                if depth > 128 {
                    return Err("ResourceLimit: Path hierarchy exceeds safe depth".into());
                }
            }
            Component::ParentDir => return Err("UnsafePath: Path traversal (ParentDir)".into()),
            Component::RootDir | Component::Prefix(_) => {
                return Err("UnsafePath: Absolute paths forbidden".into());
            }
            Component::CurDir => continue,
        }
    }

    Ok(path_str.to_string())
}

fn is_reserved_windows_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "CLOCK$"
            | "CONIN$"
            | "CONOUT$"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// Evaluates if a relative path matches any exclusion wildcard pattern.
pub fn matches_exclusion(path: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let norm_path = path.replace('\\', "/");
    let file_name = norm_path.rsplit('/').next().unwrap_or(&norm_path);

    for raw in patterns {
        let pattern = raw.trim().replace('\\', "/");
        if pattern.is_empty() {
            continue;
        }
        let target = if pattern.contains('/') {
            &norm_path
        } else {
            file_name
        };
        let target_utf16: Vec<u16> = target.encode_utf16().collect();
        let pattern_utf16: Vec<u16> = pattern.encode_utf16().collect();
        if wildcard_utf16(&target_utf16, &pattern_utf16) {
            return true;
        }
    }
    false
}

pub fn preview_output_exclusions(paths: &[String], patterns: &[String]) -> Vec<bool> {
    if patterns.is_empty() {
        return vec![false; paths.len()];
    }
    let mut excluded_directories = std::collections::HashSet::new();
    let entries: Vec<(String, bool)> = paths
        .iter()
        .map(|path| {
            let is_dir = path.ends_with('/') || path.ends_with('\\');
            let norm = path.replace('\\', "/").trim_end_matches('/').to_string();
            (norm, is_dir)
        })
        .collect();

    for (norm, is_dir) in &entries {
        if *is_dir && matches_exclusion(norm, patterns) {
            excluded_directories.insert(norm.clone());
        }
    }

    let mut result = Vec::with_capacity(paths.len());
    for (norm, _) in &entries {
        let mut is_excluded = matches_exclusion(norm, patterns);
        if !is_excluded && !excluded_directories.is_empty() {
            let mut parent = norm.as_str();
            while let Some((p, _)) = parent.rsplit_once('/') {
                if excluded_directories.contains(p) {
                    is_excluded = true;
                    break;
                }
                parent = p;
            }
        }
        result.push(is_excluded);
    }
    result
}

/// Evaluates if a relative path matches inclusion patterns (if specified).
pub fn matches_inclusion(path: &str, _is_directory: bool, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return true;
    }
    let norm_path = path.replace('\\', "/").trim_start_matches('/').to_string();
    let file_name = norm_path.rsplit('/').next().unwrap_or(&norm_path);

    for raw in patterns {
        if let Some(selected) = raw.strip_prefix("./") {
            let selected = selected
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_string();
            let relative = &norm_path;
            if relative == &selected
                || relative.starts_with(&(selected.clone() + "/"))
                || (_is_directory && selected.starts_with(&(relative.clone() + "/")))
            {
                return true;
            }
            continue;
        }
        let pattern = raw
            .trim()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        if pattern.is_empty() {
            continue;
        }
        let trimmed_pattern = pattern.trim_end_matches('/');

        if norm_path.eq_ignore_ascii_case(&pattern)
            || norm_path.eq_ignore_ascii_case(trimmed_pattern)
        {
            return true;
        }

        // If pattern is a directory, include children
        if norm_path
            .to_ascii_lowercase()
            .starts_with(&format!("{}/", trimmed_pattern.to_ascii_lowercase()))
        {
            return true;
        }

        let target = if pattern.contains('/') {
            &norm_path
        } else {
            file_name
        };
        if wildcard_match(target, &pattern) {
            return true;
        }
    }
    false
}

fn to_upper_invariant(c: u16) -> u16 {
    if c == 0x0131 {
        return 0x0131;
    }
    if c == 0x03C2 || c == 0x03C3 {
        return 0x03A3;
    }
    if (b'a' as u16..=b'z' as u16).contains(&c) {
        return c - 32;
    }
    if let Some(ch) = char::from_u32(c as u32) {
        let mut upper = ch.to_uppercase();
        if let Some(first) = upper.next()
            && upper.next().is_none()
        {
            let u = first as u32;
            if u <= 0xFFFF {
                return u as u16;
            }
        }
    }
    c
}

fn wildcard_utf16(value: &[u16], pattern: &[u16]) -> bool {
    let mut v = 0;
    let mut p = 0;
    let mut star: Option<usize> = None;
    let mut restart = 0;
    let qmark = '?' as u16;
    let star_char = '*' as u16;

    while v < value.len() {
        if p < pattern.len()
            && (pattern[p] == qmark
                || to_upper_invariant(value[v]) == to_upper_invariant(pattern[p]))
        {
            v += 1;
            p += 1;
        } else if p < pattern.len() && pattern[p] == star_char {
            star = Some(p);
            p += 1;
            restart = v;
        } else if let Some(s) = star {
            p = s + 1;
            restart += 1;
            v = restart;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == star_char {
        p += 1;
    }
    p == pattern.len()
}

fn wildcard_match(text: &str, pattern: &str) -> bool {
    let t_chars: Vec<char> = text.chars().map(|c| c.to_ascii_lowercase()).collect();
    let p_chars: Vec<char> = pattern.chars().map(|c| c.to_ascii_lowercase()).collect();

    let mut t_idx = 0;
    let mut p_idx = 0;
    let mut star_idx = None;
    let mut match_idx = 0;

    while t_idx < t_chars.len() {
        if p_idx < p_chars.len() && (p_chars[p_idx] == '?' || p_chars[p_idx] == t_chars[t_idx]) {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p_chars.len() && p_chars[p_idx] == '*' {
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

    while p_idx < p_chars.len() && p_chars[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == p_chars.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_relative_path_traversal() {
        assert!(validate_relative_path("../evil.txt").is_err());
        assert!(validate_relative_path("a/../../evil.txt").is_err());
        assert!(validate_relative_path("/root/evil.txt").is_err());
        assert!(validate_relative_path("C:/evil.txt").is_err());
        assert!(validate_relative_path("CON.txt").is_err());
        assert!(validate_relative_path("folder/NUL").is_err());
        assert!(validate_relative_path("good/sub/file.txt").is_ok());
        assert_eq!(
            validate_relative_path("./good/sub/file.txt").unwrap(),
            "good/sub/file.txt"
        );
    }

    #[test]
    fn test_wildcard_matching() {
        assert!(wildcard_match("test.txt", "*.txt"));
        assert!(wildcard_match("abc123xyz", "abc*xyz"));
        assert!(wildcard_match("file1.png", "file?.png"));
        assert!(!wildcard_match("file12.png", "file?.png"));
        assert!(matches_exclusion(
            "node_modules/pkg/index.js",
            &["node_modules/*".into()]
        ));
        assert!(matches_exclusion("photo.tmp", &["*.tmp".into()]));
        assert!(matches_exclusion("sub/dir/photo.tmp", &["*.tmp".into()]));
        assert!(matches_exclusion(
            "sub/dir/Thumbs.db",
            &["Thumbs.db".into()]
        ));
        assert!(matches_exclusion(
            "__MACOSX/._file.txt",
            &["__MACOSX/*".into()]
        ));
        assert!(!matches_exclusion("sub/dir/photo.jpg", &["*.tmp".into()]));

        // Inclusions
        assert!(matches_inclusion(
            "docs/readme.txt",
            false,
            &["*.txt".into()]
        ));
        assert!(matches_inclusion(
            "docs/readme.txt",
            false,
            &["docs/*".into()]
        ));
        assert!(matches_inclusion(
            "docs/images/pic.png",
            false,
            &["docs".into()]
        ));
        assert!(!matches_inclusion("src/main.rs", false, &["docs/*".into()]));
    }
}
