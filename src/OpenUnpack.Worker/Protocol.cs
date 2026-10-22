namespace OpenUnpack.Worker;

public sealed record FileRecord
{
    public string RelativePath { get; init; } = "";
    public ulong Size { get; init; }
    public bool SizeKnown { get; init; } = true;
    public string Sha256 { get; init; } = "";
    public bool IsDirectory { get; init; }
    public bool IsEncrypted { get; init; }
    public bool IsCorrupted { get; init; }
}

public sealed record CopyFile
{
    public string RelativePath { get; init; } = "";
    public ulong Size { get; init; }
    public bool SizeKnown { get; init; } = true;
    public string Sha256 { get; init; } = "";
    public bool IsDirectory { get; init; }
    public string SourcePath { get; init; } = "";
    public bool IsCorrupted { get; init; }
}

public sealed record CandidateItem
{
    public string CandidateId { get; init; } = "";
    public string Source { get; init; } = "";
    public ulong Revision { get; init; }
    public string Label { get; init; } = "";
    public string Secret { get; init; } = "";

    // Records otherwise synthesize a data ToString that would echo the secret.
    public override string ToString() =>
        $"{nameof(CandidateItem)}(CandidateId={CandidateId}, Source={Source}, Revision={Revision}, Label={Label}, Secret=redacted)";
}

public sealed record WorkerCapabilities
{
    public string BackendId { get; init; } = "7-Zip";
    public string BackendVersion { get; init; } = "26.04";
    public bool ReadMetadata { get; init; } = true;
    public bool ExtractWithCandidates { get; init; }
    public bool SolidOptimization { get; init; }
    public int MaxBatchCandidates { get; init; } = 128;
    public List<WorkerBackend> Backends { get; init; } = [];
}

public sealed record WorkerBackend
{
    public string BackendId { get; init; } = "";
    public string BackendVersion { get; init; } = "";
    public List<string> Formats { get; init; } = [];
    public bool ReadMetadata { get; init; }
    public bool EncryptedArchives { get; init; }
    public bool UnknownExpandedSize { get; init; }
    public List<string> VerificationScopes { get; init; } = [];
}

public sealed record WorkerHello
{
    public uint ProtocolVersion { get; init; } = Protocol.VersionV2;
    public uint Major { get; init; } = 2;
    public uint Minor { get; init; } = 0;
    public WorkerCapabilities Capabilities { get; init; } = new();
}

public sealed record WorkerRequest
{
    public uint ProtocolVersion { get; init; } = 1;
    public string RequestId { get; init; } = "";
    public string Operation { get; init; } = "";
    public string? OperationId { get; init; } = null;
    public ulong? Generation { get; init; } = null;
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
    public List<string> Inclusions { get; init; } = [];
    public List<string> CandidatePasswords { get; init; } = [];
    public List<CandidateItem> Candidates { get; init; } = [];
    public uint EngineThreads { get; init; }
    public string WorkerPriority { get; init; } = "";
    public bool SalvageCorrupted { get; init; }

    // Records otherwise synthesize a data ToString that would echo the password.
    public override string ToString() =>
        $"{nameof(WorkerRequest)}(RequestId={RequestId}, Operation={Operation}, OperationId={OperationId}, Generation={Generation}, PasswordPresent={Password != null}, CandidatePasswords={CandidatePasswords.Count}, Candidates={Candidates.Count})";
}

public sealed record WorkerEvent
{
    public uint ProtocolVersion { get; init; } = 1;
    public string RequestId { get; init; } = "";
    public string Kind { get; init; } = "";
    [System.Text.Json.Serialization.JsonIgnore(Condition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull)]
    public WorkerHello? Hello { get; init; }
    public string? OperationId { get; init; } = null;
    public ulong? Generation { get; init; } = null;
    public ulong CompletedBytes { get; init; }
    public ulong TotalBytes { get; init; }
    public bool TotalBytesKnown { get; init; } = true;
    public string Message { get; init; } = "";
    public string ErrorCode { get; init; } = "";
    public List<FileRecord> Files { get; init; } = [];
    public string? ManifestPath { get; init; } = null;
    public string? ManifestId { get; init; } = null;
    public string? ManifestSha256 { get; init; } = null;
    public ulong? ManifestByteLength { get; init; } = null;
    public bool PasswordUsed { get; init; }
    public string? MatchedPassword { get; init; } = null;
    public string? MatchedCandidateId { get; init; } = null;
    public string? ProbeStatus { get; init; } = null;
    public string? Fingerprint { get; init; } = null;
    public string VerificationScope { get; init; } = "";

    // Records otherwise synthesize a data ToString that would echo a returned secret.
    public override string ToString() =>
        $"{nameof(WorkerEvent)}(RequestId={RequestId}, Kind={Kind}, OperationId={OperationId}, Generation={Generation}, MatchedPasswordPresent={MatchedPassword != null}, MatchedCandidateId={MatchedCandidateId})";
}

internal static class Protocol
{
    public const uint Version = 1;
    public const uint VersionV1 = 1;
    public const uint VersionV2 = 2;
    public const int MaxMessageBytes = 8 * 1024 * 1024;
    public static readonly System.Text.Json.JsonSerializerOptions Json = new(System.Text.Json.JsonSerializerDefaults.Web)
    {
        PropertyNameCaseInsensitive = false,
        NumberHandling = System.Text.Json.Serialization.JsonNumberHandling.Strict,
        MaxDepth = 32
    };
}
