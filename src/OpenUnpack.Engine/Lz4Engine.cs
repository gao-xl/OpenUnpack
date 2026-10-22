using System.Security.Cryptography;

namespace OpenUnpack.Engine;

public sealed class Lz4Engine : IArchiveEngine, IPasswordProbe
{
    public static bool CanHandle(string archivePath)
    {
        var lower = Path.GetFileName(archivePath).ToLowerInvariant();
        if (lower.EndsWith(".lz4")) return true;

        if (File.Exists(archivePath))
        {
            try
            {
                Span<byte> magic = stackalloc byte[4];
                using var sourceLease = new PathIdentityLease();
                using var stream = sourceLease.OpenRead(archivePath);
                if (stream.Read(magic) == 4 && Lz4Decoder.IsLz4Frame(magic))
                    return true;
            }
            catch { }
        }
        return false;
    }

    public static string GetExtractedFileName(string archivePath)
    {
        var name = Path.GetFileName(archivePath);
        if (name.EndsWith(".lz4", StringComparison.OrdinalIgnoreCase))
            return name[..^4];
        return name + ".extracted";
    }

    public Task<ArchiveInspection> InspectAsync(string archivePath, string? password, CancellationToken cancellationToken) =>
        InspectAsync(archivePath, password, cancellationToken, null, 100_000, 100L * 1024 * 1024 * 1024);

    public async Task<ArchiveInspection> InspectAsync(string archivePath, string? password, CancellationToken cancellationToken,
        IReadOnlyList<string>? allowedVolumes, int maxEntries, long maxTotalBytes)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ValidateBudget(maxEntries, maxTotalBytes, maxTotalBytes, 0);
        var fullPath = Path.GetFullPath(archivePath);
        ValidateSource(fullPath, allowedVolumes);
        if (!File.Exists(fullPath)) throw new ArchiveException(ArchiveFailure.MissingVolume, "文件不存在。");
        Native.SevenZipEngine.EnsureNoReparsePoint(fullPath);

