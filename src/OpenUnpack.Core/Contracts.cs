namespace OpenUnpack.Core;

public sealed record PasswordEntry
{
    public Guid Id { get; init; } = Guid.NewGuid();
    public string Label { get; init; } = "";
    public string Secret { get; init; } = "";
    public string Group { get; init; } = "默认";
    public string Notes { get; init; } = "";
    public bool Enabled { get; init; } = true;
    public int SuccessCount { get; init; }
    public DateTimeOffset? LastUsedUtc { get; init; }
    public DateTimeOffset CreatedUtc { get; init; } = DateTimeOffset.UtcNow;
}

public sealed class VaultSnapshot
{
    public int Version { get; set; } = 1;
    public List<PasswordEntry> Entries { get; set; } = [];
    public Dictionary<string, Guid> ArchiveMatches { get; set; } = [];
    public List<JobHistoryEntry> History { get; set; } = [];
}

public sealed record JobHistoryEntry(string FileName, string Status, string? OutputPath, DateTimeOffset FinishedUtc, string Message);
public sealed record PasswordImportResult(IReadOnlyList<PasswordEntry> Entries, int DuplicateCount, int IgnoredCount);

public enum ArchiveFailure { None, WrongPassword, Unsupported, Corrupt, MissingVolume, UnsafePath, IoError, ResourceLimit }
public sealed class ArchiveException(ArchiveFailure failure, string message, Exception? inner = null) : Exception(message, inner)
{
    public ArchiveFailure Failure { get; } = failure;
}
public sealed record ArchiveEntryInfo(string Path, long Size, bool IsDirectory, bool IsLink = false);
public sealed record ArchiveInspection(IReadOnlyList<ArchiveEntryInfo> Entries)
{
    public long TotalBytes => Entries.Sum(e => e.Size);
}
public sealed record ExtractionRequest(string ArchivePath, string DestinationDirectory, string? Password,
    int MaxEntries = 100_000, long MaxTotalBytes = 100L * 1024 * 1024 * 1024,
    IReadOnlyList<string>? AllowedVolumes = null, long MaxWrittenBytes = long.MaxValue,
    long MinFreeBytes = 0, IReadOnlyList<string>? Exclusions = null);
public sealed record ExtractedFile(string RelativePath, long Size, string Sha256, bool IsDirectory);
public sealed record ExtractionManifest(IReadOnlyList<ExtractedFile> Files, bool PasswordUsed, long WrittenBytes, int ExcludedEntries);
public sealed record ExtractionProgress(long CompletedBytes, long TotalBytes, int CompletedFiles, int TotalFiles);
public interface IArchiveEngine
{
    Task<ArchiveInspection> InspectAsync(string archivePath, string? password, CancellationToken cancellationToken);
    Task ExtractAsync(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken cancellationToken);
}

public enum PasswordProbeStatus { Success, WrongPassword, NoPasswordNeeded, MissingVolume, Corrupt, Unsupported, Failed }
public sealed record PasswordProbeResult(PasswordProbeStatus Status, string? MatchedPassword, string? ErrorMessage = null);
public interface IPasswordProbe
{
    Task<PasswordProbeResult> ProbeAsync(string archivePath, IReadOnlyList<string> candidatePasswords, CancellationToken cancellationToken, IReadOnlyList<string>? allowedVolumes = null);
}
public sealed record ArchiveSource(string Path, string DisplayName, IReadOnlyList<string> Volumes);
public sealed record ScanNotice(string Path, string Message);
public sealed record ScanResult(IReadOnlyList<ArchiveSource> Archives, IReadOnlyList<ScanNotice> Notices);

public enum ConflictPolicy { RenameFolder, Skip }
public sealed record BatchOptions(string? OutputRoot, ConflictPolicy ConflictPolicy = ConflictPolicy.RenameFolder,
    string? PasswordGroup = null, int MaxEntries = 100_000, long MaxTotalBytes = 100L * 1024 * 1024 * 1024,
    string? CacheRoot = null, bool CacheInput = true, long MaxCacheBytes = 20L * 1024 * 1024 * 1024,
    long MinFreeCacheBytes = 1L * 1024 * 1024 * 1024, bool PlaySoundOnCompletion = false);

public enum JobStatus { Waiting, Running, Completed, NeedsPassword, Failed, Cancelled, Skipped }
public sealed record BatchUpdate(string ArchivePath, JobStatus Status, double ProgressPercent, string Message,
    int AttemptCount = 0, string? PasswordLabel = null, string? OutputPath = null);
public sealed record BatchRunResult(int Completed, int Failed, int Skipped, int Cancelled);
