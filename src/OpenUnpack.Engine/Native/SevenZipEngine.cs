using System.Runtime.InteropServices;
using System.Buffers;
using System.Security.Cryptography;
using System.Text.RegularExpressions;
using System.Text;
using OpenUnpack.Engine;

namespace OpenUnpack.Engine.Native;

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
        InspectAsync(archivePath, password, cancellationToken, null);

    public Task<ArchiveInspection> InspectAsync(string archivePath, string? password, CancellationToken cancellationToken, IReadOnlyList<string>? allowedVolumes) =>
        InspectAsync(archivePath, password, cancellationToken, allowedVolumes, 100_000, 100L * 1024 * 1024 * 1024);

    public Task<ArchiveInspection> InspectAsync(string archivePath, string? password, CancellationToken cancellationToken,
        IReadOnlyList<string>? allowedVolumes, int maxEntries, long maxTotalBytes) =>
        Task.Run(() => Inspect(archivePath, password, cancellationToken, allowedVolumes, maxEntries, maxTotalBytes), cancellationToken);

    public Task ExtractAsync(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken cancellationToken) =>
        Task.Run(() => Extract(request, progress, cancellationToken), cancellationToken);

    public Task<PasswordProbeResult> ProbeAsync(
        string archivePath,
        IReadOnlyList<string> candidatePasswords,
        CancellationToken cancellationToken,
        IReadOnlyList<string>? allowedVolumes = null,
        int maxEntries = 100_000,
        long maxTotalBytes = 100L * 1024 * 1024 * 1024,
        long maxWrittenBytes = long.MaxValue) =>
        Task.Run(() => Probe(archivePath, candidatePasswords, cancellationToken, allowedVolumes, maxEntries, maxTotalBytes, maxWrittenBytes), cancellationToken);

    // Worker preflight reads headers and entry properties; it never decodes every file a second time.
    public Task<ArchiveInspection> PreflightAsync(ExtractionRequest request, CancellationToken cancellationToken) =>
        Task.Run(() =>
        {
            using var opened = Open(request.ArchivePath, request.Password, cancellationToken, request.AllowedVolumes);
            ValidateArchiveStatus(opened);
            return new ArchiveInspection(ReadEntries(opened, request.MaxEntries, request.MaxTotalBytes, cancellationToken), opened.Callback.PasswordRequested);
        }, cancellationToken);

    public Task<ExtractionManifest> ExtractWithManifestAsync(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken cancellationToken) =>
        Task.Run(() => Extract(request, progress, cancellationToken), cancellationToken);

    private ArchiveInspection Inspect(string path, string? password, CancellationToken token,
        IReadOnlyList<string>? allowedVolumes, int maxEntries, long maxTotalBytes)
    {
        if (maxEntries <= 0 || maxEntries > 1_000_000 || maxTotalBytes < 0)
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "目录条目数量或大小上限无效。");
        OpenedArchive opened;
        try
        {
            opened = Open(path, password, token, allowedVolumes);
        }
        catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.WrongPassword)
        {
            throw new ArchiveException(ArchiveFailure.WrongPassword, "压缩包文件名已加密，需要有效密码以查看目录。", ex);
        }

        using (opened)
        {
            ValidateArchiveStatus(opened);
            var entries = ReadEntries(opened, maxEntries, maxTotalBytes, token);
            return new ArchiveInspection(entries, opened.Callback.PasswordRequested);
        }
    }

    private ExtractionManifest Extract(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken token)
    {
        if (request.MaxEntries is < 1 or > 1_000_000 || request.MaxTotalBytes < 0 || request.MaxWrittenBytes < 0 || request.MinFreeBytes < 0)
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "解压数量或大小上限无效。");
        using var opened = Open(request.ArchivePath, request.Password, token, request.AllowedVolumes);
        var archiveDamaged = ValidateArchiveStatus(opened, request.SalvageCorrupted);
        ApplyEngineThreads(opened.Archive, request.EngineThreads);
        var entries = ReadEntries(opened, request.MaxEntries, request.MaxTotalBytes, token);
        var destination = Path.GetFullPath(request.DestinationDirectory);
        var sourceZoneId = request.SourceZoneId ?? ZoneIdentifier.DetectZone(request.ArchivePath);
        using var callback = new ExtractCallback(opened.Callback, entries, destination,
            Math.Min(request.MaxTotalBytes, request.MaxWrittenBytes), progress, token, request.MinFreeBytes, request.Exclusions, request.SalvageCorrupted, sourceZoneId, request.Inclusions);
        token.ThrowIfCancellationRequested();
        WriteScopeGuard.DemandWriteAccess(destination);
        callback.EnsureDestination();
        var result = opened.Archive.Extract(IntPtr.Zero, uint.MaxValue, 0, callback);
        var salvaged = ValidateResult(opened, callback, result, token, request.SalvageCorrupted, archiveDamaged);
        progress?.Report(new(callback.WrittenBytes, callback.TotalBytesKnown ? callback.TotalBytes : callback.WrittenBytes,
            callback.CompletedFiles, entries.Count));
        return new(callback.Files.ToArray(), opened.Callback.PasswordRequested, callback.WrittenBytes, callback.ExcludedEntries,
            salvaged ? "salvagedWithCrcWarning" : "archiveCrcAndStreamSha256");
    }

    public PasswordProbeResult Probe(
        string path,
        IReadOnlyList<string> candidatePasswords,
        CancellationToken token,
        IReadOnlyList<string>? allowedVolumes = null,
        int maxEntries = 100_000,
        long maxTotalBytes = 100L * 1024 * 1024 * 1024,
        long maxWrittenBytes = long.MaxValue)
    {
        token.ThrowIfCancellationRequested();
        if (maxEntries is < 1 or > 1_000_000 || maxTotalBytes <= 0 || maxWrittenBytes <= 0)
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "密码探测资源配额限制无效。");

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
            return new(PasswordProbeStatus.Corrupt, null, ex.Message);
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
                    try { ValidateArchiveStatus(candidateArchive); }
                    catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.Corrupt)
                    {
                        return new(PasswordProbeStatus.Corrupt, null, ex.Message);
                    }
                    ReadEntries(candidateArchive, maxEntries, maxTotalBytes, token);
                    return new(PasswordProbeStatus.Success, candidate);
                }
                catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.ResourceLimit)
                {
                    throw;
                }
                catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.MissingVolume)
                {
                    return new(PasswordProbeStatus.MissingVolume, null, ex.Message);
                }
                catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.Unsupported)
                {
                    return new(PasswordProbeStatus.Unsupported, null, ex.Message);
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
                ValidateArchiveStatus(opened);
                if (opened.Archive.GetNumberOfItems(out var count) != 0)
                    return new(PasswordProbeStatus.Corrupt, null, "无法读取压缩包目录。");
                if (maxEntries > 0 && count > (uint)maxEntries)
                    throw new ArchiveException(ArchiveFailure.ResourceLimit, "压缩包条目数量超出探测限制。");
                if (count == 0)
                    return new(PasswordProbeStatus.NoPasswordNeeded, null);

                int targetIndex = -1;
                bool hasExplicitEncrypted = false;
                bool hasUnknownEncryption = false;
                long declaredTotalBytes = 0;
                ulong smallestEncryptedSize = ulong.MaxValue;
                for (uint i = 0; i < count; i++)
                {
                    token.ThrowIfCancellationRequested();
                    var isDir = GetBool(opened.Archive, i, ArchiveProperty.IsDirectory);
                    if (isDir) continue;
                    var itemSize = GetSize(opened.Archive, i).Size;
                    if (itemSize > long.MaxValue || (maxTotalBytes > 0 && declaredTotalBytes > maxTotalBytes - (long)itemSize))
                        throw new ArchiveException(ArchiveFailure.ResourceLimit, "压缩包条目总大小超出探测资源预算。");
                    declaredTotalBytes += (long)itemSize;

                    var isEncrypted = WithProperty<bool?>(opened.Archive, i, ArchiveProperty.Encrypted,
                        value => value.Type == (ushort)VarEnum.VT_BOOL ? value.Bool : null);
                    hasUnknownEncryption |= isEncrypted is null;
                    if (isEncrypted == true)
                    {
                        hasExplicitEncrypted = true;
                        if (targetIndex < 0 || itemSize < smallestEncryptedSize)
                        { targetIndex = (int)i; smallestEncryptedSize = itemSize; }
                    }
                }

                // Reliable metadata answers whether a password is needed; integrity is
                // verified by the final extraction. Unknown metadata stays inconclusive.
                if (!hasExplicitEncrypted)
                    return new(hasUnknownEncryption ? PasswordProbeStatus.Failed : PasswordProbeStatus.NoPasswordNeeded,
                        null, hasUnknownEncryption ? "无法可靠判断全部条目的加密状态。" : null);

                if (targetIndex < 0)
                {
                    return new(PasswordProbeStatus.NoPasswordNeeded, null);
                }

                var targetSize = GetNumber(opened.Archive, (uint)targetIndex, ArchiveProperty.Size);
                if ((maxTotalBytes > 0 && targetSize > (ulong)maxTotalBytes) || (maxWrittenBytes > 0 && targetSize > (ulong)maxWrittenBytes))
                {
                    throw new ArchiveException(ArchiveFailure.ResourceLimit, "探测目标文件大小超出资源预算。");
                }
                long effectiveProbeLimit = Math.Min(maxTotalBytes > 0 ? maxTotalBytes : long.MaxValue, maxWrittenBytes > 0 ? maxWrittenBytes : long.MaxValue);



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
                            probeCallback = new ProbeExtractCallback(candidate, effectiveProbeLimit, token);
                            // testMode=1 does not call output Write in the SDK. Extract to
                            // a bounded discard stream instead, so actual output is counted.
                            extractResult = currentArchive.Archive.Extract(probeHandle.AddrOfPinnedObject(), 1, 0, probeCallback);
                        }
                        catch (OperationCanceledException) { throw; }
                        catch (Exception ex)
                        {
                            SafeDiagnostics.Emit("probe", "decode", "IoError", ex);
                            extractResult = NativeResult.Fail;
                            probeCallback = new ProbeExtractCallback(candidate, effectiveProbeLimit, token);
                        }
                        finally
                        {
                            probeHandle.Free();
                        }

                        token.ThrowIfCancellationRequested();
                        if (probeCallback.LimitExceeded)
                        {
                            SafeDiagnostics.Emit("probe", "decode", "ResourceLimit", nativeCode: extractResult);
                            throw new ArchiveException(ArchiveFailure.ResourceLimit, "密码探测解压工作量超出资源限制。");
                        }
                        if (currentArchive.Callback.MissingVolume || (currentArchive.Callback.VolumeNotFound && (extractResult != 0 || probeCallback.OperationResult != 0)))
                        {
                            return new(PasswordProbeStatus.MissingVolume, null, "压缩包缺少分卷，请检查完整性。");
                        }
                        if (probeCallback.OperationResult == 1)
                        {
                            SafeDiagnostics.Emit("probe", "decode", "Unsupported", nativeCode: 1);
                            return new(PasswordProbeStatus.Unsupported, null, "不支持的压缩方法或加密算法。");
                        }
                        if (extractResult == 0 && probeCallback.OperationResult == 0)
                        {
                            ValidateArchiveStatus(currentArchive);
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
        catch (ArchiveException ex) when (ex.Failure is ArchiveFailure.ResourceLimit or ArchiveFailure.UnsafePath)
        {
            throw;
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
        PinnedSourceFiles? sources = new(approved);
        try
        {
            var formats = ArchiveFormats.ResolveWithOffset(path, out var sfxPayloadOffset);
            foreach (var format in formats)
            {
                token.ThrowIfCancellationRequested();
                var archive = _library.Value.Create(format);
                OpenCallback? callback = null;
                InputStream? stream = null;
                int result;
                try
                {
                    callback = new OpenCallback(path, password, token, approved, sources);
                    stream = new InputStream(new JoinedFileStream(parts, sources, sfxPayloadOffset), token, callback);
                    result = archive.Open(stream, IntPtr.Zero, callback);
                }
                catch
                {
                    try { ReleaseArchive(archive); }
                    finally { try { stream?.Dispose(); } finally { callback?.Dispose(); } }
                    throw;
                }
                if (result == NativeResult.Ok)
                {
                    callback.MarkOpenSucceeded();
                    var opened = new OpenedArchive(archive, stream, callback, sources);
                    sources = null; // Ownership passes to the opened archive until codecs stop reading.
                    return opened;
                }
                try { ReleaseArchive(archive); }
                finally { try { stream.Dispose(); } finally { callback.Dispose(); } }
                token.ThrowIfCancellationRequested();
                if (callback.Error is not null) throw callback.Error;
                if (callback.PasswordRequested)
                    throw new ArchiveException(ArchiveFailure.WrongPassword, "需要有效密码，或加密头部已损坏。");
                if (callback.MissingVolume)
                    throw new ArchiveException(ArchiveFailure.MissingVolume, "压缩包缺少分卷，请将所有分卷放在同一文件夹。");
            }
            throw new ArchiveException(ArchiveFailure.Corrupt, "无法打开压缩包：文件可能损坏、分卷不完整或扩展名与格式不符。");
        }
        finally { sources?.Dispose(); }
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
            var name = GetText(opened.Archive, i, ArchiveProperty.Path) ?? GetText(opened.Archive, i, ArchiveProperty.Name);
            if (string.IsNullOrEmpty(name)) name = Path.GetFileNameWithoutExtension(opened.Callback.ArchivePath);
            var directory = GetBool(opened.Archive, i, ArchiveProperty.IsDirectory);
            var (size, sizeKnown) = directory ? (0UL, true) : GetSize(opened.Archive, i);
            var attributes = GetNumber(opened.Archive, i, ArchiveProperty.Attributes);
            var posix = GetNumber(opened.Archive, i, ArchiveProperty.PosixAttributes);
            var encrypted = !directory && (GetBool(opened.Archive, i, ArchiveProperty.Encrypted) || opened.Callback.PasswordRequested);
            var link = !string.IsNullOrEmpty(GetText(opened.Archive, i, ArchiveProperty.SymbolicLink))
                || !string.IsNullOrEmpty(GetText(opened.Archive, i, ArchiveProperty.HardLink))
                || GetBool(opened.Archive, i, ArchiveProperty.IsAlternateStream) || GetBool(opened.Archive, i, ArchiveProperty.IsAnti)
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
            entries.Add(new(safePath, directory ? 0 : (long)size, directory, false, encrypted, sizeKnown));
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
        int prefixLength = 0;
        while (prefixLength + 1 < normalized.Length && normalized[prefixLength] == '.'
            && normalized[prefixLength + 1] == Path.DirectorySeparatorChar) prefixLength += 2;
        if (prefixLength > 0) normalized = normalized[prefixLength..];
        if (string.IsNullOrEmpty(normalized) || Path.IsPathRooted(normalized) || normalized.Contains(':') || normalized.Contains('\0'))
            throw new ArchiveException(ArchiveFailure.UnsafePath, "压缩包包含不安全的绝对路径或备用数据流路径。");
        int depth = 1;
        foreach (var character in normalized)
            if (character == Path.DirectorySeparatorChar && ++depth > 128)
                throw new ArchiveException(ArchiveFailure.ResourceLimit, "输出路径层级超过安全上限。");
        foreach (var segment in normalized.Split(Path.DirectorySeparatorChar))
        {
            // Windows also treats spaces before an extension as device aliases.
            var baseName = segment.Split('.')[0].TrimEnd(' ', '.');
            if (segment is "" or "." or ".." || segment.EndsWith(' ') || segment.EndsWith('.')
                || segment.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0
                || segment.Equals(".openunpack-owner.json", StringComparison.OrdinalIgnoreCase)
                || Regex.IsMatch(baseName, @"^(?:CON|PRN|AUX|NUL|CLOCK\$|CONIN\$|CONOUT\$|COM[1-9¹²³]|LPT[1-9¹²³])$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant))
                throw new ArchiveException(ArchiveFailure.UnsafePath, "压缩包包含越界、设备名称或无效路径。");
        }
        return normalized;
    }

    public static void EnsureNoReparsePoint(string path, ISet<string>? verifiedAncestors = null)
    {
        var current = Path.GetFullPath(path);
        var isFirst = true;
        List<string>? visitedDirs = null;
        while (!string.IsNullOrEmpty(current))
        {
            if (!isFirst && verifiedAncestors is not null && verifiedAncestors.Contains(current))
            {
                break;
            }
            var exists = false;
            if (OperatingSystem.IsWindows())
            {
                var attr = GetFileAttributes(current);
                if (attr != InvalidFileAttributes)
                {
                    if ((attr & FileAttributeReparsePoint) != 0)
                        throw new ArchiveException(ArchiveFailure.UnsafePath, "目标或来源路径包含文件系统链接，已拒绝操作。");
                    exists = true;
                }
            }
            else
            {
                try
                {
                    if ((File.GetAttributes(current) & FileAttributes.ReparsePoint) != 0)
                        throw new ArchiveException(ArchiveFailure.UnsafePath, "目标或来源路径包含文件系统链接，已拒绝操作。");
                    exists = true;
                }
                catch (FileNotFoundException) { }
                catch (DirectoryNotFoundException) { }
            }
            if (!isFirst && exists && verifiedAncestors is not null)
            {
                visitedDirs ??= new List<string>(8);
                visitedDirs.Add(current);
            }
            isFirst = false;
            current = Path.GetDirectoryName(current);
        }
        if (visitedDirs is not null)
        {
            foreach (var dir in visitedDirs)
            {
                verifiedAncestors!.Add(dir);
            }
        }
    }

    private static IReadOnlyList<string> ResolveApprovedVolumes(string path, IReadOnlyList<string>? allowed)
    {
        if (allowed is null) return VolumeResolver.ResolveSource(path).Volumes;
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
            if (string.IsNullOrWhiteSpace(raw) || Encoding.UTF8.GetByteCount(raw) > 512) continue;
            var pattern = raw.Replace('\\', '/');
            var value = pattern.Contains('/') ? relative : fileName;
            if (Wildcard(value.AsSpan(), pattern.AsSpan())) return true;
        }
        return false;
    }

    public static bool MatchesInclusion(string path, bool isDirectory, IReadOnlyList<string>? patterns)
    {
        if (patterns is null || patterns.Count == 0) return true;
        var relative = path.Replace('\\', '/').TrimStart('/');
        var fileName = Path.GetFileName(path);
        foreach (var raw in patterns)
        {
            if (string.IsNullOrWhiteSpace(raw) || Encoding.UTF8.GetByteCount(raw) > 512) continue;
            var normalized = raw.Replace('\\', '/');
            if (normalized.StartsWith("./", StringComparison.Ordinal))
            {
                // Rooted selectors come from explicit archive entries, not user wildcard
                // rules. Keep them literal so a root file cannot select nested namesakes.
                var literal = normalized[2..].TrimEnd('/');
                if (literal.Length == 0) continue;
                if (string.Equals(relative.TrimEnd('/'), literal, StringComparison.Ordinal)
                    || relative.StartsWith(literal + "/", StringComparison.Ordinal)
                    || isDirectory && literal.StartsWith(relative.TrimEnd('/') + "/", StringComparison.Ordinal))
                    return true;
                continue;
            }
            var pattern = raw.Replace('\\', '/').TrimStart('/');
            var patternTrimmed = pattern.TrimEnd('/');

            if (string.Equals(relative, pattern, StringComparison.OrdinalIgnoreCase) ||
                string.Equals(relative, patternTrimmed, StringComparison.OrdinalIgnoreCase) ||
                string.Equals(fileName, pattern, StringComparison.OrdinalIgnoreCase))
                return true;

            if (relative.StartsWith(patternTrimmed + "/", StringComparison.OrdinalIgnoreCase))
                return true;

            if (isDirectory && patternTrimmed.StartsWith(relative.TrimEnd('/') + "/", StringComparison.OrdinalIgnoreCase))
                return true;

            var value = pattern.Contains('/') ? relative : fileName;
            if (Wildcard(value.AsSpan(), pattern.AsSpan())) return true;
        }
        return false;
    }

    // Preview uses the very same output planner and UTF-16/invariant casing as extraction.
    // A trailing separator represents an explicit archive directory entry.
    public static bool[] PreviewOutputExclusions(IReadOnlyList<string> paths, IReadOnlyList<string> patterns)
    {
        if (paths.Count > 1000 || patterns.Count > 100
            || paths.Any(path => path is null || Encoding.UTF8.GetByteCount(path) > 32768)
            || patterns.Any(pattern => pattern is null || Encoding.UTF8.GetByteCount(pattern) > 512))
            throw new ArgumentException("规则试算内容超出限制。");
        var entries = paths.Select(path => new ArchiveEntryInfo(
            path.Replace('/', Path.DirectorySeparatorChar).Replace('\\', Path.DirectorySeparatorChar).TrimEnd(Path.DirectorySeparatorChar),
            0, path.EndsWith('/') || path.EndsWith('\\'))).ToArray();
        return ExtractCallback.PlanOutput(entries, patterns, CancellationToken.None).Excluded;
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
        if (string.IsNullOrEmpty(root)) throw new IOException("无法确定目标卷的根目录。");
        // Keep extended drive, UNC and volume GUID roots intact. UNC queries require a trailing separator.
        if (!Path.EndsInDirectorySeparator(root)) root += Path.DirectorySeparatorChar;
        if (!GetDiskFreeSpaceEx(root, out var availableBytes, out _, out _))
            throw new IOException("无法查询目标卷的可用空间。",
                new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error()));
        if (minFreeBytes < 0 || availableBytes < (ulong)minFreeBytes)
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "可用空间低于保留空间要求。");
    }

    [DllImport("kernel32.dll", EntryPoint = "GetDiskFreeSpaceExW", CharSet = CharSet.Unicode,
        ExactSpelling = true, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetDiskFreeSpaceEx(string directoryName, out ulong availableBytes,
        out ulong totalBytes, out ulong totalFreeBytes);

    [DllImport("kernel32.dll", EntryPoint = "GetFileAttributesW", CharSet = CharSet.Unicode,
        ExactSpelling = true, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern uint GetFileAttributes(string lpFileName);

    private const uint InvalidFileAttributes = 0xFFFFFFFF;
    private const uint FileAttributeReparsePoint = 0x400;

    private static string? GetText(IInArchive archive, uint index, uint property) => WithProperty(archive, index, property, p => p.Text);
    private static bool GetBool(IInArchive archive, uint index, uint property) => WithProperty(archive, index, property, p => p.Bool);
    private static ulong GetNumber(IInArchive archive, uint index, uint property) => WithProperty(archive, index, property, p => p.Number);
    private static (ulong Size, bool Known) GetSize(IInArchive archive, uint index) =>
        WithProperty<(ulong, bool)>(archive, index, ArchiveProperty.Size, value => value.Type switch
        {
            (ushort)VarEnum.VT_EMPTY or (ushort)VarEnum.VT_NULL => (0, false),
            (ushort)VarEnum.VT_UI4 or (ushort)VarEnum.VT_UI8 => (value.Number, true),
            (ushort)VarEnum.VT_I4 when value.Signed >= 0 => (value.Number, true),
            _ => throw new ArchiveException(ArchiveFailure.Corrupt, "压缩包大小属性无效。")
        });
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

    private static ulong ReadArchiveFlags(IInArchive archive, uint property)
    {
        var result = archive.GetArchiveProperty(property, out var value);
        try
        {
            if (result != 0)
                throw new ArchiveException(ArchiveFailure.Corrupt, "压缩包完整性状态无法读取。");
            return value.Type switch
            {
                (ushort)VarEnum.VT_EMPTY or (ushort)VarEnum.VT_NULL => 0,
                (ushort)VarEnum.VT_UI4 or (ushort)VarEnum.VT_UI8 => value.Number,
                (ushort)VarEnum.VT_I4 when value.Signed >= 0 => value.Number,
                _ => throw new ArchiveException(ArchiveFailure.Corrupt, "压缩包完整性状态无效。")
            };
        }
        finally { value.Clear(); }
    }

    private static bool ValidateArchiveStatus(OpenedArchive archive, bool salvageCorrupted = false)
    {
        // Open/Extract can return S_OK even when the handler reports truncated headers
        // through archive properties. Entry callbacks only describe the entries found.
        var flags = ReadArchiveFlags(archive.Archive, ArchiveProperty.ErrorFlags)
            | ReadArchiveFlags(archive.Archive, ArchiveProperty.WarningFlags);
        if (archive.Callback.Error is not null) throw archive.Callback.Error;
        if (archive.Callback.MissingVolume || (archive.Callback.VolumeNotFound && flags != 0))
            throw new ArchiveException(ArchiveFailure.MissingVolume, "压缩包缺少分卷，请检查完整性。");
        if ((flags & (ArchiveErrorFlags.UnsupportedMethod | ArchiveErrorFlags.UnsupportedFeature)) != 0)
            throw new ArchiveException(ArchiveFailure.Unsupported, "压缩包使用了暂不支持的方法或功能。");
        if ((flags & ArchiveErrorFlags.EncryptedHeadersError) != 0 && archive.Callback.PasswordRequested)
            throw new ArchiveException(ArchiveFailure.WrongPassword, "需要有效密码，或加密头部已损坏。");
        if (flags != 0 && !salvageCorrupted)
            throw new ArchiveException(ArchiveFailure.Corrupt, "压缩包结构或完整性校验异常，文件可能损坏或不完整。");
        return flags != 0;
    }

    private static bool ValidateResult(OpenedArchive archive, ExtractCallback callback, int result, CancellationToken token,
        bool salvageCorrupted = false, bool archiveDamaged = false)
    {
        token.ThrowIfCancellationRequested();
        if (result == unchecked((int)0x8007000E))
            throw new ArchiveException(ArchiveFailure.ResourceLimit, "解压引擎可用内存不足。");
        if (callback.Error is not null) throw callback.Error;
        if (archive.Callback.Error is not null) throw archive.Callback.Error;
        if (archive.Callback.MissingVolume || (archive.Callback.VolumeNotFound && (result != 0 || callback.HasOperationErrors)))
            throw new ArchiveException(ArchiveFailure.MissingVolume, "压缩包缺少分卷，请检查完整性。");
        // A later unsupported entry must not be hidden by an earlier CRC failure. Native
        // operation results are categories, not severity-ordered numbers.
        if (callback.HasOperationError(1))
            throw new ArchiveException(ArchiveFailure.Unsupported, "压缩包使用了暂不支持的压缩方法。");
        if (callback.HasOperationError(9) || result == NativeResult.Abort && archive.Callback.PasswordRequested && archive.Callback.Password is null)
            throw new ArchiveException(ArchiveFailure.WrongPassword, "需要有效的压缩包密码。");
        if ((callback.HasOperationError(2) || callback.HasOperationError(3)) && archive.Callback.PasswordRequested)
        {
            // 抢救模式下：如果存在至少一个未损坏的非目录文件，证明密码正确，仅该条目存在 CRC 错误
            bool hasValidFile = callback.Files.Any(f => !f.IsDirectory && !f.IsCorrupted);
            if (!hasValidFile || !salvageCorrupted)
            {
                throw new ArchiveException(ArchiveFailure.WrongPassword, "加密内容校验失败：密码不匹配或文件损坏。");
            }
        }
        archiveDamaged |= ValidateArchiveStatus(archive, salvageCorrupted);
        if (result != 0 || callback.HasOperationErrors || archiveDamaged)
        {
            if (salvageCorrupted && callback.Files.Count > 0)
            {
                return true;
            }
            throw new ArchiveException(ArchiveFailure.Corrupt, "压缩包完整性校验失败，文件可能损坏或分卷不完整。");
        }
        return false;
    }

    public static uint ResolveEffectiveThreads(uint engineThreads) =>
        engineThreads == 0
            ? (uint)Math.Clamp(Environment.ProcessorCount, 1, 64)
            : Math.Clamp(engineThreads, 1u, 64u);

    private static void ApplyEngineThreads(IInArchive archive, uint engineThreads)
    {
        var effectiveThreads = ResolveEffectiveThreads(engineThreads);
        if (archive is ISetProperties setProperties)
        {
            var names = new[] { "mt" };
            var values = new[] { PropVariant.FromUInt32(effectiveThreads) };
            try
            {
                setProperties.SetProperties(names, values, 1);
            }
            catch { /* Ignore if archive handler does not support mt property */ }
            finally
            {
                values[0].Clear();
            }
        }
    }

    private static void ReleaseArchive(IInArchive archive)
    {
        try { archive.Close(); }
        finally { if (Marshal.IsComObject(archive)) Marshal.FinalReleaseComObject(archive); }
    }

    private sealed class OpenedArchive(IInArchive archive, InputStream stream, OpenCallback callback, PinnedSourceFiles sources) : IDisposable
    {
        public IInArchive Archive { get; } = archive;
        public OpenCallback Callback { get; } = callback;
        public void Dispose()
        {
            try { ReleaseArchive(Archive); }
            finally { try { stream.Dispose(); } finally { try { Callback.Dispose(); } finally { sources.Dispose(); } } }
        }
    }

    private sealed class Library
    {
        private readonly IntPtr _handle;
        private readonly CreateObject _create;
        [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int CreateObject(ref Guid classId, ref Guid interfaceId, out IntPtr archive);
        public Library(string path)
        {
            if (!Environment.Is64BitProcess) throw new ArchiveException(ArchiveFailure.Unsupported, "解压引擎需要 64 位进程。");
            if (!Path.IsPathFullyQualified(path)) throw new ArchiveException(ArchiveFailure.UnsafePath, "7-Zip 引擎路径必须是绝对路径。");
            var fullPath = Path.GetFullPath(path);
            EnsureNoReparsePoint(fullPath);
            var dir = Path.GetDirectoryName(fullPath);
            if (string.IsNullOrEmpty(dir) || !Directory.Exists(dir))
                throw new ArchiveException(ArchiveFailure.UnsafePath, "7-Zip 引擎目录无效。");
            EnsureNoReparsePoint(dir);
            if (!File.Exists(fullPath)) throw new ArchiveException(ArchiveFailure.Unsupported, "未找到官方 7-Zip 引擎。请运行 scripts/setup-engine.ps1。");
            var hashPath = fullPath + ".sha256";
            if (File.Exists(hashPath))
            {
                EnsureNoReparsePoint(hashPath);
                using var hashSource = new FileStream(hashPath, FileMode.Open, FileAccess.Read, FileShare.Read);
                byte[] hashBytes = new byte[1025];
                int hashLength = hashSource.ReadAtLeast(hashBytes, hashBytes.Length, throwOnEndOfStream: false);
                if (hashLength == hashBytes.Length) throw new ArchiveException(ArchiveFailure.Unsupported, "7-Zip 引擎校验文件超出长度限制。");
                var expected = Encoding.ASCII.GetString(hashBytes, 0, hashLength)
                    .Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries).FirstOrDefault();
                if (expected is null || expected.Length != 64 || !expected.All(Uri.IsHexDigit))
                    throw new ArchiveException(ArchiveFailure.Unsupported, "7-Zip 引擎校验文件无效。");
                using var stream = File.OpenRead(fullPath);
                var actual = Convert.ToHexString(SHA256.HashData(stream));
                if (!actual.Equals(expected, StringComparison.OrdinalIgnoreCase)) throw new ArchiveException(ArchiveFailure.Unsupported, "7-Zip 引擎校验失败，请重新运行 setup-engine.ps1。");
            }
            IntPtr loadedHandle = IntPtr.Zero;
            try
            {
                loadedHandle = NativeLibrary.Load(fullPath);
                _create = Marshal.GetDelegateForFunctionPointer<CreateObject>(NativeLibrary.GetExport(loadedHandle, "CreateObject"));
                _handle = loadedHandle;
            }
            catch (Exception ex) when (ex is DllNotFoundException or BadImageFormatException or EntryPointNotFoundException)
            {
                if (loadedHandle != IntPtr.Zero) NativeLibrary.Free(loadedHandle);
                throw new ArchiveException(ArchiveFailure.Unsupported, "无法加载官方 7-Zip 引擎，请检查系统架构和依赖。", ex);
            }
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
    private sealed class OpenCallback(string archivePath, string? password, CancellationToken token, IReadOnlyList<string> approvedVolumes, PinnedSourceFiles sources) : IArchiveOpenCallback, IArchiveOpenVolumeCallback, ICryptoGetTextPassword, IDisposable
    {
        private readonly List<InputStream> _volumes = [];
        public string ArchivePath { get; } = archivePath;
        public string? Password { get; } = password;
        public bool PasswordRequested { get; private set; }
        public bool VolumeNotFound { get; private set; }
        public bool MissingVolume { get; private set; }
        public ArchiveException? Error { get; set; }
        public void MarkOpenSucceeded() => MissingVolume = false;
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
                ArchiveProperty.Path or ArchiveProperty.Name => PropVariant.FromText(Path.GetFileName(ArchivePath)),
                ArchiveProperty.Size => PropVariant.FromNumber((ulong)new FileInfo(ArchivePath).Length),
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
                if (!File.Exists(candidate)) { VolumeNotFound = true; MissingVolume = true; return NativeResult.False; }
                if (!approvedVolumes.Contains(candidate, StringComparer.OrdinalIgnoreCase))
                    throw new ArchiveException(ArchiveFailure.UnsafePath, "压缩包要求读取不属于此分卷组的文件。");
                EnsureNoReparsePoint(candidate);
                var input = new InputStream(sources.OpenRead(candidate), token, this);
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
                var buffer = ArrayPool<byte>.Shared.Rent((int)Math.Min(size, 1024 * 1024));
                try
                {
                    var count = input.Read(buffer, 0, (int)Math.Min(size, buffer.Length));
                    Marshal.Copy(buffer, 0, data, count);
                    if (processedSize != IntPtr.Zero) Marshal.WriteInt32(processedSize, count);
                    return 0;
                }
                finally { ArrayPool<byte>.Shared.Return(buffer); }
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
        IReadOnlyList<string>? exclusions = null, bool salvageCorrupted = false, int? sourceZoneId = null,
        IReadOnlyList<string>? inclusions = null) : IArchiveExtractCallback, ICryptoGetTextPassword, IDisposable
    {
        private OutputStream? _current;
        private ArchiveEntryInfo? _pendingDirectory;
        private long _lastCallbackSpaceCheckTick;
        private readonly PathIdentityLease _outputLease = new();
        public void EnsureDestination() { if (destination is not null) _outputLease.EnsureDirectory(destination, create: true); }
        internal IncrementalHash SharedHash { get; } = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        private readonly (bool[] Excluded, long TotalBytes) _outputPlan = PlanOutput(entries, exclusions, token, inclusions);
        public List<ExtractedFile> Files { get; } = [];
        public int ExcludedEntries { get; private set; }
        public ArchiveException? Error { get; set; }
        private readonly HashSet<int> _operationErrors = [];
        public bool HasOperationErrors => _operationErrors.Count != 0;
        public bool HasOperationError(int result) => _operationErrors.Contains(result);
        public long WrittenBytes { get; set; }
        public long TotalBytes => _outputPlan.TotalBytes;
        private bool? _totalBytesKnown;
        public bool TotalBytesKnown => _totalBytesKnown ??= entries.Select((entry, index) => (entry, index))
            .All(item => item.entry.SizeKnown || _outputPlan.Excluded[item.index]);
        public int CompletedFiles { get; private set; }
        public bool SalvageCorrupted => salvageCorrupted;
        public int? SourceZoneId => sourceZoneId;
        private long _lastProgressTick;
        public int SetTotal(ulong total) => token.IsCancellationRequested ? NativeResult.Abort : 0;
        public int SetCompleted(IntPtr value)
        {
            if (token.IsCancellationRequested) return NativeResult.Abort;
            ReportWrittenProgress();
            return 0;
        }
        public void ReportWrittenProgress()
        {
            if (WrittenBytes == 0) return;
            long tick = Environment.TickCount64;
            if (tick - _lastProgressTick < 100) return;
            _lastProgressTick = tick;
            progress?.Report(new(WrittenBytes, TotalBytes, CompletedFiles, entries.Count, TotalBytesKnown));
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
                if (_outputPlan.Excluded[(int)index])
                { ExcludedEntries++; return 0; }
                var outputPath = Path.GetFullPath(Path.Combine(destination, entry.Path));
                WriteScopeGuard.DemandWriteAccess(outputPath);
                if (!outputPath.StartsWith(destination.TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
                    throw new ArchiveException(ArchiveFailure.UnsafePath, "解压路径超出了目标文件夹。");
                if (entry.IsDirectory) { _outputLease.EnsureDirectory(outputPath, create: true); _pendingDirectory = entry; return 0; }
                var parentDir = Path.GetDirectoryName(outputPath)!;
                _outputLease.EnsureDirectory(parentDir, create: true);
                var nowTick = Environment.TickCount64;
                if (_lastCallbackSpaceCheckTick == 0 || nowTick - _lastCallbackSpaceCheckTick >= 1000)
                {
                    EnsureFreeSpace(outputPath, minFreeBytes);
                    _lastCallbackSpaceCheckTick = nowTick;
                }
                _current = new OutputStream(_outputLease.CreateFile(outputPath), this,
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
                if (result != 0)
                {
                    _current?.MarkCorrupted();
                }
                _current?.Dispose(); _current = null;
                if (_pendingDirectory is not null) Files.Add(new(_pendingDirectory.Path, 0, "", true, false));
                _pendingDirectory = null;
                if (result != 0) _operationErrors.Add(result);
                CompletedFiles++;
                return token.IsCancellationRequested ? NativeResult.Abort : 0;
            }
            catch (Exception ex)
            {
                Error = ex as ArchiveException ?? new(ArchiveFailure.IoError, "完成解压文件写入失败。");
                return NativeResult.FromException(ex);
            }
        }
        public void Dispose()
        {
            try { _current?.Dispose(); }
            finally { _current = null; SharedHash.Dispose(); _outputLease.Dispose(); }
        }

        internal static (bool[] Excluded, long TotalBytes) PlanOutput(IReadOnlyList<ArchiveEntryInfo> entries,
            IReadOnlyList<string>? exclusions, CancellationToken token,
            IReadOnlyList<string>? inclusions = null)
        {
            token.ThrowIfCancellationRequested();
            var hasInclusions = inclusions is not null && inclusions.Count > 0;
            var excludedDirectories = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            for (var index = 0; index < entries.Count; index++)
            {
                token.ThrowIfCancellationRequested();
                var entry = entries[index];
                if (entry.IsDirectory && MatchesExclusion(entry.Path, exclusions)) excludedDirectories.Add(entry.Path);
            }
            token.ThrowIfCancellationRequested();
            var excluded = new bool[entries.Count];
            long totalBytes = 0;
            var hasExcludedDirs = excludedDirectories.Count > 0;
            for (var index = 0; index < entries.Count; index++)
            {
                token.ThrowIfCancellationRequested();
                var entry = entries[index];
                var isIncluded = !hasInclusions || MatchesInclusion(entry.Path, entry.IsDirectory, inclusions);
                excluded[index] = !isIncluded || MatchesExclusion(entry.Path, exclusions);
                if (hasExcludedDirs && !excluded[index])
                {
                    var parent = Path.GetDirectoryName(entry.Path);
                    while (!string.IsNullOrEmpty(parent))
                    {
                        if (excludedDirectories.Contains(parent))
                        {
                            excluded[index] = true;
                            break;
                        }
                        parent = Path.GetDirectoryName(parent);
                    }
                }
                if (!entry.IsDirectory && !excluded[index]) totalBytes += entry.Size;
            }
            token.ThrowIfCancellationRequested();
            return (excluded, totalBytes);
        }
    }

    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    private sealed class OutputStream : ISequentialOutStream, IDisposable
    {
        private const long SpaceCheckThrottleBytes = 32 * 1024 * 1024; // 32MB
        private const long SpaceCheckThrottleIntervalMs = 1000;       // 1 second
        private readonly PathIdentityLease.CreatedOutputFile _createdOutput;
        private readonly FileStream _output;
        private readonly ExtractCallback _callback;
        private readonly string _relativePath;
        private readonly string _outputPath;
        private readonly long _maxBytes;
        private readonly long _minFreeBytes;
        private readonly CancellationToken _token;
        private long _size;
        private bool _disposed;
        private bool _isCorrupted;
        private long _bytesSinceLastSpaceCheck;
        private long _lastSpaceCheckTick = Environment.TickCount64;

        public OutputStream(PathIdentityLease.CreatedOutputFile output, ExtractCallback callback, string relativePath, string outputPath,
            long maxBytes, long minFreeBytes, CancellationToken token)
        {
            _createdOutput = output;
            _output = output.Stream;
            _callback = callback;
            _relativePath = relativePath;
            _outputPath = outputPath;
            _maxBytes = maxBytes;
            _minFreeBytes = minFreeBytes;
            _token = token;
            _callback.SharedHash.GetHashAndReset();
        }

        public void MarkCorrupted() => _isCorrupted = true;
        public int Write(IntPtr data, uint size, IntPtr processedSize)
        {
            if (processedSize != IntPtr.Zero) Marshal.WriteInt32(processedSize, 0);
            try
            {
                _token.ThrowIfCancellationRequested();
                if (size > int.MaxValue || _callback.WrittenBytes > _maxBytes - size)
                    throw new ArchiveException(ArchiveFailure.ResourceLimit, "实际解压数据超过设置的总大小上限。");
                _bytesSinceLastSpaceCheck += size;
                var now = Environment.TickCount64;
                if (_bytesSinceLastSpaceCheck >= SpaceCheckThrottleBytes || now - _lastSpaceCheckTick >= SpaceCheckThrottleIntervalMs)
                {
                    EnsureFreeSpace(_outputPath, _minFreeBytes);
                    _bytesSinceLastSpaceCheck = 0;
                    _lastSpaceCheckTick = now;
                }
                var buffer = ArrayPool<byte>.Shared.Rent((int)Math.Min(size, 1024 * 1024));
                try
                {
                    var offset = 0;
                    while (offset < size)
                    {
                        _token.ThrowIfCancellationRequested();
                        var count = (int)Math.Min(buffer.Length, size - offset);
                        Marshal.Copy(IntPtr.Add(data, offset), buffer, 0, count);
                        _output.Write(buffer.AsSpan(0, count));
                        _callback.SharedHash.AppendData(buffer.AsSpan(0, count));
                        _size += count;
                        _callback.WrittenBytes += count;
                        offset += count;
                    }
                }
                finally { ArrayPool<byte>.Shared.Return(buffer, true); }
                _callback.ReportWrittenProgress();
                if (processedSize != IntPtr.Zero) Marshal.WriteInt32(processedSize, (int)size);
                return 0;
            }
            catch (Exception ex)
            {
                if (ex is not OperationCanceledException) _callback.Error = ex as ArchiveException ?? new(ArchiveFailure.IoError, "写入解压数据失败，请检查磁盘空间与权限。");
                return NativeResult.FromException(ex);
            }
        }
        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            try
            {
                _output.Flush();
                ZoneIdentifier.PropagateZone(_outputPath, _callback.SourceZoneId);
                var hashHex = Convert.ToHexStringLower(_callback.SharedHash.GetHashAndReset());
                _callback.Files.Add(new(_relativePath, _size, hashHex, false, _isCorrupted));
                _createdOutput.Keep();
            }
            finally { _createdOutput.Dispose(); }
        }
    }

    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    private sealed class BoundedNullOutStream(long maxBytes, CancellationToken token) : ISequentialOutStream
    {
        private long _written;
        public bool ExceededLimit { get; private set; }
        public int Write(IntPtr data, uint size, IntPtr processedSize)
        {
            if (processedSize != IntPtr.Zero) Marshal.WriteInt32(processedSize, 0);
            if (token.IsCancellationRequested) return NativeResult.Abort;
            if (size > int.MaxValue || size > maxBytes - _written)
            {
                ExceededLimit = true;
                return NativeResult.Abort;
            }
            _written += size;
            if (processedSize != IntPtr.Zero) Marshal.WriteInt32(processedSize, (int)size);
            return 0;
        }
    }

    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    private sealed class ProbeExtractCallback(string? password, long maxWrittenBytes, CancellationToken token) : IArchiveExtractCallback, ICryptoGetTextPassword
    {
        private readonly BoundedNullOutStream _stream = new(maxWrittenBytes, token);
        private bool _progressExceeded;
        public bool PasswordRequested { get; private set; }
        public int OperationResult { get; private set; }
        public bool LimitExceeded => _stream.ExceededLimit || _progressExceeded;
        public int SetTotal(ulong total) => CheckWork(total);
        public int SetCompleted(IntPtr completeValue) => completeValue == IntPtr.Zero
            ? (token.IsCancellationRequested ? NativeResult.Abort : 0)
            : CheckWork(unchecked((ulong)Marshal.ReadInt64(completeValue)));
        private int CheckWork(ulong bytes)
        {
            if (bytes > (ulong)maxWrittenBytes) _progressExceeded = true;
            return token.IsCancellationRequested || _progressExceeded ? NativeResult.Abort : 0;
        }
        public int CryptoGetTextPassword(out string? pwd)
        {
            PasswordRequested = true;
            pwd = password;
            return token.IsCancellationRequested || password is null ? NativeResult.Abort : 0;
        }
        public int GetStream(uint index, out ISequentialOutStream? stream, int askExtractMode)
        {
            stream = askExtractMode == 0 ? _stream : null;
            return token.IsCancellationRequested ? NativeResult.Abort : 0;
        }
        public int PrepareOperation(int askExtractMode) => token.IsCancellationRequested ? NativeResult.Abort : 0;
        public int SetOperationResult(int result)
        {
            OperationResult = result;
            return token.IsCancellationRequested ? NativeResult.Abort : 0;
        }
    }

    private sealed class PinnedSourceFiles : IDisposable
    {
        private readonly PathIdentityLease _lease = new();
        private readonly Dictionary<string, FileStream> _files = new(StringComparer.OrdinalIgnoreCase);
        public PinnedSourceFiles(IReadOnlyList<string> paths)
        {
            try { foreach (var path in paths) _files.Add(path, _lease.OpenRead(path)); }
            catch { Dispose(); throw; }
        }
        public FileStream OpenRead(string path) => _lease.OpenRead(path);
        public long Length(string path) => _files[path].Length;
        public void Dispose() { foreach (var file in _files.Values) file.Dispose(); _files.Clear(); _lease.Dispose(); }
    }

    private sealed class JoinedFileStream : Stream
    {
        private readonly IReadOnlyList<string> _paths;
        private readonly PinnedSourceFiles _sources;
        private readonly long[] _offsets;
        private readonly long _baseOffset;
        private FileStream? _current;
        private int _currentIndex = -1;
        private long _position;
        public JoinedFileStream(IReadOnlyList<string> paths, PinnedSourceFiles sources, long baseOffset = 0)
        {
            _paths = paths;
            _sources = sources;
            _baseOffset = baseOffset;
            _offsets = new long[paths.Count + 1];
            for (var i = 0; i < paths.Count; i++)
            {
                _offsets[i + 1] = checked(_offsets[i] + sources.Length(paths[i]));
            }
        }
        public override bool CanRead => true;
        public override bool CanSeek => true;
        public override bool CanWrite => false;
        public override long Length => Math.Max(0, _offsets[^1] - _baseOffset);
        public override long Position { get => _position; set => Seek(value, SeekOrigin.Begin); }
        public override int Read(byte[] buffer, int offset, int count)
        {
            if (_position >= Length || count == 0) return 0;
            var physicalPos = _position + _baseOffset;
            var index = 0;
            while (index + 1 < _offsets.Length && _offsets[index + 1] <= physicalPos) index++;
            if (_currentIndex != index) { _current?.Dispose(); _current = _sources.OpenRead(_paths[index]); _currentIndex = index; }
            _current!.Position = physicalPos - _offsets[index];
            var actual = _current.Read(buffer, offset, (int)Math.Min(count, _offsets[index + 1] - physicalPos));
            if (actual == 0 && physicalPos < _offsets[index + 1]) throw new ArchiveException(ArchiveFailure.MissingVolume, "分卷在读取过程中发生变化或被截断。");
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
