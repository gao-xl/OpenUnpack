/// The rule wire limit is measured before case folding, in UTF-8 bytes.
pub const MAX_EXCLUSION_PATTERN_BYTES: usize = 512;

pub fn is_exclusion_pattern_within_limit(pattern: &str) -> bool {
    pattern.len() <= MAX_EXCLUSION_PATTERN_BYTES
}

pub fn matches_exclusion(path: &str, patterns: &[String]) -> bool {
    if patterns.is_empty() || path.len() > 32768 {
        return false;
    }
    let normalized = path.replace('\\', "/");
    let norm_lower = normalized.to_lowercase();
    let file_name = norm_lower.rsplit('/').next().unwrap_or(&norm_lower);

    for raw in patterns {
        if raw.trim().is_empty() || !is_exclusion_pattern_within_limit(raw) {
            continue;
        }
        let pattern = raw.replace('\\', "/").to_lowercase();
        if wildcard_match(&pattern, &norm_lower) {
            return true;
        }
        if wildcard_match(&pattern, file_name) {
            return true;
        }
    }
    false
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p_chars: Vec<char> = pattern.chars().collect();
    let t_chars: Vec<char> = text.chars().collect();
    let mut p = 0;
    let mut t = 0;
    let mut star = None;
    let mut match_idx = 0;

    while t < t_chars.len() {
        if p < p_chars.len() && (p_chars[p] == '?' || p_chars[p] == t_chars[t]) {
            p += 1;
            t += 1;
        } else if p < p_chars.len() && p_chars[p] == '*' {
            star = Some(p);
            p += 1;
            match_idx = t;
        } else if let Some(s) = star {
            p = s + 1;
            match_idx += 1;
            t = match_idx;
        } else {
            return false;
        }
    }

    while p < p_chars.len() && p_chars[p] == '*' {
        p += 1;
    }
    p == p_chars.len()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
