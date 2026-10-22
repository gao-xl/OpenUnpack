using System.Diagnostics;
using System.Buffers.Binary;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using OpenUnpack.Engine;
using OpenUnpack.Engine.Native;
using OpenUnpack.Worker;

var repository = Path.GetFullPath(args.Length > 0 ? args[0] : FindRepository());
var root = Path.Combine(repository, "artifacts", "test-results", DateTime.UtcNow.ToString("yyyyMMdd-HHmmss") + "-" + Guid.NewGuid().ToString("N")[..8]);
Directory.CreateDirectory(root);
var results = new List<object>();
int passed = 0, failed = 0, skipped = 0;
var zipPath = Path.Combine(root, "中文 路径", "普通.zip");
Directory.CreateDirectory(Path.GetDirectoryName(zipPath)!);
CreateZip(zipPath, [("目录/你好.txt", Encoding.UTF8.GetBytes("OpenUnpack 中文内容\n")), ("binary.bin", RandomNumberGenerator.GetBytes(32_000))]);
var enginePath = Path.Combine(repository, "tools", "7zip", "7z.dll");
var cliPath = Path.Combine(repository, "tools", "7zip", "7z.exe");
var nativeAvailable = File.Exists(enginePath) && File.Exists(cliPath);
IArchiveEngine? engine = nativeAvailable ? new SevenZipEngine(enginePath) : null;

foreach (var (name, test) in PathIdentityChecks.CreateCases(repository, root)) await Check(name, test);
if (args.Contains("--path-identity-only"))
{
    File.WriteAllText(Path.Combine(root, "results.json"), JsonSerializer.Serialize(new { passed, failed, skipped, results }, new JsonSerializerOptions { WriteIndented = true }));
    Console.WriteLine($"Path identities: {passed} passed, {failed} failed. Evidence: {root}");
    return failed == 0 ? 0 : 1;
}
foreach (var (name, test) in WorkerBoundaryChecks.CreateCases(repository, root)) await Check(name, test);
if (args.Contains("--worker-boundary-only"))
{
    File.WriteAllText(Path.Combine(root, "results.json"), JsonSerializer.Serialize(new { passed, failed, skipped, results }, new JsonSerializerOptions { WriteIndented = true }));
    Console.WriteLine($"Worker boundaries: {passed} passed, {failed} failed. Evidence: {root}");
    return failed == 0 ? 0 : 1;
}

await Check("output rule preview shares UTF16 casing and directory policy with extraction", () =>
{
    var paths = new[] { "😀.txt", "ς.txt", "temp/", "temp/child.bin", "temporary/file.bin", "ı.txt" };
    Assert(SevenZipEngine.PreviewOutputExclusions(paths, ["?.txt"])[0] == false, "question mark consumes a UTF16 unit");
    Assert(SevenZipEngine.PreviewOutputExclusions(paths, ["??.txt"])[0], "two question marks match a surrogate pair");
    Assert(SevenZipEngine.PreviewOutputExclusions(paths, ["σ.txt"])[1], "invariant terminal sigma comparison");
    Assert(!SevenZipEngine.PreviewOutputExclusions(paths, ["I.txt"])[5], "invariant dotless i does not match I");
    Assert(SevenZipEngine.PreviewOutputExclusions(paths, ["temp"]).SequenceEqual(new[] { false, false, true, true, false, false }), "explicit directory propagation only");
    Assert(!SevenZipEngine.PreviewOutputExclusions(["temp/child.bin"], ["temp"])[0], "implicit directory does not propagate");
    Assert(!SevenZipEngine.PreviewOutputExclusions(["temp/child.bin"], ["temp*"])[0], "basename rule scope");
    Assert(SevenZipEngine.PreviewOutputExclusions(["temp/child.bin"], ["temp/*"])[0], "full path rule scope");
    return Task.CompletedTask;
});

await Check("rooted archive selectors are literal and retain directory ancestors", () =>
{
    Assert(SevenZipEngine.MatchesInclusion("root.txt", false, ["./root.txt"]), "root selector matches the exact archive name");
    Assert(!SevenZipEngine.MatchesInclusion("root.txt", false, ["./ROOT.TXT"]), "root selector preserves archive-name casing");
    Assert(!SevenZipEngine.MatchesInclusion("nested/root.txt", false, ["./root.txt"]), "root selector does not match nested namesake");
    Assert(SevenZipEngine.MatchesInclusion("ς.txt", false, ["./ς.txt"]), "root selector retains the exact Unicode name");
    Assert(!SevenZipEngine.MatchesInclusion("ς.txt", false, ["./σ.txt"]), "root selector does not fold terminal sigma into ordinary sigma");
    Assert(!SevenZipEngine.MatchesInclusion("σ.txt", false, ["./ς.txt"]), "root selector does not fold ordinary sigma into terminal sigma");
    Assert(!SevenZipEngine.MatchesInclusion("root.txt", false, ["./*.txt"]), "rooted selector never interprets wildcards");
    Assert(SevenZipEngine.MatchesInclusion("folder/file.txt", false, ["./folder/file.txt"]), "literal relative member matches");
    Assert(SevenZipEngine.MatchesInclusion("folder", true, ["./folder/file.txt"]), "selected file keeps ancestor directory");
    Assert(SevenZipEngine.MatchesInclusion("folder/sub", true, ["./folder/sub/file.txt"]), "nested file keeps every ancestor");
    Assert(SevenZipEngine.MatchesInclusion("folder/sub/file.txt", false, ["./folder/"]), "selected directory keeps descendants");
    Assert(!SevenZipEngine.MatchesInclusion("folderx/file.txt", false, ["./folder/"]), "directory selector respects path boundary");
    Assert(SevenZipEngine.MatchesInclusion("nested/root.txt", false, ["root.txt"]), "ordinary basename rule remains supported");
    Assert(SevenZipEngine.MatchesInclusion("nested/root.txt", false, ["ROOT.TXT"]), "ordinary basename rule retains case-insensitive matching");
    Assert(SevenZipEngine.MatchesInclusion("nested/root.txt", false, ["*.txt"]), "ordinary wildcard rule remains supported");
    return Task.CompletedTask;
});

await Check("credential security zeroes non-interned secret and spans", () =>
{
    var nonInterned = new string("super-secret-password-12345".ToCharArray());
    CredentialSecurity.ZeroizeString(nonInterned);
    Assert(nonInterned.All(c => c == '\0'), "non-interned string characters must be cleared to null");

    Span<byte> buffer = stackalloc byte[16];
    buffer.Fill(0xAA);
    CredentialSecurity.ZeroizeBytes(buffer);
    Assert(buffer.ToArray().All(b => b == 0), "span bytes must be cleared to zero");
    return Task.CompletedTask;
});

await Check("F3 engine threads and priority adapt to system configuration", () =>
{
    var autoThreads = SevenZipEngine.ResolveEffectiveThreads(0);
    Assert(autoThreads >= 1 && autoThreads <= 64, "auto threads must be clamped between 1 and 64");
    Assert(SevenZipEngine.ResolveEffectiveThreads(8) == 8, "explicit thread setting is preserved");
    Assert(SevenZipEngine.ResolveEffectiveThreads(128) == 64, "excessive thread setting is clamped to 64");
    return Task.CompletedTask;
});

await Check("F5 salvage extraction recovers valid files and flags corrupted CRC entries", async () =>
{
    if (!nativeAvailable) throw new TestSkippedException("Native backend is unavailable.");
    var testDir = Path.Combine(root, "salvage-test");
    Directory.CreateDirectory(testDir);
    var corruptZip = Path.Combine(testDir, "corrupted.zip");
    using (var archive = ZipFile.Open(corruptZip, ZipArchiveMode.Create))
    {
        using (var s1 = archive.CreateEntry("intact.txt", CompressionLevel.NoCompression).Open())
            s1.Write(Encoding.UTF8.GetBytes("Hello OpenUnpack! Intact file"));
        using (var s2 = archive.CreateEntry("bad.txt", CompressionLevel.NoCompression).Open())
            s2.Write(Encoding.UTF8.GetBytes("This file payload will have corrupted compressed bytes in the zip archive!"));
    }

    // 故意篡改 bad.txt 对应的压缩数据以触发 CRC 校验失败
    var zipBytes = File.ReadAllBytes(corruptZip);
    var targetText = Encoding.UTF8.GetBytes("This file payload");
    int matchIdx = -1;
    for (int i = 0; i < zipBytes.Length - targetText.Length; i++)
    {
        if (zipBytes.AsSpan(i, targetText.Length).SequenceEqual(targetText))
        {
            matchIdx = i;
            break;
        }
    }
    Assert(matchIdx >= 0, "found payload to corrupt");
    zipBytes[matchIdx + 5] ^= 0xFF;
    zipBytes[matchIdx + 6] ^= 0xAA;
    File.WriteAllBytes(corruptZip, zipBytes);

    var destNormal = Path.Combine(testDir, "out-normal");
    var destSalvage = Path.Combine(testDir, "out-salvage");

    // 1. 常规模式：必须报错 Corrupt，拒绝非安全提交
    var normalReq = new ExtractionRequest(corruptZip, destNormal, null, SalvageCorrupted: false);
    await ThrowsArchive(async () => await engine!.ExtractAsync(normalReq, null, CancellationToken.None), ArchiveFailure.Corrupt);

    // 2. 抢救模式：提取成功，完好文件正常交付，损坏文件标记 IsCorrupted = true
    var salvageReq = new ExtractionRequest(corruptZip, destSalvage, null, SalvageCorrupted: true);
    var manifest = await ((SevenZipEngine)engine!).ExtractWithManifestAsync(salvageReq, null, CancellationToken.None);
    Assert(manifest.Files.Count == 2, "salvage extracted both entries");
    var intact = manifest.Files.FirstOrDefault(f => f.RelativePath == "intact.txt");
    var bad = manifest.Files.FirstOrDefault(f => f.RelativePath == "bad.txt");
    Assert(intact != null && !intact.IsCorrupted, "intact file must not be corrupted");
    Assert(bad != null && bad.IsCorrupted, "bad file must be flagged as IsCorrupted");
    Assert(File.Exists(Path.Combine(destSalvage, "intact.txt")), "intact file output on disk");
});

