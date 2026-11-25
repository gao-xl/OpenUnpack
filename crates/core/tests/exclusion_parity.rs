//! One rule engine, three call sites.
//!
//! The scanner (file discovery), the domain preview (what the UI shows) and the
//! extractor's path guard each used to own a wildcard matcher. They disagreed
//! on case folding and on whether a separator-free pattern may match a full
//! relative path, so a rule could hide a file in the preview yet extract it.
//! This test pins the three entry points to the same answers.

use openunpack_core::scanner;
use openunpack_infrastructure::engine::path_safety;

fn rules(list: &[&str]) -> Vec<String> {
    list.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn every_entry_point_agrees_on_the_same_rules() {
    let cases: &[(&str, &[&str], bool)] = &[
        ("a/b/c.tmp", &["*.tmp"], true),
        ("a/b/c.tmp", &["*.TMP"], true),
        ("a/b/c.tmp", &["c.tmp"], true),
        ("a/b/c.tmp", &["a/*"], true),
        ("a/b/c.tmp", &["a/b/c.tmp"], true),
        ("a/b/c.tmp", &["d/*"], false),
        ("temp/nested/file.bin", &["temp*"], true),
        ("deep/test.tmp", &["*.tmp"], true),
        ("中.txt", &["?.txt"], true),
        ("😀.txt", &["?.txt"], false),
        ("😀.txt", &["??.txt"], true),
        ("中.txt", &["??.txt"], false),
        ("Ärger.TXT", &["ärger*"], true),
        ("a\\b\\c.tmp", &["a/b/*.tmp"], true),
        ("/a/b/c.tmp", &["a/*.tmp"], true),
        ("keep.txt", &["  keep.txt  "], true),
        ("dir/.hidden", &["*"], true),
        ("dir/x.log", &["", "   "], false),
    ];

    for (path, patterns, expected) in cases {
        let patterns = rules(patterns);
        let from_scanner = scanner::matches_exclusion(path, &patterns);
        let from_extractor = path_safety::matches_exclusion(path, &patterns);
        assert_eq!(
            from_scanner, *expected,
            "scanner disagreed for {path} with {patterns:?}"
        );
        assert_eq!(
            from_extractor, *expected,
            "extractor disagreed for {path} with {patterns:?}"
        );
    }
}
