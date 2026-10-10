using System.Runtime.InteropServices;
using System.Buffers;
using System.Security.Cryptography;
using System.Text.RegularExpressions;

namespace OpenUnpack.Core.Native;

public sealed class SevenZipEngine : IArchiveEngine, IPasswordProbe
{
    private readonly string _dllPath;
    private readonly Lazy<Library> _library;
    public SevenZipEngine(string dllPath)
    {
        _dllPath = Path.GetFullPath(dllPath);
        _library = new(() => new Library(_dllPath));
    }

    public Task<ArchiveInspection> InspectAsync(string archivePath, string? password, CancellationToken cancellationToken) =>
        Task.Run(() => Inspect(archivePath, password, cancellationToken), cancellationToken);

    public Task ExtractAsync(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken cancellationToken) =>
        Task.Run(() => Extract(request, progress, cancellationToken), cancellationToken);

    public Task<PasswordProbeResult> ProbeAsync(string archivePath, IReadOnlyList<string> candidatePasswords, CancellationToken cancellationToken, IReadOnlyList<string>? allowedVolumes = null) =>
        Task.Run(() => Probe(archivePath, candidatePasswords, cancellationToken, allowedVolumes), cancellationToken);

    // Worker preflight reads headers and entry properties; it never decodes every file a second time.
    public Task<ArchiveInspection> PreflightAsync(ExtractionRequest request, CancellationToken cancellationToken) =>
        Task.Run(() =>
        {
            using var opened = Open(request.ArchivePath, request.Password, cancellationToken, request.AllowedVolumes);
            return new ArchiveInspection(ReadEntries(opened, request.MaxEntries, request.MaxTotalBytes, cancellationToken));
        }, cancellationToken);

    public Task<ExtractionManifest> ExtractWithManifestAsync(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken cancellationToken) =>
        Task.Run(() => Extract(request, progress, cancellationToken), cancellationToken);

    private ArchiveInspection Inspect(string path, string? password, CancellationToken token)
    {
        using var opened = Open(path, password, token);
        var entries = ReadEntries(opened, 100_000, 100L * 1024 * 1024 * 1024, token);
        using var callback = new ExtractCallback(opened.Callback, entries, null, 100L * 1024 * 1024 * 1024, null, token);
        var result = opened.Archive.Extract(IntPtr.Zero, uint.MaxValue, 1, callback);
        ValidateResult(opened, callback, result, token);
        return new(entries);
    }

