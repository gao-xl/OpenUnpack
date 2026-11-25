//! Exclusion wildcard rules — the single canonical implementation.
//!
//! Three copies of this matcher used to live in the core scanner, this domain
//! module and the infrastructure path guard. They disagreed on case folding
//! (Unicode lowercase vs UTF-16 uppercase invariant), on whether a pattern
//! without a separator may match a full relative path, and on trimming. A rule
//! accepted by the preview therefore did not always exclude the same file
//! during extraction.
//!
//! Canonical semantics, applied identically by all callers:
//!
//! * `\` and `/` are equivalent separators; matching happens on `/`.
//! * Matching is case-insensitive using the Windows simple-uppercase folding:
//!   `ς`/`σ` compare equal, Turkish dotless `ı` does not compare equal to `I`.
//! * `*` matches any run of characters, including separators. `?` matches
//!   exactly one UTF-16 code unit — the unit the Windows filesystem actually
//!   stores — so an astral character (an emoji) needs `??`.
//! * A pattern is tested against the full relative path *and* against the final
//!   path component, so `*.tmp` also excludes `a/b/c.tmp` and `temp*` excludes
//!   everything below `temp/`.
//! * Surrounding whitespace is ignored; empty patterns and patterns above
//!   [`MAX_EXCLUSION_PATTERN_BYTES`] are rejected rather than partially applied.
//! * Paths longer than [`MAX_EXCLUSION_PATH_BYTES`] never match: they are not
//!   representable on any supported filesystem and only exist to burn CPU.

/// The rule wire limit is measured before case folding, in UTF-8 bytes.
pub const MAX_EXCLUSION_PATTERN_BYTES: usize = 512;
/// Anything longer than this cannot be a real relative path.
pub const MAX_EXCLUSION_PATH_BYTES: usize = 32_768;

pub fn is_exclusion_pattern_within_limit(pattern: &str) -> bool {
    pattern.len() <= MAX_EXCLUSION_PATTERN_BYTES
}

/// Case-insensitive wildcard match with `*` and `?`.
///
/// Iterative with single-star backtracking, so hostile patterns such as
/// `*a*a*a*a*b` cannot blow the stack or go exponential.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<u16> = pattern.encode_utf16().collect();
    let text: Vec<u16> = text.encode_utf16().collect();
    wildcard_match_utf16(&text, &pattern)
}

/// Simple uppercase folding for one UTF-16 unit.
///
/// Mirrors how Windows compares file names ordinally: multi-character uppercase
/// expansions are ignored, final sigma folds onto sigma, and the Turkish dotless
/// `ı` deliberately stays distinct from `I`.
fn fold(unit: u16) -> u16 {
    if unit == 0x0131 {
        return 0x0131;
    }
    if unit == 0x03C2 || unit == 0x03C3 {
        return 0x03A3;
    }
    if (b'a' as u16..=b'z' as u16).contains(&unit) {
        return unit - 32;
    }
    if let Some(ch) = char::from_u32(unit as u32) {
        let mut upper = ch.to_uppercase();
        if let Some(first) = upper.next()
            && upper.next().is_none()
            && (first as u32) <= 0xFFFF
        {
            return first as u32 as u16;
        }
    }
    unit
}

fn wildcard_match_utf16(value: &[u16], pattern: &[u16]) -> bool {
    const QUESTION: u16 = b'?' as u16;
    const STAR: u16 = b'*' as u16;
    let mut v = 0;
    let mut p = 0;
    let mut star: Option<usize> = None;
    let mut resume = 0;

    while v < value.len() {
        if p < pattern.len() && (pattern[p] == QUESTION || fold(pattern[p]) == fold(value[v])) {
            v += 1;
            p += 1;
        } else if p < pattern.len() && pattern[p] == STAR {
            star = Some(p);
            p += 1;
            resume = v;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            resume += 1;
            v = resume;
        } else {
            return false;
        }
    }

    while p < pattern.len() && pattern[p] == STAR {
        p += 1;
    }
    p == pattern.len()
}

/// Normalises a path for matching: separators unified, edges trimmed.
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches('/')
        .trim_end_matches('/')
        .to_string()
}

