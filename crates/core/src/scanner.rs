use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use walkdir::WalkDir;

use crate::types::{ScanNotice, ScanResult, SourceGroup};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

// Use the same non-linguistic Windows comparison for sibling discovery,
// grouping and path deduplication. Unicode lowercasing can expand characters
// (for example dotted I), so it is not an ordinal filename identity key.
#[cfg(windows)]
#[derive(Clone, Debug)]
struct FileNameKey(Vec<u16>);

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
type FileNameKey = String;

fn file_name_key(value: &str) -> FileNameKey {
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
enum VolumeFamilyKey {
    Numeric(FileNameKey, String),
    PartRar(FileNameKey),
    OldRar(FileNameKey),
    SplitZip(FileNameKey),
}

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x00000400;
// Wire rules are bounded by UTF-8 bytes, matching Domain, UI and Worker.
pub const MAX_EXCLUSION_PATTERN_BYTES: usize = 512;
/// Hard ceiling on collected archive candidates. A user-selected tree can be
/// arbitrarily large, and an unbounded `Vec<PathBuf>` would exhaust memory long
/// before the UI could show a result.
pub const MAX_SCAN_FILES: usize = 200_000;
/// Ceiling on visited entries, bounding CPU time for trees that are mostly
/// non-archive files (where the candidate cap would never trigger).
pub const MAX_SCAN_ENTRIES: usize = 1_000_000;

static RE_NUMERIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.(?:7z|zip)\.\d+$").expect("valid RE_NUMERIC regex"));
static RE_PART: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.part\d+\.rar$").expect("valid RE_PART regex"));
static RE_OLD_RAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.r\d{2,}$").expect("valid RE_OLD_RAR regex"));
static RE_SPLIT_ZIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.z\d{2,}$").expect("valid RE_SPLIT_ZIP regex"));
static RE_DISGUISED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(?:zip|7z|rar)\.[a-zA-Z0-9_-]+$").expect("valid RE_DISGUISED regex")
});
static RE_MULTI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(?:(?:7z|zip)\.\d+|part\d+\.rar|r\d+|z\d+)$").expect("valid RE_MULTI regex")
});
static RE_CLASSIFY_NUM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?i)(.+)\.(7z|zip)\.(\d+)$").expect("valid RE_CLASSIFY_NUM regex")
});
static RE_CLASSIFY_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?i)(.+)\.part(\d+)\.rar$").expect("valid RE_CLASSIFY_PART regex")
});
static RE_CLASSIFY_OLD_R: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i)(.+)\.r(\d+)$").expect("valid RE_CLASSIFY_OLD_R regex"));
static RE_CLASSIFY_OLD_RAR_BASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i)(.+)\.rar$").expect("valid RE_CLASSIFY_OLD_RAR_BASE regex"));
static RE_CLASSIFY_Z_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i)(.+)\.z(\d+)$").expect("valid RE_CLASSIFY_Z_SPLIT regex"));
static RE_CLASSIFY_ZIP_BASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i)(.+)\.zip$").expect("valid RE_CLASSIFY_ZIP_BASE regex"));
static RE_PASSWORD_EXPLICIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:解压密码|解压码|提取码|提取|密码|(?:^|[^\w])(?:password|passwd|pwd|pass|code))[：:=_\s-]*([^\s\[\]【】()（）{}]+)")
        .expect("valid RE_PASSWORD_EXPLICIT regex")
});
static RE_PASSWORD_BRACKET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\[【({]([^\[\]【】(){}（）]+)[\]】)}]").expect("valid RE_PASSWORD_BRACKET regex")
});
static RE_DOMAIN_IN_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:https?://)?([a-zA-Z0-9-]+\.(?:com|cn|net|org|cc|me|vip|top|xyz|info|io|tv|club|online|site|icu))")
        .expect("valid RE_DOMAIN_IN_NAME regex")
});

pub fn is_archive_candidate_name(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    let single_exts = [
        ".zip", ".7z", ".rar", ".tar", ".gz", ".bz2", ".xz", ".cab", ".wim", ".swm", ".esd",
        ".lzh", ".lha", ".zst", ".zstd", ".tzst", ".lz4", ".sfx",
    ];
    if single_exts.iter().any(|ext| lower.ends_with(ext)) {
        return true;
    }
    RE_MULTI.is_match(&lower)
}

