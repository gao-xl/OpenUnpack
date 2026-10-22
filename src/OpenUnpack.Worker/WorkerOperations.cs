using System.Buffers;
using System.Security.Cryptography;
using System.Text;
using OpenUnpack.Engine;
using OpenUnpack.Engine.Native;

namespace OpenUnpack.Worker;

internal sealed class WorkerOperations(string enginePath)
{
    // Mirror of SevenZipEngine.OutputStream throttling: a free-space probe costs a syscall
    // and the previous code paid it per 1MB chunk. Check at most every 32MB or 1 second.
    private const long SpaceCheckThrottleBytes = 32 * 1024 * 1024; // 32MB
    private const long SpaceCheckThrottleIntervalMs = 1000;       // 1 second
    private readonly CompositeArchiveEngine _engine = new(enginePath);

    public async Task<WorkerEvent> ExecuteAsync(WorkerRequest request, Action<WorkerEvent> progress, CancellationToken token)
    {
        ValidateRequest(request);
        ApplyProcessPriority(request.WorkerPriority);
        using var scope = EnterOperationSandboxScope(request);
        using var identityScope = PathIdentityLease.EnterOperation();
        token.ThrowIfCancellationRequested();
        return request.Operation switch
        {
            "extract" => await ExtractAsync(request, progress, token),
            "copy" => await TransferAsync(request, false, progress, token),
            "readback" => await TransferAsync(request, true, progress, token),
            "probe" => await ProbeAsync(request, token),
            "inspect" => await InspectAsync(request, token),
            _ => throw new WorkerProtocolException()
        };
    }

    private static WriteScopeGuard EnterOperationSandboxScope(WorkerRequest request)
    {
        var forbidden = new List<string>();
        if (!string.IsNullOrWhiteSpace(request.ArchivePath)) forbidden.Add(request.ArchivePath);
        if (request.AllowedVolumes is not null) forbidden.AddRange(request.AllowedVolumes);
        if (request.Files is not null) forbidden.AddRange(request.Files.Select(f => f.SourcePath));

        return request.Operation switch
        {
            "extract" or "copy" => WriteScopeGuard.Enter(
                allowedRoots: [request.DestinationDirectory],
                forbiddenPaths: forbidden,
                allowedManifestDirectories: [request.DestinationDirectory]),
            "inspect" or "readback" => WriteScopeGuard.EnterReadOnly(
                forbiddenPaths: forbidden,
                allowedManifestDirectories: [request.DestinationDirectory]),
            "probe" => WriteScopeGuard.EnterReadOnly(
                forbiddenPaths: forbidden),
            _ => WriteScopeGuard.EnterReadOnly(forbiddenPaths: forbidden)
        };
    }

    private static void ApplyProcessPriority(string? priority)
    {
        if (string.IsNullOrWhiteSpace(priority)) return;
        try
        {
            var p = priority.Trim().ToLowerInvariant() switch
            {
                "idle" => System.Diagnostics.ProcessPriorityClass.Idle,
                "normal" => System.Diagnostics.ProcessPriorityClass.Normal,
                _ => System.Diagnostics.ProcessPriorityClass.BelowNormal
            };
            System.Diagnostics.Process.GetCurrentProcess().PriorityClass = p;
        }
        catch { /* Ignore if security privileges prevent setting priority */ }
    }

