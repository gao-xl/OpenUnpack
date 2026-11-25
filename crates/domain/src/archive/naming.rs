//! 输出命名规则：冲突改名与分卷名解析。

/// 基于 `existing` 谓词生成不冲突的输出名。
///
/// 冲突时追加 `_1`、`_2`……序号直到空闲；谓词由调用方提供
/// （通常查询目标目录或内存索引）。
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

/// 判断压缩包是否只包含一个顶层目录，若是则返回该目录名。
///
/// 用于"智能解包"：单一根目录时把内容直接释放到输出根，
/// 避免出现 `输出/唯一文件夹/...` 的冗余层级。
///
/// 返回 `None` 的情形：条目为空、存在顶层散文件、或顶层目录多于一个。
/// 目录名比较使用 ASCII 忽略大小写（与 Windows 行为一致）。
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