/// 判断 `path` 是否命中任一排除规则（全路径与 basename 双匹配）。
///
/// 这是预览、扫描与解压三个阶段共用的唯一判定入口；语义见模块级文档。
pub fn matches_exclusion(path: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() || path.len() > MAX_EXCLUSION_PATH_BYTES {
        return false;
    }
    let normalized = normalize_path(path);
    if normalized.is_empty() {
        return false;
    }
    let file_name = normalized.rsplit('/').next().unwrap_or(&normalized);

    for raw in patterns {
        let trimmed = raw.trim();
        if trimmed.is_empty() || !is_exclusion_pattern_within_limit(raw) {
            continue;
        }
        let pattern = normalize_path(trimmed);
        if pattern.is_empty() {
            continue;
        }
        if wildcard_match(&pattern, &normalized) {
            return true;
        }
        if wildcard_match(&pattern, file_name) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(list: &[&str]) -> Vec<String> {
        list.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn test_exclusions() {
        let patterns = vec![
            "*.tmp".to_string(),
            "temp/*".to_string(),
            "ignore.txt".to_string(),
            "?.txt".to_string(),
        ];
        assert!(matches_exclusion("foo/bar/test.tmp", &patterns));
        assert!(matches_exclusion("temp/nested/file.bin", &patterns));
        assert!(matches_exclusion("ignore.txt", &patterns));
        assert!(matches_exclusion("中.txt", &patterns));
        assert!(!matches_exclusion("foo/bar/keep.txt", &patterns));
    }

    #[test]
    fn rule_limit_counts_utf8_bytes_before_matching() {
        for length in [257, 512] {
            let pattern = "a".repeat(length);
            assert!(is_exclusion_pattern_within_limit(&pattern));
            assert!(matches_exclusion(&pattern, std::slice::from_ref(&pattern)));
        }
        let at_limit = format!("{}aa", "中".repeat(170));
        assert_eq!(at_limit.len(), 512);
        assert!(matches_exclusion(
            &at_limit,
            std::slice::from_ref(&at_limit)
        ));
        let too_long = format!("{}a", at_limit);
        assert!(!is_exclusion_pattern_within_limit(&too_long));
        assert!(!matches_exclusion(
            &too_long,
            std::slice::from_ref(&too_long)
        ));
        assert!(!is_exclusion_pattern_within_limit(&"a".repeat(513)));
    }

    #[test]
    fn star_crosses_separators_and_question_mark_is_one_scalar() {
        assert!(wildcard_match("*.tmp", "a/b/c.tmp"));
        assert!(wildcard_match("a/*/c", "a/b/c"));
        assert!(wildcard_match("?.txt", "中.txt"));
        assert!(!wildcard_match("?.txt", "ab.txt"));
        assert!(!wildcard_match("*.tmp", "a/b/c.tmpx"));
    }

    #[test]
    fn matching_is_case_insensitive_for_non_ascii() {
        assert!(wildcard_match("ÄRGER*", "ärger.txt"));
        assert!(matches_exclusion("Ordner/Ärger.TXT", &rules(&["ärger*"])));
    }

    #[test]
    fn question_mark_counts_utf16_units_like_the_windows_filesystem() {
        assert!(wildcard_match("?.txt", "a.txt"));
        assert!(wildcard_match("?.txt", "中.txt"));
        // An emoji is one scalar but two UTF-16 units: the extractor always
        // treated it as two, so the rule engine must agree.
        assert!(!wildcard_match("?.txt", "😀.txt"));
        assert!(wildcard_match("??.txt", "😀.txt"));
    }

    #[test]
    fn folding_follows_windows_ordinal_rules() {
        // Final sigma folds onto sigma.
        assert!(wildcard_match("σ.txt", "ς.txt"));
        // Turkish dotless i stays distinct from ASCII I.
        assert!(!wildcard_match("I.txt", "ı.txt"));
    }

    #[test]
    fn basename_and_full_path_are_both_considered() {
        assert!(matches_exclusion(
            "deep/nested/test.tmp",
            &rules(&["*.tmp"])
        ));
        assert!(matches_exclusion(
            "temp/nested/file.bin",
            &rules(&["temp*"])
        ));
        assert!(matches_exclusion("keep.txt", &rules(&["keep.txt"])));
        assert!(!matches_exclusion("foo/bar/keep.txt", &rules(&["*.tmp"])));
    }

    #[test]
    fn separators_are_interchangeable_and_trimmed() {
        assert!(matches_exclusion("a\\b\\c.tmp", &rules(&["  a/b/*.tmp  "])));
        assert!(matches_exclusion("/a/b/c.tmp", &rules(&["a/*.tmp"])));
    }

    #[test]
    fn oversize_and_empty_rules_are_rejected() {
        assert!(!is_exclusion_pattern_within_limit(
            &"a".repeat(MAX_EXCLUSION_PATTERN_BYTES + 1)
        ));
        assert!(is_exclusion_pattern_within_limit(
            &"a".repeat(MAX_EXCLUSION_PATTERN_BYTES)
        ));
        let huge = "a".repeat(MAX_EXCLUSION_PATH_BYTES + 1);
        assert!(!matches_exclusion(&huge, &rules(&["*"])));
        assert!(!matches_exclusion("a.txt", &rules(&["", "   "])));
    }

    #[test]
    fn pathological_patterns_stay_linear() {
        let pattern = "*a*a*a*a*a*a*a*a*b".to_string();
        let text = "a".repeat(4096);
        assert!(!matches_exclusion(&text, &[pattern]));
    }
}