    private static void ValidateRequest(WorkerRequest request)
    {
        if (request.Operation == "inspect")
        {
            if (string.IsNullOrEmpty(request.ArchivePath) || !Path.IsPathFullyQualified(request.ArchivePath)
                || request.AllowedVolumes is null || request.AllowedVolumes.Count == 0 || request.AllowedVolumes.Count > 10_000
                || request.AllowedVolumes.Any(p => string.IsNullOrEmpty(p) || !Path.IsPathFullyQualified(p))
                || request.MaxEntries == 0 || request.MaxEntries > 1_000_000 || request.MaxTotalBytes > long.MaxValue
                || string.IsNullOrEmpty(request.DestinationDirectory) || !Path.IsPathFullyQualified(request.DestinationDirectory)
                || (request.Password is not null && request.Password.Length > 32768))
                throw new WorkerProtocolException();
            SevenZipEngine.EnsureNoReparsePoint(request.DestinationDirectory);
            if (!Directory.Exists(request.DestinationDirectory)) throw new WorkerProtocolException();
            return;
        }

        if (request.Operation == "probe")
        {
            if (string.IsNullOrEmpty(request.ArchivePath) || !Path.IsPathFullyQualified(request.ArchivePath)
                || request.AllowedVolumes is null || request.AllowedVolumes.Count == 0 || request.AllowedVolumes.Count > 10_000
                || request.CandidatePasswords is null || request.CandidatePasswords.Count > 100_000
                || request.CandidatePasswords.Any(p => p is null || p.Length > 32768)
                || request.AllowedVolumes.Any(p => string.IsNullOrEmpty(p) || !Path.IsPathFullyQualified(p))
                || request.MaxEntries > 1_000_000 || request.MaxTotalBytes > long.MaxValue || request.MaxWrittenBytes > long.MaxValue
                || (request.ProtocolVersion == Protocol.VersionV2 && (request.MaxEntries == 0 || request.MaxTotalBytes == 0 || request.MaxWrittenBytes == 0)))
                throw new WorkerProtocolException();
            if (request.Candidates is null || request.Candidates.Count > 128
                || request.Candidates.Any(c => c is null || string.IsNullOrWhiteSpace(c.CandidateId)
                    || c.CandidateId.Length > 200 || string.IsNullOrEmpty(c.Secret) || c.Secret.Length > 32768
                    || c.Source is null || c.Label is null)
                || request.Candidates.Select(c => c.CandidateId).Distinct(StringComparer.Ordinal).Count() != request.Candidates.Count
                || (request.ProtocolVersion == Protocol.VersionV2 && request.CandidatePasswords.Count != 0))
                throw new WorkerProtocolException();
            return;
        }

        if (request.Files is null || request.AllowedVolumes is null || request.Exclusions is null
            || request.Files.Count > 100_000 || request.AllowedVolumes.Count > 10_000 || request.Exclusions.Count > 256
            || request.Exclusions.Any(p => p is null || Encoding.UTF8.GetByteCount(p) > 512)
            || (request.Inclusions is not null && (request.Inclusions.Count > 10_000 || request.Inclusions.Any(p => p is null || Encoding.UTF8.GetByteCount(p) > 512)))
            || request.Files.Any(f => f is null || f.RelativePath is null || f.SourcePath is null || f.Sha256 is null || !f.SizeKnown)
            || request.MaxTotalBytes > long.MaxValue || request.MaxWrittenBytes > long.MaxValue || request.MinFreeBytes > long.MaxValue
            || request.MaxEntries > 1_000_000 || string.IsNullOrEmpty(request.DestinationDirectory)
            || !Path.IsPathFullyQualified(request.DestinationDirectory)
            || (request.Password is not null && request.Password.Length > 32768))
            throw new WorkerProtocolException();
    }