pub fn matches_exclusion(relative: &str, patterns: &[String]) -> bool {
    if relative.len() > 32768 {
        return false;
    }
    let norm_path = relative.replace('\\', "/");
    let norm_path_lower = norm_path.to_lowercase();

    for pat in patterns {
        if pat.is_empty() || pat.len() > MAX_EXCLUSION_PATTERN_BYTES {
            continue;
        }
        let norm_pat = pat.replace('\\', "/").to_lowercase();
        if wildcard_match(&norm_pat, &norm_path_lower) {
            return true;
        }
        // Also allow matching solely against file/directory base name
        if let Some(filename) = norm_path_lower.rsplit('/').next()
            && wildcard_match(&norm_pat, filename)
        {
            return true;
        }
    }
    false
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    // '?' denotes one Unicode scalar value, not one byte of UTF-8.
    let p_bytes: Vec<char> = pattern.chars().collect();
    let t_bytes: Vec<char> = text.chars().collect();
    let mut p_idx = 0;
    let mut t_idx = 0;
    let mut star_idx = None;
    let mut match_idx = 0;

    while t_idx < t_bytes.len() {
        if p_idx < p_bytes.len() && (p_bytes[p_idx] == '?' || p_bytes[p_idx] == t_bytes[t_idx]) {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p_bytes.len() && p_bytes[p_idx] == '*' {
            star_idx = Some(p_idx);
            p_idx += 1;
            match_idx = t_idx;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            t_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < p_bytes.len() && p_bytes[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == p_bytes.len()
}

pub fn output_name(path: &str) -> String {
    let p = Path::new(path);
    let file_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("archive");

    if let Some(mat) = RE_NUMERIC.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    if let Some(mat) = RE_PART.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    if let Some(mat) = RE_OLD_RAR.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    if let Some(mat) = RE_SPLIT_ZIP.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    let exts = [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.lz4", ".sfx.exe", ".zip", ".7z",
        ".rar", ".tar", ".gz", ".bz2", ".xz", ".cab", ".wim", ".swm", ".esd", ".lzh", ".lha",
        ".zst", ".zstd", ".tzst", ".lz4", ".sfx",
    ];
    let lower_name = file_name.to_ascii_lowercase();
    for ext in &exts {
        if lower_name.ends_with(ext) {
            let base = &file_name[..file_name.len() - ext.len()];
            if !base.is_empty() {
                return base.to_string();
            }
        }
    }

    // Secondary disguised extensions like foo.zip.mp4, bar.7z.png, etc.
    if let Some(mat) = RE_DISGUISED.find(file_name) {
        let base = &file_name[..mat.start()];
        if !base.is_empty() {
            return base.to_string();
        }
    }

    if let Some(stem) = p.file_stem().and_then(|s| s.to_str())
        && !stem.is_empty()
    {
        return stem.to_string();
    }
    "archive".to_string()
}

fn is_reparse_point_or_symlink(path: &Path) -> bool {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return true;
        }
        #[cfg(windows)]
        {
            if (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
                return true;
            }
        }
    }
    false
}

fn is_container_extension(lower: &str) -> bool {
    let containers = [
        ".docx", ".xlsx", ".pptx", ".jar", ".war", ".ear", ".apk", ".aab", ".ipa", ".epub", ".xpi",
        ".vsix", ".nupkg", ".whl", ".odt", ".ods", ".odp", ".crx",
    ];
    containers.iter().any(|ext| lower.ends_with(ext))
}

fn has_archive_magic_in_buf(buf: &[u8]) -> bool {
    let n = buf.len();
    if n < 2 {
        return false;
    }
    // GZIP: 1F 8B
    if buf[0] == 0x1F && buf[1] == 0x8B {
        return true;
    }
    // BZIP2: 42 5A 68 ('B', 'Z', 'h')
    if n >= 3 && buf[0] == 0x42 && buf[1] == 0x5A && buf[2] == 0x68 {
        return true;
    }
    if n < 4 {
        return false;
    }
    // ZIP: PK\x03\x04, PK\x05\x06 (empty), PK\x07\x08 (spanned)
    if buf[0] == 0x50
        && buf[1] == 0x4B
        && ((buf[2] == 0x03 && buf[3] == 0x04)
            || (buf[2] == 0x05 && buf[3] == 0x06)
            || (buf[2] == 0x07 && buf[3] == 0x08))
    {
        return true;
    }
    // CAB: 'M', 'S', 'C', 'F'
    if buf[0] == 0x4D && buf[1] == 0x53 && buf[2] == 0x43 && buf[3] == 0x46 {
        return true;
    }
    // 7z: 37 7A BC AF 27 1C
    if n >= 6 && buf[..6] == [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C] {
        return true;
    }
    // XZ: FD 37 7A 58 5A 00
    if n >= 6 && buf[..6] == [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00] {
        return true;
    }
    // RAR4 / RAR5: 52 61 72 21 1A 07 00 or 52 61 72 21 1A 07 01 00
    if n >= 7
        && buf[..6] == [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07]
        && (buf[6] == 0x00 || (n >= 8 && buf[6] == 0x01 && buf[7] == 0x00))
    {
        return true;
    }
    // WIM: 4D 53 57 49 4D 00 00 00 (MSWIM\0\0\0) or WLPWM\0\0\0
    if n >= 8 && (&buf[..8] == b"MSWIM\0\0\0" || &buf[..8] == b"WLPWM\0\0\0") {
        return true;
    }
    // LZH: offset 2 has -lh or -lz with trailing -
    if n >= 7 && buf[2] == b'-' && (buf[3] == b'l' || buf[3] == b'z') && buf[6] == b'-' {
        return true;
    }
    // LZ4: 04 22 4D 18
    if n >= 4 && buf[..4] == [0x04, 0x22, 0x4D, 0x18] {
        return true;
    }
    // Zstandard: 28 B5 2F FD
    if n >= 4 && buf[..4] == [0x28, 0xB5, 0x2F, 0xFD] {
        return true;
    }
    // TAR (POSIX ustar header at offset 257)
    if has_tar_header(buf) {
        return true;
    }
    false
}

fn has_sfx_archive_payload(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 1024 * 512]; // scan up to 512KB for SFX embedded payload
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    if n < 64 || &buf[..2] != b"MZ" {
        return false;
    }
    // Look for 7z, ZIP or RAR magic after the DOS/PE stub (skip first 2 bytes "MZ")
    let haystack = &buf[2..n];
    haystack
        .windows(6)
        .any(|w| w == [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C])
        || haystack.windows(4).any(|w| w == [0x50, 0x4B, 0x03, 0x04])
        || haystack
            .windows(7)
            .any(|w| w == [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00])
        || haystack
            .windows(8)
            .any(|w| w == [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00])
}

fn has_tar_header(bytes: &[u8]) -> bool {
    if bytes.len() < 512 {
        return false;
    }
    let (mut expected, mut digit, mut ended) = (0u32, false, false);
    for &value in &bytes[148..156] {
        if value == 0 || value == b' ' {
            if digit {
                ended = true;
            }
            continue;
        }
        if ended || !(b'0'..=b'7').contains(&value) {
            return false;
        }
        digit = true;
        expected = expected * 8 + u32::from(value - b'0');
    }
    digit
        && expected > 0
        && expected
            == bytes[..512]
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    if (148..156).contains(&i) {
                        32
                    } else {
                        u32::from(v)
                    }
                })
                .sum::<u32>()
}

