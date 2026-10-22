using System.Buffers.Binary;
using System.ComponentModel;
using System.Diagnostics;
using System.IO.Compression;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using Microsoft.Win32.SafeHandles;
using OpenUnpack.Engine;
using OpenUnpack.Engine.Native;
using OpenUnpack.Worker;

internal static class PathIdentityChecks
{
    public static IEnumerable<(string Name, Func<Task> Test)> CreateCases(string repository, string root)
    {
        var directory = Path.Combine(root, "path-identity");
        Directory.CreateDirectory(directory);
        yield return ("S06 129-component archive paths fail preflight without creating output", async () =>
        {
            var archive = Path.Combine(directory, "too-deep.zip");
            using (var zip = ZipFile.Open(archive, ZipArchiveMode.Create))
            {
                using var output = zip.CreateEntry(string.Join('/', Enumerable.Repeat("a", 128)) + "/payload.txt").Open();
                output.Write("bounded"u8);
            }
            var destination = Path.Combine(directory, "deep-output");
            try
            {
                await new SevenZipEngine(Path.Combine(repository, "tools", "7zip", "7z.dll"))
                    .ExtractWithManifestAsync(new(archive, destination, null), null, default);
                throw new Exception("129-component archive path was accepted.");
            }
            catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.ResourceLimit) { }
            Assert(!Directory.Exists(destination), "deep path must be rejected before creating any output directory");
            var copyDestination = Path.Combine(directory, "deep-copy-output");
            var copy = new WorkerRequest
            {
                ProtocolVersion = 2, RequestId = "deep-copy", OperationId = "deep-copy", Generation = 1,
                Operation = "copy", DestinationDirectory = copyDestination, MaxEntries = 100,
                Files = [new() { SourcePath = archive, RelativePath = string.Join('/', Enumerable.Repeat("a", 128)) + "/file", Size = 0 }]
            };
            try
            {
                await new WorkerOperations(Path.Combine(repository, "tools", "7zip", "7z.dll")).ExecuteAsync(copy, _ => { }, default);
                throw new Exception("129-component copy path was accepted.");
            }
            catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.ResourceLimit) { }
            Assert(!Directory.Exists(copyDestination), "deep copy path must be rejected before creating an output root");
            Assert(SevenZipEngine.ValidateRelativePath(string.Join('/', Enumerable.Repeat("a", 127)) + "/file.txt").Length > 0,
                "128 components must remain allowed");
            Assert(SevenZipEngine.ValidateRelativePath(string.Concat(Enumerable.Repeat("./", 15_000)) + "safe.txt") == "safe.txt",
                "GNU prefixes must normalize in linear time");
        });
        yield return ("S06 absolute directory depth is bounded before ancestor allocation or creation", () =>
        {
            var destination = Path.Combine(directory, "absolute-depth");
            using var lease = new PathIdentityLease();
            try
            {
                lease.EnsureDirectory(Path.Combine(destination, Path.Combine(Enumerable.Repeat("a", 257).ToArray())), create: true);
                throw new Exception("257-component absolute path was accepted.");
            }
            catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.ResourceLimit) { }
            Assert(!Directory.Exists(destination), "over-depth absolute path must not create a prefix");
            lease.EnsureDirectory(Path.Combine(destination, "legal", "nested"), create: true);
            using var output = lease.CreateFile(Path.Combine(destination, "legal", "nested", "file.txt"));
            output.Stream.Write("bounded"u8); output.Keep();
            return Task.CompletedTask;
        });
        yield return ("S06 directory identities block rename and refuse writes after attribute-only reparse conversion", () =>
        {
            var parent = Path.Combine(directory, "locked-parent");
            var child = Path.Combine(parent, "empty-child");
            var foreign = Path.Combine(directory, "foreign");
            Directory.CreateDirectory(child); Directory.CreateDirectory(foreign);
            File.WriteAllText(Path.Combine(foreign, "marker"), "foreign");
            using (var lease = new PathIdentityLease())
            {
                lease.EnsureDirectory(child);
                AssertBlocked(() => Directory.Move(parent, parent + "-moved"));
                AssertBlocked(() => Directory.Move(child, child + "-moved"));
                // FILE_WRITE_ATTRIBUTES alone is not constrained by data-sharing flags.
                // Probe the actual filesystem's FSCTL authorization rather than assuming
                // that a directory read handle makes every metadata mutation impossible.
                int attributeResult = ConvertToJunction(child, foreign, 0x100);
                Console.WriteLine($"S06 FILE_WRITE_ATTRIBUTES FSCTL_SET_REPARSE_POINT result: {attributeResult}");
                if (attributeResult == 0)
                {
                    try { using var rejected = lease.CreateFile(Path.Combine(child, "redirected.txt")); throw new Exception("Changed junction accepted."); }
                    catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.UnsafePath) { }
                }
                using var output = lease.CreateFile(Path.Combine(parent, "safe-child", "local.txt"));
                output.Stream.Write("local"u8); output.Keep();
            }
            if ((File.GetAttributes(child) & FileAttributes.ReparsePoint) != 0)
                Assert(RemoveJunction(child) == 0, "restore mutated directory before checking release");
            MoveAfterDisposal(parent, parent + "-moved");
            Assert(File.ReadAllText(Path.Combine(parent + "-moved", "safe-child", "local.txt")) == "local", "lock release must restore normal rename");
            Assert(!File.Exists(Path.Combine(foreign, "redirected.txt")) && File.ReadAllText(Path.Combine(foreign, "marker")) == "foreign", "foreign directory must remain unchanged");
            return Task.CompletedTask;
        });

        yield return ("S06 concurrent attribute-only junction conversions never create or mark foreign files", async () =>
        {
            var child = Path.Combine(directory, "reparse-race-child");
            var foreign = Path.Combine(directory, "reparse-race-foreign");
            Directory.CreateDirectory(child); Directory.CreateDirectory(foreign);
            var marker = Path.Combine(foreign, "marker"); File.WriteAllText(marker, "foreign");
            using var lease = new PathIdentityLease(); lease.EnsureDirectory(child);
            // Establish that this filesystem really permits attribute-only conversion and restoration.
            Assert(ConvertToJunction(child, foreign, 0x100) == 0, "attribute-only race setup failed");
            Assert(RemoveJunction(child) == 0, "attribute-only race restoration failed");
            int foreignEvents = 0;
            using var watcher = new FileSystemWatcher(foreign) { NotifyFilter = NotifyFilters.FileName | NotifyFilters.LastWrite };
            watcher.Created += (_, _) => Interlocked.Increment(ref foreignEvents);
            watcher.Changed += (_, _) => Interlocked.Increment(ref foreignEvents);
            watcher.EnableRaisingEvents = true;
            using var stop = new CancellationTokenSource();
            int conversions = 0;
            var adversary = Task.Run(() =>
            {
                while (!stop.IsCancellationRequested)
                {
                    if (ConvertToJunction(child, foreign, 0x100) == 0) Interlocked.Increment(ref conversions);
                    Thread.Yield(); RemoveJunction(child);
                }
            });
            int safeWrites = 0, rejected = 0;
            try
            {
                for (int index = 0; index < 250; index++)
                {
                    try
                    {
                        var path = Path.Combine(child, $"race-{index}.txt");
                        using var output = lease.CreateFile(path);
                        output.Stream.Write("safe"u8); output.Stream.Flush();
                        ZoneIdentifier.PropagateZone(path, 3);
                        lease.Publish(output, Path.Combine(child, $"published-{index}.txt"));
                        safeWrites++;
                        // Do not keep this fixture: exact-object rollback must also stay local.
                    }
                    catch (ArchiveException ex) when (ex.Failure is ArchiveFailure.UnsafePath or ArchiveFailure.IoError) { rejected++; }
                    Thread.Yield();
                }
            }
            finally { stop.Cancel(); await adversary.WaitAsync(TimeSpan.FromSeconds(10)); RemoveJunction(child); }
            await Task.Delay(100);
            Console.WriteLine($"S06 junction race: {conversions} successful conversions; {safeWrites} safe writes, {rejected} rejected; {foreignEvents} foreign filesystem events.");
            Assert(conversions > 0 && safeWrites + rejected == 250, "concurrent mutation was not exercised");
            Assert(foreignEvents == 0 && Directory.GetFileSystemEntries(foreign).Length == 1 && File.ReadAllText(marker) == "foreign", "foreign directory was touched during root-relative IO");
            Assert(Directory.GetFileSystemEntries(child).Length == 0, "same-object failure cleanup left an output");
        });

        yield return ("S06 existing junctions are rejected through native handle attributes", () =>
        {
            var junction = Path.Combine(directory, "existing-junction");
            var foreign = Path.Combine(directory, "existing-foreign");
            Directory.CreateDirectory(junction); Directory.CreateDirectory(foreign);
            int control = ConvertToJunction(junction, foreign, 0x40000000);
            Assert(control == 0, "native junction fixture creation must succeed: " + control);
            using var lease = new PathIdentityLease();
            try { lease.EnsureDirectory(Path.Combine(junction, "child"), create: true); }
            catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.UnsafePath)
            { Assert(!Directory.Exists(Path.Combine(foreign, "child")), "junction target changed"); return Task.CompletedTask; }
            throw new Exception("Existing junction was accepted.");
        });

        yield return ("S06 created output retains object identity through ADS and exact-object cleanup", () =>
        {
            var outputDir = Path.Combine(directory, "output-object");
            using var lease = new PathIdentityLease();
            lease.EnsureDirectory(outputDir, create: true);
            var path = Path.Combine(outputDir, "kept.txt");
            using (var output = lease.CreateFile(path))
            {
                output.Stream.Write("safe"u8); output.Stream.Flush();
                AssertBlocked(() => File.Move(path, path + ".moved"));
                AssertBlocked(() => File.WriteAllText(path, "changed"));
                ZoneIdentifier.PropagateZone(path, 3);
                output.Keep();
            }
            Assert(File.ReadAllText(path) == "safe" && ZoneIdentifier.DetectZone(path) == 3, "kept output/ADS changed");
            var failed = Path.Combine(outputDir, "failed.txt");
            using (var output = lease.CreateFile(failed)) output.Stream.Write("partial"u8);
            Assert(!File.Exists(failed), "failed output must be deleted by the original handle");
            lease.Dispose();
            File.Move(path, path + ".moved");
            return Task.CompletedTask;
        });

        yield return ("S06 handle publication never replaces an existing file or leaves a failed temporary", () =>
        {
            var outputDir = Path.Combine(directory, "publish");
            using var lease = new PathIdentityLease(); lease.EnsureDirectory(outputDir, create: true);
            var target = Path.Combine(outputDir, "target");
            File.WriteAllText(target, "existing");
            var temp = Path.Combine(outputDir, "temp");
            using (var output = lease.CreateFile(temp))
            {
                output.Stream.Write("new"u8); output.Stream.Flush();
                try { lease.Publish(output, target); throw new Exception("Existing output was replaced."); }
                catch (ArchiveException ex) when (ex.Failure == ArchiveFailure.IoError) { }
            }
            Assert(File.ReadAllText(target) == "existing" && !File.Exists(temp), "collision must preserve existing object and clean only the created temporary");
            using (var output = lease.CreateFile(temp))
            {
                output.Stream.Write("new"u8); output.Stream.Flush();
                try { lease.Publish(output, target + ".new"); output.Keep(); }
                catch (ArchiveException ex) when (ex.InnerException is Win32Exception native)
                { Console.WriteLine("S06 publication native error: " + native.NativeErrorCode); throw; }
            }
            Assert(File.ReadAllText(target + ".new") == "new" && !File.Exists(temp), "handle publication failed");
            return Task.CompletedTask;
        });

        yield return ("S06 LZ4 extraction blocks live source and output ancestor swaps", async () =>
        {
            var sourceDir = Path.Combine(directory, "lz4-source");
            var parent = Path.Combine(directory, "lz4-target");
            var destination = Path.Combine(parent, "nested");
            Directory.CreateDirectory(sourceDir);
            var source = Path.Combine(sourceDir, "payload.lz4");
            var payload = RandomNumberGenerator.GetBytes(32_000); File.WriteAllBytes(source, Frame(payload));
            File.WriteAllText(source + ZoneIdentifier.StreamSuffix, "[ZoneTransfer]\r\nZoneId=3\r\n");
            bool attempted = false;
            var progress = new ImmediateProgress(value =>
            {
                if (attempted || value.CompletedBytes == 0) return;
                attempted = true;
                AssertBlocked(() => Directory.Move(parent, parent + "-moved"));
                AssertBlocked(() => Directory.Move(sourceDir, sourceDir + "-moved"));
                AssertBlocked(() => File.Move(source, source + ".moved"));
                AssertBlocked(() => File.WriteAllBytes(source, new byte[1]));
            });
            var result = await new Lz4Engine().ExtractWithManifestAsync(new(source, destination, null), progress, default);
            Assert(attempted && result.Files.Count == 1 && File.ReadAllBytes(Path.Combine(destination, "payload")).SequenceEqual(payload), "live LZ4 regression did not preserve content");
            Assert(ZoneIdentifier.DetectZone(Path.Combine(destination, "payload")) == 3, "ADS missing");
            MoveAfterDisposal(parent, parent + "-moved"); MoveAfterDisposal(sourceDir, sourceDir + "-moved");
        });

        yield return ("S06 native split archives hold every source volume during output writes", async () =>
        {
            var sourceDir = Path.Combine(directory, "split-source");
            var parent = Path.Combine(directory, "split-target");
            var destination = Path.Combine(parent, "nested");
            Directory.CreateDirectory(sourceDir);
            var payload = RandomNumberGenerator.GetBytes(80_000);
            var payloadPath = Path.Combine(sourceDir, "payload.bin"); File.WriteAllBytes(payloadPath, payload);
            var archive = Path.Combine(sourceDir, "parts.7z");
            var start = new ProcessStartInfo(Path.Combine(repository, "tools", "7zip", "7z.exe"))
            { UseShellExecute = false, CreateNoWindow = true, RedirectStandardOutput = true, RedirectStandardError = true };
            foreach (var arg in new[] { "a", "-t7z", "-mx=0", "-v8k", archive, payloadPath }) start.ArgumentList.Add(arg);
            using (var cli = Process.Start(start)!)
            {
                var stdout = cli.StandardOutput.ReadToEndAsync(); var stderr = cli.StandardError.ReadToEndAsync();
                await cli.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(30)); await stdout; await stderr;
                Assert(cli.ExitCode == 0, "native fixture creation failed");
            }
            var parts = Directory.GetFiles(sourceDir, "parts.7z.*").Order(StringComparer.OrdinalIgnoreCase).ToArray();
            Assert(parts.Length > 2, "fixture must contain multiple volumes");
            bool attempted = false;
            var progress = new ImmediateProgress(value =>
            {
                if (attempted || value.CompletedBytes == 0) return;
                attempted = true;
                AssertBlocked(() => Directory.Move(parent, parent + "-moved"));
                AssertBlocked(() => Directory.Move(sourceDir, sourceDir + "-moved"));
                AssertBlocked(() => File.Move(parts[^1], parts[^1] + ".moved"));
                AssertBlocked(() => File.WriteAllBytes(parts[^1], new byte[1]));
            });
            var result = await new SevenZipEngine(Path.Combine(repository, "tools", "7zip", "7z.dll"))
                .ExtractWithManifestAsync(new(parts[0], destination, null, AllowedVolumes: parts), progress, default);
            Assert(attempted && result.Files.Count == 1 && File.ReadAllBytes(Path.Combine(destination, "payload.bin")).SequenceEqual(payload), "native split content changed");
            MoveAfterDisposal(parent, parent + "-moved"); MoveAfterDisposal(sourceDir, sourceDir + "-moved");
        });

        yield return ("S06 Worker copy and readback hold source and destination ancestors and release the request lease", async () =>
        {
            var sourceDir = Path.Combine(directory, "copy-source");
            var destination = Path.Combine(directory, "copy-target");
            Directory.CreateDirectory(sourceDir);
            var source = Path.Combine(sourceDir, "payload.bin"); var payload = RandomNumberGenerator.GetBytes(80_000);
            File.WriteAllBytes(source, payload);
            var digest = Convert.ToHexStringLower(SHA256.HashData(payload));
            var worker = new WorkerOperations(Path.Combine(repository, "tools", "7zip", "7z.dll"));
            bool copied = false;
            var request = new WorkerRequest
            {
                ProtocolVersion = 2, RequestId = "s06-copy", OperationId = "s06-copy", Generation = 1,
                Operation = "copy", DestinationDirectory = destination, MaxEntries = 100, MaxWrittenBytes = 1024 * 1024,
                Files = [new() { SourcePath = source, RelativePath = "nested/payload.bin", Size = (ulong)payload.Length, Sha256 = digest }]
            };
            var result = await worker.ExecuteAsync(request, value =>
            {
                if (copied || value.CompletedBytes == 0) return; copied = true;
                AssertBlocked(() => Directory.Move(sourceDir, sourceDir + "-moved"));
                AssertBlocked(() => Directory.Move(destination, destination + "-moved"));
                AssertBlocked(() => Directory.Move(Path.Combine(destination, "nested"), Path.Combine(destination, "swapped")));
            }, default);
            Assert(copied && result.Files.Count == 1, "copy progress was not exercised");
            var target = Path.Combine(destination, "nested", "payload.bin");
            bool read = false;
            var readback = request with { Operation = "readback", Files = [request.Files[0] with { SourcePath = target }] };
            var verification = await worker.ExecuteAsync(readback, value =>
            {
                if (read || value.CompletedBytes == 0) return; read = true;
                AssertBlocked(() => Directory.Move(destination, destination + "-moved"));
                AssertBlocked(() => File.Move(target, target + ".moved"));
            }, default);
            Assert(read && verification.VerificationScope == "targetReadback", "readback not exercised");
            MoveAfterDisposal(sourceDir, sourceDir + "-moved"); MoveAfterDisposal(destination, destination + "-moved");
        });
    }

    private static void AssertBlocked(Action action)
    {
        try { action(); }
        catch (IOException) { return; }
        catch (UnauthorizedAccessException) { return; }
        throw new Exception("Live object replacement unexpectedly succeeded.");
    }
    private static void MoveAfterDisposal(string source, string destination)
    {
        // Native extraction can attract short-lived filesystem/antivirus readers.
        // Bound only this post-disposal control; all live-lock assertions stay immediate.
        for (int attempt = 0; ; attempt++)
        {
            try
            {
                Directory.Move(source, destination);
                if (attempt > 0) Console.WriteLine($"S06 post-disposal rename needed {attempt} bounded filesystem retries.");
                return;
            }
            catch (IOException ex) when (attempt < 25 && (ex.HResult & 0xFFFF) is 5 or 32)
            { Thread.Sleep(20); }
        }
    }
    private static void Assert(bool condition, string message) { if (!condition) throw new Exception(message); }
    private sealed class ImmediateProgress(Action<ExtractionProgress> action) : IProgress<ExtractionProgress>
    { public void Report(ExtractionProgress value) => action(value); }
    private static byte[] Frame(byte[] payload)
    {
        using var stream = new MemoryStream(); stream.Write([0x04, 0x22, 0x4D, 0x18]);
        byte[] descriptor = [0x60, 0x40]; stream.Write(descriptor); stream.WriteByte((byte)(XxHash32.Compute(descriptor) >> 8));
        byte[] size = new byte[4]; BinaryPrimitives.WriteUInt32LittleEndian(size, 0x80000000u | (uint)payload.Length);
        stream.Write(size); stream.Write(payload); stream.Write(new byte[4]); return stream.ToArray();
    }
    private static int ConvertToJunction(string directory, string target, uint access)
    {
        using var handle = CreateFile(directory, access, 7, IntPtr.Zero, 3, 0x02200000, IntPtr.Zero);
        if (handle.IsInvalid) return Marshal.GetLastWin32Error();
        var substitute = Encoding.Unicode.GetBytes(@"\??\" + Path.GetFullPath(target));
        var print = Encoding.Unicode.GetBytes(Path.GetFullPath(target));
        byte[] buffer = new byte[16 + substitute.Length + 2 + print.Length + 2];
        BinaryPrimitives.WriteUInt32LittleEndian(buffer, 0xA0000003);
        BinaryPrimitives.WriteUInt16LittleEndian(buffer.AsSpan(4), checked((ushort)(buffer.Length - 8)));
        BinaryPrimitives.WriteUInt16LittleEndian(buffer.AsSpan(10), checked((ushort)substitute.Length));
        BinaryPrimitives.WriteUInt16LittleEndian(buffer.AsSpan(12), checked((ushort)(substitute.Length + 2)));
        BinaryPrimitives.WriteUInt16LittleEndian(buffer.AsSpan(14), checked((ushort)print.Length));
        substitute.CopyTo(buffer, 16); print.CopyTo(buffer, 16 + substitute.Length + 2);
        return DeviceIoControl(handle, 0x000900A4, buffer, (uint)buffer.Length, IntPtr.Zero, 0, out _, IntPtr.Zero)
            ? 0 : Marshal.GetLastWin32Error();
    }
    private static int RemoveJunction(string directory)
    {
        using var handle = CreateFile(directory, 0x100, 7, IntPtr.Zero, 3, 0x02200000, IntPtr.Zero);
        if (handle.IsInvalid) return Marshal.GetLastWin32Error();
        byte[] buffer = new byte[8]; BinaryPrimitives.WriteUInt32LittleEndian(buffer, 0xA0000003);
        return DeviceIoControl(handle, 0x000900AC, buffer, (uint)buffer.Length, IntPtr.Zero, 0, out _, IntPtr.Zero)
            ? 0 : Marshal.GetLastWin32Error();
    }
    [DllImport("kernel32.dll", EntryPoint = "CreateFileW", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern SafeFileHandle CreateFile(string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DeviceIoControl(SafeFileHandle handle, uint code, byte[] input, uint size, IntPtr output, uint outputSize, out uint returned, IntPtr overlapped);
}