    private async Task<WorkerEvent> ExtractAsync(WorkerRequest request, Action<WorkerEvent> progress, CancellationToken token)
    {
        if (string.IsNullOrEmpty(request.ArchivePath) || !Path.IsPathFullyQualified(request.ArchivePath)
            || request.AllowedVolumes.Count == 0 || request.MaxEntries == 0)
            throw new WorkerProtocolException();
        var engine = _engine;
        using var sourceLease = new PathIdentityLease();
        var sourcePins = new List<FileStream>();
        try { foreach (var volume in request.AllowedVolumes) sourcePins.Add(sourceLease.OpenRead(volume)); }
        catch { foreach (var pin in sourcePins) pin.Dispose(); throw; }
        using var sourcePinLifetime = new SourcePinLifetime(sourcePins);
        var extraction = new ExtractionRequest(request.ArchivePath, request.DestinationDirectory, request.Password,
            (int)request.MaxEntries, (long)request.MaxTotalBytes, request.AllowedVolumes,
            (long)request.MaxWrittenBytes, (long)request.MinFreeBytes, request.Exclusions,
            request.EngineThreads, request.SalvageCorrupted,
            SourceZoneId: ZoneIdentifier.DetectZone(request.ArchivePath),
            Inclusions: request.Inclusions);
        // ExtractWithManifestAsync opens headers, preflights all paths, and decodes exactly once.
        var manifest = await engine.ExtractWithManifestAsync(extraction, new DirectProgress(value => progress(new()
        {
            ProtocolVersion = request.ProtocolVersion, RequestId = request.RequestId,
            OperationId = request.OperationId,
            Generation = request.Generation,
            Kind = "progress",
            CompletedBytes = (ulong)value.CompletedBytes,
            TotalBytes = (ulong)value.TotalBytes,
            TotalBytesKnown = value.TotalBytesKnown
        })), token);
        var files = manifest.Files.Select(f => new FileRecord
        { RelativePath = f.RelativePath, Size = (ulong)f.Size, Sha256 = f.Sha256, IsDirectory = f.IsDirectory, IsCorrupted = f.IsCorrupted }).ToList();
        var corruptedCount = manifest.Files.Count(f => f.IsCorrupted);
        var salvaged = corruptedCount > 0 || manifest.VerificationScope == "salvagedWithCrcWarning";
        var message = corruptedCount > 0
            ? $"抢救解压完成：检测到 CRC 错误，已抢救输出 {files.Count} 项（其中 {corruptedCount} 项损坏）。"
            : salvaged ? $"抢救解压完成：归档结构或数据校验异常，已恢复 {files.Count} 项，输出可能不完整。"
            : manifest.ExcludedEntries > 0 ? $"解压完成，按规则排除了 {manifest.ExcludedEntries} 项。" : "解压完成。";
        var verificationScope = salvaged ? "salvagedWithCrcWarning" : manifest.VerificationScope;
        return await CreateCompletedEventAsync(request, files, (ulong)manifest.WrittenBytes, (ulong)manifest.WrittenBytes,
            message, manifest.PasswordUsed, verificationScope, token);
    }

    private async Task<WorkerEvent> ProbeAsync(WorkerRequest request, CancellationToken token)
    {
        var engine = _engine;
        var candidates = request.CandidatePasswords ?? [];
        if (candidates.Count == 0 && request.Candidates is { Count: > 0 })
        {
            candidates = request.Candidates.Select(c => c.Secret).ToList();
        }
        int maxEntries = request.MaxEntries > 0 ? (int)Math.Min(request.MaxEntries, int.MaxValue) : 100_000;
        long maxTotalBytes = request.MaxTotalBytes > 0 ? (long)request.MaxTotalBytes : 100L * 1024 * 1024 * 1024;
        long maxWrittenBytes = request.MaxWrittenBytes > 0 ? (long)request.MaxWrittenBytes : long.MaxValue;
        var probeResult = await engine.ProbeAsync(
            request.ArchivePath,
            candidates,
            token,
            request.AllowedVolumes,
            maxEntries,
            maxTotalBytes,
            maxWrittenBytes);
        string? matchedCandidateId = null;
        if (probeResult.MatchedPassword != null && request.Candidates is { Count: > 0 })
        {
            matchedCandidateId = request.Candidates.FirstOrDefault(c => c.Secret == probeResult.MatchedPassword)?.CandidateId;
        }
        return new WorkerEvent
        {
            ProtocolVersion = request.ProtocolVersion, RequestId = request.RequestId,
            OperationId = request.OperationId,
            Generation = request.Generation,
            Kind = "completed",
            MatchedPassword = request.ProtocolVersion == Protocol.VersionV1 ? probeResult.MatchedPassword : null,
            MatchedCandidateId = matchedCandidateId,
            ProbeStatus = probeResult.Status.ToString(),
            PasswordUsed = probeResult.Status == PasswordProbeStatus.Success,
            Message = probeResult.Status switch
            {
                PasswordProbeStatus.Success => "已匹配到有效密码。",
                PasswordProbeStatus.NoPasswordNeeded => "归档无需密码。",
                PasswordProbeStatus.WrongPassword => "所有候选密码均未匹配。",
                PasswordProbeStatus.MissingVolume => "缺少分卷。",
                PasswordProbeStatus.Corrupt => "归档损坏或加密头部不匹配。",
                PasswordProbeStatus.Unsupported => "不支持的压缩方法或加密算法。",
                _ => probeResult.ErrorMessage ?? "探测完成。"
            }
        };
    }

