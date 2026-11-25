//! 分卷压缩包命名识别。

/// 分卷文件名的解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumePattern {
    /// 数字分卷（`.7z.001` / `.zip.001`），`base` 不含尾部分卷号。
    Numeric { base: String, part: u32 },
    /// WinRAR 新式分卷（`archive.part01.rar`），`width` 保留补零宽度。
    RarPart {
        base: String,
        part: u32,
        width: usize,
    },
    /// 旧式分卷扩展名（`.r00` / `.z01`），`kind` 记录 r 或 z。
    Legacy { base: String, kind: char, part: u32 },
    /// 非分卷的普通文件。
    Single(String),
}

/// 解析分卷压缩包文件名，识别三种分卷命名风格。
///
/// 按优先级依次尝试：`.partNN.rar` → `.7z/.zip.NNN` → 旧式 `.rNN`/`.zNN`；
/// 均不匹配时归为 [`VolumePattern::Single`]。扩展名比较使用 ASCII 小写。
pub fn parse_volume_name(file_name: &str) -> VolumePattern {
    let lower = file_name.to_ascii_lowercase();

    // Check .part01.rar
    if let Some(pos) = lower.find(".part") {
        let after_part = &file_name[pos + 5..];
        if let Some(dot_rar) = after_part.to_ascii_lowercase().find(".rar") {
            let num_str = &after_part[..dot_rar];
            if let Ok(part) = num_str.parse::<u32>() {
                return VolumePattern::RarPart {
                    base: file_name[..pos].to_string(),
                    part,
                    width: num_str.len(),
                };
            }
        }
    }

    // Check .7z.001 or .zip.001
    if let Some(pos) = lower.rfind('.') {
        let ext = &lower[pos + 1..];
        if let Ok(part) = ext.parse::<u32>() {
            let before_dot = &file_name[..pos];
            if before_dot.to_ascii_lowercase().ends_with(".7z")
                || before_dot.to_ascii_lowercase().ends_with(".zip")
            {
                return VolumePattern::Numeric {
                    base: before_dot.to_string(),
                    part,
                };
            }
        }
    }

    // Check legacy .r00, .z01
    if let Some(pos) = lower.rfind('.') {
        let ext = &lower[pos + 1..];
        if ext.len() >= 3
            && let Some(first_char) = ext.chars().next()
            && (first_char == 'r' || first_char == 'z')
            && let Ok(part) = ext[1..].parse::<u32>()
        {
            return VolumePattern::Legacy {
                base: file_name[..pos].to_string(),
                kind: first_char,
                part,
            };
        }
    }

    VolumePattern::Single(file_name.to_string())
}
