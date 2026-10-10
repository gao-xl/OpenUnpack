using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;

namespace OpenUnpack.Core;

public sealed partial class BatchRunner(IArchiveEngine engine, VaultService vaultStore, VaultSnapshot vault)
{
    public async Task<BatchRunResult> RunAsync(IReadOnlyList<ArchiveSource> archives, BatchOptions options,
        IProgress<BatchUpdate>? progress, CancellationToken cancellationToken)
    {
        using var powerScope = new ExecutionStateScope();
        int completed = 0, failed = 0, skipped = 0, cancelled = 0;
        foreach (var source in archives)
        {
            if (cancellationToken.IsCancellationRequested)
            {
                cancelled++;
                progress?.Report(new(source.Path, JobStatus.Cancelled, 0, "已取消，原文件保留"));
                continue;
            }

            string? staging = null;
            CacheWorkspace? cache = null;
            int attempts = 0;
            try
            {
                progress?.Report(new(source.Path, JobStatus.Running, 0, "准备解压"));
                var root = Path.GetFullPath(string.IsNullOrWhiteSpace(options.OutputRoot)
                    ? Path.GetDirectoryName(source.Path)! : options.OutputRoot);
                Directory.CreateDirectory(root);
                EnsureNoReparseAncestors(root);
                var output = Path.Combine(root, GetOutputName(source.Path));
                if (options.ConflictPolicy == ConflictPolicy.Skip && (Directory.Exists(output) || File.Exists(output)))
                {
                    skipped++;
                    Finish(source, JobStatus.Skipped, "目标目录已存在", output, attempts, progress);
                    continue;
                }
                ArchiveSource workingSource = source;
                if (!string.IsNullOrWhiteSpace(options.CacheRoot))
                {
                    long freeBytes = 0;
                    try
                    {
                        var rootPath = Path.GetPathRoot(Path.GetFullPath(options.CacheRoot));
                        if (!string.IsNullOrEmpty(rootPath)) freeBytes = new DriveInfo(rootPath).AvailableFreeSpace;
                    }
                    catch { }

                    var probeSummary = new ArchiveProbeSummary(
                        TotalSizeBytes: new FileInfo(source.Path).Length,
                        EstimatedExtractedBytes: new FileInfo(source.Path).Length * 2,
                        EntryCount: 100,
                        HasNestedArchives: false,
                        IsSourceOnHdd: false,
                        IsTargetOnHdd: false,
                        IsSamePhysicalDisk: false
                    );
                    var route = CacheRouteDecider.Decide(probeSummary, options, output, freeBytes);
                    if (route.Type == ExtractionRouteType.SsdCache)
                    {
                        progress?.Report(new(source.Path, JobStatus.Running, 0, options.CacheInput ? "正在复制压缩包到 SSD 缓存" : "准备 SSD 暂存目录"));
                        cache = new CacheWorkspace(options);
                        workingSource = await cache.PrepareInputAsync(source, options.CacheInput, cancellationToken);
                    }
                    else
                    {
                        progress?.Report(new(source.Path, JobStatus.Running, 0, $"智能直通解压: {route.Reason}"));
                    }
                }
                var fingerprint = await FingerprintAsync(workingSource, cancellationToken);
                var candidates = VaultService.GetCandidates(vault, fingerprint, options.PasswordGroup).ToList();
                var hint = ArchiveScanner.ExtractPasswordFromFilename(source.DisplayName)
                    ?? ArchiveScanner.ExtractPasswordFromFilename(source.Path);
                if (!string.IsNullOrWhiteSpace(hint))
                {
                    var existingPos = candidates.FindIndex(c => c.Secret == hint);
                    if (existingPos >= 0)
                    {
                        var existing = candidates[existingPos];
                        candidates.RemoveAt(existingPos);
                        candidates.Insert(0, existing);
                    }
                    else
                    {
                        candidates.Insert(0, new() { Label = "文件名提取", Secret = hint });
                    }
                }

                PasswordEntry? matched = null;
                var choices = new List<PasswordEntry?> { null };
                choices.AddRange(candidates);

                if (engine is IPasswordProbe probeEngine)
                {
                    try
                    {
                        progress?.Report(new(source.Path, JobStatus.Running, 0, "正在探测密码"));
                        var candidateSecrets = candidates.Select(c => c.Secret).ToList();
                        var probeResult = await probeEngine.ProbeAsync(workingSource.Path, candidateSecrets, cancellationToken, workingSource.Volumes);
                        switch (probeResult.Status)
                        {
                            case PasswordProbeStatus.NoPasswordNeeded:
                                choices = [null];
                                break;
                            case PasswordProbeStatus.Success when !string.IsNullOrEmpty(probeResult.MatchedPassword):
                                var matchEntry = candidates.FirstOrDefault(c => c.Secret == probeResult.MatchedPassword)
                                    ?? new() { Label = "探测匹配", Secret = probeResult.MatchedPassword };
                                choices = [matchEntry];
                                break;
                            case PasswordProbeStatus.WrongPassword:
                                choices = [];
                                break;
                            case PasswordProbeStatus.MissingVolume:
                                throw new ArchiveException(ArchiveFailure.MissingVolume, probeResult.ErrorMessage ?? "压缩包缺少分卷。");
                            case PasswordProbeStatus.Corrupt:
                                throw new ArchiveException(ArchiveFailure.Corrupt, probeResult.ErrorMessage ?? "压缩包损坏或加密数据异常。");
                            case PasswordProbeStatus.Unsupported:
                                throw new ArchiveException(ArchiveFailure.Unsupported, probeResult.ErrorMessage ?? "不支持的压缩方法或加密算法。");
                            default:
                                break;
                        }
                    }
                    catch (OperationCanceledException) { throw; }
                    catch (ArchiveException) { throw; }
                    catch
                    {
                        choices = [null];
                        choices.AddRange(candidates);
                    }
                }
                foreach (var candidate in choices)
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    attempts++;
                    var stagingRoot = cache?.DirectoryPath ?? root;
                    staging = Path.Combine(stagingRoot, ".openunpack-" + Guid.NewGuid().ToString("N"));
                    Directory.CreateDirectory(staging);
                    var attemptNumber = attempts;
                    progress?.Report(new(source.Path, JobStatus.Running, 0,
                        candidate is null ? "尝试无密码解压" : $"尝试已保存密码 {attempts - 1}/{candidates.Count}", attempts));
                    try
                    {
                        // Inline reporting preserves event order; the outer desktop Progress dispatches to the UI.
                        var extractionProgress = new InlineProgress<ExtractionProgress>(p => progress?.Report(new(
                            source.Path, JobStatus.Running,
                            p.TotalBytes > 0 ? Math.Clamp(p.CompletedBytes * 100d / p.TotalBytes, 0, 99.9) : 0,
                            "正在写入暂存目录", attemptNumber)));
                        var maxOutputBytes = Math.Min(options.MaxTotalBytes, cache?.ExtractionBudget ?? long.MaxValue);
                        await engine.ExtractAsync(new(workingSource.Path, staging, candidate?.Secret,
                            options.MaxEntries, maxOutputBytes), extractionProgress, cancellationToken);
                        cancellationToken.ThrowIfCancellationRequested();
                        EnsureSafeTree(staging);
                        matched = candidate;
                        break;
                    }
                    catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.WrongPassword)
                    {
                        DeleteStaging(stagingRoot, staging);
                        staging = null;
                    }
                }
                if (staging is null)
                {
                    failed++;
                    Finish(source, JobStatus.NeedsPassword, candidates.Count == 0
                        ? "需要密码，请加入密码库后重试"
                        : "已保存密码均未解开；请检查密码或加密数据是否损坏", null, attempts, progress);
                    continue;
                }
                string? finalOutput;
                if (cache is not null)
                {
                    progress?.Report(new(source.Path, JobStatus.Running, 99, "SSD 解压完成，正在写入最终目录", attempts));
                    finalOutput = await PublishFromCacheAsync(staging, output, options.ConflictPolicy, cancellationToken);
                    DeleteStaging(cache.DirectoryPath, staging);
                }
                else finalOutput = Promote(staging, output, options.ConflictPolicy);
                staging = null;
                if (finalOutput is null)
                {
                    skipped++;
                    Finish(source, JobStatus.Skipped, "目标在解压期间已创建，已跳过", output, attempts, progress);
                    continue;
                }
                if (matched is not null && vault.Entries.Any(e => e.Id == matched.Id))
                    VaultService.RecordSuccess(vault, matched.Id, fingerprint);
                completed++;
                Finish(source, JobStatus.Completed, "解压完成", finalOutput, attempts, progress, matched?.Label);
            }
            catch (OperationCanceledException)
            {
                cancelled++;
                progress?.Report(new(source.Path, JobStatus.Cancelled, 0, "已取消，原文件保留", attempts));
            }
            catch (ArchiveException ex)
            {
                failed++;
                Finish(source, JobStatus.Failed, FailureMessage(ex.Failure), null, attempts, progress);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or CryptographicException)
            {
                failed++;
                Finish(source, JobStatus.Failed, "文件读取或输出失败，请检查路径、权限与可用空间", null, attempts, progress);
            }
            finally
            {
                if (staging is not null)
                {
                    try { DeleteStaging(Path.GetDirectoryName(staging)!, staging); }
                    catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
                    {
                        progress?.Report(new(source.Path, JobStatus.Failed, 0, "暂存目录清理失败，请手动检查输出目录", attempts));
                    }
                }
                if (cache is not null)
                {
                    try { cache.Dispose(); }
                    catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
                    {
                        progress?.Report(new(source.Path, JobStatus.Failed, 0, "SSD 缓存清理失败，请手动检查所选缓存目录", attempts));
                    }
                }
            }
        }
        if (options.PlaySoundOnCompletion && completed > 0)
        {
            try { MessageBeep(MbIconAsterisk); } catch { }
        }
        return new(completed, failed, skipped, cancelled);
    }

    [System.Runtime.InteropServices.DllImport("kernel32.dll", ExactSpelling = true)]
    private static extern uint SetThreadExecutionState(uint esFlags);
    [System.Runtime.InteropServices.DllImport("user32.dll", ExactSpelling = true)]
    private static extern bool MessageBeep(uint uType);

    private const uint EsContinuous = 0x80000000;
    private const uint EsSystemRequired = 0x00000001;
    private const uint MbIconAsterisk = 0x00000040;

    public sealed class ExecutionStateScope : IDisposable
    {
        public ExecutionStateScope()
        {
            try { SetThreadExecutionState(EsContinuous | EsSystemRequired); } catch { }
        }
        public void Dispose()
        {
            try { SetThreadExecutionState(EsContinuous); } catch { }
        }
    }

    private void Finish(ArchiveSource source, JobStatus status, string message, string? output, int attempts,
        IProgress<BatchUpdate>? progress, string? label = null)
    {
        vault.History.Insert(0, new(source.DisplayName, status.ToString(), output, DateTimeOffset.UtcNow, message));
        if (vault.History.Count > 500) vault.History.RemoveRange(500, vault.History.Count - 500);
        try { vaultStore.Save(vault); }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or CryptographicException or InvalidDataException)
        {
            message += "；本地记录保存失败，请检查数据目录";
        }
        progress?.Report(new(source.Path, status, status == JobStatus.Completed ? 100 : 0, message, attempts, label, output));
    }

    private static string FailureMessage(ArchiveFailure failure) => failure switch
    {
        ArchiveFailure.Unsupported => "无法识别或暂不支持此压缩格式",
        ArchiveFailure.Corrupt => "压缩包损坏或内容校验失败",
        ArchiveFailure.MissingVolume => "分卷缺失，请将所有分卷放在同一目录",
        ArchiveFailure.UnsafePath => "压缩包含不安全路径、链接或重复条目，已拒绝解压",
        ArchiveFailure.ResourceLimit => "超过条目、解压大小或 SSD 缓存空间限制",
        ArchiveFailure.WrongPassword => "密码不匹配或加密数据损坏",
        _ => "文件读取或输出失败，请检查权限与可用空间"
    };

    public static string GetOutputName(string path)
    {
        var fileName = Path.GetFileName(path);
        fileName = SevenZipVolume().Replace(fileName, "");
        fileName = RarVolume().Replace(fileName, "");
        fileName = ZipVolume().Replace(fileName, "");
        if (fileName.EndsWith(".zip", StringComparison.OrdinalIgnoreCase) || fileName.EndsWith(".7z", StringComparison.OrdinalIgnoreCase)
            || fileName.EndsWith(".rar", StringComparison.OrdinalIgnoreCase)) fileName = Path.GetFileNameWithoutExtension(fileName);
        fileName = fileName.TrimEnd('.', ' ');
        return string.IsNullOrWhiteSpace(fileName) ? "解压结果" : fileName;
    }

    private static string? Promote(string staging, string output, ConflictPolicy policy)
    {
        for (var suffix = 0; suffix < 100_000; suffix++)
        {
            var candidate = suffix == 0 ? output : $"{output} ({suffix})";
            if (Directory.Exists(candidate) || File.Exists(candidate))
            {
                if (policy == ConflictPolicy.Skip) { DeleteStaging(Path.GetDirectoryName(staging)!, staging); return null; }
                continue;
            }
            try { Directory.Move(staging, candidate); return candidate; }
            catch (IOException) when (Directory.Exists(candidate) || File.Exists(candidate))
            {
                if (policy == ConflictPolicy.Skip) { DeleteStaging(Path.GetDirectoryName(staging)!, staging); return null; }
            }
        }
        throw new IOException("Cannot reserve an output folder.");
    }

    internal static void EnsureNoReparseAncestors(string path)
    {
        for (var directory = new DirectoryInfo(path); directory is not null; directory = directory.Parent)
            if (directory.Exists && (directory.Attributes & FileAttributes.ReparsePoint) != 0)
                throw new ArchiveException(ArchiveFailure.UnsafePath, "输出目录不能包含链接。");
    }

    private static void EnsureSafeTree(string root)
    {
        EnsureNoReparseAncestors(root);
        var pending = new Stack<string>();
        pending.Push(root);
        while (pending.TryPop(out var directory))
            foreach (var entry in Directory.EnumerateFileSystemEntries(directory))
            {
                var attributes = File.GetAttributes(entry);
                if ((attributes & FileAttributes.ReparsePoint) != 0)
                    throw new ArchiveException(ArchiveFailure.UnsafePath, "不允许输出链接。");
                if ((attributes & FileAttributes.Directory) != 0) pending.Push(entry);
            }
    }

    internal static void DeleteStaging(string root, string staging)
    {
        var fullRoot = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
        var fullStaging = Path.GetFullPath(staging);
        if (!fullStaging.StartsWith(fullRoot, StringComparison.OrdinalIgnoreCase)
            || !Path.GetFileName(fullStaging).StartsWith(".openunpack-", StringComparison.Ordinal))
            throw new IOException("Invalid staging path.");
        if (!Directory.Exists(fullStaging)) return;
        // Do not follow attacker-controlled reparse points during cleanup.
        DeleteTreeWithoutFollowingLinks(fullStaging);
    }

    private static void DeleteTreeWithoutFollowingLinks(string directory)
    {
        if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) != 0) { Directory.Delete(directory); return; }
        foreach (var path in Directory.EnumerateFileSystemEntries(directory))
        {
            var attributes = File.GetAttributes(path);
            if ((attributes & FileAttributes.Directory) != 0) DeleteTreeWithoutFollowingLinks(path);
            else { File.SetAttributes(path, FileAttributes.Normal); File.Delete(path); }
        }
        Directory.Delete(directory);
    }

    private static async Task<string?> PublishFromCacheAsync(string staging, string output, ConflictPolicy policy,
        CancellationToken token)
    {
        var outputRoot = Path.GetDirectoryName(output)!;
        EnsureNoReparseAncestors(outputRoot);
        var temporaryOutput = Path.Combine(outputRoot, ".openunpack-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(temporaryOutput);
        try
        {
            var pending = new Stack<(string Source, string Target)>();
            pending.Push((staging, temporaryOutput));
            while (pending.TryPop(out var directory))
                foreach (var entry in Directory.EnumerateFileSystemEntries(directory.Source))
                {
                    token.ThrowIfCancellationRequested();
                    var attributes = File.GetAttributes(entry);
                    if ((attributes & FileAttributes.ReparsePoint) != 0)
                        throw new ArchiveException(ArchiveFailure.UnsafePath, "不允许输出链接。");
                    var target = Path.Combine(directory.Target, Path.GetFileName(entry));
                    if ((attributes & FileAttributes.Directory) != 0)
                    {
                        Directory.CreateDirectory(target);
                        pending.Push((entry, target));
                    }
                    else await CopyFileAsync(entry, target, token);
                }
            token.ThrowIfCancellationRequested();
            var final = Promote(temporaryOutput, output, policy);
            temporaryOutput = "";
            return final;
        }
        finally
        {
            if (!string.IsNullOrEmpty(temporaryOutput)) DeleteStaging(outputRoot, temporaryOutput);
        }
    }

    internal static async Task CopyFileAsync(string source, string target, CancellationToken token)
    {
        await using (var input = new FileStream(source, FileMode.Open, FileAccess.Read, FileShare.Read,
            1024 * 1024, FileOptions.Asynchronous | FileOptions.SequentialScan))
        await using (var output = new FileStream(target, FileMode.CreateNew, FileAccess.Write, FileShare.None,
            1024 * 1024, FileOptions.Asynchronous | FileOptions.SequentialScan))
        {
            var buffer = new byte[1024 * 1024];
            int read;
            while ((read = await input.ReadAsync(buffer, token)) > 0) await output.WriteAsync(buffer.AsMemory(0, read), token);
            await output.FlushAsync(token);
        }
        File.SetLastWriteTimeUtc(target, File.GetLastWriteTimeUtc(source));
    }

    private const int SparseThresholdBytes = 256 * 1024;
    private const int SparseSampleSize = 64 * 1024;

    public static async Task<string> FingerprintAsync(ArchiveSource source, CancellationToken cancellationToken)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        var paths = source.Volumes.Count > 0 ? source.Volumes : [source.Path];
        var buffer = new byte[SparseSampleSize];
        foreach (var path in paths)
        {
            var fileInfo = new FileInfo(path);
            var length = fileInfo.Length;
            hash.AppendData(BitConverter.GetBytes(length));

            if (length <= SparseThresholdBytes)
            {
                await using var file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read,
                    buffer.Length, FileOptions.Asynchronous | FileOptions.SequentialScan);
                int read;
                while ((read = await file.ReadAsync(buffer.AsMemory(0, buffer.Length), cancellationToken)) > 0)
                {
                    hash.AppendData(buffer.AsSpan(0, read));
                }
            }
            else
            {
                await using var file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read,
                    buffer.Length, FileOptions.Asynchronous | FileOptions.RandomAccess);

                int headRead = await ReadExactOrAvailableAsync(file, buffer, 0, SparseSampleSize, cancellationToken);
                hash.AppendData(buffer.AsSpan(0, headRead));

                long midOffset = (length - SparseSampleSize) / 2;
                if (midOffset > headRead)
                {
                    file.Seek(midOffset, SeekOrigin.Begin);
                    int midRead = await ReadExactOrAvailableAsync(file, buffer, 0, SparseSampleSize, cancellationToken);
                    hash.AppendData(buffer.AsSpan(0, midRead));
                }

                long tailOffset = Math.Max(0, length - SparseSampleSize);
                if (tailOffset > headRead && tailOffset > midOffset)
                {
                    file.Seek(tailOffset, SeekOrigin.Begin);
                    int tailRead = await ReadExactOrAvailableAsync(file, buffer, 0, SparseSampleSize, cancellationToken);
                    hash.AppendData(buffer.AsSpan(0, tailRead));
                }
            }
        }
        return Convert.ToHexString(hash.GetHashAndReset());
    }

    private static async Task<int> ReadExactOrAvailableAsync(FileStream stream, byte[] buffer, int offset, int count, CancellationToken cancellationToken)
    {
        int total = 0;
        while (total < count)
        {
            int read = await stream.ReadAsync(buffer.AsMemory(offset + total, count - total), cancellationToken);
            if (read == 0) break;
            total += read;
        }
        return total;
    }

    [GeneratedRegex(@"\.(?:7z|zip)\.\d{3,}$", RegexOptions.IgnoreCase)] private static partial Regex SevenZipVolume();
    [GeneratedRegex(@"\.part\d+\.rar$", RegexOptions.IgnoreCase)] private static partial Regex RarVolume();
    [GeneratedRegex(@"\.z\d+$", RegexOptions.IgnoreCase)] private static partial Regex ZipVolume();
    private sealed class InlineProgress<T>(Action<T> report) : IProgress<T> { public void Report(T value) => report(value); }
}