    private async Task<WorkerEvent> InspectAsync(WorkerRequest request, CancellationToken token)
    {
        var engine = _engine;
        var inspection = await engine.InspectAsync(request.ArchivePath, request.Password, token,
            request.AllowedVolumes, (int)request.MaxEntries, (long)request.MaxTotalBytes);
        var files = inspection.Entries
            .Where(e => !string.IsNullOrEmpty(e.Path))
            .Select(e => new FileRecord
            {
                RelativePath = e.Path,
                Size = (ulong)Math.Max(0, e.Size),
                SizeKnown = e.SizeKnown,
                Sha256 = "",
                IsDirectory = e.IsDirectory,
                IsEncrypted = e.IsEncrypted
            }).ToList();
        var totalBytes = (ulong)Math.Max(0, inspection.TotalBytes);
        var completed = await CreateCompletedEventAsync(request, files, totalBytes, totalBytes,
            $"已读取归档目录，共 {files.Count} 项。",
            // Reading plain headers does not validate an encrypted file's password.
            // Only a password actually requested for opening headers was used.
            inspection.EncryptedHeaders,
            "metadataOnly", token);
        return completed with { TotalBytesKnown = inspection.TotalBytesKnown };
    }

    private static async Task<WorkerEvent> TransferAsync(WorkerRequest request, bool readback,
        Action<WorkerEvent> progress, CancellationToken token)
    {
        var destination = Path.GetFullPath(request.DestinationDirectory);
        using var identityLease = new PathIdentityLease();
        if (!readback) WriteScopeGuard.DemandWriteAccess(destination);
        var mapped = new Dictionary<string, bool>(StringComparer.OrdinalIgnoreCase);
        ulong total = 0;
        foreach (var file in request.Files)
        {
            token.ThrowIfCancellationRequested();
            var relative = SevenZipEngine.ValidateRelativePath(file.RelativePath);
            if (!mapped.TryAdd(relative, file.IsDirectory))
                throw new ArchiveException(ArchiveFailure.UnsafePath, "输出文件路径重复。");
            if (file.Sha256.Length > 0 && (file.Sha256.Length != 64 || !file.Sha256.All(Uri.IsHexDigit)))
                throw new WorkerProtocolException();
            if (readback && !file.IsDirectory && file.Sha256.Length == 0) throw new WorkerProtocolException();
            if (file.Size > long.MaxValue || ulong.MaxValue - total < file.Size)
                throw new ArchiveException(ArchiveFailure.ResourceLimit, "传输大小超过限制。");
            if (!file.IsDirectory) total += file.Size;
            if (!readback && total > request.MaxWrittenBytes)
                throw new ArchiveException(ArchiveFailure.ResourceLimit, "传输大小超过限制。");
            if (!Path.IsPathFullyQualified(file.SourcePath)) throw new WorkerProtocolException();
            var source = Path.GetFullPath(file.SourcePath);
            if (readback && !IsWithin(destination, source))
                throw new ArchiveException(ArchiveFailure.UnsafePath, "回读路径超出目标目录。");
            identityLease.EnsureDirectory(file.IsDirectory ? source : Path.GetDirectoryName(source)!);
        }
        foreach (var relative in mapped.Keys)
        {
            var parent = Path.GetDirectoryName(relative);
            while (!string.IsNullOrEmpty(parent))
            {
                if (mapped.TryGetValue(parent, out var directory) && !directory)
                    throw new ArchiveException(ArchiveFailure.UnsafePath, "输出文件与目录冲突。");
                parent = Path.GetDirectoryName(parent);
            }
        }
        // All relative paths and file/directory collisions are checked before any output creation.
        identityLease.EnsureDirectory(destination, create: !readback);
        var records = new List<FileRecord>(request.Files.Count);
        var buffer = ArrayPool<byte>.Shared.Rent(1024 * 1024);
        ulong completed = 0;
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        try
        {
            foreach (var item in request.Files)
            {
                token.ThrowIfCancellationRequested();
                var relative = SevenZipEngine.ValidateRelativePath(item.RelativePath);
                var source = Path.GetFullPath(item.SourcePath);
                var target = Path.GetFullPath(Path.Combine(destination, relative));
                if (!IsWithin(destination, target)) throw new ArchiveException(ArchiveFailure.UnsafePath, "输出路径越界。");
                if (readback && !source.Equals(target, StringComparison.OrdinalIgnoreCase))
                    throw new ArchiveException(ArchiveFailure.UnsafePath, "回读文件与清单路径不一致。");
                if (item.IsDirectory)
                {
                    identityLease.EnsureDirectory(source);
                    if (!readback)
                    {
                        WriteScopeGuard.DemandWriteAccess(target);
                        identityLease.EnsureDirectory(target, create: true);
                    }
                    records.Add(new() { RelativePath = relative, IsDirectory = true });
                    continue;
                }
                using var input = identityLease.OpenRead(source);
                if ((ulong)input.Length != item.Size) throw new ArchiveException(ArchiveFailure.Corrupt, "来源文件大小已变化。");
                if (!readback)
                {
                    var parentDir = Path.GetDirectoryName(target)!;
                    identityLease.EnsureDirectory(parentDir, create: true);
                    SevenZipEngine.EnsureFreeSpace(target, (long)request.MinFreeBytes);
                    WriteScopeGuard.DemandWriteAccess(target);
                }
                ulong current = 0;
                hash.GetHashAndReset();
                using var createdOutput = readback ? null : identityLease.CreateFile(target);
                var output = createdOutput?.Stream;
                {
                    long bytesSinceSpaceCheck = 0;
                    var lastSpaceCheckTick = Environment.TickCount64;
                    while (true)
                    {
                        var count = await input.ReadAsync(buffer.AsMemory(), token);
                        if (count == 0) break;
                        current += (uint)count;
                        if (current > item.Size || (!readback && (completed > request.MaxWrittenBytes || (ulong)count > request.MaxWrittenBytes - completed)))
                            throw new ArchiveException(ArchiveFailure.ResourceLimit, "实际传输大小超过限制。");
                        if (output is not null)
                        {
                            bytesSinceSpaceCheck += count;
                            var nowTick = Environment.TickCount64;
                            if (bytesSinceSpaceCheck >= SpaceCheckThrottleBytes || nowTick - lastSpaceCheckTick >= SpaceCheckThrottleIntervalMs)
                            {
                                SevenZipEngine.EnsureFreeSpace(target, (long)request.MinFreeBytes);
                                bytesSinceSpaceCheck = 0;
                                lastSpaceCheckTick = nowTick;
                            }
                            await output.WriteAsync(buffer.AsMemory(0, count), token);
                        }
                        hash.AppendData(buffer.AsSpan(0, count));
                        completed += (uint)count;
                        progress(new()
                        {
                            ProtocolVersion = request.ProtocolVersion, RequestId = request.RequestId,
                            OperationId = request.OperationId,
                            Generation = request.Generation,
                            Kind = "progress",
                            CompletedBytes = completed,
                            TotalBytes = total
                        });
                    }
                    if (current != item.Size) throw new ArchiveException(ArchiveFailure.Corrupt, "文件在传输期间发生变化。");
                    if (output is not null)
                    {
                        await output.FlushAsync(token);
                    }
                }
                if (!readback)
                {
                    var sourceZoneId = ZoneIdentifier.DetectZone(source);
                    ZoneIdentifier.PropagateZone(target, sourceZoneId);
                }
                var digest = Convert.ToHexStringLower(hash.GetHashAndReset());
                if (item.Sha256.Length > 0 && !digest.Equals(item.Sha256, StringComparison.OrdinalIgnoreCase))
                    throw new ArchiveException(ArchiveFailure.Corrupt, "文件内容摘要不匹配。");
                createdOutput?.Keep();
                records.Add(new() { RelativePath = relative, Size = current, Sha256 = digest });
            }
        }
        finally { ArrayPool<byte>.Shared.Return(buffer, true); }
        var message = readback ? "目标目录逐文件摘要验证通过。" : "流式传输完成。" ;
        var scope = readback ? "targetReadback" : "sourceStreamSha256";
        return await CreateCompletedEventAsync(request, records, completed, total, message, false, scope, token);
    }