pub fn is_supported_archive_path(path: &Path, filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    if is_container_extension(&lower) {
        return false;
    }
    let is_explicit_sfx = lower.ends_with(".sfx") || lower.ends_with(".sfx.exe");
    if (lower.ends_with(".exe") || lower.ends_with(".com")) && !is_explicit_sfx {
        return false;
    }
    // Continuation volumes are raw compressed bytes and may coincidentally start
    // with MZ. Only the leading volume has an archive signature to inspect.
    let continuation = matches!(
        classify_volume(filename),
        VolumeType::NumericSplit { num: 2.., .. }
            | VolumeType::PartRar { num: 2.., .. }
            | VolumeType::OldRar { num: Some(_), .. }
            | VolumeType::SplitZip { num: Some(2..), .. }
    );
    if RE_MULTI.is_match(&lower) && continuation {
        return true;
    }
    let mut header = [0u8; 512];
    let read_count = {
        use std::io::Read;
        fs::File::open(path)
            .and_then(|mut file| file.read(&mut header))
            .unwrap_or(0)
    };

    if read_count >= 2 && &header[..2] == b"MZ" {
        if !is_explicit_sfx {
            if !has_tar_header(&header[..read_count]) {
                return false;
            }
        } else if !has_sfx_archive_payload(path) {
            return false;
        }
    }

    if read_count >= 4
        && header[..4] == [0x28, 0xB5, 0x2F, 0xFD]
        && !lower.ends_with(".zst")
        && !lower.ends_with(".zstd")
        && !lower.ends_with(".tzst")
    {
        return false;
    }

    if is_explicit_sfx {
        return has_sfx_archive_payload(path);
    }

    if RE_MULTI.is_match(&lower) {
        return true;
    }

    let single_exts = [
        ".zip", ".7z", ".rar", ".tar", ".gz", ".bz2", ".xz", ".cab", ".wim", ".swm", ".esd",
        ".lzh", ".lha", ".zst", ".zstd", ".tzst", ".lz4", ".sfx",
    ];
    if single_exts.iter().any(|ext| lower.ends_with(ext)) {
        return true;
    }

    // Disguised archive detection via Magic Bytes in already-read header buffer
    has_archive_magic_in_buf(&header[..read_count])
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

fn volume_family(filename: &str) -> Option<VolumeFamilyKey> {
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

pub fn extract_passwords_from_filename(filename: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    let mut add = |candidate: &str| {
        let mut trimmed = candidate
            .trim()
            .trim_matches(|c: char| {
                c == '.'
                    || c == ','
                    || c == '!'
                    || c == '?'
                    || c == ';'
                    || c == ':'
                    || c == '。'
                    || c == '！'
                    || c == '；'
                    || c == '：'
                    || c == '-'
                    || c == '_'
            })
            .trim();

        // Strip trailing archive extension if candidate captured the file extension at end
        let exts = [
            ".zip", ".rar", ".7z", ".tar.gz", ".tar.bz2", ".tar.xz", ".tar", ".gz", ".bz2", ".xz",
            ".sfx.exe", ".sfx",
        ];
        for ext in &exts {
            if trimmed.to_ascii_lowercase().ends_with(ext) {
                trimmed = trimmed[..trimmed.len() - ext.len()].trim();
                break;
            }
        }

        if trimmed.len() >= 2 && trimmed.len() <= 128 {
            let lower = trimmed.to_ascii_lowercase();
            if !lower.ends_with(".part")
                && !lower.starts_with("part")
                && seen.insert(trimmed.to_string())
            {
                results.push(trimmed.to_string());
            }
        }
    };

    // 1. Explicit matches on full filename: 密码123456, pwd=abc, 解压密码:xyz
    for caps in RE_PASSWORD_EXPLICIT.captures_iter(filename) {
        if let Some(m) = caps.get(1) {
            add(m.as_str());
        }
    }

    // 2. Bracket hints like [123456], 【密码: 123456】, (pwd=abc), [bbs.site.com]
    for caps in RE_PASSWORD_BRACKET.captures_iter(filename) {
        if let Some(m) = caps.get(1) {
            let inner = m.as_str().trim();
            if let Some(sub) = RE_PASSWORD_EXPLICIT.captures(inner)
                && let Some(sub_m) = sub.get(1)
            {
                add(sub_m.as_str());
            } else if inner.to_ascii_lowercase().starts_with("pw")
                || inner.to_ascii_lowercase().starts_with("pass")
            {
                let stripped = inner
                    .trim_start_matches(|c: char| {
                        c.is_alphabetic()
                            || c == ':'
                            || c == '：'
                            || c == '='
                            || c == '_'
                            || c == '-'
                    })
                    .trim();
                if !stripped.is_empty() {
                    add(stripped);
                }
            } else if inner.contains('.') && RE_DOMAIN_IN_NAME.is_match(inner) {
                add(inner);
            }
        }
    }

    // 3. Domain hints anywhere in filename (frequently used as passwords for forum releases)
    for caps in RE_DOMAIN_IN_NAME.captures_iter(filename) {
        if let Some(m) = caps.get(1) {
            add(m.as_str());
        }
    }

    results
}

pub fn extract_password_from_filename(filename: &str) -> Option<String> {
    extract_passwords_from_filename(filename).into_iter().next()
}

pub fn parse_passwords_from_text(content: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    let lines: Vec<&str> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    for line in &lines {
        for caps in RE_PASSWORD_EXPLICIT.captures_iter(line) {
            if let Some(m) = caps.get(1) {
                let s = m
                    .as_str()
                    .trim()
                    .trim_matches(|c: char| {
                        c == '.'
                            || c == ','
                            || c == '!'
                            || c == '?'
                            || c == ';'
                            || c == ':'
                            || c == '。'
                            || c == '！'
                            || c == '；'
                            || c == '：'
                    })
                    .trim();
                if s.len() >= 2 && s.len() <= 128 && seen.insert(s.to_string()) {
                    results.push(s.to_string());
                }
            }
        }
    }

    // If file is very concise (<= 3 lines, e.g. dedicated password.txt),
    // check each line as a direct candidate
    if lines.len() <= 3 {
        for line in lines {
            let s = line
                .trim()
                .trim_matches(|c: char| {
                    c == '.'
                        || c == ','
                        || c == '!'
                        || c == '?'
                        || c == ';'
                        || c == ':'
                        || c == '。'
                        || c == '！'
                        || c == '；'
                        || c == '：'
                })
                .trim();
            if s.len() >= 2 && s.len() <= 64 && !s.contains(' ') && seen.insert(s.to_string()) {
                results.push(s.to_string());
            }
        }
    }

    results
}

pub fn sniff_accompanying_passwords(archive_path: &Path) -> Vec<String> {
    sniff_accompanying_passwords_authorized(archive_path, |_| true)
}

/// A companion is optional input, so denied or unreadable files are skipped.
/// The caller supplies its current task policy rather than granting access to
/// every file beside an otherwise authorized archive.
pub fn sniff_accompanying_passwords_authorized(
    archive_path: &Path,
    mut can_read: impl FnMut(&Path) -> bool,
) -> Vec<String> {
    let parent = match archive_path.parent() {
        Some(p) if p.is_dir() => p,
        _ => return Vec::new(),
    };
    // Pin the whole parent chain, then open each immediate child without
    // following reparses or permitting writers/replacement during inspection.
    let directory = match crate::paths::DirectoryGuard::open(parent) {
        Ok(directory) => directory,
        Err(_) => return Vec::new(),
    };

    let stem = archive_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    let mut candidate_files = Vec::new();

    // 1. Same stem companions: <stem>.txt, <stem>.nfo, <stem>.md
    if !stem.is_empty() {
        candidate_files.push(parent.join(format!("{stem}.txt")));
        candidate_files.push(parent.join(format!("{stem}.nfo")));
        candidate_files.push(parent.join(format!("{stem}.md")));
        if let Some(pos) = stem.to_ascii_lowercase().rfind(".part") {
            let base_stem = &stem[..pos];
            if !base_stem.is_empty() {
                candidate_files.push(parent.join(format!("{base_stem}.txt")));
                candidate_files.push(parent.join(format!("{base_stem}.nfo")));
            }
        }
    }

    // 2. Generic companion files
    let generic_names = [
        "password.txt",
        "passwords.txt",
        "pwd.txt",
        "pw.txt",
        "pass.txt",
        "code.txt",
        "密码.txt",
        "解压密码.txt",
        "提取码.txt",
        "说明.txt",
        "readme.txt",
        "README.txt",
        "read_me.txt",
        "info.txt",
    ];
    for name in &generic_names {
        candidate_files.push(parent.join(name));
    }

    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for file_path in candidate_files {
        if !can_read(&file_path) {
            continue;
        }
        let file = match directory.open_file_with_share(
            &file_path,
            windows_sys::Win32::Foundation::GENERIC_READ,
            windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ,
        ) {
            Ok(file) => file,
            Err(_) => continue,
        };
        let meta = match file.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        const MAX_COMPANION_BYTES: u64 = 64 * 1024;
        if !meta.is_file() || meta.len() > MAX_COMPANION_BYTES || meta.len() == 0 {
            continue;
        }
        // Recheck policy against the pinned namespace before reading content.
        // The bound is on the actual read, not just an earlier length sample.
        if !can_read(&file_path) {
            continue;
        }
        let mut raw_bytes = Vec::with_capacity(meta.len() as usize);
        if file
            .take(MAX_COMPANION_BYTES + 1)
            .read_to_end(&mut raw_bytes)
            .is_err()
            || raw_bytes.len() as u64 > MAX_COMPANION_BYTES
        {
            continue;
        }

        let content = match std::str::from_utf8(&raw_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                let (cow, _, _) = encoding_rs::GB18030.decode(&raw_bytes);
                cow.into_owned()
            }
        };

        for p in parse_passwords_from_text(&content) {
            if seen.insert(p.clone()) {
                results.push(p);
            }
        }
    }

    results
}

fn push_candidate(files: &mut Vec<PathBuf>, path: PathBuf) -> Result<(), String> {
    if files.len() >= MAX_SCAN_FILES {
        return Err(format!(
            "扫描到的压缩包超过上限（{MAX_SCAN_FILES} 个），请缩小目录范围或增加排除规则。"
        ));
    }
    files.push(path);
    Ok(())
}

pub fn scan(
    paths: &[String],
    recursive: bool,
    exclusions: &[String],
) -> Result<ScanResult, String> {
    scan_with_cancel(paths, recursive, exclusions, &AtomicBool::new(false))
}
pub fn scan_with_cancel(
    paths: &[String],
    recursive: bool,
    exclusions: &[String],
    cancel: &AtomicBool,
) -> Result<ScanResult, String> {
    if exclusions.len() > 100
        || exclusions
            .iter()
            .any(|pattern| pattern.len() > MAX_EXCLUSION_PATTERN_BYTES)
    {
        return Err("排除规则过多或过长：最多 100 条，单条不超过 512 UTF-8 字节。".into());
    }
    let mut notices = Vec::new();
    let mut collected_files = Vec::new();
    let mut seen_canonical = BTreeSet::new();
    let mut scanned = 0usize;

    for p in paths {
        if cancel.load(Ordering::Acquire) {
            return Err("扫描已取消。".into());
        }
        let path = Path::new(p);
        if !path.exists() {
            notices.push(ScanNotice {
                path: p.clone(),
                message: "指定路径不存在".to_string(),
            });
            continue;
        }

        if path.is_file() {
            if is_reparse_point_or_symlink(path) {
                continue;
            }
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if matches_exclusion(file_name, exclusions) {
                continue;
            }
            if !is_supported_archive_path(path, file_name) {
                continue;
            }
            match path.canonicalize() {
                Ok(cpath) => {
                    let key = file_name_key(&cpath.to_string_lossy());
                    if seen_canonical.insert(key) {
                        push_candidate(&mut collected_files, cpath)?;
                    }
                }
                Err(_) => {
                    notices.push(ScanNotice {
                        path: p.clone(),
                        message: "无法访问文件".to_string(),
                    });
                }
            }
            // Selecting any volume authorizes only its sibling family, not unrelated archives.
            if let (Some(family), Some(parent)) = (volume_family(file_name), path.parent())
                && let Ok(siblings) = fs::read_dir(parent)
            {
                for sibling in siblings.flatten() {
                    if cancel.load(Ordering::Acquire) {
                        return Err("扫描已取消。".into());
                    }
                    let sibling_path = sibling.path();
                    if is_reparse_point_or_symlink(&sibling_path) || !sibling_path.is_file() {
                        continue;
                    }
                    let name = sibling.file_name().to_string_lossy().into_owned();
                    if volume_family(&name).as_ref() != Some(&family)
                        || matches_exclusion(&name, exclusions)
                    {
                        continue;
                    }
                    if let Ok(cpath) = sibling_path.canonicalize()
                        && seen_canonical.insert(file_name_key(&cpath.to_string_lossy()))
                    {
                        push_candidate(&mut collected_files, cpath)?;
                    }
                }
            }
            continue;
        }

        if path.is_dir() {
            let max_depth = if recursive { usize::MAX } else { 1 };
            let walker = WalkDir::new(path)
                .follow_links(false)
                .max_depth(max_depth)
                .into_iter()
                .filter_entry(|entry| {
                    !cancel.load(Ordering::Acquire)
                        && !is_reparse_point_or_symlink(entry.path())
                        && (!entry.file_type().is_dir()
                            || entry.depth() == 0
                            || (!entry.path().join(crate::paths::OWNER_FILE).is_file()
                                && !matches_exclusion(
                                    &entry
                                        .path()
                                        .strip_prefix(path)
                                        .unwrap_or(entry.path())
                                        .to_string_lossy(),
                                    exclusions,
                                )))
                });

            for entry_res in walker {
                scanned += 1;
                if scanned > MAX_SCAN_ENTRIES {
                    return Err(format!(
                        "扫描遍历的条目超过上限（{MAX_SCAN_ENTRIES}），请缩小目录范围或增加排除规则。"
                    ));
                }
                if cancel.load(Ordering::Acquire) {
                    return Err("扫描已取消。".into());
                }
                let entry = match entry_res {
                    Ok(e) => e,
                    Err(_) => {
                        notices.push(ScanNotice {
                            path: p.clone(),
                            message: "遍历目录时发生读取错误".to_string(),
                        });
                        continue;
                    }
                };

                let entry_path = entry.path();
                if is_reparse_point_or_symlink(entry_path) {
                    continue;
                }

                if entry_path.is_file() {
                    let rel = match entry_path.strip_prefix(path) {
                        Ok(r) => r.to_string_lossy().to_string(),
                        Err(_) => entry_path.to_string_lossy().to_string(),
                    };

                    if matches_exclusion(&rel, exclusions) {
                        continue;
                    }

                    let file_name = match entry_path.file_name().and_then(|n| n.to_str()) {
                        Some(n) => n,
                        None => continue,
                    };

                    if !is_supported_archive_path(entry_path, file_name) {
                        continue;
                    }

                    match entry_path.canonicalize() {
                        Ok(cpath) => {
                            let key = file_name_key(&cpath.to_string_lossy());
                            if seen_canonical.insert(key) {
                                push_candidate(&mut collected_files, cpath)?;
                            }
                        }
                        Err(_) => {
                            notices.push(ScanNotice {
                                path: entry_path.to_string_lossy().to_string(),
                                message: "无法读取文件信息".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    let mut dir_map: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for f in collected_files {
        if cancel.load(Ordering::Acquire) {
            return Err("扫描已取消。".into());
        }
        if let Some(parent) = f.parent() {
            dir_map.entry(parent.to_path_buf()).or_default().push(f);
        }
    }

    let mut sources = Vec::new();

    for (dir, files) in dir_map {
        if cancel.load(Ordering::Acquire) {
            return Err("扫描已取消。".into());
        }
        let mut numeric_groups: BTreeMap<(FileNameKey, String), BTreeMap<u32, (PathBuf, usize)>> =
            BTreeMap::new();
        let mut part_groups: BTreeMap<FileNameKey, BTreeMap<u32, (PathBuf, usize)>> =
            BTreeMap::new();
        let mut old_rar_groups: BTreeMap<FileNameKey, (Option<PathBuf>, BTreeMap<u32, PathBuf>)> =
            BTreeMap::new();
        let mut split_zip_groups: BTreeMap<FileNameKey, (Option<PathBuf>, BTreeMap<u32, PathBuf>)> =
            BTreeMap::new();
        let mut standalone_files: Vec<PathBuf> = Vec::new();

        for file in files {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            let fname = match file.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            match classify_volume(fname) {
                VolumeType::NumericSplit {
                    stem,
                    ext,
                    num,
                    width,
                } => {
                    numeric_groups
                        .entry((file_name_key(&stem), ext))
                        .or_default()
                        .insert(num, (file, width));
                }
                VolumeType::PartRar { stem, num, width } => {
                    part_groups
                        .entry(file_name_key(&stem))
                        .or_default()
                        .insert(num, (file, width));
                }
                VolumeType::OldRar { stem, num } => {
                    let entry = old_rar_groups.entry(file_name_key(&stem)).or_default();
                    if let Some(n) = num {
                        entry.1.insert(n, file);
                    } else {
                        entry.0 = Some(file);
                    }
                }
                VolumeType::SplitZip { stem, num } => {
                    let entry = split_zip_groups.entry(file_name_key(&stem)).or_default();
                    if let Some(n) = num {
                        entry.1.insert(n, file);
                    } else {
                        entry.0 = Some(file);
                    }
                }
                VolumeType::Single => {
                    standalone_files.push(file);
                }
            }
        }

        for (_, map) in numeric_groups {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            let Some(&min_k) = map.keys().next() else {
                continue;
            };
            let mut missing = min_k != 1;
            let mut expected = min_k;
            for &k in map.keys() {
                if k != expected {
                    missing = true;
                }
                expected = k.saturating_add(1);
            }
            // Check disk for missing volumes
            if let Some((first_file, width)) = map.values().next() {
                let first_stem = output_name(&first_file.to_string_lossy());
                let full_stem = first_file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&first_stem);
                if min_k > 1 {
                    let candidate = dir.join(format!("{}.{:0width$}", full_stem, 1, width = width));
                    if !candidate.exists() {
                        missing = true;
                    }
                }
            }

            let mut vols = Vec::new();
            let mut total_size = 0u64;
            for (p, _) in map.values() {
                vols.push(p.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(p) {
                    total_size += meta.len();
                }
            }
            let first_path = vols[0].clone();
            let disp = output_name(&first_path);
            sources.push(SourceGroup {
                path: first_path,
                display_name: disp,
                volumes: vols,
                size: total_size,
                missing_volume: missing,
            });
        }

        for (_, map) in part_groups {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            let Some(&min_k) = map.keys().next() else {
                continue;
            };
            let mut missing = min_k != 1;
            let mut expected = min_k;
            for &k in map.keys() {
                if k != expected {
                    missing = true;
                }
                expected = k.saturating_add(1);
            }

            let mut vols = Vec::new();
            let mut total_size = 0u64;
            for (p, _) in map.values() {
                vols.push(p.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(p) {
                    total_size += meta.len();
                }
            }
            let first_path = vols[0].clone();
            let disp = output_name(&first_path);
            sources.push(SourceGroup {
                path: first_path,
                display_name: disp,
                volumes: vols,
                size: total_size,
                missing_volume: missing,
            });
        }

        for (_, (rar_base, r_map)) in old_rar_groups {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            if rar_base.is_none() && r_map.is_empty() {
                continue;
            }
            if let (Some(file), true) = (rar_base.as_ref(), r_map.is_empty()) {
                let size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                let p_str = file.to_string_lossy().to_string();
                let disp = output_name(&p_str);
                sources.push(SourceGroup {
                    path: p_str.clone(),
                    display_name: disp,
                    volumes: vec![p_str],
                    size,
                    missing_volume: false,
                });
                continue;
            }

            let mut missing = rar_base.is_none();
            let mut vols = Vec::new();
            let mut total_size = 0u64;

            if let Some(ref base) = rar_base {
                vols.push(base.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(base) {
                    total_size += meta.len();
                }
            }

            let mut expected = 0;
            for (&k, p) in &r_map {
                if k != expected {
                    missing = true;
                }
                expected = k.saturating_add(1);
                vols.push(p.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(p) {
                    total_size += meta.len();
                }
            }

            let first_path = vols[0].clone();
            let disp = output_name(&first_path);
            sources.push(SourceGroup {
                path: first_path,
                display_name: disp,
                volumes: vols,
                size: total_size,
                missing_volume: missing,
            });
        }

        for (_, (zip_base, z_map)) in split_zip_groups {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            if zip_base.is_none() && z_map.is_empty() {
                continue;
            }
            if let (Some(file), true) = (zip_base.as_ref(), z_map.is_empty()) {
                let size = fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                let p_str = file.to_string_lossy().to_string();
                let disp = output_name(&p_str);
                sources.push(SourceGroup {
                    path: p_str.clone(),
                    display_name: disp,
                    volumes: vec![p_str],
                    size,
                    missing_volume: false,
                });
                continue;
            }

            let mut missing = zip_base.is_none();
            let mut vols = Vec::new();
            let mut total_size = 0u64;

            let mut expected = 1;
            for (&k, p) in &z_map {
                if k != expected {
                    missing = true;
                }
                expected = k.saturating_add(1);
                vols.push(p.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(p) {
                    total_size += meta.len();
                }
            }

            if let Some(ref base) = zip_base {
                vols.push(base.to_string_lossy().to_string());
                if let Ok(meta) = fs::metadata(base) {
                    total_size += meta.len();
                }
            }

            let first_path = zip_base
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| vols[0].clone());
            let disp = output_name(&first_path);
            sources.push(SourceGroup {
                path: first_path,
                display_name: disp,
                volumes: vols,
                size: total_size,
                missing_volume: missing,
            });
        }

        for file in standalone_files {
            if cancel.load(Ordering::Acquire) {
                return Err("扫描已取消。".into());
            }
            let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
            let p_str = file.to_string_lossy().to_string();
            let disp = output_name(&p_str);
            sources.push(SourceGroup {
                path: p_str.clone(),
                display_name: disp,
                volumes: vec![p_str],
                size,
                missing_volume: false,
            });
        }
    }

    if cancel.load(Ordering::Acquire) {
        return Err("扫描已取消。".into());
    }
    sources.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ScanResult { sources, notices })
}

#[cfg(test)]
mod tests {
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
                    fs::write(dir.path().join(format!("{stem}.7z.{number:03}")), b"volume")
                        .unwrap();
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
}