await Check("F4 multi-backend engine expands WIM, Zstandard, LZ4, and verified SFX support", async () =>
{
    if (!nativeAvailable) throw new TestSkippedException("Native backend is unavailable.");
    var composite = new CompositeArchiveEngine(enginePath);
    var f4Dir = Path.Combine(root, "f4-formats");
    Directory.CreateDirectory(f4Dir);

    var sampleText = "OpenUnpack F4 Multi-Backend Verification Payload!\n";
    var sampleBytes = Encoding.UTF8.GetBytes(sampleText);
    var sampleFile = Path.Combine(f4Dir, "payload.txt");
    File.WriteAllBytes(sampleFile, sampleBytes);

    // 1. WIM archive
    var wimPath = Path.Combine(f4Dir, "sample.wim");
    using (var p = Process.Start(new ProcessStartInfo(cliPath)
    {
        WorkingDirectory = f4Dir,
        UseShellExecute = false,
        CreateNoWindow = true,
        Arguments = $"a -twim -y -bso0 \"{wimPath}\" \"payload.txt\""
    })!)
    {
        await p.WaitForExitAsync();
        Assert(p.ExitCode == 0, "7z create WIM succeeded");
    }
    var wimInspect = await composite.InspectAsync(wimPath, null, CancellationToken.None);
    Assert(wimInspect.Entries.Any(e => e.Path == "payload.txt"), "WIM inspect lists payload.txt");
    var wimOut = Path.Combine(f4Dir, "out-wim");
    var wimManifest = await composite.ExtractWithManifestAsync(new ExtractionRequest(wimPath, wimOut, null), null, CancellationToken.None);
    Assert(wimManifest.Files.Count == 1 && File.ReadAllText(Path.Combine(wimOut, "payload.txt")) == sampleText, "WIM extraction matches payload");

    // 2. Zstandard (.zst) archive (RFC 8878 frame with uncompressed block)
    var zstPath = Path.Combine(f4Dir, "payload.txt.zst");
    using (var fs = File.Create(zstPath))
    {
        fs.Write([0x28, 0xB5, 0x2F, 0xFD]);
        fs.WriteByte(0x20);
        fs.WriteByte((byte)sampleBytes.Length);
        uint blockHeader = 1u | ((uint)sampleBytes.Length << 3);
        fs.Write([(byte)(blockHeader & 0xFF), (byte)((blockHeader >> 8) & 0xFF), (byte)((blockHeader >> 16) & 0xFF)]);
        fs.Write(sampleBytes);
    }
    var zstInspect = await composite.InspectAsync(zstPath, null, CancellationToken.None);
    Assert(zstInspect.Entries.Count == 1, "ZSTD inspect lists single stream entry");
    var zstOut = Path.Combine(f4Dir, "out-zst");
    var zstManifest = await composite.ExtractWithManifestAsync(new ExtractionRequest(zstPath, zstOut, null), null, CancellationToken.None);
    Assert(zstManifest.Files.Count == 1 && File.ReadAllText(Path.Combine(zstOut, zstManifest.Files[0].RelativePath)) == sampleText, "ZSTD extraction matches payload");

    // 3. LZ4 (.lz4) frame archive via Lz4Engine
    var lz4Path = Path.Combine(f4Dir, "payload.txt.lz4");
    using (var fs = File.Create(lz4Path))
    {
        // LZ4 Frame header: Magic (4B) + FLG (0x64: v1, block indep, content checksum) + BD (0x40: 64KB) + HC (1B)
        byte flg = 0x64;
        byte bd = 0x40;
        byte hc = (byte)((XxHash32.Compute([flg, bd], 0) >> 8) & 0xFF);
        Span<byte> hdr = [0x04, 0x22, 0x4D, 0x18, flg, bd, hc];
        fs.Write(hdr);
        // Uncompressed block (0x80000000 | length)
        Span<byte> bSize = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(bSize, 0x80000000u | (uint)sampleBytes.Length);
        fs.Write(bSize);
        fs.Write(sampleBytes);
        // EndMark (0x00000000)
        Span<byte> endMark = [0, 0, 0, 0];
        fs.Write(endMark);
        // Content Checksum (4B)
        Span<byte> contentCrc = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(contentCrc, XxHash32.Compute(sampleBytes, 0));
        fs.Write(contentCrc);
    }
    var lz4Inspect = await composite.InspectAsync(lz4Path, null, CancellationToken.None);
    Assert(lz4Inspect.Entries.Count == 1 && lz4Inspect.Entries[0].Path == "payload.txt", "LZ4 inspect strips .lz4 suffix");
    var lz4Probe = await composite.ProbeAsync(lz4Path, [], CancellationToken.None);
    Assert(lz4Probe.Status == PasswordProbeStatus.NoPasswordNeeded, "LZ4 probe returns NoPasswordNeeded");
    var lz4Out = Path.Combine(f4Dir, "out-lz4");
    var lz4Manifest = await composite.ExtractWithManifestAsync(new ExtractionRequest(lz4Path, lz4Out, null), null, CancellationToken.None);
    Assert(lz4Manifest.Files.Count == 1 && File.ReadAllText(Path.Combine(lz4Out, "payload.txt")) == sampleText, "LZ4 extraction matches payload");

    // SEC-01 & SEC-02: LZ4 decompression bomb quota enforcement and unsafe filename rejection
    var lz4BombOut = Path.Combine(f4Dir, "out-lz4-bomb");
    await ThrowsArchive(
        () => composite.ExtractWithManifestAsync(
            new ExtractionRequest(lz4Path, lz4BombOut, null, MaxWrittenBytes: 4),
            null,
            CancellationToken.None),
        ArchiveFailure.ResourceLimit);

    var markerLz4Path = Path.Combine(f4Dir, ".openunpack-owner.json.lz4");
    File.Copy(lz4Path, markerLz4Path, true);
    var lz4UnsafeOut = Path.Combine(f4Dir, "out-lz4-unsafe");
    await ThrowsArchive(
        () => composite.ExtractWithManifestAsync(
            new ExtractionRequest(markerLz4Path, lz4UnsafeOut, null),
            null,
            CancellationToken.None),
        ArchiveFailure.UnsafePath);

    // 4. Verified SFX (.sfx.exe) archive
    var sfxPath = Path.Combine(f4Dir, "bundle.sfx.exe");
    var stub = new byte[128];
    stub[0] = (byte)'M'; stub[1] = (byte)'Z';
    File.WriteAllBytes(sfxPath, stub.Concat(File.ReadAllBytes(zipPath)).ToArray());
    var sfxInspect = await composite.InspectAsync(sfxPath, null, CancellationToken.None);
    Assert(sfxInspect.Entries.Count == 2, "SFX inspect reads embedded ZIP entries");
    var sfxOut = Path.Combine(f4Dir, "out-sfx");
    var sfxManifest = await composite.ExtractWithManifestAsync(new ExtractionRequest(sfxPath, sfxOut, null), null, CancellationToken.None);
    Assert(sfxManifest.Files.Count == 2, "SFX extraction delivers embedded files");
});

await Check("large Worker manifest is streamed, hashed and atomically published", async () =>
{
    var destination = Path.Combine(root, "manifest-producer"); Directory.CreateDirectory(destination);
    var files = Enumerable.Range(0, 40_000).Select(i => new OpenUnpack.Worker.FileRecord {
        RelativePath = $"folder-{i:D5}/public-fixture-metadata-only.txt", Size = 1,
        Sha256 = new string('b',64), IsDirectory = false }).ToList();
    var request = new WorkerRequest { ProtocolVersion=2, RequestId="manifest", OperationId="operation", Generation=3,
        DestinationDirectory=destination };
    var method = typeof(WorkerRequest).Assembly.GetType("OpenUnpack.Worker.WorkerOperations")!
        .GetMethod("CreateCompletedEventAsync",System.Reflection.BindingFlags.NonPublic|System.Reflection.BindingFlags.Static)!;
    var completed = await (Task<WorkerEvent>)method.Invoke(null,[request,files,40_000UL,40_000UL,"metadata fixture",false,"fixture",CancellationToken.None])!;
    Assert(completed.Files.Count==0 && completed.ManifestPath is not null,"large result must spill");
    Assert(completed.ProtocolVersion==2 && completed.OperationId=="operation" && completed.Generation==3,"manifest operation identity");
    var bytes=File.ReadAllBytes(completed.ManifestPath!);
    Assert(completed.ManifestByteLength==(ulong)bytes.Length,"manifest length");
    Assert(completed.ManifestSha256==Convert.ToHexStringLower(SHA256.HashData(bytes)),"manifest SHA256");
    Assert(Path.GetFileName(completed.ManifestPath)==$".openunpack-{completed.ManifestId}-manifest.json","owned name");
    using var decoded=JsonDocument.Parse(bytes);
    Assert(decoded.RootElement.GetArrayLength()==40_000,"complete JSON count");
    Assert(decoded.RootElement[39_999].GetProperty("relativePath").GetString()==files[39_999].RelativePath,"last record retained");
    Assert(Directory.GetFiles(destination,"*.tmp").Length==0,"temporary file must be removed");
    using var cancellation=new CancellationTokenSource(); cancellation.Cancel();
    try {
        await (Task<WorkerEvent>)method.Invoke(null,[request,files,0UL,0UL,"cancelled",false,"fixture",cancellation.Token])!;
        throw new Exception("expected cancellation");
    } catch(OperationCanceledException) { }
    Assert(Directory.GetFiles(destination).Length==1 && File.Exists(completed.ManifestPath),"cancel removes only its temporary manifest");
});

await Check("Worker manifest 128MiB resource limit fails without publishing a partial result", async () =>
{
    var destination=Path.Combine(root,"manifest-over-limit"); Directory.CreateDirectory(destination);
    var record=new FileRecord { RelativePath=string.Join('/',Enumerable.Repeat(new string('x',200),7)),Size=1 };
    // Reuse immutable fixture records so this checks the streaming byte cap
    // without constructing a correspondingly large in-memory JSON payload.
    var records=Enumerable.Repeat(record,100_000).ToList();
    var request=new WorkerRequest { ProtocolVersion=2,RequestId="metadata-limit",OperationId="metadata-limit-op",Generation=1,
        Operation="inspect",DestinationDirectory=destination };
    var method=typeof(WorkerRequest).Assembly.GetType("OpenUnpack.Worker.WorkerOperations")!
        .GetMethod("CreateCompletedEventAsync",System.Reflection.BindingFlags.NonPublic|System.Reflection.BindingFlags.Static)!;
    await ThrowsArchive(async () => await (Task<WorkerEvent>)method.Invoke(null,
        [request,records,100_000UL,100_000UL,"metadata only",false,"metadataOnly",CancellationToken.None])!,ArchiveFailure.ResourceLimit);
    Assert(Directory.GetFiles(destination).Length==0,"quota failure removes only its temporary file and publishes no manifest");
});

