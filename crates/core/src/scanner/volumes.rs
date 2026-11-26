//! 跨平台文件系统序数文件名键与分卷压缩包命名模式分类。

use super::*;

// Use the same non-linguistic Windows comparison for sibling discovery,
// grouping and path deduplication. Unicode lowercasing can expand characters
// (for example dotted I), so it is not an ordinal filename identity key.
#[cfg(windows)]
#[derive(Clone, Debug)]
pub(super) struct FileNameKey(Vec<u16>);

#[cfg(windows)]
impl Ord for FileNameKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        use windows_sys::Win32::Globalization::CompareStringOrdinal;

        if self.0 == other.0 {
            return Ordering::Equal;
        }
        // These keys come from filesystem paths, which are bounded well below
        // i32::MAX UTF-16 units. Explicit lengths also avoid NUL termination.
        let left_len = i32::try_from(self.0.len()).expect("filesystem path length");
        let right_len = i32::try_from(other.0.len()).expect("filesystem path length");
        let result = unsafe {
            CompareStringOrdinal(self.0.as_ptr(), left_len, other.0.as_ptr(), right_len, 1)
        };
        match result {
            1 => Ordering::Less,
            2 => Ordering::Equal,
            3 => Ordering::Greater,
            // Do not merge distinct names if the platform comparison fails.
            _ => self.0.cmp(&other.0),
        }
    }
}

#[cfg(windows)]
impl PartialOrd for FileNameKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(windows)]
impl PartialEq for FileNameKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

#[cfg(windows)]
impl Eq for FileNameKey {}

#[cfg(not(windows))]
pub(super) type FileNameKey = String;

pub(super) fn file_name_key(value: &str) -> FileNameKey {
    #[cfg(windows)]
    {
        FileNameKey(value.encode_utf16().collect())
    }
    #[cfg(not(windows))]
    {
        // Preserve the pre-existing fallback on unsupported host platforms.
        value.to_ascii_lowercase()
    }
}

#[derive(PartialEq, Eq)]
pub(super) enum VolumeFamilyKey {
    Numeric(FileNameKey, String),
    PartRar(FileNameKey),
    OldRar(FileNameKey),
    SplitZip(FileNameKey),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VolumeType {
    NumericSplit {
        stem: String,
        ext: String,
        num: u32,
        width: usize,
    },
    PartRar {
        stem: String,
        num: u32,
        width: usize,
    },
    OldRar {
        stem: String,
        num: Option<u32>,
    },
    SplitZip {
        stem: String,
        num: Option<u32>,
    },
    Single,
}

impl VolumeType {
    pub(crate) fn stem(&self) -> Option<&str> {
        match self {
            VolumeType::NumericSplit { stem, .. } => Some(stem),
            VolumeType::PartRar { stem, .. } => Some(stem),
            VolumeType::OldRar { stem, .. } => Some(stem),
            VolumeType::SplitZip { stem, .. } => Some(stem),
            VolumeType::Single => None,
        }
    }
}

pub(crate) fn classify_volume(filename: &str) -> VolumeType {
    if let Some(caps) = RE_CLASSIFY_NUM.captures(filename)
        && let (Some(stem), Some(ext), Some(num_str)) = (caps.get(1), caps.get(2), caps.get(3))
        && let Ok(num) = num_str.as_str().parse::<u32>()
    {
        return VolumeType::NumericSplit {
            stem: stem.as_str().to_string(),
            ext: ext.as_str().to_ascii_lowercase(),
            num,
            width: num_str.as_str().len(),
        };
    }

    if let Some(caps) = RE_CLASSIFY_PART.captures(filename)
        && let (Some(stem), Some(num_str)) = (caps.get(1), caps.get(2))
        && let Ok(num) = num_str.as_str().parse::<u32>()
    {
        return VolumeType::PartRar {
            stem: stem.as_str().to_string(),
            num,
            width: num_str.as_str().len(),
        };
    }

    if let Some(caps) = RE_CLASSIFY_OLD_R.captures(filename)
        && let (Some(stem), Some(num_str)) = (caps.get(1), caps.get(2))
        && let Ok(num) = num_str.as_str().parse::<u32>()
    {
        return VolumeType::OldRar {
            stem: stem.as_str().to_string(),
            num: Some(num),
        };
    }

    if let Some(caps) = RE_CLASSIFY_OLD_RAR_BASE.captures(filename)
        && let Some(stem) = caps.get(1)
    {
        return VolumeType::OldRar {
            stem: stem.as_str().to_string(),
            num: None,
        };
    }

    if let Some(caps) = RE_CLASSIFY_Z_SPLIT.captures(filename)
        && let (Some(stem), Some(num_str)) = (caps.get(1), caps.get(2))
        && let Ok(num) = num_str.as_str().parse::<u32>()
    {
        return VolumeType::SplitZip {
            stem: stem.as_str().to_string(),
            num: Some(num),
        };
    }

    if let Some(caps) = RE_CLASSIFY_ZIP_BASE.captures(filename)
        && let Some(stem) = caps.get(1)
    {
        return VolumeType::SplitZip {
            stem: stem.as_str().to_string(),
            num: None,
        };
    }

    VolumeType::Single
}

pub(super) fn volume_family(filename: &str) -> Option<VolumeFamilyKey> {
    match classify_volume(filename) {
        VolumeType::NumericSplit { stem, ext, .. } => {
            Some(VolumeFamilyKey::Numeric(file_name_key(&stem), ext))
        }
        VolumeType::PartRar { stem, .. } => Some(VolumeFamilyKey::PartRar(file_name_key(&stem))),
        VolumeType::OldRar { stem, .. } => Some(VolumeFamilyKey::OldRar(file_name_key(&stem))),
        VolumeType::SplitZip { stem, .. } => Some(VolumeFamilyKey::SplitZip(file_name_key(&stem))),
        VolumeType::Single => None,
    }
}