    private static bool IsWithin(string root, string child) => child.StartsWith(root.TrimEnd(Path.DirectorySeparatorChar)
        + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase);

    private static async Task<WorkerEvent> CreateCompletedEventAsync(WorkerRequest request, List<FileRecord> files,
        ulong completedBytes, ulong totalBytes, string message, bool passwordUsed, string verificationScope, CancellationToken token)
    {
        // Conservatively bound inline JSON before allocating it (JSON escaping can use
        // six bytes for each UTF-16 code unit). Large lists serialize straight to disk.
        var inlineBound = files.Sum(f => 192L + 6L * f.RelativePath.Length);
        if (inlineBound <= Protocol.MaxMessageBytes - 65536)
        {
            return new()
            {
                ProtocolVersion = request.ProtocolVersion, RequestId = request.RequestId,
                OperationId = request.OperationId,
                Generation = request.Generation,
                Kind = "completed",
                Files = files,
                CompletedBytes = completedBytes,
                TotalBytes = totalBytes,
                Message = message,
                PasswordUsed = passwordUsed,
                VerificationScope = verificationScope
            };
        }

        // When manifest exceeds IPC budget, spill to a temporary manifest file in the destination directory
        var manifestId = Guid.NewGuid().ToString("N");
        var manifestFileName = $".openunpack-{manifestId}-manifest.json";
        var manifestFilePath = Path.Combine(request.DestinationDirectory, manifestFileName);
        using var identityLease = new PathIdentityLease();
        identityLease.EnsureDirectory(request.DestinationDirectory);
        var temporaryPath = manifestFilePath + ".tmp";
        string manifestHash;
        ulong manifestLength;
        using (var createdManifest = identityLease.CreateFile(temporaryPath))
        {
            var file = createdManifest.Stream;
            {
                using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
                await using var output = new ManifestWriteStream(file, hash);
                await System.Text.Json.JsonSerializer.SerializeAsync(output, files, Protocol.Json, token);
                await file.FlushAsync(token);
                file.Flush(true);
                manifestLength = output.WrittenBytes;
                manifestHash = Convert.ToHexStringLower(hash.GetHashAndReset());
            }
            identityLease.Publish(createdManifest, manifestFilePath);
            createdManifest.Keep();
        }

        return new()
        {
            ProtocolVersion = request.ProtocolVersion, RequestId = request.RequestId,
            OperationId = request.OperationId,
            Generation = request.Generation,
            Kind = "completed",
            Files = [],
            ManifestPath = manifestFilePath,
            ManifestId = manifestId,
            ManifestSha256 = manifestHash,
            ManifestByteLength = manifestLength,
            CompletedBytes = completedBytes,
            TotalBytes = totalBytes,
            Message = message,
            PasswordUsed = passwordUsed,
            VerificationScope = verificationScope
        };
    }