if (nativeAvailable)
{
    foreach (var fixture in new[] { "complete.zip", "complete.tar", "no-end.tar", "partial-header.tar" })
        foreach (var salvage in new[] { false, true })
            await Check($"RR-01 archive completeness {fixture}, salvage={salvage}: Engine and Worker preserve quality", async () =>
            {
                var directory = Path.Combine(root, $"archive-completeness-{fixture}-{salvage}");
                Directory.CreateDirectory(directory);
                var archive = Path.Combine(directory, fixture);
                var payloads = new[] { "first.txt", "second.txt" }.ToDictionary(name => name,
                    name => Encoding.UTF8.GetBytes("synthetic-audit-data:" + name + "\n"));
                if (fixture.EndsWith(".zip", StringComparison.Ordinal))
                    CreateZip(archive, payloads.Select(pair => (pair.Key, pair.Value)).ToArray());
                else
                {
                    using var buffer = new MemoryStream();
                    using (var tar = new System.Formats.Tar.TarWriter(buffer, System.Formats.Tar.TarEntryFormat.Ustar, leaveOpen: true))
                        foreach (var (name, bytes) in payloads)
                        {
                            using var content = new MemoryStream(bytes);
                            tar.WriteEntry(new System.Formats.Tar.UstarTarEntry(System.Formats.Tar.TarEntryType.RegularFile, name)
                            { DataStream = content });
                        }
                    var tarBytes = buffer.ToArray();
                    File.WriteAllBytes(archive, fixture switch
                    {
                        "no-end.tar" => tarBytes[..2048],
                        "partial-header.tar" => tarBytes[..1100],
                        _ => tarBytes
                    });
                }
                var damaged = !fixture.StartsWith("complete.", StringComparison.Ordinal);
                var sourceHash = SHA256.HashData(File.ReadAllBytes(archive));
                var start = new ProcessStartInfo(cliPath)
                { UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true };
                foreach (var argument in new[] { "t", "-y", archive }) start.ArgumentList.Add(argument);
                using (var cli = Process.Start(start)!)
                {
                    var stdout = cli.StandardOutput.ReadToEndAsync();
                    var stderr = cli.StandardError.ReadToEndAsync();
                    await cli.WaitForExitAsync();
                    File.WriteAllText(Path.Combine(directory, "independent-7z.txt"), await stdout + await stderr);
                    Assert(cli.ExitCode == (damaged ? 2 : 0), "independent 7-Zip fixture integrity matches expected corruption");
                }
                var native = (SevenZipEngine)engine!;
                var engineOutput = Path.Combine(directory, "engine-output");
                var workerOutput = Path.Combine(directory, "worker-output");
                var extraction = new ExtractionRequest(archive, engineOutput, null,
                    AllowedVolumes: [archive], SalvageCorrupted: salvage);
                var request = new WorkerRequest
                {
                    ProtocolVersion = Protocol.VersionV2, RequestId = $"completeness-{fixture}-{salvage}",
                    OperationId = $"completeness-{fixture}-{salvage}-op", Generation = 1,
                    Operation = "extract", ArchivePath = archive, AllowedVolumes = [archive],
                    DestinationDirectory = workerOutput, SalvageCorrupted = salvage,
                    MaxEntries = 10, MaxTotalBytes = 1024 * 1024, MaxWrittenBytes = 1024 * 1024, MinFreeBytes = 0
                };
                var worker = new WorkerOperations(enginePath);
                if (damaged)
                {
                    await ThrowsArchive(() => native.InspectAsync(archive, null, default), ArchiveFailure.Corrupt);
                    await ThrowsArchive(() => native.PreflightAsync(extraction, default), ArchiveFailure.Corrupt);
                    var probe = await native.ProbeAsync(archive, [], default);
                    Assert(probe.Status == PasswordProbeStatus.Corrupt, "damaged archive must not be reported as needing no password");
                    await ThrowsArchive(() => worker.ExecuteAsync(request with { Operation = "inspect", DestinationDirectory = directory }, _ => { }, default), ArchiveFailure.Corrupt);
                    if (!salvage)
                    {
                        await ThrowsArchive(() => native.ExtractWithManifestAsync(extraction, null, default), ArchiveFailure.Corrupt);
                        await ThrowsArchive(() => worker.ExecuteAsync(request, _ => { }, default), ArchiveFailure.Corrupt);
                        Assert(!Directory.Exists(engineOutput) && !Directory.Exists(workerOutput), "standard mode rejects corrupt headers before creating output");
                    }
                }
                if (!damaged || salvage)
                {
                    var manifest = await native.ExtractWithManifestAsync(extraction, null, default);
                    var completed = await worker.ExecuteAsync(request, _ => { }, default);
                    var scope = damaged ? "salvagedWithCrcWarning" : "archiveCrcAndStreamSha256";
                    Assert(manifest.VerificationScope == scope && completed.VerificationScope == scope,
                        "archive-level damage must survive Engine and Worker quality reporting");
                    if (damaged) Assert(completed.Message.Contains("不完整", StringComparison.Ordinal), "salvage message discloses possible incompleteness");
                    var expected = fixture == "partial-header.tar" ? new[] { "first.txt" } : new[] { "first.txt", "second.txt" };
                    Assert(manifest.Files.Select(file => file.RelativePath).Order().SequenceEqual(expected.Order()), "Engine lists exactly the recoverable files");
                    Assert(completed.Files.Select(file => file.RelativePath).Order().SequenceEqual(expected.Order()), "Worker lists exactly the recoverable files");
                    foreach (var name in expected)
                    {
                        Assert(File.ReadAllBytes(Path.Combine(engineOutput, name)).SequenceEqual(payloads[name]), "Engine payload bytes are preserved");
                        Assert(File.ReadAllBytes(Path.Combine(workerOutput, name)).SequenceEqual(payloads[name]), "Worker payload bytes are preserved");
                    }
                }
                Assert(sourceHash.SequenceEqual(SHA256.HashData(File.ReadAllBytes(archive))), "completeness checking preserves the original archive");
            });

    foreach (var order in new[] { "cuv", "cvu", "ucv", "uvc", "vcu", "vuc", "v", "c", "cv", "vc" })
        foreach (var salvage in new[] { false, true })
            await Check($"native ZIP entry failures {order}, salvage={salvage}: Engine and Worker agree", async () =>
            {
                var directory = Path.Combine(root, $"entry-failures-{order}-{salvage}");
                Directory.CreateDirectory(directory);
                var archive = Path.Combine(directory, "fixture.zip");
                var entries = order.Select(kind => kind switch
                {
                    'c' => (Name: "crc.txt", Content: "independently known CRC member\n"u8.ToArray(), Method: (ushort)0, BadCrc: true),
                    'u' => (Name: "unsupported.txt", Content: "independently known unsupported member\n"u8.ToArray(), Method: (ushort)222, BadCrc: false),
                    _ => (Name: "valid.txt", Content: "independently known valid member\n"u8.ToArray(), Method: (ushort)0, BadCrc: false)
                }).ToArray();
                CreateStoredZip(archive, entries);
                await AssertNativeZipFailures(archive, directory, order.Contains('c'), order.Contains('u'));
                var sourceHash = SHA256.HashData(File.ReadAllBytes(archive));
                var sentinel = Path.Combine(directory, "existing-output-sentinel.bin");
                var sentinelBytes = "unrelated output must remain intact"u8.ToArray();
                File.WriteAllBytes(sentinel, sentinelBytes);
                var engineOutput = Path.Combine(directory, "engine-output");
                var workerOutput = Path.Combine(directory, "worker-output");
                var extraction = new ExtractionRequest(archive, engineOutput, null, SalvageCorrupted: salvage);
                var request = new WorkerRequest
                {
                    ProtocolVersion = Protocol.VersionV2, RequestId = $"entry-failures-{order}-{salvage}",
                    OperationId = $"entry-failures-{order}-{salvage}-op", Generation = 1,
                    Operation = "extract", ArchivePath = archive, AllowedVolumes = [archive],
                    DestinationDirectory = workerOutput, SalvageCorrupted = salvage,
                    MaxEntries = 10, MaxTotalBytes = 1024 * 1024, MaxWrittenBytes = 1024 * 1024, MinFreeBytes = 0
                };
                var worker = new WorkerOperations(enginePath);
                var failure = order.Contains('u') ? ArchiveFailure.Unsupported
                    : order.Contains('c') && !salvage ? ArchiveFailure.Corrupt : (ArchiveFailure?)null;
                if (failure is { } expectedFailure)
                {
                    await ThrowsArchive(() => ((SevenZipEngine)engine!).ExtractWithManifestAsync(extraction, null, default), expectedFailure);
                    await ThrowsArchive(() => worker.ExecuteAsync(request, _ => { }, default), expectedFailure);
                }
                else
                {
                    var manifest = await ((SevenZipEngine)engine!).ExtractWithManifestAsync(extraction, null, default);
                    AssertStoredZipOutput(engineOutput, entries, manifest.Files);
                    Assert(manifest.WrittenBytes == entries.Sum(e => (long)e.Content.Length), "Engine reports exact written bytes");
                    var completed = await worker.ExecuteAsync(request, _ => { }, default);
                    Assert(completed.Kind == "completed", "Worker returns a completed manifest for supported content");
                    Assert(completed.CompletedBytes == (ulong)entries.Sum(e => (long)e.Content.Length), "Worker reports exact written bytes");
                    AssertStoredZipOutput(workerOutput, entries, completed.Files.Select(f => new ExtractedFile(
                        f.RelativePath, (long)f.Size, f.Sha256, f.IsDirectory, f.IsCorrupted)).ToArray());
                    Assert(completed.VerificationScope == (order.Contains('c') ? "salvagedWithCrcWarning" : "archiveCrcAndStreamSha256"),
                        "Worker preserves CRC warning only for CRC salvage");
                }
                Assert(sourceHash.SequenceEqual(SHA256.HashData(File.ReadAllBytes(archive))), "source ZIP remains unchanged");
                Assert(File.ReadAllBytes(sentinel).SequenceEqual(sentinelBytes), "unrelated output is preserved");
            });

    async Task AssertNativeZipFailures(string archive, string directory, bool crc, bool unsupported)
    {
        var start = new ProcessStartInfo(cliPath)
        {
            WorkingDirectory = directory, UseShellExecute = false, RedirectStandardOutput = true,
            RedirectStandardError = true, CreateNoWindow = true
        };
        foreach (var argument in new[] { "t", "-bd", "-bso1", "-bse1", archive }) start.ArgumentList.Add(argument);
        using var process = Process.Start(start)!;
        var stdout = process.StandardOutput.ReadToEndAsync();
        var stderr = process.StandardError.ReadToEndAsync();
        await process.WaitForExitAsync();
        var text = (await stdout) + (await stderr);
        File.WriteAllText(Path.Combine(directory, "native-fixture-check.log"), text);
        Assert(process.ExitCode == (crc || unsupported ? 2 : 0), "independent native ZIP test exit code");
        Assert(text.Contains("CRC Failed", StringComparison.OrdinalIgnoreCase) == crc, "independent native test confirms CRC fixture");
        Assert(text.Contains("Unsupported Method", StringComparison.OrdinalIgnoreCase) == unsupported,
            "independent native test confirms unsupported-method fixture");
    }

    await Check("Worker copy accepts extended long paths with a nonzero free-space reserve", async () =>
    {
        var directory = CreateExtendedSpaceFixture("space-copy");
        var source = Path.Combine(directory, "source.zip");
        File.Copy(zipPath, source);
        var expected = File.ReadAllBytes(source);
        var destination = Path.Combine(directory, "缓存输出");
        var request = SpaceTransferRequest("copy", destination, [new CopyFile
        {
            SourcePath = source, RelativePath = "目录/fixture.zip", Size = (ulong)expected.Length
        }]);
        var progress = new List<WorkerEvent>();
        var completed = await new WorkerOperations(enginePath).ExecuteAsync(request, progress.Add, default);
        Assert(completed.Kind == "completed" && completed.CompletedBytes == (ulong)expected.Length,
            "extended copy completes with exact bytes");
        Assert(completed.Files.Single().Sha256 == Convert.ToHexStringLower(SHA256.HashData(expected)),
            "extended copy returns the actual source digest");
        Assert(progress.Count > 0 && progress[^1].CompletedBytes == (ulong)expected.Length,
            "extended copy reports actual completed bytes");
        Assert(File.ReadAllBytes(Path.Combine(destination, "目录", "fixture.zip")).SequenceEqual(expected),
            "extended copy preserves independent source bytes");
    });

    await Check("Worker extraction accepts extended long paths with a nonzero free-space reserve", async () =>
    {
        var directory = CreateExtendedSpaceFixture("space-extract");
        var archive = Path.Combine(directory, "归档.zip");
        File.Copy(zipPath, archive);
        var original = SHA256.HashData(File.ReadAllBytes(archive));
        var destination = Path.Combine(directory, "解压输出");
        var request = new WorkerRequest
        {
            ProtocolVersion = 2, RequestId = "space-extract", OperationId = "space-extract-op", Generation = 1,
            Operation = "extract", ArchivePath = archive, AllowedVolumes = [archive], DestinationDirectory = destination,
            MaxEntries = 2, MaxTotalBytes = 100_000, MaxWrittenBytes = 100_000, MinFreeBytes = 1024 * 1024
        };
        var completed = await new WorkerOperations(enginePath).ExecuteAsync(request, _ => { }, default);
        Assert(completed.Kind == "completed" && completed.Files.Count == 2,
            "extended extraction returns both actual files");
        VerifyZipOutput(archive, destination);
        Assert(original.SequenceEqual(SHA256.HashData(File.ReadAllBytes(archive))), "extended archive is preserved");
    });

    await Check("Worker readback accepts extended long paths with a nonzero configured reserve", async () =>
    {
        var directory = CreateExtendedSpaceFixture("space-readback");
        var destination = Path.Combine(directory, "回读输出");
        Directory.CreateDirectory(destination);
        var source = Path.Combine(destination, "fixture.zip");
        File.Copy(zipPath, source);
        var expected = File.ReadAllBytes(source);
        var digest = Convert.ToHexStringLower(SHA256.HashData(expected));
        var request = SpaceTransferRequest("readback", destination, [new CopyFile
        {
            SourcePath = source, RelativePath = "fixture.zip", Size = (ulong)expected.Length, Sha256 = digest
        }]);
        var completed = await new WorkerOperations(enginePath).ExecuteAsync(request, _ => { }, default);
        Assert(completed.Kind == "completed" && completed.VerificationScope == "targetReadback",
            "extended target readback completes with its real scope");
        Assert(completed.Files.Single().Sha256 == digest && completed.CompletedBytes == (ulong)expected.Length,
            "extended readback verifies all actual bytes");
        Assert(File.ReadAllBytes(source).SequenceEqual(expected), "readback preserves the target file");
    });

    await Check("extended-path free-space reserves still reject copy and extraction as ResourceLimit", async () =>
    {
        var directory = CreateExtendedSpaceFixture("space-over-reserve");
        var source = Path.Combine(directory, "source.zip");
        File.Copy(zipPath, source);
        var length = (ulong)new FileInfo(source).Length;
        var operations = new WorkerOperations(enginePath);
        var copyDestination = Path.Combine(directory, "copy-rejected");
        var copy = SpaceTransferRequest("copy", copyDestination, [new CopyFile
        {
            SourcePath = source, RelativePath = "fixture.zip", Size = length
        }]) with { MinFreeBytes = (ulong)long.MaxValue };
        await ThrowsArchive(() => operations.ExecuteAsync(copy, _ => { }, default), ArchiveFailure.ResourceLimit);
        Assert(!File.Exists(Path.Combine(copyDestination, "fixture.zip")), "reserve rejection writes no copied file");
        var extractDestination = Path.Combine(directory, "extract-rejected");
        var extract = new WorkerRequest
        {
            ProtocolVersion = 2, RequestId = "space-rejected-extract", OperationId = "space-rejected-extract-op", Generation = 1,
            Operation = "extract", ArchivePath = source, AllowedVolumes = [source], DestinationDirectory = extractDestination,
            MaxEntries = 2, MaxTotalBytes = 100_000, MaxWrittenBytes = 100_000, MinFreeBytes = (ulong)long.MaxValue
        };
        await ThrowsArchive(() => operations.ExecuteAsync(extract, _ => { }, default), ArchiveFailure.ResourceLimit);
        Assert(!Directory.Exists(extractDestination) || !Directory.EnumerateFiles(extractDestination, "*", SearchOption.AllDirectories).Any(),
            "reserve rejection writes no extracted file");
    });

    await Check("free-space querying supports the real volume GUID root and preserves its reserve check", async () =>
    {
        var volume = new StringBuilder(128);
        var driveRoot = Path.GetPathRoot(Path.GetFullPath(root))!;
        if (!SpaceProbe.GetVolumeNameForVolumeMountPoint(driveRoot, volume, (uint)volume.Capacity))
            throw new System.ComponentModel.Win32Exception(System.Runtime.InteropServices.Marshal.GetLastWin32Error());
        Assert(volume.ToString().StartsWith(@"\\?\Volume{", StringComparison.OrdinalIgnoreCase), "real volume GUID fixture");
        SevenZipEngine.EnsureFreeSpace(volume.ToString(), 1024 * 1024);
        await ThrowsArchive(() =>
        {
            SevenZipEngine.EnsureFreeSpace(volume.ToString(), long.MaxValue);
            return Task.CompletedTask;
        }, ArchiveFailure.ResourceLimit);
    });

    await Check("inaccessible free-space queries become IoError rather than ProtocolError", () =>
    {
        var unavailable = @"\\?\Volume{" + Guid.NewGuid().ToString() + @"}\";
        try { SevenZipEngine.EnsureFreeSpace(unavailable, 1); }
        catch (IOException ex)
        {
            Assert(WorkerOperations.ErrorCode(ex, new WorkerRequest()) == "IoError", "space-query OS errors are IO errors");
            return Task.CompletedTask;
        }
        throw new Exception("Expected IOException for an unavailable volume");
    });

    string CreateExtendedSpaceFixture(string name)
    {
        var path = Path.GetFullPath(Path.Combine(root, name, new string('a', 90), new string('b', 90)));
        var extended = path.StartsWith(@"\\?\", StringComparison.Ordinal) ? path
            : path.StartsWith(@"\\", StringComparison.Ordinal) ? @"\\?\UNC\" + path[2..] : @"\\?\" + path;
        Assert(extended.Length > 260, "fixture exercises a real extended path beyond MAX_PATH");
        Directory.CreateDirectory(extended);
        return extended;
    }

    static WorkerRequest SpaceTransferRequest(string operation, string destination, List<CopyFile> files) => new()
    {
        ProtocolVersion = 2, RequestId = "space-" + operation, OperationId = "space-" + operation + "-op", Generation = 1,
        Operation = operation, DestinationDirectory = destination, Files = files,
        MaxEntries = 100, MaxTotalBytes = 100_000, MaxWrittenBytes = 100_000, MinFreeBytes = 1024 * 1024
    };

    await Check("native inspect enforces configured count and expanded-byte limits before returning metadata", async () =>
    {
        var total = 32_000L + Encoding.UTF8.GetByteCount("OpenUnpack 中文内容\n");
        var native = (SevenZipEngine)engine!;
        await ThrowsArchive(() => native.InspectAsync(zipPath, null, default, [zipPath], 1, total), ArchiveFailure.ResourceLimit);
        await ThrowsArchive(() => native.InspectAsync(zipPath, null, default, [zipPath], 2, total - 1), ArchiveFailure.ResourceLimit);
        var accepted = await native.InspectAsync(zipPath, null, default, [zipPath], 2, total);
        Assert(accepted.Entries.Count == 2 && accepted.TotalBytes == total, "exact metadata budgets accepted");
        var destination = Path.Combine(root, "inspect-budget"); Directory.CreateDirectory(destination);
        var request = new WorkerRequest { ProtocolVersion=2, RequestId="inspect-budget", OperationId="inspect-budget-op", Generation=1,
            Operation="inspect", ArchivePath=zipPath, AllowedVolumes=[zipPath], DestinationDirectory=destination,
            MaxEntries=2, MaxTotalBytes=(ulong)total };
        var operations = new WorkerOperations(enginePath);
        await ThrowsArchive(() => operations.ExecuteAsync(request with { MaxEntries=1 }, _ => { }, default), ArchiveFailure.ResourceLimit);
        await ThrowsArchive(() => operations.ExecuteAsync(request with { MaxTotalBytes=(ulong)total-1 }, _ => { }, default), ArchiveFailure.ResourceLimit);
        foreach (var invalid in new[] { request with { MaxEntries=0 }, request with { MaxEntries=1_000_001 },
            request with { MaxTotalBytes=ulong.MaxValue }, request with { DestinationDirectory="" },
            request with { AllowedVolumes=[] } })
        {
            try { await operations.ExecuteAsync(invalid, _ => { }, default); throw new Exception("invalid inspect accepted"); }
            catch (Exception ex) when (WorkerOperations.ErrorCode(ex, invalid)=="ProtocolError") { }
        }
        var completed = await operations.ExecuteAsync(request, _ => { }, default);
        Assert(completed.Files.Count==2 && completed.Files.All(f=>f.Sha256.Length==0), "metadata requires no fake output hashes");
        Assert(completed.TotalBytes==(ulong)total && completed.VerificationScope=="metadataOnly", "metadata scope and exact total");
        Assert(Directory.GetFiles(destination).Length==0, "inline inspect writes no output");
    });

    await Check("native inspect streams metadata larger than the 8MiB IPC frame without decoding outputs", async () =>
    {
        const int count=30_000;
        var archivePath=Path.Combine(root,"large-metadata.zip");
        using (var archive=ZipFile.Open(archivePath,ZipArchiveMode.Create))
            for (var i=0;i<count;i++)
            { using var stream=archive.CreateEntry($"folder-{i:D5}/{new string('x',220)}.txt",CompressionLevel.NoCompression).Open(); stream.WriteByte(120); }
        var original=SHA256.HashData(File.ReadAllBytes(archivePath));
        var destination=Path.Combine(root,"large-metadata-spill"); Directory.CreateDirectory(destination);
        var request=new WorkerRequest { ProtocolVersion=2,RequestId="large-inspect",OperationId="large-inspect-op",Generation=9,
            Operation="inspect",ArchivePath=archivePath,AllowedVolumes=[archivePath],DestinationDirectory=destination,
            MaxEntries=count,MaxTotalBytes=count };
        var completed=await new WorkerOperations(enginePath).ExecuteAsync(request,_=>throw new Exception("inspect must not emit extraction progress"),default);
        Assert(completed.Files.Count==0 && completed.ManifestPath is not null,"large metadata spills");
        Assert(completed.ManifestByteLength>Protocol.MaxMessageBytes,"actual metadata JSON exceeds IPC frame");
        Assert(JsonSerializer.SerializeToUtf8Bytes(completed,Protocol.Json).Length<Protocol.MaxMessageBytes,"terminal IPC frame remains bounded");
        Assert(completed.VerificationScope=="metadataOnly" && completed.TotalBytes==count,"large inspect remains metadata only");
        var bytes=File.ReadAllBytes(completed.ManifestPath!);
        Assert(completed.ManifestSha256==Convert.ToHexStringLower(SHA256.HashData(bytes)),"large inspect spill digest");
        var records=JsonSerializer.Deserialize<List<FileRecord>>(bytes,Protocol.Json)!;
        Assert(records.Count==count && records[^1].RelativePath.Contains("folder-29999"),"entire metadata list retained");
        Assert(records.All(f=>f.Size==1 && f.Sha256==""),"size retained without claiming content verification");
        Assert(Directory.GetFiles(destination).Length==1,"no extracted files or temporary spill remnants");
        Assert(original.SequenceEqual(SHA256.HashData(File.ReadAllBytes(archivePath))),"original archive unchanged");
        Console.WriteLine($"Native large inspect: {count} entries, {bytes.Length} metadata bytes, bounded IPC terminal, source SHA256 unchanged.");
    });

    await Check("exclusion patterns consistently use the 512 UTF8-byte boundary", async () =>
    {
        var ascii=new string('a',512);
        var multibyte=new string('中',170)+"ab";
        Assert(Encoding.UTF8.GetByteCount(multibyte)==512,"multibyte boundary fixture");
        foreach(var accepted in new[]{ascii,multibyte})
            Assert(SevenZipEngine.MatchesExclusion(accepted,[accepted]),"512-byte native rule accepted");
        foreach(var rejected in new[]{ascii+"a",multibyte+"c"})
        {
            Assert(!SevenZipEngine.MatchesExclusion(rejected,[rejected]),"513-byte native rule ignored");
            var request=new WorkerRequest { Operation="extract",ArchivePath=zipPath,AllowedVolumes=[zipPath],
                DestinationDirectory=Path.Combine(root,"overlong-rule-"+Guid.NewGuid().ToString("N")),
                MaxEntries=2,MaxTotalBytes=100_000,MaxWrittenBytes=100_000,Exclusions=[rejected] };
            try { await new WorkerOperations(enginePath).ExecuteAsync(request,_=>{},default); throw new Exception("overlong Worker rule accepted"); }
            catch(Exception ex) when(WorkerOperations.ErrorCode(ex,request)=="ProtocolError") { }
            Assert(!Directory.Exists(request.DestinationDirectory),"invalid rule creates no output");
        }
        foreach(var accepted in new[]{ascii,multibyte})
        {
            var request=new WorkerRequest { Operation="extract",ArchivePath=zipPath,AllowedVolumes=[zipPath],
                DestinationDirectory=Path.Combine(root,"boundary-rule-"+Guid.NewGuid().ToString("N")),
                MaxEntries=2,MaxTotalBytes=100_000,MaxWrittenBytes=100_000,Exclusions=[accepted] };
            var completed=await new WorkerOperations(enginePath).ExecuteAsync(request,_=>{},default);
            Assert(completed.Files.Count==2,"512-byte Worker rule accepted");
        }
    });

    await Check("native ZIP extraction and Unicode paths match original bytes", async () =>
    {
        var inspect = await engine!.InspectAsync(zipPath, null, default);
        Assert(inspect.Entries.Count == 2, "ZIP listing");
        var output = Path.Combine(root, "plain-output");
        await engine.ExtractAsync(new(zipPath, output, null), null, default);
        VerifyZipOutput(zipPath, output);
    });

    var excludedPayload = RandomNumberGenerator.GetBytes(4 * 1024 * 1024);
    var retainedPayload = RandomNumberGenerator.GetBytes(1024 * 1024);
    var nearMatchPayload = Encoding.UTF8.GetBytes("kept near exclusion name 中文");
    await Check("native ZIP progress excludes a large file and preserves near-match names", () =>
        VerifyExcludedProgress("large-file", [
            ("skip.bin", excludedPayload), ("skip.bin.keep", nearMatchPayload),
            ("kept.bin", retainedPayload), ("empty.txt", [])
        ], ["SKIP.BIN"], ["skip.bin.keep", "kept.bin", "empty.txt"]));
    await Check("native ZIP progress excludes directory descendants with exact path boundaries", () =>
        VerifyExcludedProgress("directory", [
            ("discard/", []), ("discard/big.bin", excludedPayload),
            ("discard/deep/", []), ("discard/deep/small.txt", nearMatchPayload),
            ("discarded/small.txt", nearMatchPayload), ("discard.txt", nearMatchPayload),
            ("kept.bin", retainedPayload)
        ], ["DISCARD"], ["discarded/small.txt", "discard.txt", "kept.bin"]));
    await Check("native ZIP progress has zero output total when all entries are excluded", () =>
        VerifyExcludedProgress("all-excluded", [
            ("gone/", []), ("gone/big.bin", excludedPayload), ("empty.txt", [])
        ], ["*"], []));
    await Check("native ZIP progress retains empty files with a zero byte total", () =>
        VerifyExcludedProgress("empty-file", [
            ("keep/", []), ("keep/empty.txt", []), ("skip.bin", excludedPayload)
        ], ["skip.bin"], ["keep/", "keep/empty.txt"]));

    await Check("native extraction pre-cancellation creates no output and planning stops on active cancellation", async () =>
    {
        var output = Path.Combine(root, "cancelled-exclusion-output");
        using (var cancellation = new CancellationTokenSource())
        {
            cancellation.Cancel();
            await ThrowsAsync<OperationCanceledException>(() => engine!.ExtractAsync(
                new(zipPath, output, null, Exclusions: ["*"]), null, cancellation.Token));
        }
        Assert(!Directory.Exists(output), "pre-cancelled extraction creates no output directory");

        var entries = Enumerable.Range(0, 100_000).Select(i => i % 2 == 0
            ? new ArchiveEntryInfo($"excluded-{i}", 0, true)
            : new ArchiveEntryInfo($"excluded-{i - 1}/value.bin".Replace('/', Path.DirectorySeparatorChar), 1, false)).ToArray();
        var planner = typeof(SevenZipEngine).GetNestedType("ExtractCallback", System.Reflection.BindingFlags.NonPublic)!
            .GetMethod("PlanOutput", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Static)!;
        // Cancel while the large metadata list is being consumed, without sleeps or timing thresholds.
        foreach (var cancelAtRead in new[] { 20, entries.Length + 20 })
        {
            using var cancellation = new CancellationTokenSource();
            var cancellableEntries = new CancellingArchiveEntries(entries, cancellation, cancelAtRead);
            bool cancelled = false;
            try { planner.Invoke(null, [cancellableEntries, new[] { "excluded-*" }, cancellation.Token, null]); }
            catch (System.Reflection.TargetInvocationException ex) when (ex.InnerException is OperationCanceledException)
            { cancelled = true; }
            Assert(cancelled, "planning honours cancellation while scanning or calculating output");
            Assert(cancellableEntries.ReadCount <= cancelAtRead + 1, "cancelled planning stops consuming the metadata list promptly");
        }
    });

    async Task VerifyExcludedProgress(string caseName, (string Name, byte[] Content)[] entries,
        string[] exclusions, string[] retainedNames)
    {
        var archive = Path.Combine(root, "progress-exclusions-" + caseName + ".zip");
        var output = Path.Combine(root, "progress-exclusions-output-" + caseName);
        CreateZip(archive, entries);
        var sourceHash = SHA256.HashData(File.ReadAllBytes(archive));
        var retained = retainedNames.ToHashSet(StringComparer.Ordinal);
        var expectedBytes = entries.Where(e => retained.Contains(e.Name) && !e.Name.EndsWith('/'))
            .Sum(e => (long)e.Content.Length);
        var progressEvents = new List<ExtractionProgress>();
        // Progress<T> posts asynchronously; this collector observes every native callback before return.
        var manifest = await ((SevenZipEngine)engine!).ExtractWithManifestAsync(
            new(archive, output, null, Exclusions: exclusions),
            new InlineProgress<ExtractionProgress>(progressEvents.Add), default);
        Assert(progressEvents.Count > 0, "native extraction reports progress");
        long previous = 0;
        foreach (var progress in progressEvents)
        {
            Assert(progress.TotalBytes == expectedBytes, "progress total must exclude every omitted output byte");
            Assert(progress.CompletedBytes >= previous && progress.CompletedBytes <= expectedBytes, "bounded monotonic written bytes");
            previous = progress.CompletedBytes;
        }
        Assert(progressEvents[^1].CompletedBytes == expectedBytes, "final progress reaches retained output size");
        Assert(manifest.WrittenBytes == expectedBytes, "manifest written bytes match retained size");
        var actualNames = manifest.Files.Select(f => f.RelativePath.Replace('\\', '/') + (f.IsDirectory ? "/" : ""))
            .ToHashSet(StringComparer.Ordinal);
        Assert(actualNames.SetEquals(retained), "manifest contains exactly the retained entries");
        var actualFiles = Directory.EnumerateFiles(output, "*", SearchOption.AllDirectories)
            .Select(p => Path.GetRelativePath(output, p).Replace('\\', '/')).ToHashSet(StringComparer.Ordinal);
        Assert(actualFiles.SetEquals(retainedNames.Where(n => !n.EndsWith('/'))), "output contains exactly retained files");
        foreach (var entry in entries)
        {
            var outputPath = Path.Combine(output, entry.Name.Replace('/', Path.DirectorySeparatorChar));
            if (entry.Name.EndsWith('/'))
                Assert(Directory.Exists(outputPath) == retained.Contains(entry.Name), "excluded directory creates no output");
            else if (retained.Contains(entry.Name))
            {
                Assert(File.ReadAllBytes(outputPath).SequenceEqual(entry.Content), "retained output preserves original bytes");
                var file = manifest.Files.Single(f => f.RelativePath.Replace('\\', '/') == entry.Name);
                Assert(file.Size == entry.Content.Length && file.Sha256 == Convert.ToHexStringLower(SHA256.HashData(entry.Content)),
                    "retained manifest size and SHA256 match original bytes");
            }
        }
        Assert(sourceHash.SequenceEqual(SHA256.HashData(File.ReadAllBytes(archive))), "source ZIP remains unchanged");
        Console.WriteLine($"Native exclusion progress {caseName}: {progressEvents.Count} samples, {expectedBytes} retained bytes.");
    }

    var fixtureDirectory = Path.Combine(root, "fixtures");
    Directory.CreateDirectory(fixtureDirectory);
    File.WriteAllText(Path.Combine(fixtureDirectory, "中文文件.txt"), "synthetic native archive fixture", new UTF8Encoding(false));
    File.WriteAllBytes(Path.Combine(fixtureDirectory, "data.bin"), RandomNumberGenerator.GetBytes(32_000));
    const string password = "synthetic-native-pwd";
    var encrypted7z = Path.Combine(root, "encrypted.7z");
    var encryptedZip = Path.Combine(root, "encrypted.zip");
    var zipCrypto = Path.Combine(root, "zipcrypto.zip");
    await CreateNativeArchive(encrypted7z, "7z", ["-p" + password, "-mhe=on"]);
    await CreateNativeArchive(encryptedZip, "zip", ["-p" + password, "-mem=AES256"]);
    await CreateNativeArchive(zipCrypto, "zip", ["-p" + password, "-mem=ZipCrypto"]);
    foreach (var archive in new[] { encrypted7z, encryptedZip, zipCrypto })
        await Check($"native encrypted {Path.GetFileName(archive)} wrong/correct password", async () =>
        {
            foreach (var salvage in new[] { false, true })
                await ThrowsArchive(() => engine!.ExtractAsync(new(archive, Path.Combine(root, Guid.NewGuid().ToString("N")), "wrong",
                    SalvageCorrupted: salvage), null, default), ArchiveFailure.WrongPassword);
            var output = Path.Combine(root, Path.GetFileName(archive) + "-out");
            await engine!.ExtractAsync(new(archive, output, password), null, default);
            VerifyDirectory(fixtureDirectory, output);
        });

    var probeEngine = (IPasswordProbe)engine!;
    await Check("memory probe: unencrypted archive returns NoPasswordNeeded", async () =>
    {
        var probeResult = await probeEngine.ProbeAsync(zipPath, ["fake1", "fake2"], default);
        Assert(probeResult.Status == PasswordProbeStatus.NoPasswordNeeded, "probe unencrypted status");
        Assert(probeResult.MatchedPassword is null, "probe unencrypted matched");
    });

    await Check("ordinary native Probe reads metadata without decoding payload and preserves cancellation", async () =>
    {
        var largeProbe = Path.Combine(root, "ordinary-probe-cancel.zip");
        using (var archive = ZipFile.Open(largeProbe, ZipArchiveMode.Create))
        using (var output = archive.CreateEntry("payload.bin", CompressionLevel.NoCompression).Open())
        {
            var chunk = new byte[65536];
            for (var i = 0; i < 128; i++) output.Write(chunk);
        }
        await probeEngine.ProbeAsync(zipPath, Array.Empty<string>(), CancellationToken.None);
        using var process = Process.GetCurrentProcess();
        Assert(ProbeIo.GetProcessIoCounters(process.Handle, out var initial), "read counters available");
        var result = await probeEngine.ProbeAsync(largeProbe, [], default);
        Assert(result.Status == PasswordProbeStatus.NoPasswordNeeded, "reliable plain metadata needs no password");
        Assert(ProbeIo.GetProcessIoCounters(process.Handle, out var current), "read counters available");
        Assert(current.ReadTransferCount - initial.ReadTransferCount < 2UL * 1024 * 1024, "probe must not read the 8MiB payload");
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();
        await ThrowsAsync<OperationCanceledException>(() => probeEngine.ProbeAsync(largeProbe, [], cancellation.Token));
    });

    await Check("memory probe: encrypted 7z and zip matched in candidates", async () =>
    {
        foreach (var archive in new[] { encrypted7z, encryptedZip, zipCrypto })
        {
            var hit = await probeEngine.ProbeAsync(archive, ["wrong1", "wrong2", password, "wrong3"], default);
            Assert(hit.Status == PasswordProbeStatus.Success, "probe hit status for " + Path.GetFileName(archive));
            Assert(hit.MatchedPassword == password, "probe hit password for " + Path.GetFileName(archive));

            var miss = await probeEngine.ProbeAsync(archive, ["wrong1", "wrong2"], default);
            Assert(miss.Status == PasswordProbeStatus.WrongPassword, "probe miss status for " + Path.GetFileName(archive));
            Assert(miss.MatchedPassword is null, "probe miss password for " + Path.GetFileName(archive));

            var empty = await probeEngine.ProbeAsync(archive, [], default);
            Assert(empty.Status == PasswordProbeStatus.WrongPassword, "probe empty status for " + Path.GetFileName(archive));
        }
    });

    async Task<(string Status, string? Password)> Probe(string path, IReadOnlyList<string> candidates)
    {
        var workerResult = await probeEngine.ProbeAsync(path, candidates, default, [path]);
        return (workerResult.Status.ToString(), workerResult.MatchedPassword);
    }

    async Task Extract(string path, string output, string? secret)
    {
        await engine!.ExtractAsync(new(path, output, secret, AllowedVolumes: [path]), null, default);
    }

    async Task ExpectExtractionFailure(string path, string output, string? secret, string expected)
    {
        string? failure = null;
        try { await Extract(path, output, secret); }
        catch (ArchiveException ex) { failure = ex.Failure.ToString(); }
        Assert(failure == expected, "expected " + expected + " extraction failure, got " + (failure ?? "success"));
    }

    await Check("native Engine: unsafe Windows aliases and owner markers are rejected before any entry is written", async () =>
    {
        string[] unsafeNames = [
            ".openunpack-owner.json", ".OPENUNPACK-OWNER.JSON", "nested/.openunpack-owner.json",
            "CLOCK$", "clock$.txt", "CONIN$", "conout$.log",
            "CON .txt", "COM1 .log", "LPT1 .log", "COM¹ .log", "LPT² .log",
        ];
        foreach (var name in unsafeNames)
        {
            var caseId = Guid.NewGuid().ToString("N");
            var archive = Path.Combine(root, $"Engine-unsafe-alias-{caseId}.zip");
            var output = Path.Combine(root, $"Engine-unsafe-alias-output-{caseId}");
            // An implementation that validates only while writing would leak this first file.
            CreateZip(archive, [("safe-first.txt", Encoding.UTF8.GetBytes("safe public fixture")),
                (name, Encoding.UTF8.GetBytes("unsafe public fixture"))]);
            await ExpectExtractionFailure(archive, output, null, "UnsafePath");
            Assert(!Directory.Exists(output), "unsafe archive must be rejected before creating an output directory: " + name);
        }
    });

    await Check("native Engine: GNU relative prefixes and directory entries remain supported", async () =>
    {
        var archive = Path.Combine(root, "Engine-gnu-relative.zip");
        var output = Path.Combine(root, "Engine-gnu-relative-output");
        var content = Encoding.UTF8.GetBytes("GNU relative path fixture 中文");
        CreateZip(archive, [("./", []), ("./kept/", []), ("./kept/value.txt", content),
            ("./CLOCKwork.txt", content), ("./.openunpack-owner.jsonx", content)]);
        await Extract(archive, output, null);
        foreach (var relative in new[] { "kept/value.txt", "CLOCKwork.txt", ".openunpack-owner.jsonx" })
            Assert(File.ReadAllBytes(Path.Combine(output, relative.Replace('/', Path.DirectorySeparatorChar))).SequenceEqual(content),
                "safe relative path must preserve original bytes: " + relative);
    });

    foreach (var encryption in new[] { "AES256", "ZipCrypto" })
        await Check($"Engine mixed {encryption} ZIP probes encrypted member after plain member", async () =>
        {
            var mixedInput = Path.Combine(root, "mixed-input-Engine-" + encryption);
            Directory.CreateDirectory(mixedInput);
            File.WriteAllText(Path.Combine(mixedInput, "a-plain.txt"), "ordinary first member", new UTF8Encoding(false));
            File.WriteAllText(Path.Combine(mixedInput, "b-encrypted.txt"), "encrypted second member", new UTF8Encoding(false));
            var mixed = Path.Combine(root, "mixed-Engine-" + encryption + ".zip");
            await RunSevenZip(mixedInput, ["a", "-tzip", "-mx=0", "-y", "-bso0", "-bse0", mixed, "a-plain.txt"]);
            await RunSevenZip(mixedInput, ["a", "-tzip", "-mx=0", "-y", "-bso0", "-bse0", "-mem=" + encryption, "-p" + password, mixed, "b-encrypted.txt"]);
            AssertMixedZipOrder(mixed, encryption == "AES256" ? (ushort)99 : (ushort)0);

            var empty = await Probe(mixed, []);
            var wrong = await Probe(mixed, ["wrong1", "wrong2"]);
            var correct = await Probe(mixed, ["wrong1", password, "wrong2"]);
            Assert(empty.Status == "WrongPassword" && empty.Password is null, "mixed ZIP with no candidates");
            Assert(wrong.Status == "WrongPassword" && wrong.Password is null, "mixed ZIP wrong candidates");
            Assert(correct.Status == "Success" && correct.Password == password, "mixed ZIP correct candidate must be tried");
            await ExpectExtractionFailure(mixed, Path.Combine(root, "mixed-wrong-Engine-" + encryption), "wrong", "WrongPassword");
            var output = Path.Combine(root, "mixed-output-Engine-" + encryption);
            await Extract(mixed, output, password);
            VerifyDirectory(mixedInput, output);
        });

    await Check("Engine byte signatures dispatch disguised ZIP, 7z and RAR without renaming", async () =>
    {
        var plain7z = Path.Combine(root, "disguised-source-Engine.7z");
        await CreateNativeArchive(plain7z, "7z", []);
        var archives = new[]
        {
            zipPath, plain7z,
            Path.Combine(repository, "tests", "fixtures", "upstream-archives", "Rar.rar"),
            Path.Combine(repository, "tests", "fixtures", "upstream-archives", "Rar5.rar")
        };
        for (var index = 0; index < archives.Length; index++)
        {
            var original = archives[index];
            Assert(File.Exists(original), "required real archive fixture missing");
            var expected = Path.Combine(root, $"signature-expected-Engine-{index}");
            await Extract(original, expected, null);
            foreach (var suffix in new[] { ".bin", ".mp4", ".zip", ".7z" })
            {
                var disguised = Path.Combine(root, $"signature-Engine-{index}{suffix}");
                File.Copy(original, disguised);
                var originalHash = SHA256.HashData(File.ReadAllBytes(original));
                var probe = await Probe(disguised, ["wrong"]);
                Assert(probe.Status == "NoPasswordNeeded", "ordinary disguised archive probe");
                var output = Path.Combine(root, $"signature-output-Engine-{index}-{suffix[1..]}");
                await Extract(disguised, output, null);
                VerifyDirectory(expected, output);
                Assert(originalHash.SequenceEqual(SHA256.HashData(File.ReadAllBytes(disguised))), "disguised source bytes changed");
            }
        }
    });

    await Check("Engine unknown TAR encryption properties preserve normal extraction fallback", async () =>
    {
        var tar = Path.Combine(root, "unknown-encryption-Engine.tar");
        await CreateNativeArchive(tar, "tar", []);
        var probe = await Probe(tar, ["wrong"]);
        Assert(probe.Status == "Failed" && probe.Password is null, "unknown encryption must not claim NoPasswordNeeded");
        var output = Path.Combine(root, "unknown-encryption-output-Engine");
        await Extract(tar, output, null);
        VerifyDirectory(fixtureDirectory, output);
        foreach (var format in new[] { System.Formats.Tar.TarEntryFormat.Ustar, System.Formats.Tar.TarEntryFormat.V7 })
        {
            var mzTar = Path.Combine(root, $"mz-filename-Engine-{format}.bin");
            var original = "valid TAR filename begins with MZ"u8.ToArray();
            using (var file = File.Create(mzTar))
            using (var writer = new System.Formats.Tar.TarWriter(file, format, leaveOpen: false))
            {
                System.Formats.Tar.TarEntry entry = format == System.Formats.Tar.TarEntryFormat.Ustar
                    ? new System.Formats.Tar.UstarTarEntry(System.Formats.Tar.TarEntryType.RegularFile, "MZ-file.txt")
                    : new System.Formats.Tar.V7TarEntry(System.Formats.Tar.TarEntryType.V7RegularFile, "MZ-file.txt");
                entry.DataStream = new MemoryStream(original);
                writer.WriteEntry(entry);
            }
            Assert(File.ReadAllBytes(mzTar).AsSpan(0, 2).SequenceEqual("MZ"u8), "fixture really begins with filename MZ");
            var mzOutput = Path.Combine(root, $"mz-filename-out-Engine-{format}");
            await Extract(mzTar, mzOutput, null);
            Assert(File.ReadAllBytes(Path.Combine(mzOutput, "MZ-file.txt")).SequenceEqual(original), "TAR payload preserved despite MZ filename");
        }
    });

    await Check("Engine SFX, executable suffix and unsupported Zstandard fail closed", async () =>
    {
        var disguisedExe = Path.Combine(root, "signature-denied-Engine.exe");
        File.Copy(zipPath, disguisedExe);
        var sfx = Path.Combine(root, "sfx-denied-Engine.zip");
        File.WriteAllBytes(sfx, "MZ"u8.ToArray().Concat(new byte[62]).Concat(File.ReadAllBytes(zipPath)).ToArray());
        var zstd = Path.Combine(root, "zstd-denied-Engine.bin");
        File.WriteAllBytes(zstd, [0x28, 0xB5, 0x2F, 0xFD, 0, 0, 0, 0]);
        foreach (var path in new[] { disguisedExe, sfx, zstd })
        {
            var probe = await Probe(path, []);
            Assert(probe.Status == "Unsupported", "excluded format probe should be Unsupported");
            var destination = Path.Combine(root, "denied-output-" + Guid.NewGuid().ToString("N"));
            await ExpectExtractionFailure(path, destination, null, "Unsupported");
            Assert(!Directory.Exists(destination), "excluded format extraction should create no destination");
        }
    });

    await Check("split 7z volumes discovered once and reconstructed", async () =>
    {
        var splitPath = Path.Combine(root, "split.7z");
        await CreateNativeArchive(splitPath, "7z", ["-v8k", "-mx=0", "-p" + password, "-mhe=on"]);
        var first = splitPath + ".001";
        var resolved = VolumeResolver.ResolveSource(first);
        Assert(resolved.Volumes.Count > 1 && resolved.DisplayName == "split.7z", "split grouping");
        var output = Path.Combine(root, "split-out");
        await engine!.ExtractAsync(new(first, output, password), null, default);
        VerifyDirectory(fixtureDirectory, output);
        var missingDir = Path.Combine(root, "missing-volume");
        Directory.CreateDirectory(missingDir);
        File.Copy(first, Path.Combine(missingDir, Path.GetFileName(first)));
        foreach (var salvage in new[] { false, true })
            await ThrowsArchive(() => engine.ExtractAsync(new(Path.Combine(missingDir, Path.GetFileName(first)),
                Path.Combine(root, "missing-out-" + salvage), password, SalvageCorrupted: salvage), null, default),
                ArchiveFailure.MissingVolume, ArchiveFailure.Corrupt);
    });

    await Check("path traversal, ADS and case duplicate entries rejected", async () =>
    {
        foreach (var names in new[] { new[] { "../escape.txt" }, new[] { "C:/absolute.txt" }, new[] { "safe.txt:stream" }, new[] { "same.txt", "SAME.txt" } })
        {
            var archive = Path.Combine(root, "unsafe-" + Guid.NewGuid().ToString("N") + ".zip");
            CreateZip(archive, names.Select(n => (n, Encoding.UTF8.GetBytes("fixture"))).ToArray());
            var output = Path.Combine(root, "unsafe-output-" + Guid.NewGuid().ToString("N"));
            await ThrowsArchive(() => engine!.ExtractAsync(new(archive, output, null), null, default), ArchiveFailure.UnsafePath);
        }
        Assert(!File.Exists(Path.Combine(root, "escape.txt")), "path escaped");
    });

    await Check("resource limits and cancellation reject work", async () =>
    {
        await ThrowsArchive(() => engine!.ExtractAsync(new(zipPath, Path.Combine(root, "over-limit"), null, MaxTotalBytes: 100), null, default), ArchiveFailure.ResourceLimit);
        using var cancelled = new CancellationTokenSource();
        cancelled.Cancel();
        await ThrowsAsync<OperationCanceledException>(() => engine!.ExtractAsync(new(zipPath, Path.Combine(root, "cancelled"), null), null, cancelled.Token));
    });

    await Check("WorkerOperations batch ZIP/7z extraction, real-time progress events and Protocol JSON sync", async () =>
    {
        var workerOps = new WorkerOperations(enginePath);
        var plain7z = Path.Combine(root, "plain-batch.7z");
        await CreateNativeArchive(plain7z, "7z", []);

        var workerHello = new WorkerHello();
        var helloJson = JsonSerializer.Serialize(workerHello, Protocol.Json);
        Assert(helloJson.Contains("\"protocolVersion\":2") && helloJson.Contains("\"backendId\":\"7-Zip\""), "WorkerHello JSON format");

        var batchItems = new (string Path, string? Password, string OpId, Action<string> Verify)[]
        {
            (zipPath, null, "op-zip-1", outDir => VerifyZipOutput(zipPath, outDir)),
            (plain7z, null, "op-7z-2", outDir => VerifyDirectory(fixtureDirectory, outDir)),
            (encrypted7z, password, "op-7z-enc-3", outDir => VerifyDirectory(fixtureDirectory, outDir)),
            (encryptedZip, password, "op-zip-enc-4", outDir => VerifyDirectory(fixtureDirectory, outDir))
        };

        ulong gen = 1;
        foreach (var item in batchItems)
        {
            var dest = Path.Combine(root, "worker-batch-" + item.OpId);
            var req = new WorkerRequest
            {
                ProtocolVersion = Protocol.VersionV1,
                RequestId = "req-" + item.OpId,
                Operation = "extract",
                OperationId = item.OpId,
                Generation = gen,
                ArchivePath = item.Path,
                DestinationDirectory = dest,
                Password = item.Password,
                AllowedVolumes = [item.Path],
                MaxEntries = 10_000,
                MaxTotalBytes = 100 * 1024 * 1024,
                MaxWrittenBytes = 100 * 1024 * 1024,
                MinFreeBytes = 0
            };

            var reqJson = JsonSerializer.Serialize(req, Protocol.Json);
            var roundtripReq = JsonSerializer.Deserialize<WorkerRequest>(reqJson, Protocol.Json)!;
            Assert(roundtripReq.RequestId == req.RequestId && roundtripReq.OperationId == item.OpId && roundtripReq.Generation == gen, "WorkerRequest camelCase roundtrip");

            var progressEvents = new List<WorkerEvent>();
            var completed = await workerOps.ExecuteAsync(roundtripReq, progressEvents.Add, default);

            Assert(progressEvents.Count > 0, $"expected progress events for {item.OpId}");
            ulong prevBytes = 0;
            foreach (var ev in progressEvents)
            {
                Assert(ev.Kind == "progress", "progress event kind");
                Assert(ev.RequestId == req.RequestId && ev.OperationId == item.OpId && ev.Generation == gen, "progress event metadata propagation");
                Assert(ev.TotalBytes > 0 && ev.CompletedBytes <= ev.TotalBytes, "progress byte bounds");
                Assert(ev.CompletedBytes >= prevBytes, "monotonic progress bytes");
                prevBytes = ev.CompletedBytes;
            }
            var lastProgress = progressEvents[^1];
            Assert(lastProgress.CompletedBytes == lastProgress.TotalBytes, "final progress reaches 100%");

            var progressJson = JsonSerializer.Serialize(lastProgress, Protocol.Json);
            Assert(progressJson.Contains("\"kind\":\"progress\"") && progressJson.Contains("\"completedBytes\":"), "progress event camelCase JSON");

            Assert(completed.Kind == "completed", "completed event kind");
            Assert(completed.RequestId == req.RequestId && completed.OperationId == item.OpId && completed.Generation == gen, "completed metadata");
            Assert(completed.CompletedBytes == lastProgress.TotalBytes && completed.TotalBytes == lastProgress.TotalBytes, "completed bytes match total");
            Assert(completed.Files.Count == 2 && completed.Files.All(f => !f.IsDirectory && f.Sha256.Length == 64), "completed manifest files and sha256");
            Assert(completed.PasswordUsed == (item.Password is not null), "passwordUsed flag accuracy");
            item.Verify(dest);
            gen++;
        }
    });

    await Check("Inspect metadata-only reading, entry encryption flags and header encryption rejection", async () =>
    {
        var workerOps = new WorkerOperations(enginePath);

        // 1. Plain ZIP inspect: metadata only, unencrypted entries
        var plainInspect = await engine!.InspectAsync(zipPath, null, default);
        Assert(plainInspect.Entries.Count == 2, "plain ZIP inspect entries count");
        Assert(!plainInspect.EncryptedHeaders, "plain ZIP EncryptedHeaders is false");
        Assert(plainInspect.Entries.All(e => !e.IsEncrypted), "plain ZIP entries are not encrypted");
        Assert(plainInspect.TotalBytes > 0, "plain ZIP total bytes > 0");

        // 2. Encrypted ZIP (data encrypted, headers plain): metadata read succeeds without password!
        var encZipInspect = await engine.InspectAsync(encryptedZip, null, default);
        Assert(encZipInspect.Entries.Count == 2, "encrypted ZIP inspect entries count");
        Assert(!encZipInspect.EncryptedHeaders, "encrypted ZIP EncryptedHeaders is false");
        Assert(encZipInspect.Entries.All(e => e.IsEncrypted), "encrypted ZIP entries IsEncrypted is true");

        // 3. Encrypted 7z (header encrypted): requires password to inspect
        await ThrowsArchive(() => engine.InspectAsync(encrypted7z, null, default), ArchiveFailure.WrongPassword);
        await ThrowsArchive(() => engine.InspectAsync(encrypted7z, "wrong-password", default), ArchiveFailure.WrongPassword);
        var enc7zInspect = await engine.InspectAsync(encrypted7z, password, default);
        Assert(enc7zInspect.Entries.Count == 2, "encrypted 7z inspect entries count with password");
        Assert(enc7zInspect.EncryptedHeaders, "encrypted 7z EncryptedHeaders is true");
        Assert(enc7zInspect.Entries.All(e => e.IsEncrypted), "encrypted 7z entries IsEncrypted is true");

        // 4. WorkerOperations inspect operation
        var plainInspectReq = new WorkerRequest
        {
            ProtocolVersion = Protocol.VersionV2,
            RequestId = "req-inspect-plain",
            Operation = "inspect",
            OperationId = "op-inspect-plain",
            Generation = 1,
            ArchivePath = zipPath, AllowedVolumes = [zipPath], DestinationDirectory = root,
            MaxEntries = 100_000, MaxTotalBytes = 100 * 1024 * 1024
        };
        var plainInspectEv = await workerOps.ExecuteAsync(plainInspectReq, _ => { }, default);
        Assert(plainInspectEv.Kind == "completed", "inspect plain event kind");
        Assert(plainInspectEv.Files.Count == 2, "inspect plain file count");
        Assert(plainInspectEv.Files.All(f => !f.IsEncrypted), "inspect plain file records unencrypted");
        Assert(!plainInspectEv.PasswordUsed, "inspect plain PasswordUsed is false");
        Assert(plainInspectEv.VerificationScope == "metadataOnly", "inspect plain VerificationScope");

        // Encrypted ZIP via WorkerOperations without password
        var encZipInspectReq = new WorkerRequest
        {
            ProtocolVersion = Protocol.VersionV2,
            RequestId = "req-inspect-enczip",
            Operation = "inspect",
            OperationId = "op-inspect-enczip",
            Generation = 2,
            ArchivePath = encryptedZip, AllowedVolumes = [encryptedZip], DestinationDirectory = root,
            MaxEntries = 100_000, MaxTotalBytes = 100 * 1024 * 1024
        };
        var encZipInspectEv = await workerOps.ExecuteAsync(encZipInspectReq, _ => { }, default);
        Assert(encZipInspectEv.Kind == "completed", "inspect enczip event kind");
        Assert(encZipInspectEv.Files.All(f => f.IsEncrypted), "inspect enczip files are encrypted");
        Assert(!encZipInspectEv.PasswordUsed, "inspect enczip PasswordUsed is false");
        var encZipWrongPasswordMetadata = await workerOps.ExecuteAsync(encZipInspectReq with { Password="wrong" }, _=>{}, default);
        Assert(!encZipWrongPasswordMetadata.PasswordUsed && encZipWrongPasswordMetadata.VerificationScope=="metadataOnly",
            "plain encrypted-entry headers cannot claim password validation without decoding file data");

        // Encrypted 7z via WorkerOperations without password -> NeedsPassword
        var enc7zNoPwdReq = new WorkerRequest
        {
            ProtocolVersion = Protocol.VersionV2,
            RequestId = "req-inspect-enc7z-nopwd",
            Operation = "inspect",
            OperationId = "op-inspect-enc7z-nopwd",
            Generation = 3,
            ArchivePath = encrypted7z, AllowedVolumes = [encrypted7z], DestinationDirectory = root,
            MaxEntries = 100_000, MaxTotalBytes = 100 * 1024 * 1024
        };
        try
        {
            await workerOps.ExecuteAsync(enc7zNoPwdReq, _ => { }, default);
            throw new Exception("expected ArchiveException");
        }
        catch (ArchiveException ex)
        {
            Assert(WorkerOperations.ErrorCode(ex, enc7zNoPwdReq) == "NeedsPassword", "enc 7z no pwd error code is NeedsPassword");
        }

        // Encrypted 7z via WorkerOperations with wrong password -> PasswordMismatchOrCorrupt
        var enc7zWrongPwdReq = new WorkerRequest
        {
            ProtocolVersion = Protocol.VersionV2,
            RequestId = "req-inspect-enc7z-wrongpwd",
            Operation = "inspect",
            OperationId = "op-inspect-enc7z-wrongpwd",
            Generation = 4,
            ArchivePath = encrypted7z,
            Password = "wrong", AllowedVolumes = [encrypted7z], DestinationDirectory = root,
            MaxEntries = 100_000, MaxTotalBytes = 100 * 1024 * 1024
        };
        try
        {
            await workerOps.ExecuteAsync(enc7zWrongPwdReq, _ => { }, default);
            throw new Exception("expected ArchiveException");
        }
        catch (ArchiveException ex)
        {
            Assert(WorkerOperations.ErrorCode(ex, enc7zWrongPwdReq) == "PasswordMismatchOrCorrupt", "enc 7z wrong pwd error code is PasswordMismatchOrCorrupt");
        }

        // Encrypted 7z via WorkerOperations with correct password -> Completed
        var enc7zCorrectPwdReq = new WorkerRequest
        {
            ProtocolVersion = Protocol.VersionV2,
            RequestId = "req-inspect-enc7z-correct",
            Operation = "inspect",
            OperationId = "op-inspect-enc7z-correct",
            Generation = 5,
            ArchivePath = encrypted7z,
            Password = password, AllowedVolumes = [encrypted7z], DestinationDirectory = root,
            MaxEntries = 100_000, MaxTotalBytes = 100 * 1024 * 1024
        };
        var enc7zCorrectEv = await workerOps.ExecuteAsync(enc7zCorrectPwdReq, _ => { }, default);
        Assert(enc7zCorrectEv.Kind == "completed", "inspect enc7z correct event kind");
        Assert(enc7zCorrectEv.Files.Count == 2, "inspect enc7z correct file count");
        Assert(enc7zCorrectEv.Files.All(f => f.IsEncrypted), "inspect enc7z correct files encrypted");
        Assert(enc7zCorrectEv.PasswordUsed, "inspect enc7z correct PasswordUsed is true");

        // Protocol JSON roundtrip test
        var json = JsonSerializer.Serialize(enc7zCorrectEv.Files[0], Protocol.Json);
        Assert(json.Contains("\"isEncrypted\":true"), "FileRecord JSON contains isEncrypted");
        var roundtrip = JsonSerializer.Deserialize<FileRecord>(json, Protocol.Json)!;
        Assert(roundtrip.IsEncrypted, "FileRecord deserialized isEncrypted is true");
    });

    await Check("Worker Sandbox: token privilege stripping removes non-essential privileges", async () =>
    {
        if (OperatingSystem.IsWindows())
        {
            var initialCount = WorkerSandbox.GetTokenPrivilegeCount();
            Assert(initialCount >= 0, "token privilege query succeeds");
            var stripped = WorkerSandbox.StripTokenPrivileges();
            var remainingCount = WorkerSandbox.GetTokenPrivilegeCount();
            Assert(remainingCount <= 1, $"stripped token has at most 1 privilege remaining (actual: {remainingCount})");
        }
        await Task.CompletedTask;
    });

    await Check("Worker Sandbox: WriteScopeGuard restricts writes to allowed root and blocks outside writes", async () =>
    {
        var workspace = Path.Combine(root, "sandbox-test-workspace");
        Directory.CreateDirectory(workspace);
        var outside = Path.Combine(root, "sandbox-test-outside");
        Directory.CreateDirectory(outside);

        using (WriteScopeGuard.Enter(allowedRoots: [workspace]))
        {
            // Inside allowed root: succeeds
            WriteScopeGuard.DemandWriteAccess(workspace);
            WriteScopeGuard.DemandWriteAccess(Path.Combine(workspace, "sub", "extracted.txt"));

            // Outside allowed root: throws UnsafePath
            await ThrowsArchive(async () =>
            {
                WriteScopeGuard.DemandWriteAccess(Path.Combine(outside, "escaped.txt"));
                await Task.CompletedTask;
            }, ArchiveFailure.UnsafePath);

            // Null or whitespace target: throws UnsafePath
            await ThrowsArchive(async () =>
            {
                WriteScopeGuard.DemandWriteAccess("   ");
                await Task.CompletedTask;
            }, ArchiveFailure.UnsafePath);
        }
        await Task.CompletedTask;
    });

    await Check("Worker Sandbox: WriteScopeGuard protects source archive and system paths", async () =>
    {
        var workspace = Path.Combine(root, "sandbox-forbidden-workspace");
        Directory.CreateDirectory(workspace);
        var archiveFile = Path.Combine(workspace, "input.zip");
        File.WriteAllText(archiveFile, "test archive");

        using (WriteScopeGuard.Enter(allowedRoots: [workspace], forbiddenPaths: [archiveFile]))
        {
            // Writing to legitimate extracted file inside workspace: succeeds
            WriteScopeGuard.DemandWriteAccess(Path.Combine(workspace, "valid.txt"));

            // Attempting to overwrite source archive in workspace: throws UnsafePath
            await ThrowsArchive(async () =>
            {
                WriteScopeGuard.DemandWriteAccess(archiveFile);
                await Task.CompletedTask;
            }, ArchiveFailure.UnsafePath);

            // Attempting to write to system folder: throws UnsafePath
            var winDir = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
            if (!string.IsNullOrEmpty(winDir))
            {
                await ThrowsArchive(async () =>
                {
                    WriteScopeGuard.DemandWriteAccess(Path.Combine(winDir, "System32", "payload.dll"));
                    await Task.CompletedTask;
                }, ArchiveFailure.UnsafePath);
            }
        }
        await Task.CompletedTask;
    });

    await Check("Worker Sandbox: ReadOnly scope blocks all writes except manifest spill files", async () =>
    {
        var workspace = Path.Combine(root, "sandbox-readonly-workspace");
        Directory.CreateDirectory(workspace);

        using (WriteScopeGuard.EnterReadOnly(allowedManifestDirectories: [workspace]))
        {
            // Ordinary file write in workspace: blocked in read-only mode
            await ThrowsArchive(async () =>
            {
                WriteScopeGuard.DemandWriteAccess(Path.Combine(workspace, "normal.txt"));
                await Task.CompletedTask;
            }, ArchiveFailure.UnsafePath);

            // Manifest spill files in workspace: permitted
            var manifestName = $".openunpack-{Guid.NewGuid():N}-manifest.json";
            WriteScopeGuard.DemandWriteAccess(Path.Combine(workspace, manifestName));
            WriteScopeGuard.DemandWriteAccess(Path.Combine(workspace, manifestName + ".tmp"));

            // Manifest spill files outside allowed workspace: blocked
            var outside = Path.Combine(root, "sandbox-outside");
            await ThrowsArchive(async () =>
            {
                WriteScopeGuard.DemandWriteAccess(Path.Combine(outside, manifestName));
                await Task.CompletedTask;
            }, ArchiveFailure.UnsafePath);
        }
        await Task.CompletedTask;
    });

    await Check("Worker Sandbox: WorkerOperations rejects operations writing outside sandbox boundary", async () =>
    {
        var workspace = Path.Combine(root, "sandbox-worker-ops");
        Directory.CreateDirectory(workspace);
        var sourceArchive = Path.Combine(workspace, "source.zip");
        File.Copy(zipPath, sourceArchive);

        var workerOps = new WorkerOperations(enginePath);

        // Copy operation that tries to overwrite its own source file throws UnsafePath
        var illegalCopyReq = SpaceTransferRequest("copy", workspace, [new CopyFile
        {
            SourcePath = sourceArchive,
            RelativePath = "source.zip",
            Size = (ulong)new FileInfo(sourceArchive).Length
        }]);
        await ThrowsArchive(() => workerOps.ExecuteAsync(illegalCopyReq, _ => { }, default), ArchiveFailure.UnsafePath);

        // Valid extract operation executes cleanly within its scoped workspace
        var validExtractDest = Path.Combine(workspace, "extracted");
        var validExtractReq = new WorkerRequest
        {
            ProtocolVersion = Protocol.VersionV2,
            RequestId = "req-sandbox-valid-extract",
            Operation = "extract",
            OperationId = "op-sandbox-valid-extract",
            Generation = 1,
            ArchivePath = sourceArchive,
            AllowedVolumes = [sourceArchive],
            DestinationDirectory = validExtractDest,
            MaxEntries = 10,
            MaxTotalBytes = 10 * 1024 * 1024,
            MaxWrittenBytes = 10 * 1024 * 1024,
            MinFreeBytes = 0
        };
        var extractEv = await workerOps.ExecuteAsync(validExtractReq, _ => { }, default);
        Assert(extractEv.Kind == "completed", "sandbox extract completed successfully");
        Assert(Directory.Exists(validExtractDest), "extracted output directory exists in workspace");
    });

    await Check("Security S01: password probe enforces entry and decoded byte budgets", async () =>
    {
        var workerOps = new WorkerOperations(enginePath);
        var smallBudgetProbe = new WorkerRequest
        {
            ProtocolVersion = Protocol.VersionV2,
            RequestId = "req-probe-budget",
            Operation = "probe",
            OperationId = "op-probe-budget",
            Generation = 1,
            ArchivePath = zipPath,
            AllowedVolumes = [zipPath],
            Candidates = [],
            MaxEntries = 1,
            MaxTotalBytes = 1,
            MaxWrittenBytes = 1
        };
        await ThrowsArchive(() => workerOps.ExecuteAsync(smallBudgetProbe, _ => { }, default), ArchiveFailure.ResourceLimit);
    });

    await Check("Security S02: Zone.Identifier (MOTW) propagates across extraction and transfer", async () =>
    {
        if (OperatingSystem.IsWindows())
        {
            var motwDir = Path.Combine(root, "motw-test");
            Directory.CreateDirectory(motwDir);
            var motwZip = Path.Combine(motwDir, "internet.zip");
            File.Copy(zipPath, motwZip, true);
            ZoneIdentifier.PropagateZone(motwZip, 3);
            {
                Assert(ZoneIdentifier.DetectZone(motwZip) == 3, "source archive marked with ZoneId=3");
                var motwOut = Path.Combine(motwDir, "extracted");
                var workerOps = new WorkerOperations(enginePath);
                var extractReq = new WorkerRequest
                {
                    ProtocolVersion = Protocol.VersionV2,
                    RequestId = "req-motw-extract",
                    Operation = "extract",
                    OperationId = "op-motw-extract",
                    Generation = 1,
                    ArchivePath = motwZip,
                    AllowedVolumes = [motwZip],
                    DestinationDirectory = motwOut,
                    MaxEntries = 100,
                    MaxTotalBytes = 10 * 1024 * 1024,
                    MaxWrittenBytes = 10 * 1024 * 1024
                };
                var ev = await workerOps.ExecuteAsync(extractReq, _ => { }, default);
                Assert(ev.Kind == "completed" && ev.Files.Count > 0, "MOTW archive extracted");
                foreach (var f in ev.Files.Where(f => !f.IsDirectory))
                {
                    var extractedFile = Path.Combine(motwOut, f.RelativePath);
                    Assert(ZoneIdentifier.DetectZone(extractedFile) == 3, $"extracted file {f.RelativePath} retains ZoneId=3");
                }
            }
        }
        await Task.CompletedTask;
    });

    await Check("Security S07: LZ4 frame rejects bad header checksum, missing EndMark, and bad content checksum", async () =>
    {
        var s07Dir = Path.Combine(root, "s07-lz4");
        Directory.CreateDirectory(s07Dir);
        var composite = new CompositeArchiveEngine(enginePath);
        byte[] payload = Encoding.UTF8.GetBytes("test-payload");

        // 1. Bad header checksum
        var badHcPath = Path.Combine(s07Dir, "bad-hc.txt.lz4");
        using (var fs = File.Create(badHcPath))
        {
            Span<byte> hdr = [0x04, 0x22, 0x4D, 0x18, 0x60, 0x40, 0x00]; // 0x00 is wrong HC
            fs.Write(hdr);
            Span<byte> bSize = stackalloc byte[4];
            BinaryPrimitives.WriteUInt32LittleEndian(bSize, 0x80000000u | (uint)payload.Length);
            fs.Write(bSize);
            fs.Write(payload);
            fs.Write([0, 0, 0, 0]);
        }
        await ThrowsArchive(
            () => composite.ExtractWithManifestAsync(new ExtractionRequest(badHcPath, Path.Combine(s07Dir, "out-bad-hc"), null), null, default),
            ArchiveFailure.Corrupt);

        // 2. Truncated frame without EndMark
        var noEndPath = Path.Combine(s07Dir, "no-endmark.txt.lz4");
        using (var fs = File.Create(noEndPath))
        {
            byte hc = (byte)((XxHash32.Compute([0x60, 0x40], 0) >> 8) & 0xFF);
            fs.Write([0x04, 0x22, 0x4D, 0x18, 0x60, 0x40, hc]);
            Span<byte> bSize = stackalloc byte[4];
            BinaryPrimitives.WriteUInt32LittleEndian(bSize, 0x80000000u | (uint)payload.Length);
            fs.Write(bSize);
            fs.Write(payload);
            // Missing 4-byte EndMark
        }
        await ThrowsArchive(
            () => composite.ExtractWithManifestAsync(new ExtractionRequest(noEndPath, Path.Combine(s07Dir, "out-no-end"), null), null, default),
            ArchiveFailure.Corrupt);

        // 3. Bad content checksum
        var badContentCrcPath = Path.Combine(s07Dir, "bad-ccrc.txt.lz4");
        using (var fs = File.Create(badContentCrcPath))
        {
            byte hc = (byte)((XxHash32.Compute([0x64, 0x40], 0) >> 8) & 0xFF);
            fs.Write([0x04, 0x22, 0x4D, 0x18, 0x64, 0x40, hc]);
            Span<byte> bSize = stackalloc byte[4];
            BinaryPrimitives.WriteUInt32LittleEndian(bSize, 0x80000000u | (uint)payload.Length);
            fs.Write(bSize);
            fs.Write(payload);
            fs.Write([0, 0, 0, 0]); // EndMark
            fs.Write([0xDE, 0xAD, 0xBE, 0xEF]); // Wrong content checksum
        }
        await ThrowsArchive(
            () => composite.ExtractWithManifestAsync(new ExtractionRequest(badContentCrcPath, Path.Combine(s07Dir, "out-bad-ccrc"), null), null, default),
            ArchiveFailure.Corrupt);
    });

    await Check("Security S08: WorkerSandbox enables ProcessExtensionPointDisablePolicy (enum 6)", async () =>
    {
        if (OperatingSystem.IsWindows())
        {
            WorkerSandbox.ApplyProcessMitigations();
            Assert(WorkerSandbox.IsExtensionPointDisablePolicyEnabled(), "ProcessExtensionPointDisablePolicy is active");
            Assert(WorkerSandbox.IsNoRemoteImagesPolicyEnabled(), "ProcessImageLoadPolicy NoRemoteImages is active");
        }
        await Task.CompletedTask;
    });

    var rarFixtureDirectory = Path.Combine(repository, "artifacts", "rar-fixtures");
    if (Directory.Exists(rarFixtureDirectory) && Directory.EnumerateFiles(rarFixtureDirectory, "*.rar").Any())
        foreach (var fixture in Directory.EnumerateFiles(rarFixtureDirectory, "*.rar"))
            await Check("upstream RAR fixture " + Path.GetFileName(fixture), async () =>
            {
                // Upstream fixtures using a password include '_password' in their name; their synthetic test password is 'test'.
                var candidatePassword = Path.GetFileName(fixture).Contains("password", StringComparison.OrdinalIgnoreCase) || Path.GetFileName(fixture).Contains("encrypted", StringComparison.OrdinalIgnoreCase) ? "test" : null;
                var output = Path.Combine(root, "rar-" + Path.GetFileName(fixture));
                var inspection = await engine!.InspectAsync(fixture, candidatePassword, default);
                Assert(inspection.Entries.Count > 0, "RAR listing empty");
                await engine.ExtractAsync(new(fixture, output, candidatePassword), null, default);
                Assert(Directory.EnumerateFiles(output, "*", SearchOption.AllDirectories).Any(), "RAR extraction empty");
            });
    else
        await Check("upstream RAR fixtures unavailable", () => throw new TestSkippedException("RAR fixture directory is unavailable."));

    async Task CreateNativeArchive(string target, string format, string[] switches)
    {
        await RunSevenZip(fixtureDirectory, new[] { "a", "-t" + format, "-y", "-bso0", "-bse0" }.Concat(switches).Concat([target, "."]).ToArray());
    }

    async Task RunSevenZip(string workingDirectory, string[] arguments)
    {
        var info = new ProcessStartInfo(cliPath) { WorkingDirectory = workingDirectory, UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true };
        foreach (var item in arguments) info.ArgumentList.Add(item);
        using var process = Process.Start(info)!;
        var output = process.StandardOutput.ReadToEndAsync();
        var error = process.StandardError.ReadToEndAsync();
        await process.WaitForExitAsync();
        await Task.WhenAll(output, error);
        if (process.ExitCode != 0) throw new Exception("Synthetic fixture creation failed, exit " + process.ExitCode);
    }
}
else
{
    Console.WriteLine("Native tests unavailable: run scripts/setup-engine.ps1 first.");
    failed++;
}

File.WriteAllText(Path.Combine(root, "results.json"), JsonSerializer.Serialize(new { passed, failed, skipped, nativeAvailable, results }, new JsonSerializerOptions { WriteIndented = true }));
Console.WriteLine($"Tests: {passed} passed, {failed} failed, {skipped} skipped. Evidence: {root}");
return failed == 0 ? 0 : 1;

async Task Check(string name, Func<Task> test)
{
    var timer = Stopwatch.StartNew();
    try { await test(); passed++; results.Add(new { name, status = "passed", milliseconds = timer.ElapsedMilliseconds }); Console.WriteLine("PASS " + name); }
    catch (TestSkippedException ex) { skipped++; results.Add(new { name, status = "skipped", reason = ex.Message }); Console.WriteLine("SKIP " + name); }
    catch (Exception ex) { failed++; results.Add(new { name, status = "failed", error = ex.GetType().Name + ": " + ex.Message }); Console.WriteLine("FAIL " + name + " :: " + ex.GetType().Name + ": " + ex.Message); }
}
static void Assert(bool condition, string message) { if (!condition) throw new Exception(message); }
static async Task ThrowsAsync<T>(Func<Task> action) where T : Exception { try { await action(); } catch (T) { return; } throw new Exception("Expected " + typeof(T).Name); }
static async Task ThrowsArchive(Func<Task> action, params ArchiveFailure[] expected)
{
    try { await action(); }
    catch (ArchiveException ex) when (expected.Contains(ex.Failure)) { return; }
    throw new Exception("Expected archive failure " + string.Join('/', expected));
}
static void CreateZip(string path, (string Name, byte[] Content)[] entries)
{
    using var archive = ZipFile.Open(path, ZipArchiveMode.Create);
    foreach (var item in entries) { using var stream = archive.CreateEntry(item.Name).Open(); stream.Write(item.Content); }
}
static void CreateStoredZip(string path, (string Name, byte[] Content, ushort Method, bool BadCrc)[] entries)
{
    using var stream = File.Create(path);
    using var writer = new BinaryWriter(stream, Encoding.UTF8, leaveOpen: true);
    var offsets = new List<uint>();
    foreach (var entry in entries)
    {
        offsets.Add(checked((uint)stream.Position));
        var name = Encoding.UTF8.GetBytes(entry.Name);
        var crc = Crc32(entry.Content) ^ (entry.BadCrc ? 1u : 0);
        writer.Write(0x04034b50u); writer.Write((ushort)20); writer.Write((ushort)0x800); writer.Write(entry.Method);
        writer.Write((ushort)0); writer.Write((ushort)0x21); writer.Write(crc);
        writer.Write((uint)entry.Content.Length); writer.Write((uint)entry.Content.Length);
        writer.Write((ushort)name.Length); writer.Write((ushort)0); writer.Write(name); writer.Write(entry.Content);
    }
    var centralOffset = checked((uint)stream.Position);
    for (var index = 0; index < entries.Length; index++)
    {
        var entry = entries[index];
        var name = Encoding.UTF8.GetBytes(entry.Name);
        var crc = Crc32(entry.Content) ^ (entry.BadCrc ? 1u : 0);
        writer.Write(0x02014b50u); writer.Write((ushort)20); writer.Write((ushort)20);
        writer.Write((ushort)0x800); writer.Write(entry.Method); writer.Write((ushort)0); writer.Write((ushort)0x21);
        writer.Write(crc); writer.Write((uint)entry.Content.Length); writer.Write((uint)entry.Content.Length);
        writer.Write((ushort)name.Length); writer.Write((ushort)0); writer.Write((ushort)0);
        writer.Write((ushort)0); writer.Write((ushort)0); writer.Write(0u); writer.Write(offsets[index]); writer.Write(name);
    }
    var centralSize = checked((uint)stream.Position - centralOffset);
    writer.Write(0x06054b50u); writer.Write((ushort)0); writer.Write((ushort)0);
    writer.Write((ushort)entries.Length); writer.Write((ushort)entries.Length);
    writer.Write(centralSize); writer.Write(centralOffset); writer.Write((ushort)0);

    static uint Crc32(byte[] bytes)
    {
        var crc = uint.MaxValue;
        foreach (var value in bytes)
        {
            crc ^= value;
            for (var bit = 0; bit < 8; bit++) crc = (crc >> 1) ^ ((crc & 1) == 0 ? 0 : 0xedb88320u);
        }
        return ~crc;
    }
}
static void AssertStoredZipOutput(string output, (string Name, byte[] Content, ushort Method, bool BadCrc)[] entries,
    IReadOnlyList<ExtractedFile> files)
{
    Assert(files.Count == entries.Length && files.All(f => !f.IsDirectory), "manifest contains exactly the fixture files");
    var actualNames = Directory.EnumerateFiles(output, "*", SearchOption.AllDirectories)
        .Select(p => Path.GetRelativePath(output, p)).ToHashSet(StringComparer.Ordinal);
    Assert(actualNames.SetEquals(entries.Select(e => e.Name)), "disk output contains exactly the fixture files");
    foreach (var entry in entries)
    {
        var file = files.Single(f => f.RelativePath == entry.Name);
        Assert(file.IsCorrupted == entry.BadCrc, "only CRC-failed members are marked corrupted");
        Assert(file.Size == entry.Content.Length && file.Sha256 == Convert.ToHexStringLower(SHA256.HashData(entry.Content)),
            "manifest sizes and hashes match the independent payload");
        Assert(File.ReadAllBytes(Path.Combine(output, entry.Name)).SequenceEqual(entry.Content), "output matches the independent payload");
    }
}
static void AssertMixedZipOrder(string path, ushort encryptedMethod)
{
    var bytes = File.ReadAllBytes(path);
    var end = bytes.Length - 22;
    while (end >= 0 && BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(end, 4)) != 0x06054b50) end--;
    Assert(end >= 0, "ZIP end record missing");
    Assert(BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(end + 10, 2)) == 2, "mixed ZIP member count");
    var offset = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(end + 16, 4)));
    for (var index = 0; index < 2; index++)
    {
        Assert(BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(offset, 4)) == 0x02014b50, "ZIP central record missing");
        var flags = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(offset + 8, 2));
        var method = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(offset + 10, 2));
        var nameLength = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(offset + 28, 2));
        var extraLength = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(offset + 30, 2));
        var commentLength = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(offset + 32, 2));
        var name = Encoding.UTF8.GetString(bytes, offset + 46, nameLength);
        Assert(name == (index == 0 ? "a-plain.txt" : "b-encrypted.txt"), "mixed ZIP ordinary member must be first");
        Assert(((flags & 1) != 0) == (index == 1), "mixed ZIP encryption flags");
        if (index == 1) Assert(method == encryptedMethod, "mixed ZIP encryption method");
        offset += 46 + nameLength + extraLength + commentLength;
    }
}
static void VerifyZipOutput(string zipPath, string output)
{
    using var archive = ZipFile.OpenRead(zipPath);
    foreach (var entry in archive.Entries)
    {
        using var stream = entry.Open(); using var expected = new MemoryStream(); stream.CopyTo(expected);
        Assert(expected.ToArray().SequenceEqual(File.ReadAllBytes(Path.Combine(output, entry.FullName.Replace('/', Path.DirectorySeparatorChar)))), "ZIP bytes differ");
    }
}
static void VerifyDirectory(string source, string output)
{
    foreach (var path in Directory.EnumerateFiles(source, "*", SearchOption.AllDirectories))
        Assert(File.ReadAllBytes(path).SequenceEqual(File.ReadAllBytes(Path.Combine(output, Path.GetRelativePath(source, path)))), "extracted bytes differ");
}
static string FindRepository()
{
    for (var directory = new DirectoryInfo(Environment.CurrentDirectory); directory is not null; directory = directory.Parent)
        if (File.Exists(Path.Combine(directory.FullName, "OpenUnpack.sln"))) return directory.FullName;
    throw new Exception("Repository not found.");
}
sealed class InlineProgress<T>(Action<T> report) : IProgress<T>
{
    public void Report(T value) => report(value);
}
sealed class TestSkippedException(string message) : Exception(message);
sealed class CancellingArchiveEntries(ArchiveEntryInfo[] entries, CancellationTokenSource cancellation,
    int cancelAtRead) : IReadOnlyList<ArchiveEntryInfo>
{
    public int Count => entries.Length;
    public int ReadCount { get; private set; }
    public ArchiveEntryInfo this[int index]
    {
        get
        {
            ReadCount++;
            if (ReadCount == cancelAtRead) cancellation.Cancel();
            return entries[index];
        }
    }
    public IEnumerator<ArchiveEntryInfo> GetEnumerator()
    {
        for (var index = 0; index < Count; index++) yield return this[index];
    }
    System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
}
static class ProbeIo {
    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    public struct Counters {
        public ulong ReadOperationCount,WriteOperationCount,OtherOperationCount,ReadTransferCount,WriteTransferCount,OtherTransferCount;
    }
    [System.Runtime.InteropServices.DllImport("kernel32.dll",SetLastError=true)]
    [System.Runtime.InteropServices.DefaultDllImportSearchPaths(System.Runtime.InteropServices.DllImportSearchPath.System32)]
    [return:System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.Bool)]
    public static extern bool GetProcessIoCounters(IntPtr process,out Counters counters);
}
static class SpaceProbe
{
    [System.Runtime.InteropServices.DllImport("kernel32.dll", EntryPoint = "GetVolumeNameForVolumeMountPointW",
        CharSet = System.Runtime.InteropServices.CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [System.Runtime.InteropServices.DefaultDllImportSearchPaths(System.Runtime.InteropServices.DllImportSearchPath.System32)]
    [return: System.Runtime.InteropServices.MarshalAs(System.Runtime.InteropServices.UnmanagedType.Bool)]
    public static extern bool GetVolumeNameForVolumeMountPoint(string mountPoint, StringBuilder volumeName, uint bufferLength);
}
