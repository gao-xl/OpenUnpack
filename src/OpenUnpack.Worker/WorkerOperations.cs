using System.Buffers;
using System.Security.Cryptography;
using OpenUnpack.Core;
using OpenUnpack.Core.Native;

namespace OpenUnpack.Worker;

internal sealed class WorkerOperations(string enginePath)
{
    public async Task<WorkerEvent> ExecuteAsync(WorkerRequest request, Action<WorkerEvent> progress, CancellationToken token)
    {
        ValidateRequest(request);
        return request.Operation switch
        {
            "extract" => await ExtractAsync(request, progress, token),
            "copy" => await TransferAsync(request, false, progress, token),
            "readback" => await TransferAsync(request, true, progress, token),
            "probe" => await ProbeAsync(request, token),
            _ => throw new WorkerProtocolException()
        };
    }

    private static void ValidateRequest(WorkerRequest request)
    {
        if (request.Operation == "probe")
        {
            if (string.IsNullOrEmpty(request.ArchivePath) || !Path.IsPathFullyQualified(request.ArchivePath)
                || request.AllowedVolumes is null || request.AllowedVolumes.Count == 0 || request.AllowedVolumes.Count > 10_000
                || request.CandidatePasswords is null || request.CandidatePasswords.Count > 100_000)
                throw new WorkerProtocolException();
            return;
        }

        if (request.Files is null || request.AllowedVolumes is null || request.Exclusions is null
            || request.Files.Count > 100_000 || request.AllowedVolumes.Count > 10_000 || request.Exclusions.Count > 256
            || request.Exclusions.Any(p => p is null || p.Length > 512)
            || request.Files.Any(f => f is null || f.RelativePath is null || f.SourcePath is null || f.Sha256 is null)
            || request.MaxTotalBytes > long.MaxValue || request.MaxWrittenBytes > long.MaxValue || request.MinFreeBytes > long.MaxValue
            || request.MaxEntries > 1_000_000 || string.IsNullOrEmpty(request.DestinationDirectory)
            || !Path.IsPathFullyQualified(request.DestinationDirectory))
            throw new WorkerProtocolException();
    }

    private async Task<WorkerEvent> ExtractAsync(WorkerRequest request, Action<WorkerEvent> progress, CancellationToken token)
    {
        if (string.IsNullOrEmpty(request.ArchivePath) || !Path.IsPathFullyQualified(request.ArchivePath)
            || request.AllowedVolumes.Count == 0 || request.MaxEntries == 0)
            throw new WorkerProtocolException();
        var engine = new SevenZipEngine(enginePath);
        var extraction = new ExtractionRequest(request.ArchivePath, request.DestinationDirectory, request.Password,
            (int)request.MaxEntries, (long)request.MaxTotalBytes, request.AllowedVolumes,
            (long)request.MaxWrittenBytes, (long)request.MinFreeBytes, request.Exclusions);
        // ExtractWithManifestAsync opens headers, preflights all paths, and decodes exactly once.
        var manifest = await engine.ExtractWithManifestAsync(extraction, new DirectProgress(value => progress(new()
        {
            RequestId = request.RequestId, Kind = "progress", CompletedBytes = (ulong)value.CompletedBytes,
            TotalBytes = (ulong)value.TotalBytes
        })), token);
        var files = manifest.Files.Select(f => new FileRecord
        { RelativePath = f.RelativePath, Size = (ulong)f.Size, Sha256 = f.Sha256, IsDirectory = f.IsDirectory }).ToList();
        var message = manifest.ExcludedEntries > 0 ? $"解压完成，按规则排除了 {manifest.ExcludedEntries} 项。" : "解压完成。";
        return await CreateCompletedEventAsync(request, files, (ulong)manifest.WrittenBytes, (ulong)manifest.WrittenBytes,
            message, manifest.PasswordUsed, "archiveCrcAndStreamSha256", token);
    }