        using var sourceLease = new PathIdentityLease();
        using var stream = sourceLease.OpenRead(fullPath);
        Lz4Decoder.FrameInfo info;
        try { info = await Lz4Decoder.ReadHeaderAsync(stream, maxTotalBytes, cancellationToken); }
        catch (Exception ex) when (ex is InvalidDataException or EndOfStreamException)
        { throw new ArchiveException(ArchiveFailure.Corrupt, "LZ4 帧头无效。", ex); }
        var entryName = GetExtractedFileName(fullPath);
        Native.SevenZipEngine.ValidateRelativePath(entryName);
        var entries = new List<ArchiveEntryInfo>
        {
            new(entryName, info.ContentSize ?? 0, IsDirectory: false, IsLink: false, IsEncrypted: false,
                SizeKnown: info.ContentSize.HasValue)
        };
        return new ArchiveInspection(entries, EncryptedHeaders: false);
    }

    public async Task ExtractAsync(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken cancellationToken)
    {
        await ExtractWithManifestAsync(request, progress, cancellationToken);
    }

    public async Task<ExtractionManifest> ExtractWithManifestAsync(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ValidateBudget(request.MaxEntries, request.MaxTotalBytes, request.MaxWrittenBytes, request.MinFreeBytes);
        var sourcePath = Path.GetFullPath(request.ArchivePath);
        ValidateSource(sourcePath, request.AllowedVolumes);
        if (!File.Exists(sourcePath)) throw new ArchiveException(ArchiveFailure.MissingVolume, "来源文件不存在。");
        using var sourceLease = new PathIdentityLease();
        using var inStream = sourceLease.OpenRead(sourcePath);
        var destination = Path.GetFullPath(request.DestinationDirectory);
        WriteScopeGuard.DemandWriteAccess(destination);
        using var outputLease = new PathIdentityLease();
        outputLease.EnsureDirectory(destination, create: true);

        var outFileName = GetExtractedFileName(sourcePath);
        Native.SevenZipEngine.ValidateRelativePath(outFileName);
        if (Native.SevenZipEngine.MatchesExclusion(outFileName, request.Exclusions))
        {
            progress?.Report(new(0, 0, 1, 1));
            return new ExtractionManifest([], PasswordUsed: false, 0, ExcludedEntries: 1,
                VerificationScope: "frameValidationAndStreamSha256");
        }
        var outFilePath = Path.Combine(destination, outFileName);
        Native.SevenZipEngine.EnsureNoReparsePoint(outFilePath);
        WriteScopeGuard.DemandWriteAccess(outFilePath);

        long maxBytes = Math.Min(request.MaxTotalBytes, request.MaxWrittenBytes);

        long writtenBytes;
        string sha256;
        try
        {
            var info = await Lz4Decoder.ReadHeaderAsync(inStream, maxBytes, cancellationToken);
            inStream.Position = 0;
            var sourceZoneId = request.SourceZoneId ?? ZoneIdentifier.DetectZone(sourcePath);
            using var createdOutput = outputLease.CreateFile(outFilePath);
            var rawOutStream = createdOutput.Stream;
            using var sha256Hasher = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            using (var hashingOutStream = new HashingWriteStream(rawOutStream, sha256Hasher, progress,
                info.ContentSize ?? 0, info.ContentSize.HasValue))
            {
                writtenBytes = await Lz4Decoder.DecompressFrameAsync(
                    inStream,
                    hashingOutStream,
                    maxBytes,
                    request.MinFreeBytes,
                    destination,
                    cancellationToken);
                await hashingOutStream.FlushAsync(cancellationToken);
            }
            sha256 = Convert.ToHexStringLower(sha256Hasher.GetHashAndReset());
            ZoneIdentifier.PropagateZone(outFilePath, sourceZoneId);
            createdOutput.Keep();
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (ArchiveException)
        {
            throw;
        }
        catch (OutOfMemoryException)
        {
            throw;
        }
        catch (Exception ex)
        {
            throw new ArchiveException(ex is IOException and not EndOfStreamException || ex is UnauthorizedAccessException
                ? ArchiveFailure.IoError : ArchiveFailure.Corrupt, "LZ4 解压失败。", ex);
        }

        progress?.Report(new(writtenBytes, writtenBytes, 1, 1));
        var files = new List<ExtractedFile>
        {
            new(outFileName, writtenBytes, sha256, IsDirectory: false, IsCorrupted: false)
        };

        return new ExtractionManifest(files, PasswordUsed: false, writtenBytes, ExcludedEntries: 0,
            VerificationScope: "frameValidationAndStreamSha256");

    }

    private sealed class HashingWriteStream(Stream target, IncrementalHash hasher, IProgress<ExtractionProgress>? progress,
        long totalBytes, bool totalBytesKnown) : Stream
    {
        private long _lastProgressTick;
        public long BytesWritten { get; private set; }
        public override bool CanRead => false;
        public override bool CanSeek => false;
        public override bool CanWrite => true;
        public override long Length => BytesWritten;
        public override long Position { get => BytesWritten; set => throw new NotSupportedException(); }

        public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();

        public override void Write(byte[] buffer, int offset, int count) => Write(buffer.AsSpan(offset, count));

        public override void Write(ReadOnlySpan<byte> buffer)
        {
            target.Write(buffer);
            hasher.AppendData(buffer);
            BytesWritten += buffer.Length;
            ReportProgress();
        }

        public override async ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken cancellationToken = default)
        {
            await target.WriteAsync(buffer, cancellationToken);
            hasher.AppendData(buffer.Span);
            BytesWritten += buffer.Length;
            ReportProgress();
        }

        private void ReportProgress()
        {
            var tick = Environment.TickCount64;
            if (tick - _lastProgressTick < 100) return;
            _lastProgressTick = tick;
            progress?.Report(new(BytesWritten, totalBytes, 0, 1, totalBytesKnown));
        }

        public override Task WriteAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken) =>
            WriteAsync(buffer.AsMemory(offset, count), cancellationToken).AsTask();

        public override void Flush() => target.Flush();
        public override Task FlushAsync(CancellationToken cancellationToken) => target.FlushAsync(cancellationToken);
    }

    public async Task<PasswordProbeResult> ProbeAsync(
        string archivePath,
        IReadOnlyList<string> candidatePasswords,
        CancellationToken cancellationToken,
        IReadOnlyList<string>? allowedVolumes = null,
        int maxEntries = 100_000,
        long maxTotalBytes = 100L * 1024 * 1024 * 1024,
        long maxWrittenBytes = long.MaxValue)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ValidateBudget(maxEntries, maxTotalBytes, maxWrittenBytes, 0);
        await InspectAsync(archivePath, null, cancellationToken, allowedVolumes, maxEntries, Math.Min(maxTotalBytes, maxWrittenBytes));
        return new PasswordProbeResult(PasswordProbeStatus.NoPasswordNeeded, null);
    }

    private static void ValidateBudget(int maxEntries, long maxTotalBytes, long maxWrittenBytes, long minFreeBytes)
    {
        if (maxEntries is < 1 or > 1_000_000 || maxTotalBytes < 0 || maxWrittenBytes < 0 || minFreeBytes < 0)
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "LZ4 资源预算无效。");
    }

    private static void ValidateSource(string fullPath, IReadOnlyList<string>? allowedVolumes)
    {
        if (allowedVolumes is not null && !allowedVolumes.Any(p =>
            string.Equals(Path.GetFullPath(p), fullPath, StringComparison.OrdinalIgnoreCase)))
            throw new ArchiveException(ArchiveFailure.MissingVolume, "来源文件不在已授权分卷清单中。");
    }
}
