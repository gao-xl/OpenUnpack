using System.Buffers.Binary;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using OpenUnpack.Engine;
using OpenUnpack.Engine.Native;
using OpenUnpack.Worker;

internal static class WorkerBoundaryChecks
{
    public static IEnumerable<(string Name, Func<Task> Test)> CreateCases(string repository, string root)
    {
        string directory = Path.Combine(root, "worker-boundaries");
        Directory.CreateDirectory(directory);
        string dll = Path.Combine(repository, "tools", "7zip", "7z.dll");
        var engine = new Lz4Engine();
        var payload = Encoding.UTF8.GetBytes("safe bounded payload");
        string unknown = Path.Combine(directory, "unknown.txt.lz4");
        string known = Path.Combine(directory, "known.txt.lz4");
        File.WriteAllBytes(unknown, Frame(payload));
        File.WriteAllBytes(known, Frame(payload, (ulong)payload.Length));

        yield return ("LZ4 metadata declares unknown expanded size without decoding payload", async () =>
        {
            var info = await engine.InspectAsync(unknown, null, default);
            Assert(info.Entries.Count == 1 && !info.Entries[0].SizeKnown && info.Entries[0].Size == 0 && !info.TotalBytesKnown);
            var declared = await engine.InspectAsync(known, null, default);
            Assert(declared.TotalBytesKnown && declared.TotalBytes == payload.Length);
            // A header-only file is inspectable, but final extraction must reject truncation.
            var headerOnly = Path.Combine(directory, "header-only.txt.lz4");
            File.WriteAllBytes(headerOnly, Frame(payload)[..7]);
            Assert(!(await engine.InspectAsync(headerOnly, null, default)).TotalBytesKnown);
            await Failure(() => engine.ExtractWithManifestAsync(new(headerOnly, Path.Combine(directory, "truncated"), null), null, default), ArchiveFailure.Corrupt);
        });
        yield return ("Worker metadata-only preserves unknown size and extraction publishes known size", async () =>
        {
            var worker = new WorkerOperations(dll);
            var result = await worker.ExecuteAsync(Request("inspect", unknown, directory), _ => { }, default);
            Assert(result.VerificationScope == "metadataOnly" && !result.TotalBytesKnown && !result.Files.Single().SizeKnown);
            var events = new List<WorkerEvent>();
            var extract = await worker.ExecuteAsync(Request("extract", unknown, Path.Combine(directory, "worker-extract")), events.Add, default);
            Assert(extract.TotalBytesKnown && extract.Files.Single().SizeKnown && extract.Files.Single().Size == (ulong)payload.Length);
            Assert(events.Any(e => e.Kind == "progress" && !e.TotalBytesKnown && e.TotalBytes == 0));
            Assert(extract.VerificationScope == "frameValidationAndStreamSha256");
            Assert(extract.Files.Single().Sha256 == Convert.ToHexStringLower(SHA256.HashData(payload)));
        });
        yield return ("LZ4 entry and declared-size budgets reach the backend", async () =>
        {
            var composite = new CompositeArchiveEngine(dll);
            await Failure(() => composite.InspectAsync(known, null, default, [known], 0, 1000), ArchiveFailure.ResourceLimit);
            await Failure(() => composite.InspectAsync(known, null, default, [known], 1, 1), ArchiveFailure.ResourceLimit);
            await Failure(() => engine.ProbeAsync(known, [], default, [known], 1, 1, 1), ArchiveFailure.ResourceLimit);
            await Failure(() => engine.ExtractWithManifestAsync(new(unknown, Path.Combine(directory, "no-entries"), null, MaxEntries: 0), null, default), ArchiveFailure.ResourceLimit);
            await Failure(() => engine.ExtractWithManifestAsync(new(unknown, Path.Combine(directory, "zero-byte"), null, MaxTotalBytes: 0, MaxWrittenBytes: 0), null, default), ArchiveFailure.ResourceLimit);
            Assert(!File.Exists(Path.Combine(directory, "zero-byte", "unknown.txt")));
            // Valid compressed independent block: four literal bytes, a 48-byte
            // match at offset four, then twelve trailing literal bytes.
            byte[] compressed = [0x04, 0x22, 0x4D, 0x18, 0x60, 0x40, 0x82,
                21, 0, 0, 0, 0x4F, 0x73, 0x61, 0x66, 0x65, 4, 0, 29, 0xC0,
                0x73, 0x61, 0x66, 0x65, 0x73, 0x61, 0x66, 0x65, 0x73, 0x61, 0x66, 0x65,
                0, 0, 0, 0];
            var compressedPath = Path.Combine(directory, "compressed.txt.lz4"); File.WriteAllBytes(compressedPath, compressed);
            Assert(compressed.Length < 60);
            await Failure(() => engine.ExtractWithManifestAsync(new(compressedPath, Path.Combine(directory, "compressed-limit"), null, MaxTotalBytes: 60), null, default), ArchiveFailure.ResourceLimit);
            var compressedOutput = Path.Combine(directory, "compressed-good");
            var decoded = await engine.ExtractWithManifestAsync(new(compressedPath, compressedOutput, null, MaxTotalBytes: 64), null, default);
            Assert(decoded.WrittenBytes == 64 && File.ReadAllText(Path.Combine(compressedOutput, "compressed.txt")) == string.Concat(Enumerable.Repeat("safe", 16)));
        });
        yield return ("LZ4 one-frame policy rejects concatenation and trailing skippable bytes", async () =>
        {
            foreach (var (name, suffix) in new[] { ("trailing", new byte[] { 1 }), ("concat", Frame(payload)),
                ("skippable", new byte[] { 0x50, 0x2A, 0x4D, 0x18, 0, 0, 0, 0 }) })
            {
                var source = Path.Combine(directory, name + ".txt.lz4");
                File.WriteAllBytes(source, Frame(payload).Concat(suffix).ToArray());
                var output = Path.Combine(directory, name);
                await Failure(() => engine.ExtractWithManifestAsync(new(source, output, null), null, default), ArchiveFailure.Unsupported);
                Assert(!File.Exists(Path.Combine(output, name + ".txt")));
            }
        });
        yield return ("LZ4 rejects reserved descriptor and blocks exceeding declared maximum", async () =>
        {
            var badDescriptor = Frame(payload); badDescriptor[5] = 0x30;
            badDescriptor[6] = (byte)(XxHash32.Compute(badDescriptor.AsSpan(4, 2)) >> 8);
            var source = Path.Combine(directory, "reserved.txt.lz4"); File.WriteAllBytes(source, badDescriptor);
            await Failure(() => engine.InspectAsync(source, null, default), ArchiveFailure.Corrupt);
            var oversized = Frame(new byte[65537]);
            File.WriteAllBytes(source, oversized);
            await Failure(() => engine.ExtractWithManifestAsync(new(source, Path.Combine(directory, "oversized"), null), null, default), ArchiveFailure.Corrupt);
        });
        yield return ("LZ4 failed CreateNew and malformed source preserve existing destination", async () =>
        {
            var output = Path.Combine(directory, "existing"); Directory.CreateDirectory(output);
            var target = Path.Combine(output, "unknown.txt"); File.WriteAllText(target, "existing-data");
            await Failure(() => engine.ExtractWithManifestAsync(new(unknown, output, null), null, default), ArchiveFailure.IoError);
            Assert(File.ReadAllText(target) == "existing-data");
            var malformed = Path.Combine(directory, "unknown.txt.bad.lz4"); File.WriteAllBytes(malformed, [0]);
            var second = Path.Combine(output, "unknown.txt.bad"); File.WriteAllText(second, "existing-data");
            await Failure(() => engine.ExtractWithManifestAsync(new(malformed, output, null), null, default), ArchiveFailure.Corrupt);
            Assert(File.ReadAllText(second) == "existing-data");
        });
        yield return ("LZ4 exclusions return no output and source authorization is enforced", async () =>
        {
            var output = Path.Combine(directory, "excluded");
            var manifest = await engine.ExtractWithManifestAsync(new(unknown, output, null, Exclusions: ["*.txt"]), null, default);
            Assert(manifest.Files.Count == 0 && manifest.ExcludedEntries == 1 && !File.Exists(Path.Combine(output, "unknown.txt")));
            await Failure(() => engine.InspectAsync(unknown, null, default, [known], 10, 1000), ArchiveFailure.MissingVolume);
        });
        yield return ("Worker rejects unknown sizes for copy and readback requests", async () =>
        {
            var worker = new WorkerOperations(dll);
            foreach (var operation in new[] { "copy", "readback" })
            {
                var request = Request(operation, unknown, directory) with { Files = [new() { SourcePath = unknown, RelativePath = "unknown.txt.lz4", SizeKnown = false }] };
                bool refused = false;
                try { await worker.ExecuteAsync(request, _ => { }, default); }
                catch (Exception ex) { refused = WorkerOperations.ErrorCode(ex, request) == "ProtocolError"; }
                Assert(refused);
            }
            Assert(JsonSerializer.Deserialize<FileRecord>("{\"relativePath\":\"legacy\"}", Protocol.Json)!.SizeKnown);
        });
        yield return ("Worker Hello advertises both actual backends and native file version", () =>
        {
            Assert(File.Exists(dll));
            var hello = WorkerHost.CreateHello(dll);
            var native = hello.Capabilities.Backends.Single(b => b.BackendId == "7-Zip");
            var managed = hello.Capabilities.Backends.Single(b => b.BackendId == "managed-lz4-frame");
            var version = System.Diagnostics.FileVersionInfo.GetVersionInfo(dll);
            Assert(native.BackendVersion == $"{version.FileMajorPart}.{version.FileMinorPart:D2}");
            Assert(managed.UnknownExpandedSize && !managed.EncryptedArchives && managed.Formats.SequenceEqual(["lz4IndependentSingleFrame"]));
            return Task.CompletedTask;
        });
        yield return ("MOTW bounded source read rejects oversized ADS and preserves the source", () =>
        {
            Assert(OperatingSystem.IsWindows());
            var source = Path.Combine(directory, "oversized-zone.txt"); File.WriteAllText(source, "source-data");
            File.WriteAllBytes(source + ZoneIdentifier.StreamSuffix, new byte[ZoneIdentifier.MaxStreamBytes + 1]);
            bool rejected = false;
            try { ZoneIdentifier.DetectZone(source); } catch (ArchiveException ex) { rejected = ex.Failure == ArchiveFailure.ResourceLimit; }
            Assert(rejected && File.ReadAllText(source) == "source-data");
            var malformed = Path.Combine(directory, "malformed-zone.txt"); File.WriteAllText(malformed, "source-data");
            File.WriteAllText(malformed + ZoneIdentifier.StreamSuffix, "ZoneId=99999");
            Assert(ZoneIdentifier.DetectZone(malformed) == 3);
            return Task.CompletedTask;
        });
        yield return ("MOTW extraction copy and readback chain retains zone with real files", async () =>
        {
            Assert(OperatingSystem.IsWindows());
            ZoneIdentifier.PropagateZone(unknown, 3);
            var worker = new WorkerOperations(dll);
            var extractedDirectory = Path.Combine(directory, "zone-extract");
            var extracted = await worker.ExecuteAsync(Request("extract", unknown, extractedDirectory), _ => { }, default);
            var record = extracted.Files.Single();
            var source = Path.Combine(extractedDirectory, record.RelativePath);
            Assert(ZoneIdentifier.DetectZone(source) == 3);
            var copiedDirectory = Path.Combine(directory, "zone-copy");
            var copy = Request("copy", unknown, copiedDirectory) with { Files = [new() { SourcePath = source, RelativePath = record.RelativePath, Size = record.Size, Sha256 = record.Sha256 }] };
            var copied = await worker.ExecuteAsync(copy, _ => { }, default);
            var target = Path.Combine(copiedDirectory, record.RelativePath);
            Assert(copied.Files.Count == 1 && ZoneIdentifier.DetectZone(target) == 3);
            var readback = copy with { Operation = "readback", Files = [copy.Files.Single() with { SourcePath = target }] };
            Assert((await worker.ExecuteAsync(readback, _ => { }, default)).Files.Single().Sha256 == record.Sha256);
        });
        yield return ("MOTW required propagation failure rejects LZ4 native extraction and copy publication", async () =>
        {
            Assert(OperatingSystem.IsWindows());
            var source = Path.Combine(directory, "zone-fail.txt.lz4"); File.WriteAllBytes(source, Frame(payload));
            ZoneIdentifier.PropagateZone(source, 3);
            var output = Path.Combine(directory, "zone-fail-lz4"); Directory.CreateDirectory(output);
            var target = Path.Combine(output, "zone-fail.txt");
            var blocker = new ImmediateProgress(value =>
            {
                if (value.CompletedBytes > 0) File.SetAttributes(target, FileAttributes.ReadOnly);
            });
            try { await Failure(() => engine.ExtractWithManifestAsync(new(source, output, null), blocker, default), ArchiveFailure.IoError); }
            finally { if (File.Exists(target)) File.SetAttributes(target, FileAttributes.Normal); }
            Assert(File.Exists(source) && ZoneIdentifier.DetectZone(source) == 3);

            var zip = Path.Combine(directory, "zone-fail.zip");
            using (var archive = ZipFile.Open(zip, ZipArchiveMode.Create))
            using (var stream = archive.CreateEntry("native.txt", CompressionLevel.NoCompression).Open()) stream.Write(payload);
            ZoneIdentifier.PropagateZone(zip, 3);
            var nativeOutput = Path.Combine(directory, "zone-fail-native");
            var nativeTarget = Path.Combine(nativeOutput, "native.txt");
            var nativeBlocker = new ImmediateProgress(value =>
            {
                if (value.CompletedBytes > 0) File.SetAttributes(nativeTarget, FileAttributes.ReadOnly);
            });
            try { await Failure(() => new SevenZipEngine(dll).ExtractWithManifestAsync(new(zip, nativeOutput, null), nativeBlocker, default), ArchiveFailure.IoError); }
            finally { if (File.Exists(nativeTarget)) File.SetAttributes(nativeTarget, FileAttributes.Normal); }
            Assert(File.Exists(zip) && ZoneIdentifier.DetectZone(zip) == 3);

            var copiedOutput = Path.Combine(directory, "zone-fail-copy");
            var copiedTarget = Path.Combine(copiedOutput, "copy.txt");
            var copyRequest = Request("copy", source, copiedOutput) with
            {
                Files = [new() { SourcePath = source, RelativePath = "copy.txt", Size = (ulong)new FileInfo(source).Length }]
            };
            try
            {
                await Failure(() => new WorkerOperations(dll).ExecuteAsync(copyRequest, value =>
                { if (value.CompletedBytes > 0) File.SetAttributes(copiedTarget, FileAttributes.ReadOnly); }, default), ArchiveFailure.IoError);
            }
            finally { if (File.Exists(copiedTarget)) File.SetAttributes(copiedTarget, FileAttributes.Normal); }
            Assert(File.Exists(source) && ZoneIdentifier.DetectZone(source) == 3);
        });
        yield return ("Native probe validates complete metadata budgets", async () =>
        {
            Assert(File.Exists(dll));
            var zip = Path.Combine(directory, "whole-budget.zip");
            using (var archive = ZipFile.Open(zip, ZipArchiveMode.Create))
            {
                using (var stream = archive.CreateEntry("first.bin", CompressionLevel.NoCompression).Open()) stream.Write([1]);
                using (var stream = archive.CreateEntry("second.bin", CompressionLevel.NoCompression).Open()) stream.Write(new byte[4096]);
            }
            await Failure(() => new SevenZipEngine(dll).ProbeAsync(zip, [], default, [zip], 10, 2, 2), ArchiveFailure.ResourceLimit);
        });
        yield return ("Sandbox verifies extension-point and remote-image mitigation policies", () =>
        {
            Assert(OperatingSystem.IsWindows());
            WorkerSandbox.ApplyProcessMitigations();
            Assert(WorkerSandbox.IsExtensionPointDisablePolicyEnabled() && WorkerSandbox.IsNoRemoteImagesPolicyEnabled());
            return Task.CompletedTask;
        });
        yield return ("Structured diagnostics are bounded and never serialize exception messages", () =>
        {
            var old = Console.Error;
            using var capture = new StringWriter();
            Console.SetError(capture);
            try
            {
                for (int i = 0; i < SafeDiagnostics.MaxEvents + 2; i++)
                    SafeDiagnostics.Emit("private-path", "secret-password", "sensitive-code", new IOException("private-value"));
            }
            finally { Console.SetError(old); }
            string[] lines = capture.ToString().Split(Environment.NewLine, StringSplitOptions.RemoveEmptyEntries);
            Assert(lines.Length > 0 && lines.Length <= SafeDiagnostics.MaxEvents);
            foreach (var line in lines)
            {
                Assert(Encoding.UTF8.GetByteCount(line) <= SafeDiagnostics.MaxLineBytes && !line.Contains("private") && !line.Contains("secret"));
                using var json = JsonDocument.Parse(line);
                Assert(json.RootElement.GetProperty("kind").GetString() == "openunpackDiagnostic" && json.RootElement.GetProperty("version").GetInt32() == 1);
                Assert(json.RootElement.GetProperty("operation").GetString() == "unknown");
            }
            return Task.CompletedTask;
        });
    }

