namespace OpenUnpack.Core;

/// <summary>A per-job workspace; cleanup is restricted to the directory created by this instance.</summary>
internal sealed class CacheWorkspace : IDisposable
{
    private readonly long _quota;
    private readonly long _reserve;
    private readonly string _cacheRoot;
    private long _inputBytes;
    public string DirectoryPath { get; }

    public CacheWorkspace(BatchOptions options)
    {
        _cacheRoot = Path.GetFullPath(options.CacheRoot!);
        if (_cacheRoot.StartsWith(@"\\", StringComparison.Ordinal) || options.MaxCacheBytes < 1024 * 1024
            || options.MinFreeCacheBytes < 0)
            throw new ArchiveException(ArchiveFailure.IoError, "缓存需要本地目录和有效容量设置。");
        _quota = options.MaxCacheBytes;
        _reserve = options.MinFreeCacheBytes;
        Directory.CreateDirectory(_cacheRoot);
        BatchRunner.EnsureNoReparseAncestors(_cacheRoot);
        DirectoryPath = Path.Combine(_cacheRoot, ".openunpack-cache-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(DirectoryPath);
        File.WriteAllText(Path.Combine(DirectoryPath, ".owner"), "OpenUnpack temporary workspace v1");
    }

    public long ExtractionBudget
    {
        get
        {
            var usedByOtherJobs = CountOtherWorkspaces();
            var free = AvailableFreeSpace();
            var result = Math.Min(_quota - _inputBytes - usedByOtherJobs, free - _reserve);
            if (result <= 0) throw new ArchiveException(ArchiveFailure.ResourceLimit, "缓存容量或剩余空间不足。");
            return result;
        }
    }

    public async Task<ArchiveSource> PrepareInputAsync(ArchiveSource source, bool copyInput, CancellationToken token)
    {
        if (!copyInput) return source;
        var originals = source.Volumes.Count > 0 ? source.Volumes : [source.Path];
        var required = originals.Sum(p => new FileInfo(p).Length);
        if (required > ExtractionBudget)
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "输入压缩包超过缓存容量或剩余空间限制。");
        var inputDirectory = Path.Combine(DirectoryPath, "input");
        Directory.CreateDirectory(inputDirectory);
        var copied = new List<string>();
        var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var sourcePath in originals)
        {
            token.ThrowIfCancellationRequested();
            var name = Path.GetFileName(sourcePath);
            if (!names.Add(name)) throw new ArchiveException(ArchiveFailure.MissingVolume, "分卷文件名冲突。");
            var target = Path.Combine(inputDirectory, name);
            await BatchRunner.CopyFileAsync(sourcePath, target, token);
            _inputBytes += new FileInfo(target).Length;
            copied.Add(target);
        }
        return source with { Path = Path.Combine(inputDirectory, Path.GetFileName(source.Path)), Volumes = copied };
    }

    private long AvailableFreeSpace()
    {
        try { return new DriveInfo(Path.GetPathRoot(_cacheRoot)!).AvailableFreeSpace; }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        { throw new ArchiveException(ArchiveFailure.IoError, "无法读取缓存磁盘的剩余空间。", ex); }
    }

    private long CountOtherWorkspaces()
    {
        long total = 0;
        foreach (var path in Directory.EnumerateDirectories(_cacheRoot, ".openunpack-cache-*"))
        {
            if (path.Equals(DirectoryPath, StringComparison.OrdinalIgnoreCase)) continue;
            if ((File.GetAttributes(path) & FileAttributes.ReparsePoint) != 0 || !File.Exists(Path.Combine(path, ".owner"))) continue;
            var pending = new Stack<string>();
            pending.Push(path);
            while (pending.TryPop(out var directory))
                foreach (var entry in Directory.EnumerateFileSystemEntries(directory))
                {
                    var attributes = File.GetAttributes(entry);
                    if ((attributes & FileAttributes.ReparsePoint) != 0) continue;
                    if ((attributes & FileAttributes.Directory) != 0) pending.Push(entry);
                    else total = checked(total + new FileInfo(entry).Length);
                }
        }
        return total;
    }

    public void Dispose()
    {
        BatchRunner.DeleteStaging(_cacheRoot, DirectoryPath);
    }
}
