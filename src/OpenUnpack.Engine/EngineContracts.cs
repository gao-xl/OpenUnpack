namespace OpenUnpack.Engine;

public enum ArchiveFailure
{
    None,
    WrongPassword,
    Unsupported,
    Corrupt,
    MissingVolume,
    UnsafePath,
    IoError,
    ResourceLimit
}

public sealed class ArchiveException(ArchiveFailure failure, string message, Exception? inner = null) : Exception(message, inner)
{
    public ArchiveFailure Failure { get; } = failure;
}

public sealed record ArchiveEntryInfo(string Path, long Size, bool IsDirectory, bool IsLink = false, bool IsEncrypted = false,
    bool SizeKnown = true);

public sealed record ArchiveInspection(IReadOnlyList<ArchiveEntryInfo> Entries, bool EncryptedHeaders = false)
{
    public long TotalBytes => Entries.Sum(e => e.Size);
    public bool TotalBytesKnown => Entries.All(e => e.SizeKnown);
}

public sealed record ExtractionRequest(
    string ArchivePath,
    string DestinationDirectory,
    string? Password,
    int MaxEntries = 100_000,
    long MaxTotalBytes = 100L * 1024 * 1024 * 1024,
    IReadOnlyList<string>? AllowedVolumes = null,
    long MaxWrittenBytes = long.MaxValue,
    long MinFreeBytes = 0,
    IReadOnlyList<string>? Exclusions = null,
    uint EngineThreads = 0,
    bool SalvageCorrupted = false,
    int? SourceZoneId = null,
    IReadOnlyList<string>? Inclusions = null);

public sealed record ExtractedFile(string RelativePath, long Size, string Sha256, bool IsDirectory, bool IsCorrupted = false);

public sealed record ExtractionManifest(
    IReadOnlyList<ExtractedFile> Files,
    bool PasswordUsed,
    long WrittenBytes,
    int ExcludedEntries,
    string VerificationScope = "archiveCrcAndStreamSha256");

public sealed record ExtractionProgress(long CompletedBytes, long TotalBytes, int CompletedFiles, int TotalFiles,
    bool TotalBytesKnown = true);

public interface IArchiveEngine
{
    Task<ArchiveInspection> InspectAsync(string archivePath, string? password, CancellationToken cancellationToken);
    Task ExtractAsync(ExtractionRequest request, IProgress<ExtractionProgress>? progress, CancellationToken cancellationToken);
}

public enum PasswordProbeStatus
{
    Success,
    WrongPassword,
    NoPasswordNeeded,
    MissingVolume,
    Corrupt,
    Unsupported,
    Failed
}

public sealed record PasswordProbeResult(PasswordProbeStatus Status, string? MatchedPassword, string? ErrorMessage = null);

public interface IPasswordProbe
{
    Task<PasswordProbeResult> ProbeAsync(
        string archivePath,
        IReadOnlyList<string> candidatePasswords,
        CancellationToken cancellationToken,
        IReadOnlyList<string>? allowedVolumes = null,
        int maxEntries = 100_000,
        long maxTotalBytes = 100L * 1024 * 1024 * 1024,
        long maxWrittenBytes = long.MaxValue);
}

public static class XxHash32
{
    private const uint Prime1 = 0x9E3779B1U;
    private const uint Prime2 = 0x85EBCA77U;
    private const uint Prime3 = 0xC2B2AE3DU;
    private const uint Prime4 = 0x27D4EB2FU;
    private const uint Prime5 = 0x165667B1U;

    public static uint Compute(ReadOnlySpan<byte> data, uint seed = 0)
    {
        uint h32;
        int index = 0;
        int len = data.Length;

        if (len >= 16)
        {
            int limit = len - 16;
            uint v1 = seed + Prime1 + Prime2;
            uint v2 = seed + Prime2;
            uint v3 = seed + 0;
            uint v4 = seed - Prime1;

            do
            {
                v1 = Round(v1, System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(data.Slice(index, 4)));
                index += 4;
                v2 = Round(v2, System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(data.Slice(index, 4)));
                index += 4;
                v3 = Round(v3, System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(data.Slice(index, 4)));
                index += 4;
                v4 = Round(v4, System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(data.Slice(index, 4)));
                index += 4;
            } while (index <= limit);

            h32 = ((v1 << 1) | (v1 >> 31)) +
                  ((v2 << 7) | (v2 >> 25)) +
                  ((v3 << 12) | (v3 >> 20)) +
                  ((v4 << 18) | (v4 >> 14));
        }
        else
        {
            h32 = seed + Prime5;
        }

        h32 += (uint)len;

        while (index <= len - 4)
        {
            h32 += System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(data.Slice(index, 4)) * Prime3;
            h32 = ((h32 << 17) | (h32 >> 15)) * Prime4;
            index += 4;
        }

        while (index < len)
        {
            h32 += data[index] * Prime5;
            h32 = ((h32 << 11) | (h32 >> 21)) * Prime1;
            index++;
        }

        h32 ^= h32 >> 15;
        h32 *= Prime2;
        h32 ^= h32 >> 13;
        h32 *= Prime3;
        h32 ^= h32 >> 16;

        return h32;
    }

