using System.Text;
using OpenUnpack.Engine.Native;

namespace OpenUnpack.Engine;

public static class ZoneIdentifier
{
    public const string StreamSuffix = ":Zone.Identifier";
    public const int MaxStreamBytes = 16 * 1024;

    public static int? DetectZone(string filePath)
    {
        if (!OperatingSystem.IsWindows()) return null;
        SevenZipEngine.EnsureNoReparsePoint(filePath);
        try
        {
            using var identityLease = new PathIdentityLease();
            using var stream = identityLease.OpenAlternateStream(filePath, write: false);
            // Length is checked again through a bounded read to cover growth after opening.
            if (stream.Length > MaxStreamBytes) throw new ArchiveException(ArchiveFailure.ResourceLimit, "来源标记超过长度限制。");
            byte[] buffer = new byte[MaxStreamBytes + 1];
            int count = 0, read;
            while (count < buffer.Length && (read = stream.Read(buffer, count, buffer.Length - count)) != 0) count += read;
            if (count > MaxStreamBytes) throw new ArchiveException(ArchiveFailure.ResourceLimit, "来源标记超过长度限制。");
            var text = Encoding.UTF8.GetString(buffer, 0, count).TrimStart('\uFEFF');
            foreach (var line in text.Split('\n'))
            {
                var trimmed = line.Trim();
                if (trimmed.StartsWith("ZoneId=", StringComparison.OrdinalIgnoreCase)
                    && int.TryParse(trimmed.AsSpan(7), out var id) && id is >= 0 and <= 4) return id;
            }
            // A present but malformed mark is kept conservatively as an internet mark.
            return 3;
        }
        catch (Exception ex) when (ex is FileNotFoundException or DirectoryNotFoundException) { return null; }
        catch (ArchiveException ex) when (ex.InnerException is System.ComponentModel.Win32Exception { NativeErrorCode: 2 or 3 }) { return null; }
        catch (ArchiveException ex)
        { SafeDiagnostics.Emit("unknown", "sourceZone", ex.Failure.ToString(), ex); throw; }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            SafeDiagnostics.Emit("unknown", "sourceZone", "IoError", ex);
            throw new ArchiveException(ArchiveFailure.IoError, "无法读取来源安全标记。", ex);
        }
    }

    // A controlled source zone must be written successfully before a manifest can be published.
    public static void PropagateZone(string destinationPath, int? zoneId)
    {
        if (!zoneId.HasValue) return;
        if (!OperatingSystem.IsWindows()) throw new ArchiveException(ArchiveFailure.IoError, "目标文件系统无法保存来源安全标记。");
        try
        {
            SevenZipEngine.EnsureNoReparsePoint(destinationPath);
            WriteScopeGuard.DemandWriteAccess(destinationPath);
            int safeZone = zoneId.Value is >= 0 and <= 4 ? zoneId.Value : 3;
            // The base-file owner can retain DELETE access for identity-safe rollback.
            // Sharing deletion on this short-lived ADS stream is safe because that
            // owner still denies deletion on the base stream throughout propagation.
            using var identityLease = new PathIdentityLease();
            using var stream = identityLease.OpenAlternateStream(destinationPath, write: true);
            using var writer = new StreamWriter(stream);
            writer.Write($"[ZoneTransfer]\r\nZoneId={safeZone}\r\n");
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArchiveException)
        {
            SafeDiagnostics.Emit("unknown", "targetZone", "IoError", ex);
            throw new ArchiveException(ArchiveFailure.IoError, "无法保存来源安全标记，已拒绝发布结果。", ex);
        }
    }

    public static bool TryPropagateZone(string destinationPath, int? zoneId)
    {
        if (!zoneId.HasValue || !OperatingSystem.IsWindows()) return false;
        try { PropagateZone(destinationPath, zoneId); return true; }
        catch (ArchiveException) { return false; }
    }
}
