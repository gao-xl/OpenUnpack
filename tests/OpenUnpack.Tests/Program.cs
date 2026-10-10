using System.Diagnostics;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using OpenUnpack.Core;
using OpenUnpack.Core.Native;

var repository = Path.GetFullPath(args.Length > 0 ? args[0] : FindRepository());
var root = Path.Combine(repository, "artifacts", "test-results", DateTime.UtcNow.ToString("yyyyMMdd-HHmmss") + "-" + Guid.NewGuid().ToString("N")[..8]);
Directory.CreateDirectory(root);
var results = new List<object>();
int passed = 0, failed = 0;
var zipPath = Path.Combine(root, "中文 路径", "普通.zip");
Directory.CreateDirectory(Path.GetDirectoryName(zipPath)!);
CreateZip(zipPath, [("目录/你好.txt", Encoding.UTF8.GetBytes("OpenUnpack 中文内容\n")), ("binary.bin", RandomNumberGenerator.GetBytes(32_000))]);
var enginePath = Path.Combine(repository, "tools", "7zip", "7z.dll");
var cliPath = Path.Combine(repository, "tools", "7zip", "7z.exe");
var nativeAvailable = File.Exists(enginePath) && File.Exists(cliPath);
IArchiveEngine? engine = nativeAvailable ? new SevenZipEngine(enginePath) : null;

await Check("password import preserves whitespace, BOM, groups and deduplicates", () =>
{
    var imported = PasswordImport.ParseText("\uFEFF  spaced  \r\n中文测试\r\n  spaced  \r\n\r\n", "下载");
    Assert(imported.Entries.Count == 2 && imported.DuplicateCount == 1, "import counts");
    Assert(imported.Entries[0].Secret == "  spaced  " && imported.Entries[0].Group == "下载", "whitespace preservation");
    Assert(!imported.Entries[0].Label.Contains("spaced"), "masked automatic labels");
    var tsv = PasswordImport.ParseText("#openunpack-tsv-v1\n资料\t secret \t网站\t说明\n");
    Assert(tsv.Entries[0].Secret == " secret " && tsv.Entries[0].Label == "资料", "TSV metadata");
    Throws<FormatException>(() => PasswordImport.ParseText("#openunpack-tsv-v1\nwrong\tcolumns"));
    return Task.CompletedTask;
});

await Check("DPAPI round-trip, encrypted primary/backup and corrupt vault preservation", () =>
{
    var data = Path.Combine(root, "vault");
    var store = new VaultService(data);
    var snapshot = new VaultSnapshot { Entries = [new() { Label = "test", Secret = "SYNTHETIC-ONLY-口令-928467" }] };
    Assert(store.Load().Entries.Count == 0, "missing vault");
    store.Save(snapshot);
    Assert(store.Load().Entries.Single().Secret == snapshot.Entries[0].Secret, "DPAPI roundtrip");
    store.Save(snapshot);
    foreach (var path in new[] { Path.Combine(data, "vault.dat"), Path.Combine(data, "vault.dat.bak") })
        Assert(!Encoding.UTF8.GetString(File.ReadAllBytes(path)).Contains(snapshot.Entries[0].Secret), "plaintext in disk vault");
    var corrupt = new byte[] { 1, 2, 3, 4 };
    File.WriteAllBytes(Path.Combine(data, "vault.dat"), corrupt);
    Throws<InvalidDataException>(() => store.Load());
    Throws<InvalidDataException>(() => store.Save(new VaultSnapshot()));
    Assert(File.ReadAllBytes(Path.Combine(data, "vault.dat")).SequenceEqual(corrupt), "corrupt vault overwritten");
    return Task.CompletedTask;
});

await Check("encrypted portable backup, wrong passphrase and tamper rejection", () =>
{
    var snapshot = new VaultSnapshot { Entries = [new() { Label = "backup", Secret = "SYNTHETIC-BACKUP-秘密" }] };
    var path = Path.Combine(root, "backup.ouvault");
    VaultService.ExportBackup(snapshot, path, "synthetic-backup-passphrase");
    Assert(!File.ReadAllText(path).Contains(snapshot.Entries[0].Secret), "backup plaintext");
    Assert(VaultService.ImportBackup(path, "synthetic-backup-passphrase").Entries[0].Secret == snapshot.Entries[0].Secret, "backup roundtrip");
    Throws<InvalidDataException>(() => VaultService.ImportBackup(path, "wrong-passphrase"));
    var content = File.ReadAllBytes(path);
    content[content.Length / 2] ^= 1;
    File.WriteAllBytes(path, content);
    Throws<InvalidDataException>(() => VaultService.ImportBackup(path, "synthetic-backup-passphrase"));
    return Task.CompletedTask;
});

