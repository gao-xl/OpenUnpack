//! Windows OEM / ANSI code page transcoding adapter.
//!
//! Provides conversion for 7-Zip console output, Windows pipe output, and legacy
//! archive entries (such as CP936, CP932, CP950, etc.) to valid UTF-8 strings.

fn score_gbk(bytes: &[u8]) -> Option<usize> {
    let _ = encoding_rs::GB18030.decode_without_bom_handling_and_without_replacement(bytes)?;
    let mut score = 0usize;
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
    Some(score)
}

fn score_sjis(bytes: &[u8]) -> Option<usize> {
    let decoded =
        encoding_rs::SHIFT_JIS.decode_without_bom_handling_and_without_replacement(bytes)?;
    let mut score = 0usize;
    for c in decoded.chars() {
        let u = c as u32;
        if (0x3041..=0x3096).contains(&u) || (0x30A1..=0x30FA).contains(&u) || u == 0x30FC {
            score += 10; // 平假名、全角片假名、长音符
        } else if (0x4E00..=0x9FFF).contains(&u) {
            score += 3; // 日文汉字
        } else if c.is_ascii_alphanumeric() {
            score += 1;
        }
    }
    Some(score)
}

fn score_big5(bytes: &[u8]) -> Option<usize> {
    let _ = encoding_rs::BIG5.decode_without_bom_handling_and_without_replacement(bytes)?;
    let mut score = 0usize;
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
                score += 5; // Big5 常用字区
            } else if (0xC9..=0xF9).contains(&b1) {
                score += 2; // Big5 次常用字区
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    Some(score)
}

fn score_euckr(bytes: &[u8]) -> Option<usize> {
    let decoded = encoding_rs::EUC_KR.decode_without_bom_handling_and_without_replacement(bytes)?;
    let mut score = 0usize;
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
    Some(score)
}

/// Decodes bytes that may be encoded in UTF-8, Windows OEM code page (CP936, CP932, etc.),
/// or Windows ANSI code page (CP_ACP), guaranteeing a valid UTF-8 string.
pub fn decode_oem_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    // 1. If it is already valid UTF-8 without errors, return immediately.
    if let Ok(valid_utf8) = std::str::from_utf8(bytes) {
        return valid_utf8.to_string();
    }

    // 2. Score candidate encodings based on linguistic distribution
    let mut scored: Vec<(&'static encoding_rs::Encoding, usize)> = Vec::new();
    if let Some(s) = score_sjis(bytes) {
        scored.push((encoding_rs::SHIFT_JIS, s));
    }
    if let Some(s) = score_gbk(bytes) {
        scored.push((encoding_rs::GB18030, s));
    }
    if let Some(s) = score_big5(bytes) {
        scored.push((encoding_rs::BIG5, s));
    }
    if let Some(s) = score_euckr(bytes) {
        scored.push((encoding_rs::EUC_KR, s));
    }

    // Bonus for matching system OEM/ACP code page
    #[cfg(windows)]
    let (system_oem, system_acp) = {
        unsafe extern "system" {
            fn GetOEMCP() -> u32;
            fn GetACP() -> u32;
        }
        unsafe { (GetOEMCP(), GetACP()) }
    };

    #[cfg(not(windows))]
    let (system_oem, system_acp): (u32, u32) = (65001, 65001);

    for (enc, score) in &mut scored {
        let is_system = match system_oem {
            936 | 54936 if *enc == encoding_rs::GB18030 => true,
            932 if *enc == encoding_rs::SHIFT_JIS => true,
            950 if *enc == encoding_rs::BIG5 => true,
            949 if *enc == encoding_rs::EUC_KR => true,
            _ => match system_acp {
                936 | 54936 if *enc == encoding_rs::GB18030 => true,
                932 if *enc == encoding_rs::SHIFT_JIS => true,
                950 if *enc == encoding_rs::BIG5 => true,
                949 if *enc == encoding_rs::EUC_KR => true,
                _ => false,
            },
        };
        if is_system {
            *score = score.saturating_add(2);
        }
    }

    scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));

    if let Some((enc, _)) = scored.first()
        && let Some(decoded) = enc.decode_without_bom_handling_and_without_replacement(bytes)
    {
        return decoded.to_string();
    }

    // 3. Fallback to WINDOWS_1252 if valid
    if let Some(decoded) =
        encoding_rs::WINDOWS_1252.decode_without_bom_handling_and_without_replacement(bytes)
    {
        return decoded.to_string();
    }

    // 4. On Windows, attempt OS-native MultiByteToWideChar with CP_OEMCP / CP_ACP
    #[cfg(windows)]
    {
        use windows_sys::Win32::Globalization::{CP_ACP, CP_OEMCP, MultiByteToWideChar};
        let len = bytes.len() as i32;
        for &cp in &[CP_OEMCP, CP_ACP] {
            let wide_len =
                unsafe { MultiByteToWideChar(cp, 0, bytes.as_ptr(), len, std::ptr::null_mut(), 0) };
            if wide_len > 0 {
                let mut wide_buf = vec![0u16; wide_len as usize];
                let res = unsafe {
                    MultiByteToWideChar(cp, 0, bytes.as_ptr(), len, wide_buf.as_mut_ptr(), wide_len)
                };
                if res > 0 {
                    return String::from_utf16_lossy(&wide_buf);
                }
            }
        }
    }

    // 5. Final fallback: lossy UTF-8
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf8_passthrough() {
        let original = "Hello, 世界! / test / 123";
        assert_eq!(decode_oem_text(original.as_bytes()), original);
    }

    #[test]
    fn test_cp936_chinese_decoding() {
        let (encoded, _, _) = encoding_rs::GB18030.encode("中文解压测试目录/报表.xlsx");
        let decoded = decode_oem_text(&encoded);
        assert_eq!(decoded, "中文解压测试目录/报表.xlsx");
    }

    #[test]
    fn test_cp932_japanese_decoding() {
        let (encoded, _, _) = encoding_rs::SHIFT_JIS.encode("ゲームデータ/日本語テスト.txt");
        let decoded = decode_oem_text(&encoded);
        assert_eq!(decoded, "ゲームデータ/日本語テスト.txt");
    }

    #[test]
    fn test_cp950_traditional_chinese_decoding() {
        let (encoded, _, _) = encoding_rs::BIG5.encode("繁體中文目錄/檔案.doc");
        let decoded = decode_oem_text(&encoded);
        assert_eq!(decoded, "繁體中文目錄/檔案.doc");
    }

    #[test]
    fn test_empty_bytes() {
        assert_eq!(decode_oem_text(&[]), "");
    }
}