    internal static string ErrorCode(Exception exception, WorkerRequest request) => exception switch
    {
        OutOfMemoryException => "ResourceLimit",
        WorkerProtocolException or ArgumentException => "ProtocolError",
        ArchiveException archive => archive.Failure switch
        {
            ArchiveFailure.WrongPassword => string.IsNullOrEmpty(request.Password) ? "NeedsPassword" : "PasswordMismatchOrCorrupt",
            ArchiveFailure.Corrupt => "Corrupt",
            ArchiveFailure.MissingVolume => "MissingVolume",
            ArchiveFailure.Unsupported => "Unsupported",
            ArchiveFailure.UnsafePath => "UnsafePath",
            ArchiveFailure.ResourceLimit => "ResourceLimit",
            _ => "IoError"
        },
        _ => "IoError"
    };

    internal static string ErrorMessage(string code) => code switch
    {
        "ProtocolError" => "解压请求无效。",
        "NeedsPassword" => "需要提供有效密码。",
        "PasswordMismatchOrCorrupt" => "密码不匹配或压缩包损坏。",
        "Corrupt" => "文件损坏或校验失败。",
        "MissingVolume" => "分卷文件不完整或缺失。",
        "Unsupported" => "不支持此格式或解压引擎不可用。",
        "UnsafePath" => "路径包含非法字符或链接，已拒绝处理。",
        "ResourceLimit" => "解压大小、条目数或磁盘空间超出限制。",
        _ => "文件读写失败，请检查磁盘和权限。"
    };