await Check("candidate association priority, disabled/group filters and merge", () =>
{
    var a = new PasswordEntry { Label = "a", Secret = "a", Group = "A", SuccessCount = 9 };
    var b = new PasswordEntry { Label = "b", Secret = "b", Group = "A" };
    var snapshot = new VaultSnapshot { Entries = [a, b, new() { Secret = "c", Enabled = false }, new() { Secret = "a", Group = "B" }] };
    VaultService.RecordSuccess(snapshot, b.Id, "fixture-key");
    var candidates = VaultService.GetCandidates(snapshot, "fixture-key", "A");
    Assert(candidates.Count == 2 && candidates[0].Id == b.Id, "association ordering");
    Assert(VaultService.GetCandidates(snapshot, "fixture-key", null).Count == 2, "dedup and disabled filter");
    Assert(PasswordImport.Merge(snapshot, [new() { Secret = "a" }, new() { Secret = "new" }]) == 1, "merge counts");
    Assert(BatchRunner.GetOutputName("foo.part01.rar") == "foo" && BatchRunner.GetOutputName("foo.7z.001") == "foo", "output stems");
    return Task.CompletedTask;
});

if (nativeAvailable)
{
    await Check("native ZIP extraction and Unicode paths match original bytes", async () =>
    {
        var inspect = await engine!.InspectAsync(zipPath, null, default);
        Assert(inspect.Entries.Count == 2, "ZIP listing");
        var output = Path.Combine(root, "plain-output");
        await engine.ExtractAsync(new(zipPath, output, null), null, default);
        VerifyZipOutput(zipPath, output);
    });

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
            await ThrowsArchive(() => engine!.ExtractAsync(new(archive, Path.Combine(root, Guid.NewGuid().ToString("N")), "wrong"), null, default), ArchiveFailure.WrongPassword);
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

    await Check("split 7z volumes discovered once and reconstructed", async () =>
    {
        var splitPath = Path.Combine(root, "split.7z");
        await CreateNativeArchive(splitPath, "7z", ["-v8k", "-mx=0", "-p" + password, "-mhe=on"]);
        var first = splitPath + ".001";
        var discovery = ArchiveScanner.Scan([first, splitPath + ".002", first]);
        Assert(discovery.Archives.Count == 1 && discovery.Archives[0].Volumes.Count > 1, "split grouping");
        var output = Path.Combine(root, "split-out");
        await engine!.ExtractAsync(new(first, output, password), null, default);
        VerifyDirectory(fixtureDirectory, output);
        var missingDir = Path.Combine(root, "missing-volume");
        Directory.CreateDirectory(missingDir);
        File.Copy(first, Path.Combine(missingDir, Path.GetFileName(first)));
        await ThrowsArchive(() => engine.ExtractAsync(new(Path.Combine(missingDir, Path.GetFileName(first)), Path.Combine(root, "missing-out"), password), null, default), ArchiveFailure.MissingVolume, ArchiveFailure.Corrupt);
    });

    await Check("scanner recursion, supported extensions and duplicate paths", () =>
    {
        var scanned = ArchiveScanner.Scan([zipPath, Path.GetDirectoryName(zipPath)!, zipPath]);
        Assert(scanned.Archives.Count == 1, "duplicate input");
        return Task.CompletedTask;
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

    await Check("batch password fallback, remembered association and conflict preservation", async () =>
    {
        var store = new VaultService(Path.Combine(root, "batch-vault"));
        var right = new PasswordEntry { Label = "fixture label", Secret = password };
        var snapshot = new VaultSnapshot { Entries = [new() { Label = "wrong", Secret = "wrong" }, right] };
        var updates = new List<BatchUpdate>();
        var source = ArchiveScanner.Scan([encrypted7z]).Archives.Single();
        var options = new BatchOptions(Path.Combine(root, "batch-out"));
        var runner = new BatchRunner(engine!, store, snapshot);
        var result = await runner.RunAsync([source], options, new CaptureProgress<BatchUpdate>(updates.Add), default);
        Assert(result.Completed == 1 && result.Failed == 0, "password fallback");
        VerifyDirectory(fixtureDirectory, updates.Last().OutputPath!);
        Assert(store.Load().Entries.Single(e => e.Id == right.Id).SuccessCount == 1, "password success persistence");
        var fingerprint = await BatchRunner.FingerprintAsync(source, default);
        Assert(VaultService.GetCandidates(snapshot, fingerprint, null)[0].Id == right.Id, "remembered priority");
        var priorOutput = updates.Last().OutputPath!;
        var second = await runner.RunAsync([source], options, new CaptureProgress<BatchUpdate>(updates.Add), default);
        Assert(second.Completed == 1 && updates.Last().OutputPath != priorOutput, "rename conflict");
        VerifyDirectory(fixtureDirectory, priorOutput);
        var skippedResult = await runner.RunAsync([source], options with { ConflictPolicy = ConflictPolicy.Skip }, null, default);
        Assert(skippedResult.Skipped == 1, "skip existing output");
    });

    await Check("filename password hint extraction and batch auto-discovery", async () =>
    {
        Assert(ArchiveScanner.ExtractPasswordFromFilename("资料包_密码123456.zip") == "123456", "explicit pwd");
        Assert(ArchiveScanner.ExtractPasswordFromFilename("项目[pwd:abc888].7z") == "abc888", "bracket pwd");
        Assert(ArchiveScanner.ExtractPasswordFromFilename("普通文件.zip") == null, "no pwd");

        var hintArchive = Path.Combine(root, $"test[密码_{password}].7z");
        File.Copy(encrypted7z, hintArchive, true);
        var emptySnapshot = new VaultSnapshot { Entries = [] };
        var store = new VaultService(Path.Combine(root, "hint-vault"));
        var runner = new BatchRunner(engine!, store, emptySnapshot);
        var source = ArchiveScanner.Scan([hintArchive]).Archives.Single();
        var updates = new List<BatchUpdate>();
        var result = await runner.RunAsync([source], new(Path.Combine(root, "hint-out")), new CaptureProgress<BatchUpdate>(updates.Add), default);
        Assert(result.Completed == 1 && result.Failed == 0, $"filename hint batch extraction failed: completed={result.Completed}, failed={result.Failed}, lastMessage={updates.LastOrDefault()?.Message}");
        VerifyDirectory(fixtureDirectory, updates.Last().OutputPath!);
    });

    await Check("execution state scope and completion sound", async () =>
    {
        using (var scope = new BatchRunner.ExecutionStateScope())
        {
            Assert(scope is not null, "scope created");
        }
        var store = new VaultService(Path.Combine(root, "sound-vault"));
        var runner = new BatchRunner(engine!, store, new());
        var source = ArchiveScanner.Scan([zipPath]).Archives.Single();
        var result = await runner.RunAsync([source], new(Path.Combine(root, "sound-out"), PlaySoundOnCompletion: true), null, default);
        Assert(result.Completed == 1, "batch completed with sound option");
    });

    await Check("SSD source caching, safe final output, quota and owned cache cleanup", async () =>
    {
        var store = new VaultService(Path.Combine(root, "cache-vault"));
        var snapshot = new VaultSnapshot { Entries = [new() { Label = "test", Secret = password }] };
        var source = ArchiveScanner.Scan([encrypted7z]).Archives.Single();
        var before = SHA256.HashData(File.ReadAllBytes(encrypted7z));
        var options = new BatchOptions(Path.Combine(root, "cache-final"), CacheRoot: Path.Combine(root, "ssd-cache"), MaxCacheBytes: 10 * 1024 * 1024, MinFreeCacheBytes: 0);
        var updates = new List<BatchUpdate>();
        var runner = new BatchRunner(engine!, store, snapshot);
        var result = await runner.RunAsync([source], options, new CaptureProgress<BatchUpdate>(updates.Add), default);
        Assert(result.Completed == 1, "SSD cached job");
        VerifyDirectory(fixtureDirectory, updates.Last().OutputPath!);
        Assert(!Directory.EnumerateDirectories(options.CacheRoot!, ".openunpack-cache-*").Any(), "cache not cleaned");
        Assert(before.SequenceEqual(SHA256.HashData(File.ReadAllBytes(encrypted7z))), "source changed");
        var capacityResult = await runner.RunAsync([source], options with { MinFreeCacheBytes = long.MaxValue }, null, default);
        Assert(capacityResult.Failed == 1 && !Directory.EnumerateDirectories(options.CacheRoot!, ".openunpack-cache-*").Any(), "capacity failure cleanup");
        using var cancel = new CancellationTokenSource();
        var canceller = new CaptureProgress<BatchUpdate>(u => { if (u.Message.Contains("写入暂存")) cancel.Cancel(); });
        var cancelledResult = await runner.RunAsync([source], options, canceller, cancel.Token);
        Assert(cancelledResult.Cancelled == 1 && !Directory.EnumerateDirectories(options.CacheRoot!, ".openunpack-cache-*").Any(), "cached cancellation cleanup");
    });

    await Check("bad archive does not block remaining batch items", async () =>
    {
        var broken = Path.Combine(root, "broken.zip");
        File.WriteAllBytes(broken, [1, 2, 3, 4]);
        var scanned = ArchiveScanner.Scan([broken, zipPath]).Archives;
        var runner = new BatchRunner(engine!, new VaultService(Path.Combine(root, "bad-batch-vault")), new());
        var result = await runner.RunAsync(scanned, new(Path.Combine(root, "bad-batch-out")), null, default);
        Assert(result.Completed == 1 && result.Failed == 1, "per-archive isolation");
    });

    await Check("large archive sparse fingerprinting is sub-millisecond and differentiates modifications", async () =>
    {
        var largeArchive1 = Path.Combine(root, "large-sparse-1.zip");
        var largeArchive2 = Path.Combine(root, "large-sparse-2.zip");
        await using (var fs1 = new FileStream(largeArchive1, FileMode.Create, FileAccess.Write))
        {
            fs1.SetLength(20 * 1024 * 1024);
            fs1.Write(new byte[] { 0x50, 0x4B, 0x03, 0x04, 0x01 });
            fs1.Seek(-5, SeekOrigin.End);
            fs1.Write(new byte[] { 0x50, 0x4B, 0x05, 0x06, 0x01 });
        }
        await using (var fs2 = new FileStream(largeArchive2, FileMode.Create, FileAccess.Write))
        {
            fs2.SetLength(20 * 1024 * 1024);
            fs2.Write(new byte[] { 0x50, 0x4B, 0x03, 0x04, 0x02 });
            fs2.Seek(-5, SeekOrigin.End);
            fs2.Write(new byte[] { 0x50, 0x4B, 0x05, 0x06, 0x02 });
        }

        var sw = Stopwatch.StartNew();
        var fp1 = await BatchRunner.FingerprintAsync(new ArchiveSource(largeArchive1, "large-sparse-1.zip", [largeArchive1]), default);
        var elapsed1 = sw.ElapsedMilliseconds;
        var fp2 = await BatchRunner.FingerprintAsync(new ArchiveSource(largeArchive2, "large-sparse-2.zip", [largeArchive2]), default);
        var fp1Repeat = await BatchRunner.FingerprintAsync(new ArchiveSource(largeArchive1, "large-sparse-1.zip", [largeArchive1]), default);

        Assert(fp1 == fp1Repeat, "deterministic sparse fingerprint");
        Assert(fp1 != fp2, "distinct files produce distinct fingerprints");
        Assert(elapsed1 < 100, $"sparse fingerprint took {elapsed1}ms, expected sub-100ms");
    });

    await Check("intelligent cache route decider handles nesting, space gate, sequential bypass and HDD contention", () =>
    {
        var options = new BatchOptions(
            OutputRoot: @"D:\output",
            CacheRoot: @"C:\ssd-cache",
            MinFreeCacheBytes: 1024L * 1024 * 1024);

        var nestedProbe = new ArchiveProbeSummary(
            TotalSizeBytes: 500L * 1024 * 1024,
            EstimatedExtractedBytes: 1024L * 1024 * 1024,
            EntryCount: 15,
            HasNestedArchives: true,
            IsSourceOnHdd: false,
            IsTargetOnHdd: false,
            IsSamePhysicalDisk: false);
        var r1 = CacheRouteDecider.Decide(nestedProbe, options, @"D:\output", 50L * 1024 * 1024 * 1024);
        Assert(r1.Type == ExtractionRouteType.SsdCache && r1.Score >= 50, "nested archive triggers SsdCache");

        var lowSpaceProbe = new ArchiveProbeSummary(
            TotalSizeBytes: 5L * 1024 * 1024 * 1024,
            EstimatedExtractedBytes: 10L * 1024 * 1024 * 1024,
            EntryCount: 1000,
            HasNestedArchives: true,
            IsSourceOnHdd: true,
            IsTargetOnHdd: true,
            IsSamePhysicalDisk: true);
        var r2 = CacheRouteDecider.Decide(lowSpaceProbe, options, @"D:\output", 2L * 1024 * 1024 * 1024);
        Assert(r2.Type == ExtractionRouteType.Direct && r2.Reason.Contains("不足安全预算"), "low space triggers Direct fallback");

        var hugeSeqProbe = new ArchiveProbeSummary(
            TotalSizeBytes: 20L * 1024 * 1024 * 1024,
            EstimatedExtractedBytes: 20L * 1024 * 1024 * 1024,
            EntryCount: 1,
            HasNestedArchives: false,
            IsSourceOnHdd: true,
            IsTargetOnHdd: false,
            IsSamePhysicalDisk: false);
        var r3 = CacheRouteDecider.Decide(hugeSeqProbe, options, @"E:\output", 50L * 1024 * 1024 * 1024);
        Assert(r3.Type == ExtractionRouteType.Direct && r3.Reason.Contains("纯顺序大块"), "huge sequential triggers Direct bypass");

        var sameHddHugeProbe = new ArchiveProbeSummary(
            TotalSizeBytes: 20L * 1024 * 1024 * 1024,
            EstimatedExtractedBytes: 20L * 1024 * 1024 * 1024,
            EntryCount: 1,
            HasNestedArchives: false,
            IsSourceOnHdd: true,
            IsTargetOnHdd: true,
            IsSamePhysicalDisk: true);
        var r4 = CacheRouteDecider.Decide(sameHddHugeProbe, options, @"D:\output", 50L * 1024 * 1024 * 1024);
        Assert(r4.Type == ExtractionRouteType.SsdCache && r4.Score >= 40, "same HDD overrides sequential bypass");

        var smallFilesProbe = new ArchiveProbeSummary(
            TotalSizeBytes: 200L * 1024 * 1024,
            EstimatedExtractedBytes: 400L * 1024 * 1024,
            EntryCount: 600,
            HasNestedArchives: false,
            IsSourceOnHdd: false,
            IsTargetOnHdd: false,
            IsSamePhysicalDisk: false);
        var r5 = CacheRouteDecider.Decide(smallFilesProbe, options, @"D:\output", 50L * 1024 * 1024 * 1024);
        Assert(r5.Type == ExtractionRouteType.SsdCache && r5.Score >= 35, "small files cluster triggers SsdCache");

        return Task.CompletedTask;
    });

    var rarFixtureDirectory = Path.Combine(repository, "artifacts", "rar-fixtures");
    if (Directory.Exists(rarFixtureDirectory))
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

    async Task CreateNativeArchive(string target, string format, string[] switches)
    {
        var info = new ProcessStartInfo(cliPath) { WorkingDirectory = fixtureDirectory, UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true };
        foreach (var item in new[] { "a", "-t" + format, "-y", "-bso0", "-bse0" }.Concat(switches).Concat([target, "."])) info.ArgumentList.Add(item);
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

File.WriteAllText(Path.Combine(root, "results.json"), JsonSerializer.Serialize(new { passed, failed, nativeAvailable, results }, new JsonSerializerOptions { WriteIndented = true }));
Console.WriteLine($"Tests: {passed} passed, {failed} failed. Evidence: {root}");
return failed == 0 ? 0 : 1;

async Task Check(string name, Func<Task> test)
{
    var timer = Stopwatch.StartNew();
    try { await test(); passed++; results.Add(new { name, status = "passed", milliseconds = timer.ElapsedMilliseconds }); Console.WriteLine("PASS " + name); }
    catch (Exception ex) { failed++; results.Add(new { name, status = "failed", error = ex.GetType().Name + ": " + ex.Message }); Console.WriteLine("FAIL " + name + " :: " + ex.GetType().Name + ": " + ex.Message); }
}
static void Assert(bool condition, string message) { if (!condition) throw new Exception(message); }
static void Throws<T>(Action action) where T : Exception { try { action(); } catch (T) { return; } throw new Exception("Expected " + typeof(T).Name); }
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
sealed class CaptureProgress<T>(Action<T> action) : IProgress<T> { public void Report(T value) => action(value); }