    private static uint Round(uint seed, uint input)
    {
        seed += input * Prime2;
        seed = (seed << 13) | (seed >> 19);
        seed *= Prime1;
        return seed;
    }
}

public sealed class IncrementalXxHash32
{
    private const uint Prime1 = 0x9E3779B1U;
    private const uint Prime2 = 0x85EBCA77U;
    private const uint Prime3 = 0xC2B2AE3DU;
    private const uint Prime4 = 0x27D4EB2FU;
    private const uint Prime5 = 0x165667B1U;

    private readonly uint _seed;
    private uint _v1, _v2, _v3, _v4;
    private ulong _totalLength;
    private readonly byte[] _buffer = new byte[16];
    private int _bufferLength;

    public IncrementalXxHash32(uint seed = 0)
    {
        _seed = seed;
        Reset();
    }

    public void Reset()
    {
        _v1 = _seed + Prime1 + Prime2;
        _v2 = _seed + Prime2;
        _v3 = _seed + 0;
        _v4 = _seed - Prime1;
        _totalLength = 0;
        _bufferLength = 0;
    }

    public void Update(ReadOnlySpan<byte> data)
    {
        if (data.IsEmpty) return;
        _totalLength += (ulong)data.Length;

        int index = 0;
        if (_bufferLength > 0)
        {
            int toCopy = Math.Min(16 - _bufferLength, data.Length);
            data.Slice(0, toCopy).CopyTo(_buffer.AsSpan(_bufferLength));
            _bufferLength += toCopy;
            index += toCopy;

            if (_bufferLength == 16)
            {
                ProcessStripe(_buffer);
                _bufferLength = 0;
            }
        }

        while (index <= data.Length - 16)
        {
            ProcessStripe(data.Slice(index, 16));
            index += 16;
        }

        if (index < data.Length)
        {
            int remaining = data.Length - index;
            data.Slice(index, remaining).CopyTo(_buffer.AsSpan(_bufferLength));
            _bufferLength += remaining;
        }
    }

    private void ProcessStripe(ReadOnlySpan<byte> stripe)
    {
        _v1 = Round(_v1, System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(stripe.Slice(0, 4)));
        _v2 = Round(_v2, System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(stripe.Slice(4, 4)));
        _v3 = Round(_v3, System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(stripe.Slice(8, 4)));
        _v4 = Round(_v4, System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(stripe.Slice(12, 4)));
    }

    public uint GetHash()
    {
        uint h32;
        if (_totalLength >= 16)
        {
            h32 = ((_v1 << 1) | (_v1 >> 31)) +
                  ((_v2 << 7) | (_v2 >> 25)) +
                  ((_v3 << 12) | (_v3 >> 20)) +
                  ((_v4 << 18) | (_v4 >> 14));
        }
        else
        {
            h32 = _seed + Prime5;
        }

        h32 += (uint)_totalLength;

        int index = 0;
        var bufSpan = _buffer.AsSpan(0, _bufferLength);
        while (index <= _bufferLength - 4)
        {
            h32 += System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(bufSpan.Slice(index, 4)) * Prime3;
            h32 = ((h32 << 17) | (h32 >> 15)) * Prime4;
            index += 4;
        }

        while (index < _bufferLength)
        {
            h32 += bufSpan[index] * Prime5;
            h32 = ((h32 << 11) | (h32 >> 21)) * Prime1;
            index++;
        }

        h32 ^= h32 >> 15;
        h32 *= Prime2;
        h32 ^= h32 >> 13;
        h32 *= Prime3;
        h32 ^= h32 >> 16;

        return h32;
    }

    private static uint Round(uint seed, uint input)
    {
        seed += input * Prime2;
        seed = (seed << 13) | (seed >> 19);
        seed *= Prime1;
        return seed;
    }
}