    private async Task<WorkerEvent> ProbeAsync(WorkerRequest request, CancellationToken token)
    {
        var engine = new SevenZipEngine(enginePath);
        var probeResult = await engine.ProbeAsync(request.ArchivePath, request.CandidatePasswords, token, request.AllowedVolumes);
        return new WorkerEvent
        {
            RequestId = request.RequestId,
            Kind = "completed",
            MatchedPassword = probeResult.MatchedPassword,
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

    private static async Task<WorkerEvent> TransferAsync(WorkerRequest request, bool readback,
        Action<WorkerEvent> progress, CancellationToken token)
    {
        var destination = Path.GetFullPath(request.DestinationDirectory);
        SevenZipEngine.EnsureNoReparsePoint(destination);
        if (readback)
        {
            if (!Directory.Exists(destination)) throw new IOException();
        }
        else
        {
            Directory.CreateDirectory(destination);
            SevenZipEngine.EnsureNoReparsePoint(destination);
        }
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
            SevenZipEngine.EnsureNoReparsePoint(source);
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
        var records = new List<FileRecord>(request.Files.Count);
        var buffer = ArrayPool<byte>.Shared.Rent(1024 * 1024);
        ulong completed = 0;
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
                SevenZipEngine.EnsureNoReparsePoint(source);
                SevenZipEngine.EnsureNoReparsePoint(target);
                if (item.IsDirectory)
                {
                    if (!Directory.Exists(source)) throw new IOException();
                    if (readback && !Directory.Exists(target)) throw new IOException();
                    if (!readback) Directory.CreateDirectory(target);
                    records.Add(new() { RelativePath = relative, IsDirectory = true });
                    continue;
                }
                using var input = new FileStream(source, FileMode.Open, FileAccess.Read, FileShare.Read,
                    bufferSize: 1024 * 1024, FileOptions.Asynchronous | FileOptions.SequentialScan);
                if ((ulong)input.Length != item.Size) throw new ArchiveException(ArchiveFailure.Corrupt, "来源文件大小已变化。");
                if (!readback)
                {
                    Directory.CreateDirectory(Path.GetDirectoryName(target)!);
                    SevenZipEngine.EnsureNoReparsePoint(target);
                    SevenZipEngine.EnsureFreeSpace(target, (long)request.MinFreeBytes);
                }
                using var output = readback ? null : new FileStream(target, FileMode.CreateNew, FileAccess.Write, FileShare.None,
                    bufferSize: 1024 * 1024, FileOptions.Asynchronous | FileOptions.SequentialScan);
                using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
                ulong current = 0;
                while (true)
                {
                    var count = await input.ReadAsync(buffer.AsMemory(), token);
                    if (count == 0) break;
                    current += (uint)count;
                    if (current > item.Size || (!readback && (completed > request.MaxWrittenBytes || (ulong)count > request.MaxWrittenBytes - completed)))
                        throw new ArchiveException(ArchiveFailure.ResourceLimit, "实际传输大小超过限制。");
                    if (output is not null)
                    {
                        SevenZipEngine.EnsureFreeSpace(target, (long)request.MinFreeBytes);
                        await output.WriteAsync(buffer.AsMemory(0, count), token);
                    }
                    hash.AppendData(buffer.AsSpan(0, count));
                    completed += (uint)count;
                    progress(new() { RequestId = request.RequestId, Kind = "progress", CompletedBytes = completed, TotalBytes = total });
                }
                if (current != item.Size) throw new ArchiveException(ArchiveFailure.Corrupt, "文件在传输期间发生变化。");
                if (output is not null) await output.FlushAsync(token);
                var digest = Convert.ToHexStringLower(hash.GetHashAndReset());
                if (item.Sha256.Length > 0 && !digest.Equals(item.Sha256, StringComparison.OrdinalIgnoreCase))
                    throw new ArchiveException(ArchiveFailure.Corrupt, "文件内容摘要不匹配。");
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
        var jsonBytes = System.Text.Json.JsonSerializer.SerializeToUtf8Bytes(files, Protocol.Json);
        // If manifest size is safely within IPC message limits (leave generous margin for event envelope)
        if (jsonBytes.Length <= Protocol.MaxMessageBytes - 65536)
        {
            return new()
            {
                RequestId = request.RequestId, Kind = "completed", Files = files, CompletedBytes = completedBytes,
                TotalBytes = totalBytes, Message = message, PasswordUsed = passwordUsed,
                VerificationScope = verificationScope
            };
        }

        // When manifest exceeds IPC budget, spill to a temporary manifest file in the destination directory
        var manifestFileName = $".openunpack-{Guid.NewGuid():N}-manifest.json";
        var manifestFilePath = Path.Combine(request.DestinationDirectory, manifestFileName);
        SevenZipEngine.EnsureNoReparsePoint(request.DestinationDirectory);
        await File.WriteAllBytesAsync(manifestFilePath, jsonBytes, token);

        return new()
        {
            RequestId = request.RequestId, Kind = "completed", Files = [], ManifestPath = manifestFilePath,
            CompletedBytes = completedBytes, TotalBytes = totalBytes, Message = message,
            PasswordUsed = passwordUsed, VerificationScope = verificationScope
        };
    }

    internal static string ErrorCode(Exception exception, WorkerRequest request) => exception switch
    {
        OutOfMemoryException => "ResourceLimit",
        WorkerProtocolException or ArgumentException => "ProtocolError",
        ArchiveException archive => archive.Failure switch
        {
            ArchiveFailure.WrongPassword => request.Password is null ? "NeedsPassword" : "PasswordMismatchOrCorrupt",
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
        "ProtocolError" => "作业请求无效。",
        "NeedsPassword" => "需要提供有效密码。",
        "PasswordMismatchOrCorrupt" => "密码不匹配或加密内容损坏。",
        "Corrupt" => "内容或摘要校验失败。",
        "MissingVolume" => "起始卷或其他分卷缺失。",
        "Unsupported" => "不支持此格式或解压引擎不可用。",
        "UnsafePath" => "路径、链接或条目冲突不符合处理规则。",
        "ResourceLimit" => "数量、内存、磁盘空间或写入预算不足。",
        _ => "文件读写失败，请检查磁盘和权限。"
    };

    private sealed class WorkerProtocolException : Exception;
    private sealed class DirectProgress(Action<ExtractionProgress> report) : IProgress<ExtractionProgress>
    { public void Report(ExtractionProgress value) => report(value); }
}