    private sealed class WorkerProtocolException : Exception;

    private sealed class ManifestWriteStream(Stream destination, IncrementalHash hash) : Stream
    {
        private const ulong Limit = 128UL * 1024 * 1024;
        public ulong WrittenBytes { get; private set; }
        public override bool CanRead => false;
        public override bool CanSeek => false;
        public override bool CanWrite => true;
        public override long Length => (long)WrittenBytes;
        public override long Position { get => Length; set => throw new NotSupportedException(); }
        private void Check(int length)
        {
            if ((ulong)length > Limit - WrittenBytes)
                throw new ArchiveException(ArchiveFailure.ResourceLimit, "结果清单超过限制。");
        }
        public override void Write(byte[] buffer, int offset, int count) => Write(buffer.AsSpan(offset, count));
        public override void Write(ReadOnlySpan<byte> buffer)
        {
            Check(buffer.Length); destination.Write(buffer);
            hash.AppendData(buffer); WrittenBytes += (ulong)buffer.Length;
        }
        public override async ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken token = default)
        {
            Check(buffer.Length); await destination.WriteAsync(buffer, token);
            hash.AppendData(buffer.Span); WrittenBytes += (ulong)buffer.Length;
        }
        public override Task WriteAsync(byte[] buffer, int offset, int count, CancellationToken token) =>
            WriteAsync(buffer.AsMemory(offset, count), token).AsTask();
        public override void Flush() => destination.Flush();
        public override Task FlushAsync(CancellationToken token) => destination.FlushAsync(token);
        public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
    }
    private sealed class DirectProgress(Action<ExtractionProgress> report) : IProgress<ExtractionProgress>
    { public void Report(ExtractionProgress value) => report(value); }

    private sealed class SourcePinLifetime(List<FileStream> streams) : IDisposable
    { public void Dispose() { foreach (var stream in streams) stream.Dispose(); } }
}
