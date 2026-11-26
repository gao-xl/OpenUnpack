#[cfg(windows)]
#[test]
fn unicode_volume_families_use_one_comparison_for_every_input_selection() {
    for (first_stem, next_stem) in [("A", "a"), ("Ä", "Ä"), ("Ä", "ä")] {
        for (first_suffix, next_suffix) in [
            (".7z.001", ".7z.002"),
            (".part01.rar", ".part02.rar"),
            (".rar", ".r00"),
            (".zip", ".z01"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let first = dir.path().join(format!("{first_stem}{first_suffix}"));
            let next = dir.path().join(format!("{next_stem}{next_suffix}"));
            fs::write(&first, b"synthetic volume").unwrap();
            fs::write(&next, b"synthetic volume").unwrap();
            let first = first.to_string_lossy().into_owned();
            let next = next.to_string_lossy().into_owned();
            for selected in [
                vec![dir.path().to_string_lossy().into_owned()],
                vec![first.clone()],
                vec![next.clone()],
                vec![first.clone(), next.clone()],
            ] {
                let result = scan(&selected, false, &[]).unwrap();
                assert_eq!(result.sources.len(), 1, "{selected:?}");
                let source = &result.sources[0];
                assert_eq!(source.volumes.len(), 2, "{selected:?}");
                assert!(!source.missing_volume, "{selected:?}");
                assert_eq!(
                    source.volumes.iter().collect::<BTreeSet<_>>().len(),
                    2,
                    "selected aliases must not duplicate a volume: {selected:?}"
                );
            }
        }
    }
}

#[cfg(windows)]
#[test]
fn distinct_unicode_names_are_not_merged_by_linguistic_case_expansion() {
    // Default Unicode lowercase merges the first pair; full uppercase can
    // merge the second. Ordinal filename comparison must keep both apart.
    for (left, right) in [("İ", "i\u{307}"), ("ß", "ss"), ("Ä", "A\u{308}")] {
        assert_ne!(file_name_key(left), file_name_key(right));
        let dir = tempfile::tempdir().unwrap();
        for stem in [left, right] {
            for number in [1, 2] {
                fs::write(dir.path().join(format!("{stem}.7z.{number:03}")), b"volume").unwrap();
            }
        }
        let result = scan(&[dir.path().to_string_lossy().into_owned()], false, &[]).unwrap();
        assert_eq!(result.sources.len(), 2, "{left:?} versus {right:?}");
        for source in result.sources {
            assert_eq!(source.volumes.len(), 2);
            assert!(!source.missing_volume);
        }
        let selected = scan(
            &[dir
                .path()
                .join(format!("{left}.7z.001"))
                .to_string_lossy()
                .into_owned()],
            false,
            &[],
        )
        .unwrap();
        assert_eq!(selected.sources.len(), 1);
        assert_eq!(selected.sources[0].volumes.len(), 2);
    }
}

#[test]
fn scanner_rejects_executables_and_zstd_including_first_volumes() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "archive.zip",
        "archive.7z.001",
        "archive.part01.rar",
        "archive.z01",
    ] {
        let path = root.path().join(name);
        std::fs::write(&path, b"MZ synthetic executable").unwrap();
        assert!(!is_supported_archive_path(&path, name), "{name}");
        std::fs::write(&path, [0x28, 0xb5, 0x2f, 0xfd]).unwrap();
        assert!(!is_supported_archive_path(&path, name), "{name}");
    }
    let executable = root.path().join("archive.exe");
    std::fs::write(&executable, b"PK\x03\x04").unwrap();
    assert!(!is_supported_archive_path(&executable, "archive.exe"));
    let continuation = root.path().join("archive.7z.002");
    std::fs::write(&continuation, b"MZ raw compressed continuation").unwrap();
    assert!(is_supported_archive_path(&continuation, "archive.7z.002"));
}

#[test]
fn sfx_intake_matches_engine_header_and_signature_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("boundary.sfx");
    for size in [63usize, 64, 67, 68] {
        let mut bytes = vec![0u8; size];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[8..14].copy_from_slice(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]);
        fs::write(&path, &bytes).unwrap();
        assert_eq!(is_supported_archive_path(&path, "boundary.sfx"), size >= 64);
        bytes[..2].copy_from_slice(b"PK");
        fs::write(&path, &bytes).unwrap();
        assert!(!is_supported_archive_path(&path, "boundary.sfx"));
    }
    for signature in [
        vec![0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00],
        vec![0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00],
    ] {
        let mut bytes = vec![0u8; 64];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[8..8 + signature.len()].copy_from_slice(&signature);
        fs::write(&path, &bytes).unwrap();
        assert!(is_supported_archive_path(&path, "boundary.sfx"));
        bytes[14] = 0x02;
        fs::write(&path, &bytes).unwrap();
        assert!(!is_supported_archive_path(&path, "boundary.sfx"));
    }
}

