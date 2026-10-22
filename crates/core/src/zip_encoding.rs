use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipRawEntry {
    pub raw_bytes: Vec<u8>,
    pub is_utf8_flag: bool,
    pub is_directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipProbeResult {
    pub detected_encoding: String,
    pub decoded_paths: Vec<String>,
}

pub fn probe_zip_encoding(archive_path: &Path) -> Result<ZipProbeResult, String> {
    let mut file = File::open(archive_path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    probe_zip_encoding_from_reader(&mut file, len)
}

pub fn probe_zip_encoding_from_reader<R: Read + Seek>(
    reader: &mut R,
    len: u64,
) -> Result<ZipProbeResult, String> {
    if len < 22 {
        return Err("File too small for ZIP archive".into());
    }

    // 从文件末尾读取最多 65557 字节以寻找 EOCD 签名 0x06054b50
    let read_len = (len as usize).min(65557);
    let offset = len - read_len as u64;
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut tail = vec![0u8; read_len];
    reader.read_exact(&mut tail).map_err(|e| e.to_string())?;

    let eocd_sig = [0x50, 0x4b, 0x05, 0x06];
    let mut eocd_idx = None;
    for i in (0..=tail.len().saturating_sub(22)).rev() {
        if tail[i..i + 4] == eocd_sig
            && i + 22 + u16::from_le_bytes([tail[i + 20], tail[i + 21]]) as usize == tail.len()
        {
            eocd_idx = Some(i);
            break;
        }
    }

    let Some(eocd_pos) = eocd_idx else {
        return Err("Zip EOCD signature not found".into());
    };

    let eocd = &tail[eocd_pos..];
    let total_entries = u16::from_le_bytes([eocd[10], eocd[11]]) as usize;
    let cd_size = u32::from_le_bytes([eocd[12], eocd[13], eocd[14], eocd[15]]) as u64;
    let cd_offset = u32::from_le_bytes([eocd[16], eocd[17], eocd[18], eocd[19]]) as u64;
    if eocd[4..8] != [0; 4]
        || eocd[8..10] != eocd[10..12]
        || total_entries == u16::MAX as usize
        || cd_size == u32::MAX as u64
        || cd_offset == u32::MAX as u64
    {
        return Err("Multipart or ZIP64 encoding probe is unsupported".into());
    }

    const MAX_CD_SIZE: u64 = 16 * 1024 * 1024; // 16 MiB max central directory
    const MAX_ENTRIES: usize = 100_000;
    const MAX_NAME_LEN: usize = 4096;

    if total_entries > MAX_ENTRIES {
        return Err("ZIP central directory entry count exceeds safety limit (100,000)".into());
    }
    if cd_size > MAX_CD_SIZE {
        return Err("ZIP central directory size exceeds safety limit (16 MiB)".into());
    }
    if total_entries > 0 && cd_size == 0 {
        return Err(
            "Invalid ZIP central directory: declared cd_size is zero with non-zero entries".into(),
        );
    }
    if cd_offset + cd_size > offset + eocd_pos as u64 {
        return Err("Invalid ZIP central directory offset".into());
    }

    reader
        .seek(SeekFrom::Start(cd_offset))
        .map_err(|e| e.to_string())?;
    let mut entries = Vec::with_capacity(total_entries.min(100_000));
    let mut non_ascii_count = 0usize;
    let mut bytes_read: u64 = 0;

    for _ in 0..total_entries {
        if bytes_read + 46 > cd_size {
            return Err("Truncated ZIP central directory header".into());
        }
        let mut header = [0u8; 46];
        if reader.read_exact(&mut header).is_err() {
            return Err("Truncated ZIP central directory header".into());
        }
        if header[0..4] != [0x50, 0x4b, 0x01, 0x02] {
            return Err("Invalid ZIP central directory signature".into());
        }
        let bit_flag = u16::from_le_bytes([header[8], header[9]]);
        let is_utf8_flag = (bit_flag & 0x0800) != 0;
        let file_name_len = u16::from_le_bytes([header[28], header[29]]) as usize;
        let extra_len = u16::from_le_bytes([header[30], header[31]]) as usize;
        let comment_len = u16::from_le_bytes([header[32], header[33]]) as usize;

        let record_payload_len = (file_name_len + extra_len + comment_len) as u64;
        if bytes_read + 46 + record_payload_len > cd_size || file_name_len > MAX_NAME_LEN {
            return Err("Invalid ZIP central directory record length".into());
        }

        let mut name_bytes = vec![0u8; file_name_len];
        if reader.read_exact(&mut name_bytes).is_err() {
            return Err("Truncated ZIP central directory filename".into());
        }

        let is_directory = name_bytes.ends_with(b"/") || name_bytes.ends_with(b"\\");
        let has_non_ascii = name_bytes.iter().any(|&b| b >= 0x80);
        if has_non_ascii {
            non_ascii_count += 1;
        }

        entries.push(ZipRawEntry {
            raw_bytes: name_bytes,
            is_utf8_flag,
            is_directory,
        });

        bytes_read += 46 + record_payload_len;

        // 跳过 extra field 和 comment
        if extra_len + comment_len > 0 {
            reader
                .seek(SeekFrom::Current((extra_len + comment_len) as i64))
                .map_err(|e| e.to_string())?;
        }
    }

    // 判定编码 (借用 entries，不进行二次克隆)
    let non_utf8_flag_non_ascii: Vec<&[u8]> = entries
        .iter()
        .filter(|e| !e.is_utf8_flag && e.raw_bytes.iter().any(|&b| b >= 0x80))
        .map(|e| e.raw_bytes.as_slice())
        .collect();

    let detected_encoding = if non_utf8_flag_non_ascii.is_empty() {
        if non_ascii_count > 0 {
            "UTF-8".to_string()
        } else {
            "ASCII".to_string()
        }
    } else if non_utf8_flag_non_ascii
        .iter()
        .all(|bytes| std::str::from_utf8(bytes).is_ok())
    {
        "UTF-8".to_string()
    } else {
        let score_gbk = || -> Option<usize> {
            let mut score = 0usize;
            for bytes in &non_utf8_flag_non_ascii {
                let _ = encoding_rs::GB18030
                    .decode_without_bom_handling_and_without_replacement(bytes)?;
                let mut i = 0;
                while i < bytes.len() {
                    let b1 = bytes[i];
                    if b1 < 0x80 {
                        if b1.is_ascii_alphanumeric() {
                            score += 1;
                        }
                        i += 1;
                    } else if i + 1 < bytes.len() {
                        let b2 = bytes[i + 1];
                        if (0xB0..=0xF7).contains(&b1) && (0xA1..=0xFE).contains(&b2) {
                            score += 5; // GB2312 常用一二级汉字
                        } else if (0xA1..=0xA9).contains(&b1) && (0xA1..=0xFE).contains(&b2) {
                            score += 2; // GB2312 常用符号区
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            Some(score)
        };

        let score_sjis = || -> Option<usize> {
            let mut score = 0usize;
            for bytes in &non_utf8_flag_non_ascii {
                let decoded = encoding_rs::SHIFT_JIS
                    .decode_without_bom_handling_and_without_replacement(bytes)?;
                for c in decoded.chars() {
                    let u = c as u32;
                    if (0x3041..=0x3096).contains(&u)
                        || (0x30A1..=0x30FA).contains(&u)
                        || u == 0x30FC
                    {
                        score += 6; // 平假名、全角片假名、长音符
                    } else if (0x4E00..=0x9FFF).contains(&u) {
                        score += 3; // 日文汉字
                    } else if c.is_ascii_alphanumeric() {
                        score += 1;
                    }
                }
            }
            Some(score)
        };

        let score_big5 = || -> Option<usize> {
            let mut score = 0usize;
            for bytes in &non_utf8_flag_non_ascii {
                let _ =
                    encoding_rs::BIG5.decode_without_bom_handling_and_without_replacement(bytes)?;
                let mut i = 0;
                while i < bytes.len() {
                    let b1 = bytes[i];
                    if b1 < 0x80 {
                        if b1.is_ascii_alphanumeric() {
                            score += 1;
                        }
                        i += 1;
                    } else if i + 1 < bytes.len() {
                        let b2 = bytes[i + 1];
                        if (0xA4..=0xC6).contains(&b1)
                            && ((0x40..=0x7E).contains(&b2) || (0xA1..=0xFE).contains(&b2))
                        {
                            score += 4; // Big5 常用字区
                        } else if (0xC9..=0xF9).contains(&b1) {
                            score += 1; // Big5 次常用字区
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            Some(score)
        };

        let score_euckr = || -> Option<usize> {
            let mut score = 0usize;
            for bytes in &non_utf8_flag_non_ascii {
                let decoded = encoding_rs::EUC_KR
                    .decode_without_bom_handling_and_without_replacement(bytes)?;
                for c in decoded.chars() {
                    let u = c as u32;
                    if (0xAC00..=0xD7A3).contains(&u) {
                        score += 8; // Hangul syllables
                    } else if (0x1100..=0x11FF).contains(&u) || (0x3130..=0x318F).contains(&u) {
                        score += 4; // Hangul Jamo
                    } else if c.is_ascii_alphanumeric() {
                        score += 1;
                    }
                }
            }
            Some(score)
        };

        let mut candidates = Vec::new();
        if let Some(s) = score_gbk() {
            candidates.push(("GBK", s));
        }
        if let Some(s) = score_sjis() {
            candidates.push(("Shift-JIS", s));
        }
        if let Some(s) = score_big5() {
            candidates.push(("Big5", s));
        }
        if let Some(s) = score_euckr() {
            candidates.push(("EUC-KR", s));
        }
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));
        candidates
            .first()
            .map(|(name, _)| name.to_string())
            .unwrap_or_else(|| "GBK".to_string())
    };

    // 使用 detected_encoding 对所有条目解码
    let encoding = match detected_encoding.as_str() {
        "GBK" => encoding_rs::GB18030,
        "Shift-JIS" => encoding_rs::SHIFT_JIS,
        "Big5" => encoding_rs::BIG5,
        "EUC-KR" => encoding_rs::EUC_KR,
        _ => encoding_rs::UTF_8,
    };

    let decoded_paths = entries
        .iter()
        .map(|entry| {
            if entry.is_utf8_flag {
                String::from_utf8_lossy(&entry.raw_bytes).into_owned()
            } else {
                let (cow, _, _) = encoding.decode(&entry.raw_bytes);
                cow.into_owned()
            }
        })
        .collect();

    Ok(ZipProbeResult {
        detected_encoding,
        decoded_paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_synthetic_zip(name_bytes: &[u8], is_utf8: bool) -> Vec<u8> {
        let mut zip = Vec::new();
        let bit_flag: u16 = if is_utf8 { 0x0800 } else { 0x0000 };
        let name_len = name_bytes.len() as u16;

        // 1. Local File Header
        zip.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]); // sig
        zip.extend_from_slice(&20u16.to_le_bytes()); // version needed
        zip.extend_from_slice(&bit_flag.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // compression method: 0 (stored)
        zip.extend_from_slice(&0u16.to_le_bytes()); // time
        zip.extend_from_slice(&0u16.to_le_bytes()); // date
        zip.extend_from_slice(&0u32.to_le_bytes()); // crc32
        zip.extend_from_slice(&0u32.to_le_bytes()); // compressed size
        zip.extend_from_slice(&0u32.to_le_bytes()); // uncompressed size
        zip.extend_from_slice(&name_len.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // extra len
        zip.extend_from_slice(name_bytes);

        let cd_offset = zip.len() as u32;

        // 2. Central Directory Header
        zip.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]); // sig
        zip.extend_from_slice(&20u16.to_le_bytes()); // made by
        zip.extend_from_slice(&20u16.to_le_bytes()); // needed
        zip.extend_from_slice(&bit_flag.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // method
        zip.extend_from_slice(&0u16.to_le_bytes()); // time
        zip.extend_from_slice(&0u16.to_le_bytes()); // date
        zip.extend_from_slice(&0u32.to_le_bytes()); // crc32
        zip.extend_from_slice(&0u32.to_le_bytes()); // compressed size
        zip.extend_from_slice(&0u32.to_le_bytes()); // uncompressed size
        zip.extend_from_slice(&name_len.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // extra len
        zip.extend_from_slice(&0u16.to_le_bytes()); // comment len
        zip.extend_from_slice(&0u16.to_le_bytes()); // disk number start
        zip.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        zip.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        zip.extend_from_slice(&0u32.to_le_bytes()); // local header offset
        zip.extend_from_slice(name_bytes);

        let cd_size = (zip.len() as u32) - cd_offset;

        // 3. EOCD
        zip.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]); // sig
        zip.extend_from_slice(&0u16.to_le_bytes()); // disk number
        zip.extend_from_slice(&0u16.to_le_bytes()); // cd start disk
        zip.extend_from_slice(&1u16.to_le_bytes()); // total entries disk
        zip.extend_from_slice(&1u16.to_le_bytes()); // total entries
        zip.extend_from_slice(&cd_size.to_le_bytes());
        zip.extend_from_slice(&cd_offset.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // comment len

        zip
    }

    #[test]
    fn malformed_directory_never_returns_partial_encoding_results() {
        let original = build_synthetic_zip(b"safe.txt", false);
        let end = original.len() - 22;
        let cd = u32::from_le_bytes(original[end + 16..end + 20].try_into().unwrap()) as usize;
        for variant in 0..5 {
            let mut bytes = original.clone();
            match variant {
                0 => {
                    bytes[end + 8..end + 12].copy_from_slice(&[2, 0, 2, 0]);
                }
                1 => {
                    bytes[cd] = 0;
                }
                2 => {
                    bytes[cd + 28..cd + 30].copy_from_slice(&5000u16.to_le_bytes());
                }
                3 => {
                    bytes[end + 12..end + 16].copy_from_slice(&1u32.to_le_bytes());
                }
                _ => {
                    bytes[end + 20..end + 22].copy_from_slice(&1u16.to_le_bytes());
                }
            }
            let mut cursor = std::io::Cursor::new(&bytes);
            assert!(
                probe_zip_encoding_from_reader(&mut cursor, bytes.len() as u64).is_err(),
                "variant {variant}"
            );
        }
    }

    #[test]
    fn gbk_encoded_zip_is_detected_and_decoded_correctly() {
        let (gbk_bytes, _, _) = encoding_rs::GB18030.encode("测试目录/中文报告.txt");
        let zip_bytes = build_synthetic_zip(&gbk_bytes, false);
        let mut cursor = std::io::Cursor::new(&zip_bytes);

        let probe = probe_zip_encoding_from_reader(&mut cursor, zip_bytes.len() as u64).unwrap();
        assert_eq!(probe.detected_encoding, "GBK");
        assert_eq!(probe.decoded_paths.len(), 1);
        assert_eq!(probe.decoded_paths[0], "测试目录/中文报告.txt");
    }

    #[test]
    fn utf8_flagged_zip_is_detected_as_utf8() {
        let text = "Unicode目录/测试.txt";
        let zip_bytes = build_synthetic_zip(text.as_bytes(), true);
        let mut cursor = std::io::Cursor::new(&zip_bytes);

        let probe = probe_zip_encoding_from_reader(&mut cursor, zip_bytes.len() as u64).unwrap();
        assert_eq!(probe.detected_encoding, "UTF-8");
        assert_eq!(probe.decoded_paths.len(), 1);
        assert_eq!(probe.decoded_paths[0], text);
    }

    #[test]
    fn shift_jis_encoded_zip_is_detected_and_decoded_correctly() {
        let (sjis_bytes, _, _) = encoding_rs::SHIFT_JIS.encode("ゲーム資料/日本語ファイル.txt");
        let zip_bytes = build_synthetic_zip(&sjis_bytes, false);
        let mut cursor = std::io::Cursor::new(&zip_bytes);

        let probe = probe_zip_encoding_from_reader(&mut cursor, zip_bytes.len() as u64).unwrap();
        assert_eq!(probe.detected_encoding, "Shift-JIS");
        assert_eq!(probe.decoded_paths.len(), 1);
        assert_eq!(probe.decoded_paths[0], "ゲーム資料/日本語ファイル.txt");
    }

    #[test]
    fn big5_encoded_zip_is_detected_and_decoded_correctly() {
        let (big5_bytes, _, _) = encoding_rs::BIG5.encode("繁體資料夾/測試報告.txt");
        let zip_bytes = build_synthetic_zip(&big5_bytes, false);
        let mut cursor = std::io::Cursor::new(&zip_bytes);

        let probe = probe_zip_encoding_from_reader(&mut cursor, zip_bytes.len() as u64).unwrap();
        assert_eq!(probe.detected_encoding, "Big5");
        assert_eq!(probe.decoded_paths.len(), 1);
        assert_eq!(probe.decoded_paths[0], "繁體資料夾/測試報告.txt");
    }

    #[test]
    fn euckr_encoded_zip_is_detected_and_decoded_correctly() {
        let (euckr_bytes, _, _) = encoding_rs::EUC_KR.encode("한국어폴더/테스트보고서.txt");
        let zip_bytes = build_synthetic_zip(&euckr_bytes, false);
        let mut cursor = std::io::Cursor::new(&zip_bytes);

        let probe = probe_zip_encoding_from_reader(&mut cursor, zip_bytes.len() as u64).unwrap();
        assert_eq!(probe.detected_encoding, "EUC-KR");
        assert_eq!(probe.decoded_paths.len(), 1);
        assert_eq!(probe.decoded_paths[0], "한국어폴더/테스트보고서.txt");
    }
}
