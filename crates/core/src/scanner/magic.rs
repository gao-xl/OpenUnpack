//! 压缩包魔数签名、自解压载荷识别与符号链接/重解析点检查。

use super::*;

pub(super) fn is_reparse_point_or_symlink(path: &Path) -> bool {
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

pub(super) fn is_container_extension(lower: &str) -> bool {
    let containers = [
        ".docx", ".xlsx", ".pptx", ".jar", ".war", ".ear", ".apk", ".aab", ".ipa", ".epub", ".xpi",
        ".vsix", ".nupkg", ".whl", ".odt", ".ods", ".odp", ".crx",
    ];
    containers.iter().any(|ext| lower.ends_with(ext))
}

pub(super) fn has_archive_magic_in_buf(buf: &[u8]) -> bool {
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

pub(super) fn has_sfx_archive_payload(path: &Path) -> bool {
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

pub(super) fn has_tar_header(bytes: &[u8]) -> bool {
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

    // Compute the SFX payload scan exactly once. It reads up to 512 KiB, and the
    // MZ branch plus the explicit-SFX return previously re-opened and re-scanned
    // the file a second time.
    let sfx_payload = if is_explicit_sfx {
        has_sfx_archive_payload(path)
    } else {
        false
    };

    if read_count >= 2 && &header[..2] == b"MZ" {
        if !is_explicit_sfx {
            if !has_tar_header(&header[..read_count]) {
                return false;
            }
        } else if !sfx_payload {
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
        return sfx_payload;
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
