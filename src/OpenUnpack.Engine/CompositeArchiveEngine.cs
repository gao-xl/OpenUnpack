using OpenUnpack.Engine.Native;

namespace OpenUnpack.Engine;

/// <summary>
/// Composite multi-backend archive engine router (F4).
/// Distributes archive inspection and extraction requests to specialized engine backends:
/// - Lz4Engine: for modern LZ4 streaming frames (.lz4)
/// - SevenZipEngine: for native 7z, ZIP, RAR, TAR, GZ, BZ2, XZ, CAB, WIM, LZH, ZSTD, and verified SFX
/// </summary>
public sealed class CompositeArchiveEngine : IArchiveEngine, IPasswordProbe
{
    private readonly SevenZipEngine _sevenZipEngine;
    private readonly Lz4Engine _lz4Engine;

    public CompositeArchiveEngine(string sevenZipDllPath)
    {
        _sevenZipEngine = new SevenZipEngine(sevenZipDllPath);
        _lz4Engine = new Lz4Engine();
    }

    public SevenZipEngine NativeEngine => _sevenZipEngine;
    public Lz4Engine Lz4Backend => _lz4Engine;

    private bool IsLz4(string archivePath) => Lz4Engine.CanHandle(archivePath);

    public Task<ArchiveInspection> InspectAsync(string archivePath, string? password, CancellationToken cancellationToken) =>
        InspectAsync(archivePath, password, cancellationToken, null);

    public Task<ArchiveInspection> InspectAsync(string archivePath, string? password, CancellationToken cancellationToken, IReadOnlyList<string>? allowedVolumes) =>
        InspectAsync(archivePath, password, cancellationToken, allowedVolumes, 100_000, 100L * 1024 * 1024 * 1024);

    public Task<ArchiveInspection> InspectAsync(string archivePath, string? password, CancellationToken cancellationToken,
        IReadOnlyList<string>? allowedVolumes, int maxEntries, long maxTotalBytes)
    {
        if (IsLz4(archivePath))
            return _lz4Engine.InspectAsync(archivePath, password, cancellationToken, allowedVolumes, maxEntries, maxTotalBytes);
        return _sevenZipEngine.InspectAsync(archivePath, password, cancellationToken, allowedVolumes, maxEntries, maxTotalBytes);
    }

    public Task ExtractAsync(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken cancellationToken)
    {
        if (IsLz4(request.ArchivePath))
            return _lz4Engine.ExtractAsync(request, progress, cancellationToken);
        return _sevenZipEngine.ExtractAsync(request, progress, cancellationToken);
    }

    public Task<ExtractionManifest> ExtractWithManifestAsync(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken cancellationToken)
    {
        if (IsLz4(request.ArchivePath))
            return _lz4Engine.ExtractWithManifestAsync(request, progress, cancellationToken);
        return _sevenZipEngine.ExtractWithManifestAsync(request, progress, cancellationToken);
    }

    public Task<PasswordProbeResult> ProbeAsync(
        string archivePath,
        IReadOnlyList<string> candidatePasswords,
        CancellationToken cancellationToken,
        IReadOnlyList<string>? allowedVolumes = null,
        int maxEntries = 100_000,
        long maxTotalBytes = 100L * 1024 * 1024 * 1024,
        long maxWrittenBytes = long.MaxValue)
    {
        if (IsLz4(archivePath))
            return _lz4Engine.ProbeAsync(archivePath, candidatePasswords, cancellationToken, allowedVolumes, maxEntries, maxTotalBytes, maxWrittenBytes);
        return _sevenZipEngine.ProbeAsync(archivePath, candidatePasswords, cancellationToken, allowedVolumes, maxEntries, maxTotalBytes, maxWrittenBytes);
    }
}
