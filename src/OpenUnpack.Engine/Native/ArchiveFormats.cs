using System.Text.RegularExpressions;

namespace OpenUnpack.Engine.Native;

internal static class ArchiveFormats
{
    private const byte Zip = 1, BZip2 = 2, Rar4 = 3, Lzh = 6, SevenZip = 7, Cab = 8,
        Xz = 0x0C, Zstd = 0x0E, Rar5 = 0xCC, Wim = 0xE6, Tar = 0xEE, GZip = 0xEF;

    internal static byte[] Resolve(string path) => ResolveWithOffset(path, out _);

    internal static byte[] ResolveWithOffset(string path, out long payloadOffset)
    {
        payloadOffset = 0;
        var name = Path.GetFileName(path).ToLowerInvariant();
        var isExplicitSfx = name.EndsWith(".sfx") || name.EndsWith(".sfx.exe");

        if (isExplicitSfx)
        {
            if (TryResolveSfx(path, out var sfxFormats, out payloadOffset))
                return sfxFormats;
            throw new ArchiveException(ArchiveFailure.Unsupported, "自解压文件未包含受支持的归档载荷签名。");
        }

        if (Path.GetExtension(name) is ".exe" or ".com")
            throw new ArchiveException(ArchiveFailure.Unsupported, "不支持可执行文件或自解压程序。");

        Span<byte> header = stackalloc byte[512];
        using var sourceLease = new PathIdentityLease();
        using var input = sourceLease.OpenRead(path);
        var count = input.ReadAtLeast(header, header.Length, throwOnEndOfStream: false);
        var bytes = header[..count];
        // TAR begins with a filename, which can legitimately start with MZ.
        // Its full header checksum distinguishes it from an executable prefix.
        if (bytes.StartsWith("MZ"u8) && !HasTarHeader(bytes))
            throw new ArchiveException(ArchiveFailure.Unsupported, "不支持可执行文件或自解压程序。");

        // The same supported-format signatures are used by the Rust scanner.
        if (bytes.StartsWith(new byte[] { 0x50, 0x4B, 0x03, 0x04 })
            || bytes.StartsWith(new byte[] { 0x50, 0x4B, 0x05, 0x06 })
            || bytes.StartsWith(new byte[] { 0x50, 0x4B, 0x07, 0x08 })) return [Zip];
        if (bytes.StartsWith(new byte[] { 0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C })) return [SevenZip];
        if (bytes.StartsWith(new byte[] { 0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00 })) return [Rar4];
        if (bytes.StartsWith(new byte[] { 0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00 })) return [Rar5];
        if (bytes.StartsWith(new byte[] { 0x1F, 0x8B })) return [GZip];
        if (bytes.StartsWith("BZh"u8)) return [BZip2];
        if (bytes.StartsWith(new byte[] { 0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00 })) return [Xz];
        if (bytes.StartsWith("MSCF"u8)) return [Cab];
        if (bytes.StartsWith("MSWIM\0\0\0"u8) || bytes.StartsWith("WLPWM\0\0\0"u8)) return [Wim];
        if (bytes.Length >= 7 && bytes[2] == (byte)'-' && (bytes[3] == (byte)'l' || bytes[3] == (byte)'z') && bytes[6] == (byte)'-') return [Lzh];
        if (HasTarHeader(bytes) || (bytes.Length >= 262 && bytes.Slice(257, 5).SequenceEqual("ustar"u8))) return [Tar];
        if (bytes.StartsWith(new byte[] { 0x28, 0xB5, 0x2F, 0xFD }))
        {
            if (name.EndsWith(".zst") || name.EndsWith(".zstd") || name.EndsWith(".tzst"))
                return [Zstd];
            throw new ArchiveException(ArchiveFailure.Unsupported, "此压缩格式暂不支持。");
        }

        // Preserve validation of corrupt named archives, empty TAR and pre-ustar TAR.
        // Unknown suffixes have no fallback, and no input is renamed.
        var formatName = Regex.Replace(name, @"\.\d{3,}$", "");
        return Path.GetExtension(formatName) switch
        {
            ".zip" => [Zip], ".7z" => [SevenZip], ".rar" => [Rar4, Rar5],
            ".tar" => [Tar], ".gz" => [GZip], ".bz2" => [BZip2], ".xz" => [Xz], ".cab" => [Cab],
            ".wim" or ".swm" or ".esd" => [Wim],
            ".lzh" or ".lha" => [Lzh],
            ".zst" or ".zstd" or ".tzst" => [Zstd],
            _ => throw new ArchiveException(ArchiveFailure.Unsupported, "此压缩格式暂不支持。")
        };
    }

    internal static bool TryResolveSfx(string path, out byte[] formats, out long payloadOffset)
    {
        formats = [];
        payloadOffset = 0;
        using var sourceLease = new PathIdentityLease();
        using var stream = sourceLease.OpenRead(path);
        int maxScan = (int)Math.Min(stream.Length, 512 * 1024);
        if (maxScan < 64) return false;
        byte[] buf = new byte[maxScan];
        int n = stream.ReadAtLeast(buf, maxScan, throwOnEndOfStream: false);
        if (n < 64 || buf[0] != (byte)'M' || buf[1] != (byte)'Z') return false;

        var span = buf.AsSpan(2, n - 2);
        int idx7z = span.IndexOf(new byte[] { 0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C });
        if (idx7z >= 0) { formats = [SevenZip]; payloadOffset = idx7z + 2; return true; }

        int idxRar5 = span.IndexOf(new byte[] { 0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00 });
        if (idxRar5 >= 0) { formats = [Rar5]; payloadOffset = idxRar5 + 2; return true; }

        int idxRar4 = span.IndexOf(new byte[] { 0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00 });
        if (idxRar4 >= 0) { formats = [Rar4]; payloadOffset = idxRar4 + 2; return true; }

        int idxZip = span.IndexOf(new byte[] { 0x50, 0x4B, 0x03, 0x04 });
        if (idxZip >= 0) { formats = [Zip]; payloadOffset = idxZip + 2; return true; }

        return false;
    }

    private static bool HasTarHeader(ReadOnlySpan<byte> bytes)
    {
        if (bytes.Length < 512) return false;
        uint expected = 0; bool digit = false, ended = false;
        foreach (var value in bytes.Slice(148, 8)) {
            if (value is 0 or 32) { if (digit) ended = true; continue; }
            if (ended || value < (byte)'0' || value > (byte)'7') return false;
            digit = true; expected = expected * 8 + value - (byte)'0';
        }
        if (!digit || expected == 0) return false;
        uint actual = 0;
        for (var i = 0; i < 512; i++) actual += i is >= 148 and < 156 ? 32u : bytes[i];
        return expected == actual;
    }
}
