namespace OpenUnpack.Worker;

public sealed record FileRecord
{
    public string RelativePath { get; init; } = "";
    public ulong Size { get; init; }
    public string Sha256 { get; init; } = "";
    public bool IsDirectory { get; init; }
}

public sealed record CopyFile
{
    public string RelativePath { get; init; } = "";
    public ulong Size { get; init; }
    public string Sha256 { get; init; } = "";
    public bool IsDirectory { get; init; }
    public string SourcePath { get; init; } = "";
}

public sealed record WorkerRequest
{
    public uint ProtocolVersion { get; init; } = 1;
    public string RequestId { get; init; } = "";
    public string Operation { get; init; } = "";
    public string ArchivePath { get; init; } = "";
    public string DestinationDirectory { get; init; } = "";
    public string? Password { get; init; } = null;
    public List<string> AllowedVolumes { get; init; } = [];
    public ulong MaxEntries { get; init; }
    public ulong MaxTotalBytes { get; init; }
    public ulong MaxWrittenBytes { get; init; }
    public ulong MinFreeBytes { get; init; }
    public List<CopyFile> Files { get; init; } = [];
    public string Verification { get; init; } = "";
    public List<string> Exclusions { get; init; } = [];
    public List<string> CandidatePasswords { get; init; } = [];
}

public sealed record WorkerEvent
{
    public uint ProtocolVersion { get; init; } = 1;
    public string RequestId { get; init; } = "";
    public string Kind { get; init; } = "";
    public ulong CompletedBytes { get; init; }
    public ulong TotalBytes { get; init; }
    public string Message { get; init; } = "";
    public string ErrorCode { get; init; } = "";
    public List<FileRecord> Files { get; init; } = [];
    public string? ManifestPath { get; init; } = null;
    public bool PasswordUsed { get; init; }
    public string? MatchedPassword { get; init; } = null;
    public string? ProbeStatus { get; init; } = null;
    public string? Fingerprint { get; init; } = null;
    public string VerificationScope { get; init; } = "";
}

internal static class Protocol
{
    public const uint Version = 1;
    public const int MaxMessageBytes = 8 * 1024 * 1024;
    public static readonly System.Text.Json.JsonSerializerOptions Json = new(System.Text.Json.JsonSerializerDefaults.Web)
    {
        PropertyNameCaseInsensitive = false,
        MaxDepth = 32
    };
}