    private ExtractionManifest Extract(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken token)
    {
        if (request.MaxEntries <= 0 || request.MaxTotalBytes < 0 || request.MaxWrittenBytes < 0 || request.MinFreeBytes < 0)
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "解压数量或大小上限无效。");
        using var opened = Open(request.ArchivePath, request.Password, token, request.AllowedVolumes);
        var entries = ReadEntries(opened, request.MaxEntries, request.MaxTotalBytes, token);
        var destination = Path.GetFullPath(request.DestinationDirectory);
        EnsureNoReparsePoint(destination);
        Directory.CreateDirectory(destination);
        using var callback = new ExtractCallback(opened.Callback, entries, destination,
            Math.Min(request.MaxTotalBytes, request.MaxWrittenBytes), progress, token, request.MinFreeBytes, request.Exclusions);
        var result = opened.Archive.Extract(IntPtr.Zero, uint.MaxValue, 0, callback);
        ValidateResult(opened, callback, result, token);
        progress?.Report(new(callback.WrittenBytes, entries.Sum(e => e.Size), callback.CompletedFiles, entries.Count));
        return new(callback.Files.ToArray(), opened.Callback.PasswordRequested, callback.WrittenBytes, callback.ExcludedEntries);
    }

    public PasswordProbeResult Probe(string path, IReadOnlyList<string> candidatePasswords, CancellationToken token, IReadOnlyList<string>? allowedVolumes = null)
    {
        token.ThrowIfCancellationRequested();
        var fullPath = Path.GetFullPath(path);
        if (!File.Exists(fullPath))
            return new(PasswordProbeStatus.MissingVolume, null, "压缩包或起始分卷不存在。");
        EnsureNoReparsePoint(fullPath);

        OpenedArchive? opened = null;
        bool headerEncrypted = false;
        try
        {
            opened = Open(fullPath, null, token, allowedVolumes);
        }
        catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.WrongPassword)
        {
            headerEncrypted = true;
        }
        catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.MissingVolume)
        {
            return new(PasswordProbeStatus.MissingVolume, null, ex.Message);
        }
        catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.Unsupported)
        {
            return new(PasswordProbeStatus.Unsupported, null, ex.Message);
        }
        catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.Corrupt)
        {
            headerEncrypted = true;
        }

        if (headerEncrypted)
        {
            foreach (var candidate in candidatePasswords)
            {
                token.ThrowIfCancellationRequested();
                if (string.IsNullOrEmpty(candidate)) continue;
                try
                {
                    using var candidateArchive = Open(fullPath, candidate, token, allowedVolumes);
                    return new(PasswordProbeStatus.Success, candidate);
                }
                catch (ArchiveException ex) when (ex.Failure is ArchiveFailure.WrongPassword or ArchiveFailure.Corrupt)
                {
                    // Candidate failed
                }
            }
            return new(PasswordProbeStatus.WrongPassword, null);
        }

        if (opened is null)
            return new(PasswordProbeStatus.Corrupt, null, "无法打开压缩包。");

        try
        {
            using (opened)
            {
                if (opened.Archive.GetNumberOfItems(out var count) != 0)
                    return new(PasswordProbeStatus.Corrupt, null, "无法读取压缩包目录。");
                if (count == 0)
                    return new(PasswordProbeStatus.NoPasswordNeeded, null);

                int targetIndex = -1;
                bool hasExplicitEncrypted = false;
                for (uint i = 0; i < count; i++)
                {
                    token.ThrowIfCancellationRequested();
                    var isDir = GetBool(opened.Archive, i, 6);
                    if (isDir) continue;

                    var isEncrypted = GetBool(opened.Archive, i, 19);
                    if (isEncrypted)
                    {
                        hasExplicitEncrypted = true;
                        targetIndex = (int)i;
                        break;
                    }
                }

                if (!hasExplicitEncrypted)
                {
                    for (uint i = 0; i < count; i++)
                    {
                        if (!GetBool(opened.Archive, i, 6))
                        {
                            targetIndex = (int)i;
                            break;
                        }
                    }
                }

                if (targetIndex < 0)
                {
                    return new(PasswordProbeStatus.NoPasswordNeeded, null);
                }

                uint[] targetIndices = [(uint)targetIndex];
                var handle = GCHandle.Alloc(targetIndices, GCHandleType.Pinned);
                bool needsPassword = hasExplicitEncrypted;
                try
                {
                    var nullCallback = new ProbeExtractCallback(null, token);
                    var nullResult = opened.Archive.Extract(handle.AddrOfPinnedObject(), 1, 1, nullCallback);
                    if (nullCallback.PasswordRequested || nullResult != 0 || nullCallback.OperationResult is 9 or 2 or 3)
                    {
                        needsPassword = true;
                    }
                    else if (!hasExplicitEncrypted && nullResult == 0 && nullCallback.OperationResult == 0)
                    {
                        return new(PasswordProbeStatus.NoPasswordNeeded, null);
                    }
                }
                finally
                {
                    handle.Free();
                }

                if (!needsPassword)
                {
                    return new(PasswordProbeStatus.NoPasswordNeeded, null);
                }

                if (candidatePasswords.Count == 0)
                {
                    return new(PasswordProbeStatus.WrongPassword, null);
                }

                OpenedArchive currentArchive = opened;
                bool ownsCurrent = false;
                try
                {
                    foreach (var candidate in candidatePasswords)
                    {
                        token.ThrowIfCancellationRequested();
                        if (string.IsNullOrEmpty(candidate)) continue;

                        bool matched = false;
                        uint[] probeIndices = [(uint)targetIndex];
                        var probeHandle = GCHandle.Alloc(probeIndices, GCHandleType.Pinned);
                        int extractResult;
                        ProbeExtractCallback probeCallback;
                        try
                        {
                            probeCallback = new ProbeExtractCallback(candidate, token);
                            extractResult = currentArchive.Archive.Extract(probeHandle.AddrOfPinnedObject(), 1, 1, probeCallback);
                        }
                        catch
                        {
                            extractResult = NativeResult.Fail;
                            probeCallback = new ProbeExtractCallback(candidate, token);
                        }
                        finally
                        {
                            probeHandle.Free();
                        }

                        if (extractResult == 0 && probeCallback.OperationResult == 0)
                        {
                            matched = true;
                        }
                        else if (extractResult != 0 && extractResult != NativeResult.Abort)
                        {
                            if (ownsCurrent) currentArchive.Dispose();
                            currentArchive = Open(fullPath, null, token, allowedVolumes);
                            ownsCurrent = true;
                        }

                        if (matched)
                        {
                            return new(PasswordProbeStatus.Success, candidate);
                        }
                    }
                }
                finally
                {
                    if (ownsCurrent) currentArchive.Dispose();
                }

                return new(PasswordProbeStatus.WrongPassword, null);
            }
        }
        catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.MissingVolume)
        {
            return new(PasswordProbeStatus.MissingVolume, null, ex.Message);
        }
        catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.Unsupported)
        {
            return new(PasswordProbeStatus.Unsupported, null, ex.Message);
        }
        catch (Exception ex) when (ex is not OperationCanceledException)
        {
            return new(PasswordProbeStatus.Corrupt, null, ex.Message);
        }
    }

    private OpenedArchive Open(string input, string? password, CancellationToken token, IReadOnlyList<string>? allowedVolumes = null)
    {
        token.ThrowIfCancellationRequested();
        var path = Path.GetFullPath(input);
        if (!File.Exists(path))
            throw new ArchiveException(ArchiveFailure.MissingVolume, "压缩包或起始分卷不存在。");
        EnsureNoReparsePoint(path);
        var approved = ResolveApprovedVolumes(path, allowedVolumes);
        var parts = NumericJoinedParts(path, approved);
        var name = Path.GetFileName(path).ToLowerInvariant();
        var formatName = Regex.Replace(name, @"\.\d{3,}$", "");
        var formats = Path.GetExtension(formatName) switch
        {
            ".7z" => new byte[] { 7 }, ".zip" => [1], ".rar" => [3, 0xCC],
            ".tar" => [0xEE], ".gz" => [0xEF], ".bz2" => [2], ".xz" => [0xC], ".cab" => [8],
            _ => throw new ArchiveException(ArchiveFailure.Unsupported, "此压缩格式暂不支持。")
        };
        foreach (var format in formats)
        {
            token.ThrowIfCancellationRequested();
            var archive = _library.Value.Create(format);
            var callback = new OpenCallback(path, password, token, approved);
            var stream = new InputStream(new JoinedFileStream(parts), token, callback);
            int result;
            try { result = archive.Open(stream, IntPtr.Zero, callback); }
            catch { ReleaseArchive(archive); stream.Dispose(); callback.Dispose(); throw; }
            if (result == NativeResult.Ok)
                return new(archive, stream, callback);
            ReleaseArchive(archive);
            stream.Dispose();
            callback.Dispose();
            token.ThrowIfCancellationRequested();
            if (callback.Error is not null) throw callback.Error;
            if (callback.PasswordRequested)
                throw new ArchiveException(ArchiveFailure.WrongPassword, "需要有效密码，或加密头部已损坏。");
            if (callback.MissingVolume)
                throw new ArchiveException(ArchiveFailure.MissingVolume, "压缩包缺少分卷，请将所有分卷放在同一文件夹。");
        }
        throw new ArchiveException(ArchiveFailure.Corrupt, "无法打开压缩包：文件可能损坏、分卷不完整或扩展名与格式不符。");
    }

    private static List<ArchiveEntryInfo> ReadEntries(OpenedArchive opened, int maxEntries, long maxBytes, CancellationToken token)
    {
        if (opened.Archive.GetNumberOfItems(out var count) != 0)
            throw new ArchiveException(ArchiveFailure.Corrupt, "无法读取压缩包目录。");
        if (count > maxEntries)
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "压缩包条目数量超过上限。");
        var entries = new List<ArchiveEntryInfo>((int)count);
        var destinations = new Dictionary<string, bool>(StringComparer.OrdinalIgnoreCase);
        long total = 0;
        for (uint i = 0; i < count; i++)
        {
            token.ThrowIfCancellationRequested();
            var name = GetText(opened.Archive, i, 3) ?? GetText(opened.Archive, i, 4);
            if (string.IsNullOrEmpty(name)) name = Path.GetFileNameWithoutExtension(opened.Callback.ArchivePath);
            var directory = GetBool(opened.Archive, i, 6);
            var size = GetNumber(opened.Archive, i, 7);
            var attributes = GetNumber(opened.Archive, i, 9);
            var posix = GetNumber(opened.Archive, i, 53);
            var link = !string.IsNullOrEmpty(GetText(opened.Archive, i, 54))
                || !string.IsNullOrEmpty(GetText(opened.Archive, i, 90))
                || GetBool(opened.Archive, i, 63) || GetBool(opened.Archive, i, 21)
                || (attributes & (ulong)FileAttributes.ReparsePoint) != 0 || (posix & 0xF000) == 0xA000;
            if (link) throw new ArchiveException(ArchiveFailure.UnsafePath, "压缩包包含链接、备用数据流或删除标记，已拒绝解压。");
            var rootDirectory = directory && name.Replace('/', '\\').TrimEnd('\\') == ".";
            var safePath = rootDirectory ? "" : ValidateRelativePath(name);
            if (destinations.ContainsKey(safePath))
                throw new ArchiveException(ArchiveFailure.UnsafePath, "压缩包包含重复或大小写冲突的路径。");
            destinations.Add(safePath, directory);
            if (size > long.MaxValue || total > maxBytes - (long)size)
                throw new ArchiveException(ArchiveFailure.ResourceLimit, "解压后的总大小超过上限。");
            total += (long)size;
            entries.Add(new(safePath, directory ? 0 : (long)size, directory));
        }
        foreach (var entry in entries)
        {
            var parent = Path.GetDirectoryName(entry.Path);
            while (!string.IsNullOrEmpty(parent))
            {
                if (destinations.TryGetValue(parent, out var isDirectory) && !isDirectory)
                    throw new ArchiveException(ArchiveFailure.UnsafePath, "压缩包中文件与文件夹路径冲突。");
                parent = Path.GetDirectoryName(parent);
            }
        }
        return entries;
    }

    public static string ValidateRelativePath(string name)
    {
        var normalized = name.Replace('/', Path.DirectorySeparatorChar).TrimEnd(Path.DirectorySeparatorChar);
        // GNU tar commonly prefixes entries with ./; removing that prefix cannot escape the root.
        while (normalized.StartsWith("." + Path.DirectorySeparatorChar, StringComparison.Ordinal)) normalized = normalized[2..];
        if (string.IsNullOrEmpty(normalized) || Path.IsPathRooted(normalized) || normalized.Contains(':') || normalized.Contains('\0'))
            throw new ArchiveException(ArchiveFailure.UnsafePath, "压缩包包含不安全的绝对路径或备用数据流路径。");
        foreach (var segment in normalized.Split(Path.DirectorySeparatorChar))
        {
            var baseName = segment.Split('.')[0];
            if (segment is "" or "." or ".." || segment.EndsWith(' ') || segment.EndsWith('.')
                || segment.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0
                || Regex.IsMatch(baseName, @"^(?:CON|PRN|AUX|NUL|COM[1-9¹²³]|LPT[1-9¹²³])$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant))
                throw new ArchiveException(ArchiveFailure.UnsafePath, "压缩包包含越界、设备名称或无效路径。");
        }
        return normalized;
    }

    public static void EnsureNoReparsePoint(string path)
    {
        var current = Path.GetFullPath(path);
        while (!string.IsNullOrEmpty(current))
        {
            try
            {
                if ((File.GetAttributes(current) & FileAttributes.ReparsePoint) != 0)
                    throw new ArchiveException(ArchiveFailure.UnsafePath, "目标或来源路径包含文件系统链接，已拒绝操作。");
            }
            catch (FileNotFoundException) { }
            catch (DirectoryNotFoundException) { }
            current = Path.GetDirectoryName(current);
        }
    }

    private static IReadOnlyList<string> ResolveApprovedVolumes(string path, IReadOnlyList<string>? allowed)
    {
        if (allowed is null) return ArchiveScanner.ResolveSource(path).Volumes;
        if (allowed.Count == 0 || allowed.Count > 10_000)
            throw new ArchiveException(ArchiveFailure.UnsafePath, "没有批准压缩包输入文件。");
        var approved = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var directory = Path.GetDirectoryName(path);
        foreach (var input in allowed)
        {
            if (!Path.IsPathFullyQualified(input))
                throw new ArchiveException(ArchiveFailure.UnsafePath, "分卷必须使用绝对路径。");
            var full = Path.GetFullPath(input);
            if (!Path.GetDirectoryName(full)!.Equals(directory, StringComparison.OrdinalIgnoreCase) || !approved.Add(full))
                throw new ArchiveException(ArchiveFailure.UnsafePath, "分卷必须属于同一已批准的目录。");
            EnsureNoReparsePoint(full);
            if (!File.Exists(full)) throw new ArchiveException(ArchiveFailure.MissingVolume, "已批准分卷不存在。");
        }
        if (!approved.Contains(path)) throw new ArchiveException(ArchiveFailure.UnsafePath, "起始卷没有获准读取。");
        return approved.ToArray();
    }

    private static IReadOnlyList<string> NumericJoinedParts(string path, IReadOnlyList<string> approved)
    {
        var name = Path.GetFileName(path);
        var match = Regex.Match(name, @"^(.+\.(?:7z|zip))\.(\d{3,})$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
        if (!match.Success) return [path];
        var pattern = new Regex("^" + Regex.Escape(match.Groups[1].Value) + @"\.(\d{3,})$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
        var parts = new SortedDictionary<int, string>();
        foreach (var approvedPath in approved)
        {
            var part = pattern.Match(Path.GetFileName(approvedPath));
            if (!part.Success || !int.TryParse(part.Groups[1].Value, out var index) || !parts.TryAdd(index, approvedPath))
                throw new ArchiveException(ArchiveFailure.UnsafePath, "已批准输入包含不属于此数字分卷组的文件。");
        }
        var next = 1;
        foreach (var index in parts.Keys)
            if (index != next++) throw new ArchiveException(ArchiveFailure.MissingVolume, "数字分卷编号不连续。");
        return parts.Values.ToArray();
    }

    public static bool MatchesExclusion(string path, IReadOnlyList<string>? patterns)
    {
        if (patterns is null) return false;
        var relative = path.Replace('\\', '/');
        var fileName = Path.GetFileName(path);
        foreach (var raw in patterns)
        {
            if (string.IsNullOrWhiteSpace(raw) || raw.Length > 512) continue;
            var pattern = raw.Replace('\\', '/');
            var value = pattern.Contains('/') ? relative : fileName;
            if (Wildcard(value.AsSpan(), pattern.AsSpan())) return true;
        }
        return false;
    }

    private static bool Wildcard(ReadOnlySpan<char> value, ReadOnlySpan<char> pattern)
    {
        var v = 0; var p = 0; var star = -1; var restart = 0;
        while (v < value.Length)
        {
            if (p < pattern.Length && (pattern[p] == '?' || char.ToUpperInvariant(value[v]) == char.ToUpperInvariant(pattern[p]))) { v++; p++; }
            else if (p < pattern.Length && pattern[p] == '*') { star = p++; restart = v; }
            else if (star >= 0) { p = star + 1; v = ++restart; }
            else return false;
        }
        while (p < pattern.Length && pattern[p] == '*') p++;
        return p == pattern.Length;
    }

    public static void EnsureFreeSpace(string destination, long minFreeBytes)
    {
        if (minFreeBytes == 0) return;
        var root = Path.GetPathRoot(Path.GetFullPath(destination));
        if (string.IsNullOrEmpty(root) || new DriveInfo(root).AvailableFreeSpace < minFreeBytes)
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "可用空间低于保留空间要求。");
    }

    private static string? GetText(IInArchive archive, uint index, uint property) => WithProperty(archive, index, property, p => p.Text);
    private static bool GetBool(IInArchive archive, uint index, uint property) => WithProperty(archive, index, property, p => p.Bool);
    private static ulong GetNumber(IInArchive archive, uint index, uint property) => WithProperty(archive, index, property, p => p.Number);
    private static T WithProperty<T>(IInArchive archive, uint index, uint property, Func<PropVariant, T> read)
    {
        var result = archive.GetProperty(index, property, out var value);
        try
        {
            if (result != 0) throw new ArchiveException(ArchiveFailure.Corrupt, "压缩包条目属性无法读取。");
            return read(value);
        }
        finally { value.Clear(); }
    }

    private static void ValidateResult(OpenedArchive archive, ExtractCallback callback, int result, CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        if (result == unchecked((int)0x8007000E))
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "解压引擎可用内存不足。");
        if (callback.Error is not null) throw callback.Error;
        if (archive.Callback.Error is not null) throw archive.Callback.Error;
        if (archive.Callback.MissingVolume)
            throw new ArchiveException(ArchiveFailure.MissingVolume, "压缩包缺少分卷，请检查完整性。");
        if (callback.OperationResult is 1)
            throw new ArchiveException(ArchiveFailure.Unsupported, "压缩包使用了暂不支持的压缩方法。");
        if (callback.OperationResult is 9 || result == NativeResult.Abort && archive.Callback.PasswordRequested && archive.Callback.Password is null)
            throw new ArchiveException(ArchiveFailure.WrongPassword, "需要有效的压缩包密码。");
        if (callback.OperationResult is 2 or 3 && archive.Callback.PasswordRequested)
            throw new ArchiveException(ArchiveFailure.WrongPassword, "加密内容校验失败：密码不匹配或文件损坏。");
        if (result != 0 || callback.OperationResult != 0)
            throw new ArchiveException(ArchiveFailure.Corrupt, "压缩包完整性校验失败，文件可能损坏或分卷不完整。");
    }

    private static void ReleaseArchive(IInArchive archive)
    {
        try { archive.Close(); }
        finally { if (Marshal.IsComObject(archive)) Marshal.FinalReleaseComObject(archive); }
    }

    private sealed class OpenedArchive(IInArchive archive, InputStream stream, OpenCallback callback) : IDisposable
    {
        public IInArchive Archive { get; } = archive;
        public OpenCallback Callback { get; } = callback;
        public void Dispose() { try { ReleaseArchive(Archive); } finally { stream.Dispose(); Callback.Dispose(); } }
    }

    private sealed class Library
    {
        private readonly IntPtr _handle;
        private readonly CreateObject _create;
        [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int CreateObject(ref Guid classId, ref Guid interfaceId, out IntPtr archive);
        public Library(string path)
        {
            if (!Environment.Is64BitProcess) throw new ArchiveException(ArchiveFailure.Unsupported, "解压引擎需要 64 位进程。");
            if (!File.Exists(path)) throw new ArchiveException(ArchiveFailure.Unsupported, "未找到官方 7-Zip 引擎。请运行 scripts/setup-engine.ps1。");
            var hashPath = path + ".sha256";
            if (File.Exists(hashPath))
            {
                var expected = File.ReadAllText(hashPath).Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries)[0];
                using var stream = File.OpenRead(path);
                var actual = Convert.ToHexString(SHA256.HashData(stream));
                if (!actual.Equals(expected, StringComparison.OrdinalIgnoreCase)) throw new ArchiveException(ArchiveFailure.Unsupported, "7-Zip 引擎校验失败，请重新运行 setup-engine.ps1。");
            }
            try
            {
                _handle = NativeLibrary.Load(path);
                _create = Marshal.GetDelegateForFunctionPointer<CreateObject>(NativeLibrary.GetExport(_handle, "CreateObject"));
            }
            catch (Exception ex) when (ex is DllNotFoundException or BadImageFormatException or EntryPointNotFoundException)
            { throw new ArchiveException(ArchiveFailure.Unsupported, "无法加载官方 7-Zip 引擎，请检查系统架构和依赖。", ex); }
            // Keep the module loaded for process lifetime: COM wrappers can outlive a managed operation.
        }
        public IInArchive Create(byte format)
        {
            var classId = new Guid($"23170F69-40C1-278A-1000-000110{format:X2}0000");
            var interfaceId = typeof(IInArchive).GUID;
            var result = _create(ref classId, ref interfaceId, out var pointer);
            if (result != 0 || pointer == IntPtr.Zero) throw new ArchiveException(ArchiveFailure.Unsupported, "当前引擎没有此压缩格式的处理器。");
            try { return (IInArchive)Marshal.GetObjectForIUnknown(pointer); }
            finally { Marshal.Release(pointer); }
        }
    }

    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    private sealed class OpenCallback(string archivePath, string? password, CancellationToken token, IReadOnlyList<string> approvedVolumes) : IArchiveOpenCallback, IArchiveOpenVolumeCallback, ICryptoGetTextPassword, IDisposable
    {
        private readonly List<InputStream> _volumes = [];
        public string ArchivePath { get; } = archivePath;
        public string? Password { get; } = password;
        public bool PasswordRequested { get; private set; }
        public bool MissingVolume { get; private set; }
        public ArchiveException? Error { get; set; }
        public int SetTotal(IntPtr files, IntPtr bytes) => token.IsCancellationRequested ? NativeResult.Abort : 0;
        public int SetCompleted(IntPtr files, IntPtr bytes) => token.IsCancellationRequested ? NativeResult.Abort : 0;
        public int CryptoGetTextPassword(out string? value)
        {
            PasswordRequested = true;
            value = Password;
            return token.IsCancellationRequested || Password is null ? NativeResult.Abort : 0;
        }
        public int GetProperty(uint propertyId, out PropVariant value)
        {
            value = propertyId switch
            {
                3 or 4 => PropVariant.FromText(Path.GetFileName(ArchivePath)),
                7 => PropVariant.FromNumber((ulong)new FileInfo(ArchivePath).Length),
                _ => default
            };
            return token.IsCancellationRequested ? NativeResult.Abort : 0;
        }
        public int GetStream(string name, out IInStream? stream)
        {
            stream = null;
            try
            {
                token.ThrowIfCancellationRequested();
                if (Path.GetFileName(name) != name || Path.IsPathRooted(name) || name.Contains(':'))
                    throw new ArchiveException(ArchiveFailure.UnsafePath, "分卷名称指向了文件夹之外的路径。");
                var candidate = Path.Combine(Path.GetDirectoryName(ArchivePath)!, name);
                if (!File.Exists(candidate)) { MissingVolume = true; return NativeResult.False; }
                if (!approvedVolumes.Contains(candidate, StringComparer.OrdinalIgnoreCase))
                    throw new ArchiveException(ArchiveFailure.UnsafePath, "压缩包要求读取不属于此分卷组的文件。");
                EnsureNoReparsePoint(candidate);
                var input = new InputStream(File.OpenRead(candidate), token, this);
                _volumes.Add(input);
                stream = input;
                return 0;
            }
            catch (Exception ex)
            {
                if (ex is not OperationCanceledException) Error = ex as ArchiveException ?? new(ArchiveFailure.IoError, "分卷文件无法读取。");
                return NativeResult.FromException(ex);
            }
        }
        public void Dispose() { foreach (var volume in _volumes) volume.Dispose(); }
    }

    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    private sealed class InputStream(Stream input, CancellationToken token, OpenCallback callback) : IInStream, IDisposable
    {
        public int Read(IntPtr data, uint size, IntPtr processedSize)
        {
            if (processedSize != IntPtr.Zero) Marshal.WriteInt32(processedSize, 0);
            try
            {
                token.ThrowIfCancellationRequested();
                var buffer = new byte[(int)Math.Min(size, 1024 * 1024)];
                var count = input.Read(buffer, 0, buffer.Length);
                Marshal.Copy(buffer, 0, data, count);
                if (processedSize != IntPtr.Zero) Marshal.WriteInt32(processedSize, count);
                return 0;
            }
            catch (Exception ex)
            {
                if (ex is not OperationCanceledException) callback.Error = ex as ArchiveException ?? new(ArchiveFailure.IoError, "压缩包读取失败。");
                return NativeResult.FromException(ex);
            }
        }
        public int Seek(long offset, uint seekOrigin, IntPtr newPosition)
        {
            try
            {
                token.ThrowIfCancellationRequested();
                if (seekOrigin > 2) return NativeResult.Fail;
                var position = input.Seek(offset, (SeekOrigin)seekOrigin);
                if (newPosition != IntPtr.Zero) Marshal.WriteInt64(newPosition, position);
                return 0;
            }
            catch (Exception ex) { return NativeResult.FromException(ex); }
        }
        public void Dispose() => input.Dispose();
    }

    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    private sealed class ExtractCallback(OpenCallback open, IReadOnlyList<ArchiveEntryInfo> entries, string? destination, long maxBytes,
        IProgress<ExtractionProgress>? progress, CancellationToken token, long minFreeBytes = 0,
        IReadOnlyList<string>? exclusions = null) : IArchiveExtractCallback, ICryptoGetTextPassword, IDisposable
    {
        private OutputStream? _current;
        private ArchiveEntryInfo? _pendingDirectory;
        private readonly HashSet<string> _excludedDirectories = entries.Where(e => e.IsDirectory && MatchesExclusion(e.Path, exclusions)).Select(e => e.Path).ToHashSet(StringComparer.OrdinalIgnoreCase);
        public List<ExtractedFile> Files { get; } = [];
        public int ExcludedEntries { get; private set; }
        public ArchiveException? Error { get; set; }
        public int OperationResult { get; private set; }
        public long WrittenBytes { get; set; }
        public int CompletedFiles { get; private set; }
        public int SetTotal(ulong total) => token.IsCancellationRequested ? NativeResult.Abort : 0;
        public int SetCompleted(IntPtr value)
        {
            if (token.IsCancellationRequested) return NativeResult.Abort;
            progress?.Report(new(WrittenBytes, entries.Sum(e => e.Size), CompletedFiles, entries.Count));
            return 0;
        }
        public int CryptoGetTextPassword(out string? password) => open.CryptoGetTextPassword(out password);
        public int GetStream(uint index, out ISequentialOutStream? stream, int askExtractMode)
        {
            stream = null;
            try
            {
                token.ThrowIfCancellationRequested();
                _current?.Dispose(); _current = null;
                _pendingDirectory = null;
                if (destination is null || askExtractMode != 0) return 0;
                if (index >= entries.Count) throw new ArchiveException(ArchiveFailure.Corrupt, "压缩包返回了无效条目编号。");
                var entry = entries[(int)index];
                if (entry.IsDirectory && entry.Path.Length == 0) return 0;
                if (MatchesExclusion(entry.Path, exclusions) || _excludedDirectories.Any(d => entry.Path.StartsWith(d + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase)))
                { ExcludedEntries++; return 0; }
                var outputPath = Path.GetFullPath(Path.Combine(destination, entry.Path));
                if (!outputPath.StartsWith(destination.TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
                    throw new ArchiveException(ArchiveFailure.UnsafePath, "解压路径超出了目标文件夹。");
                EnsureNoReparsePoint(outputPath);
                if (entry.IsDirectory) { Directory.CreateDirectory(outputPath); _pendingDirectory = entry; return 0; }
                Directory.CreateDirectory(Path.GetDirectoryName(outputPath)!);
                EnsureNoReparsePoint(outputPath);
                EnsureFreeSpace(outputPath, minFreeBytes);
                _current = new OutputStream(new FileStream(outputPath, FileMode.CreateNew, FileAccess.Write, FileShare.None), this,
                    entry.Path, outputPath, maxBytes, minFreeBytes, token);
                stream = _current;
                return 0;
            }
            catch (Exception ex)
            {
                if (ex is not OperationCanceledException) Error = ex as ArchiveException ?? new(ArchiveFailure.IoError, "目标文件无法创建，请检查空间与访问权限。");
                return NativeResult.FromException(ex);
            }
        }
        public int PrepareOperation(int askExtractMode) => token.IsCancellationRequested ? NativeResult.Abort : 0;
        public int SetOperationResult(int result)
        {
            try
            {
                _current?.Dispose(); _current = null;
                if (_pendingDirectory is not null) Files.Add(new(_pendingDirectory.Path, 0, "", true));
                _pendingDirectory = null;
                if (result != 0 && OperationResult == 0) OperationResult = result;
                CompletedFiles++;
                return token.IsCancellationRequested ? NativeResult.Abort : 0;
            }
            catch (Exception ex)
            {
                Error = ex as ArchiveException ?? new(ArchiveFailure.IoError, "完成解压文件写入失败。");
                return NativeResult.FromException(ex);
            }
        }
        public void Dispose() { _current?.Dispose(); _current = null; }
    }

    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    private sealed class OutputStream(FileStream output, ExtractCallback callback, string relativePath, string outputPath,
        long maxBytes, long minFreeBytes, CancellationToken token) : ISequentialOutStream, IDisposable
    {
        private readonly IncrementalHash _hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        private long _size;
        private bool _disposed;
        public int Write(IntPtr data, uint size, IntPtr processedSize)
        {
            if (processedSize != IntPtr.Zero) Marshal.WriteInt32(processedSize, 0);
            try
            {
                token.ThrowIfCancellationRequested();
                if (size > int.MaxValue || callback.WrittenBytes > maxBytes - size)
                    throw new ArchiveException(ArchiveFailure.ResourceLimit, "实际解压数据超过设置的总大小上限。");
                EnsureFreeSpace(outputPath, minFreeBytes);
                var buffer = ArrayPool<byte>.Shared.Rent((int)Math.Min(size, 1024 * 1024));
                try
                {
                    var offset = 0;
                    while (offset < size)
                    {
                        token.ThrowIfCancellationRequested();
                        var count = (int)Math.Min(buffer.Length, size - offset);
                        Marshal.Copy(IntPtr.Add(data, offset), buffer, 0, count);
                        output.Write(buffer.AsSpan(0, count));
                        _hash.AppendData(buffer.AsSpan(0, count));
                        _size += count;
                        callback.WrittenBytes += count;
                        offset += count;
                    }
                }
                finally { ArrayPool<byte>.Shared.Return(buffer, true); }
                if (processedSize != IntPtr.Zero) Marshal.WriteInt32(processedSize, (int)size);
                return 0;
            }
            catch (Exception ex)
            {
                if (ex is not OperationCanceledException) callback.Error = ex as ArchiveException ?? new(ArchiveFailure.IoError, "写入解压数据失败，请检查磁盘空间与权限。");
                return NativeResult.FromException(ex);
            }
        }
        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            try
            {
                // Flush is an OS-level delivery point, not a promise of power-loss durability.
                output.Flush();
                callback.Files.Add(new(relativePath, _size, Convert.ToHexStringLower(_hash.GetHashAndReset()), false));
            }
            finally { output.Dispose(); _hash.Dispose(); }
        }
    }

    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    private sealed class NullOutStream : ISequentialOutStream
    {
        public static readonly NullOutStream Instance = new();
        public int Write(IntPtr data, uint size, IntPtr processedSize)
        {
            if (processedSize != IntPtr.Zero) Marshal.WriteInt32(processedSize, (int)size);
            return 0;
        }
    }

    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    private sealed class ProbeExtractCallback(string? password, CancellationToken token) : IArchiveExtractCallback, ICryptoGetTextPassword
    {
        public bool PasswordRequested { get; private set; }
        public int OperationResult { get; private set; }
        public int SetTotal(ulong total) => token.IsCancellationRequested ? NativeResult.Abort : 0;
        public int SetCompleted(IntPtr completeValue) => token.IsCancellationRequested ? NativeResult.Abort : 0;
        public int CryptoGetTextPassword(out string? pwd)
        {
            PasswordRequested = true;
            pwd = password;
            return token.IsCancellationRequested || password is null ? NativeResult.Abort : 0;
        }
        public int GetStream(uint index, out ISequentialOutStream? stream, int askExtractMode)
        {
            stream = NullOutStream.Instance;
            return token.IsCancellationRequested ? NativeResult.Abort : 0;
        }
        public int PrepareOperation(int askExtractMode) => token.IsCancellationRequested ? NativeResult.Abort : 0;
        public int SetOperationResult(int result)
        {
            OperationResult = result;
            return token.IsCancellationRequested ? NativeResult.Abort : 0;
        }
    }

    private sealed class JoinedFileStream : Stream
    {
        private readonly IReadOnlyList<string> _paths;
        private readonly long[] _offsets;
        private FileStream? _current;
        private int _currentIndex = -1;
        private long _position;
        public JoinedFileStream(IReadOnlyList<string> paths)
        {
            _paths = paths;
            _offsets = new long[paths.Count + 1];
            for (var i = 0; i < paths.Count; i++)
            {
                EnsureNoReparsePoint(paths[i]);
                _offsets[i + 1] = checked(_offsets[i] + new FileInfo(paths[i]).Length);
            }
        }
        public override bool CanRead => true;
        public override bool CanSeek => true;
        public override bool CanWrite => false;
        public override long Length => _offsets[^1];
        public override long Position { get => _position; set => Seek(value, SeekOrigin.Begin); }
        public override int Read(byte[] buffer, int offset, int count)
        {
            if (_position >= Length || count == 0) return 0;
            var index = 0;
            while (index + 1 < _offsets.Length && _offsets[index + 1] <= _position) index++;
            if (_currentIndex != index) { _current?.Dispose(); _current = File.OpenRead(_paths[index]); _currentIndex = index; }
            _current!.Position = _position - _offsets[index];
            var actual = _current.Read(buffer, offset, (int)Math.Min(count, _offsets[index + 1] - _position));
            if (actual == 0 && _position < _offsets[index + 1]) throw new ArchiveException(ArchiveFailure.MissingVolume, "分卷在读取过程中发生变化或被截断。");
            _position += actual;
            return actual;
        }
        public override long Seek(long offset, SeekOrigin origin)
        {
            var position = origin switch { SeekOrigin.Begin => offset, SeekOrigin.Current => checked(_position + offset), SeekOrigin.End => checked(Length + offset), _ => throw new IOException() };
            if (position < 0) throw new IOException("Negative archive seek.");
            return _position = position;
        }
        public override void Flush() { }
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
        protected override void Dispose(bool disposing) { if (disposing) _current?.Dispose(); base.Dispose(disposing); }
    }
}
