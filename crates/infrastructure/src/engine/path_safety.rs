//! 解压路径安全闸门：Zip Slip 防御、排除/包含规则委托入口。
//!
//! 排除匹配的唯一真源在 `openunpack-domain::archive::exclusions`，
//! 本模块只做委托，保证预览与解压永不出现判定分歧。

use std::path::{Component, Path};

/// 校验并规范化压缩包内相对路径，防御 Zip Slip / 路径穿越攻击。
///
/// 拒绝项：绝对路径（含盘符与根斜杠）、`..` 穿越段、NUL/冒号等非法字符、
/// 尾随点/空格段、Windows 保留设备名（CON/NUL/COM1…）、深度超过 128。
/// 通过时返回正斜杠规范化后的相对路径。
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

/// 判断段基名是否为 Windows 保留设备名（忽略大小写）。
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

/// 判断相对路径是否命中排除规则。委托 `openunpack-domain` 的唯一实现：
/// 预览面板与解压引擎对"是否排除"的判定必须永远一致。
pub use openunpack_domain::archive::exclusions::MAX_EXCLUSION_PATTERN_BYTES;

pub fn matches_exclusion(path: &str, patterns: &[String]) -> bool {
    openunpack_domain::archive::exclusions::matches_exclusion(path, patterns)
}

/// 批量预览输出排除结果：对每条路径给出是否会被排除。
///
/// 语义比单条匹配更强：命中排除规则的目录，其全部子孙路径
/// 也标记为排除（与实际解压时目录整棵跳过的行为一致）。
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

/// 判断相对路径是否命中包含（inclusion）规则。
///
/// 规则集为空时视为全量包含（返回 true）。支持三类形式：
/// `./前缀` 显式选择（含目录递归）、纯目录名前缀递归、通配符匹配
/// （带分隔符时对全路径匹配，纯文件名模式对 basename 匹配）。
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
        if openunpack_domain::archive::exclusions::wildcard_match(&pattern, target) {
            return true;
        }
    }
    false
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
        // Canonical rule order is (pattern, text): see openunpack-domain.
        assert!(openunpack_domain::archive::exclusions::wildcard_match(
            "*.txt", "test.txt"
        ));
        assert!(openunpack_domain::archive::exclusions::wildcard_match(
            "abc*xyz",
            "abc123xyz"
        ));
        assert!(openunpack_domain::archive::exclusions::wildcard_match(
            "file?.png",
            "file1.png"
        ));
        assert!(!openunpack_domain::archive::exclusions::wildcard_match(
            "file?.png",
            "file12.png"
        ));
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
