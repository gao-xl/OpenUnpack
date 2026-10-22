pub fn generate_unique_name(base: &str, existing: impl Fn(&str) -> bool) -> String {
    if !existing(base) {
        return base.to_string();
    }
    let mut counter = 1;
    loop {
        let candidate = format!("{}_{}", base, counter);
        if !existing(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

pub fn should_unwrap_single_root<'a, I>(entries: I) -> Option<&'a str>
where
    I: IntoIterator<Item = (&'a str, bool)>,
{
    let mut top_level_dir: Option<&'a str> = None;
    let mut entry_count = 0;

    for (path, is_dir) in entries {
        entry_count += 1;
        let mut segments = path.split(['/', '\\']).filter(|s| !s.is_empty());
        let first = segments.next()?;
        if segments.next().is_none() && !is_dir {
            return None;
        }
        match top_level_dir {
            None => top_level_dir = Some(first),
            Some(existing) if existing.eq_ignore_ascii_case(first) => {}
            _ => return None,
        }
    }

    if entry_count > 0 { top_level_dir } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_name() {
        let existing = |s: &str| s == "target" || s == "target_1";
        let unique = generate_unique_name("target", existing);
        assert_eq!(unique, "target_2");
    }

    #[test]
    fn test_single_root_detection() {
        let entries = vec![
            ("root/file1.txt", false),
            ("root/sub/file2.txt", false),
            ("root/sub", true),
        ];
        assert_eq!(should_unwrap_single_root(entries), Some("root"));

        let mixed = vec![("root/file1.txt", false), ("other.txt", false)];
        assert_eq!(should_unwrap_single_root(mixed), None);
    }
}