#[test]
fn test_f4_expanded_formats_and_sfx_support() {
    let root = tempfile::tempdir().unwrap();

    // WIM format
    let wim = root.path().join("image.wim");
    std::fs::write(&wim, b"MSWIM\0\0\0dummy payload").unwrap();
    assert!(is_supported_archive_path(&wim, "image.wim"));

    // LZH format (-lh5- at offset 2)
    let lzh = root.path().join("data.lzh");
    std::fs::write(&lzh, b"\x20\x20-lh5-dummy lzh payload").unwrap();
    assert!(is_supported_archive_path(&lzh, "data.lzh"));

    // LZ4 format (04 22 4D 18)
    let lz4 = root.path().join("log.lz4");
    std::fs::write(&lz4, [0x04, 0x22, 0x4D, 0x18, 0x64, 0x40, 0xA7]).unwrap();
    assert!(is_supported_archive_path(&lz4, "log.lz4"));

    // Zstd format (28 B5 2F FD)
    let zst = root.path().join("backup.tar.zst");
    std::fs::write(&zst, [0x28, 0xB5, 0x2F, 0xFD, 0x00, 0x00]).unwrap();
    assert!(is_supported_archive_path(&zst, "backup.tar.zst"));

    // Verified SFX archive (MZ stub followed by 7z archive payload)
    let sfx = root.path().join("bundle.sfx.exe");
    let mut sfx_data = b"MZ\x00\x00synthetic stub bytes".to_vec();
    sfx_data.resize(128, 0);
    sfx_data.extend_from_slice(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0x00, 0x04]);
    std::fs::write(&sfx, &sfx_data).unwrap();
    assert!(is_supported_archive_path(&sfx, "bundle.sfx.exe"));

    // Synthetic non-archive executable named bundle.sfx.exe (no archive payload)
    let fake_sfx = root.path().join("fake.sfx.exe");
    std::fs::write(
        &fake_sfx,
        b"MZ synthetic plain binary without archive payload",
    )
    .unwrap();
    assert!(!is_supported_archive_path(&fake_sfx, "fake.sfx.exe"));
}
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
    assert_eq!(
        extract_password_from_filename("game_解压密码=superpass_2026.zip"),
        Some("superpass_2026".to_string())
    );
    assert_eq!(
        extract_password_from_filename("forum_release_[www.gamersky.com].rar"),
        Some("www.gamersky.com".to_string())
    );
    assert_eq!(extract_password_from_filename("normal_archive.zip"), None);

    let multi = extract_passwords_from_filename("archive_[bbs.3dmgame.com]_密码：668899.7z");
    assert!(multi.contains(&"668899".to_string()));
    assert!(multi.contains(&"bbs.3dmgame.com".to_string()));
}

#[test]
fn test_parse_passwords_from_text() {
    let content = "感谢下载！\r\n解压密码：www.downsite.com\r\n提取码: 9988\r\n祝使用愉快！";
    let found = parse_passwords_from_text(content);
    assert!(found.contains(&"www.downsite.com".to_string()));
    assert!(found.contains(&"9988".to_string()));

    let short_pwd_file = "my_secret_code_2026\r\n";
    let short_found = parse_passwords_from_text(short_pwd_file);
    assert_eq!(short_found, vec!["my_secret_code_2026".to_string()]);
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
fn scan_rule_validation_uses_utf8_byte_limit() {
    for pattern in [
        "a".repeat(257),
        "a".repeat(512),
        format!("{}aa", "中".repeat(170)),
    ] {
        assert!(matches_exclusion(&pattern, std::slice::from_ref(&pattern)));
        assert!(scan(&[], false, &[pattern]).is_ok());
    }
    for pattern in [
        "a".repeat(513),
        "中".repeat(171),
        format!("{}a", "😀".repeat(128)),
    ] {
        assert!(!matches_exclusion(&pattern, std::slice::from_ref(&pattern)));
        assert!(
            scan(&[], false, &[pattern])
                .unwrap_err()
                .contains("512 UTF-8")
        );
    }
    assert!(scan(&[], false, &vec!["*.tmp".into(); 101]).is_err());
}

#[test]
fn scan_excludes_directory_children_without_changing_selected_file_scope() {
    let root = tempfile::tempdir().unwrap();
    let excluded = root.path().join("temp");
    std::fs::create_dir(&excluded).unwrap();
    let archive = excluded.join("child.zip");
    std::fs::write(&archive, b"synthetic archive candidate").unwrap();
    let patterns = vec!["temp".into()];
    let folder = scan(
        &[root.path().to_string_lossy().into_owned()],
        true,
        &patterns,
    )
    .unwrap();
    assert!(folder.sources.is_empty());
    // Selecting the file directly does not reinterpret its parent's name.
    let selected = scan(&[archive.to_string_lossy().into_owned()], true, &patterns).unwrap();
    assert_eq!(selected.sources.len(), 1);
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
    f_xz.write_all(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00, 0x00])
        .unwrap();

    let dir_str = d.path().to_string_lossy().to_string();
    let res = scan(&[dir_str], false, &[]).unwrap();
    assert_eq!(res.sources.len(), 3);
    let names: Vec<_> = res
        .sources
        .iter()
        .map(|s| s.display_name.as_str())
        .collect();
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
    assert!(
        res.sources[0].missing_volume,
        "缺少首卷必须被标记为 missing_volume"
    );
}