    private static WorkerRequest Request(string operation, string source, string destination) => new()
    {
        ProtocolVersion = 2, RequestId = "boundary", OperationId = "boundary", Generation = 1,
        Operation = operation, ArchivePath = source, AllowedVolumes = [source], DestinationDirectory = destination,
        MaxEntries = 100, MaxTotalBytes = 1024 * 1024, MaxWrittenBytes = 1024 * 1024
    };

    private static byte[] Frame(byte[] payload, ulong? contentSize = null)
    {
        using var stream = new MemoryStream();
        stream.Write([0x04, 0x22, 0x4D, 0x18]);
        byte[] descriptor = new byte[contentSize.HasValue ? 10 : 2];
        descriptor[0] = contentSize.HasValue ? (byte)0x68 : (byte)0x60; descriptor[1] = 0x40;
        if (contentSize.HasValue) BinaryPrimitives.WriteUInt64LittleEndian(descriptor.AsSpan(2), contentSize.Value);
        stream.Write(descriptor); stream.WriteByte((byte)(XxHash32.Compute(descriptor) >> 8));
        byte[] length = new byte[4]; BinaryPrimitives.WriteUInt32LittleEndian(length, 0x80000000u | (uint)payload.Length);
        stream.Write(length); stream.Write(payload); stream.Write(new byte[4]);
        return stream.ToArray();
    }

    private static void Assert(bool condition) { if (!condition) throw new Exception("Boundary assertion failed."); }
    private sealed class ImmediateProgress(Action<ExtractionProgress> action) : IProgress<ExtractionProgress>
    { public void Report(ExtractionProgress value) => action(value); }
    private static async Task Failure(Func<Task> action, ArchiveFailure expected)
    {
        try { await action(); }
        catch (ArchiveException ex) when (ex.Failure == expected) { return; }
        throw new Exception("Expected boundary failure " + expected);
    }
}
