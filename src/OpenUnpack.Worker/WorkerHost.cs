using System.Buffers;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text.Json;
using System.Threading.Channels;
using OpenUnpack.Engine;

namespace OpenUnpack.Worker;

internal sealed class WorkerHost(string prefix, int parentPid, string engine)
{
    private readonly CancellationTokenSource _lifetime = new();
    private readonly Channel<WorkerEvent> _events = Channel.CreateBounded<WorkerEvent>(new BoundedChannelOptions(32)
    { SingleReader = true, SingleWriter = false, FullMode = BoundedChannelFullMode.Wait });
    private readonly object _state = new();
    private CancellationTokenSource? _activeCancellation;
    private string? _activeId;
    private WorkerRequest? _activeRequest;
    private WorkerRequest? _lastRequest;
    private Task? _activeTask;
    private WorkerEvent? _progress;

    public async Task RunAsync()
    {
        using var parent = Process.GetProcessById(parentPid);
        using var control = new NamedPipeServerStream(prefix + "-control", PipeDirection.InOut, 1,
            PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        using var events = new NamedPipeServerStream(prefix + "-events", PipeDirection.InOut, 1,
            PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        using var connectTimeout = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token);
        connectTimeout.CancelAfter(TimeSpan.FromSeconds(30));
        await Task.WhenAll(control.WaitForConnectionAsync(connectTimeout.Token), events.WaitForConnectionAsync(connectTimeout.Token));
        ValidateClient(control); ValidateClient(events);
        var writer = WriteEventsAsync(events, _lifetime.Token);
        var monitor = MonitorParentAsync(parent, _lifetime.Token);
        // Keep the v1 envelope for old clients; v2 clients must validate this advertisement.
        Emit(new() { Kind = "ready", Hello = CreateHello(engine), Message = "解压进程已就绪。" });
        try
        {
            await foreach (var bytes in ReadLinesAsync(control, _lifetime.Token))
            {
                WorkerRequest? request;
                try { request = JsonSerializer.Deserialize<WorkerRequest>(bytes, Protocol.Json); }
                catch (JsonException) { Emit(Error("", "ProtocolError")); continue; }
                finally { CryptographicOperations.ZeroMemory(bytes); }
                if (request is null || request.ProtocolVersion is not (Protocol.VersionV1 or Protocol.VersionV2)
                    || string.IsNullOrWhiteSpace(request.RequestId) || request.RequestId.Length > 200 || request.Operation is null
                    || (request.ProtocolVersion == Protocol.VersionV2
                        && (string.IsNullOrWhiteSpace(request.OperationId) || request.OperationId.Length > 200
                            || request.Generation is null or 0)))
                { ZeroizeRequest(request); Emit(Error(request?.RequestId ?? "", "ProtocolError")); continue; }
                if (request.Operation == "exit")
                {
                    lock (_state)
                    {
                        if (request.ProtocolVersion == Protocol.VersionV2 && _lastRequest is not null
                            && !SameOperation(request, _lastRequest))
                        { ZeroizeRequest(request); Emit(ForRequest(Error(request.RequestId, "ProtocolError"), request)); continue; }
                    }
                    ZeroizeRequest(request);
                    _lifetime.Cancel(); break;
                }
                if (request.Operation == "ping")
                {
                    lock (_state)
                    {
                        if (request.ProtocolVersion == Protocol.VersionV2 && _lastRequest is not null
                            && !SameOperation(request, _lastRequest))
                        { ZeroizeRequest(request); Emit(ForRequest(Error(request.RequestId, "ProtocolError"), request)); continue; }
                    }
                    ZeroizeRequest(request);
                    Emit(ForRequest(new() { Kind = "heartbeat", Message = "解压进程运行正常。" }, request)); continue;
                }
                if (request.Operation == "cancel")
                {
                    lock (_state)
                    {
                        if (_activeRequest is not null && SameOperation(request, _activeRequest))
                            _activeCancellation?.Cancel();
                        else Emit(ForRequest(Error(request.RequestId, "ProtocolError"), request));
                    }
                    ZeroizeRequest(request);
                    continue;
                }
                if (request.Operation is not ("extract" or "copy" or "readback" or "probe" or "inspect"))
                { ZeroizeRequest(request); Emit(ForRequest(Error(request.RequestId, "ProtocolError"), request)); continue; }
                lock (_state)
                {
                    if (_activeId is not null) { ZeroizeRequest(request); Emit(ForRequest(Error(request.RequestId, "ProtocolError"), request)); continue; }
                    _activeId = request.RequestId;
                    // Store only operation identity, never retain password-bearing requests.
                    _activeRequest = IdentityOnly(request);
                    _lastRequest = _activeRequest;
                    _activeCancellation = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token);
                    var cancellation = _activeCancellation;
                    _activeTask = Task.Run(() => ExecuteAsync(request, cancellation), CancellationToken.None);
                }
            }
        }
        finally
        {
            _lifetime.Cancel();
            lock (_state) _activeCancellation?.Cancel();
            // Native codecs can ignore callbacks. The parent owns the final kill deadline.
            Task? active;
            lock (_state) active = _activeTask;
            if (active is not null) await Task.WhenAny(active, Task.Delay(TimeSpan.FromSeconds(2)));
            _events.Writer.TryComplete();
            try { await writer; } catch (Exception ex) when (ex is IOException or OperationCanceledException) { }
            try { await monitor; } catch (OperationCanceledException) { }
        }
    }

    private async Task ExecuteAsync(WorkerRequest request, CancellationTokenSource cancellation)
    {
        ApplyProcessPriority(request.WorkerPriority);
        WorkerEvent result;
        try
        {
            result = await new WorkerOperations(engine).ExecuteAsync(request,
                value => Interlocked.Exchange(ref _progress, ForRequest(value, request)), cancellation.Token);
        }
        catch (OperationCanceledException)
        { result = new() { RequestId = request.RequestId, Kind = "cancelled", ErrorCode = "Cancelled", Message = "解压已取消。" }; }
        catch (Exception ex)
        {
            var code = WorkerOperations.ErrorCode(ex, request);
            SafeDiagnostics.Emit(request.Operation, request.Operation is "inspect" or "probe" ? "metadata"
                : request.Operation == "extract" ? "decode" : "transfer", code, ex);
            result = Error(request.RequestId, code);
        }
        finally
        {
            Interlocked.Exchange(ref _progress, null);
            lock (_state)
            {
                _activeId = null;
                _activeRequest = null;
                _activeCancellation = null;
                cancellation.Dispose();
            }
        }
        Emit(ForRequest(result, request));
        ZeroizeRequest(request, result.MatchedPassword);
    }

    private void Emit(WorkerEvent value)
    {
        if (_events.Writer.TryWrite(value)) return;

        // Terminal/control traffic must never be dropped due to transient queue pressure.
        if (value.Kind is "completed" or "cancelled" or "error" or "ready")
        {
            _events.Reader.TryRead(out _);
            if (_events.Writer.TryWrite(value)) return;

            _ = Task.Run(async () =>
            {
                try { await _events.Writer.WriteAsync(value, _lifetime.Token); }
                catch { _lifetime.Cancel(); }
            });
            return;
        }
    }

    private async Task WriteEventsAsync(Stream pipe, CancellationToken token)
    {
        var lastHeartbeat = Stopwatch.GetTimestamp();
        try
        {
            while (!token.IsCancellationRequested)
            {
                while (_events.Reader.TryRead(out var important)) await WriteEventAsync(pipe, important, token);
                var progress = Interlocked.Exchange(ref _progress, null);
                if (progress is not null) await WriteEventAsync(pipe, progress, token);
                if (Stopwatch.GetElapsedTime(lastHeartbeat) >= TimeSpan.FromSeconds(2))
                {
                    WorkerRequest? active;
                    lock (_state) active = _activeRequest;
                    if (active is not null) await WriteEventAsync(pipe, ForRequest(new() { Kind = "heartbeat" }, active), token);
                    lastHeartbeat = Stopwatch.GetTimestamp();
                }
                await Task.Delay(100, token);
            }
        }
        catch (Exception ex) when (ex is IOException or OperationCanceledException) { _lifetime.Cancel(); }
    }

    private static async Task WriteEventAsync(Stream pipe, WorkerEvent value, CancellationToken token)
    {
        byte[]? bytes = null;
        try
        {
            bytes = JsonSerializer.SerializeToUtf8Bytes(value, Protocol.Json);
            if (bytes.Length > Protocol.MaxMessageBytes) throw new IOException();
            await pipe.WriteAsync(bytes, token);
            await pipe.WriteAsync(new byte[] { (byte)'\n' }, token);
            await pipe.FlushAsync(token);
        }
        finally
        {
            if (bytes is not null) CryptographicOperations.ZeroMemory(bytes);
            if (value.MatchedPassword is not null) CredentialSecurity.ZeroizeString(value.MatchedPassword);
        }
    }

    private static async IAsyncEnumerable<byte[]> ReadLinesAsync(Stream pipe,
        [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken token)
    {
        var chunk = new byte[4096];
        var message = new ArrayBufferWriter<byte>();
        try
        {
            while (true)
            {
                var count = await pipe.ReadAsync(chunk, token);
                if (count == 0) yield break;
                var start = 0;
                for (var i = 0; i < count; i++)
                {
                    if (chunk[i] != '\n') continue;
                    Append(message, chunk, start, i - start);
                    yield return message.WrittenMemory.ToArray();
                    ZeroWritten(message);
                    message.Clear();
                    start = i + 1;
                }
                Append(message, chunk, start, count - start);
            }
        }
        finally
        {
            CryptographicOperations.ZeroMemory(chunk);
            ZeroWritten(message);
        }
    }

    private static void ZeroWritten(ArrayBufferWriter<byte> buffer)
    {
        if (buffer.WrittenCount > 0)
        {
            CryptographicOperations.ZeroMemory(
                System.Runtime.InteropServices.MemoryMarshal.CreateSpan(
                    ref System.Runtime.InteropServices.MemoryMarshal.GetReference(buffer.WrittenSpan),
                    buffer.WrittenCount));
        }
    }

    private static void Append(ArrayBufferWriter<byte> message, byte[] chunk, int start, int length)
    {
        if (message.WrittenCount + length > Protocol.MaxMessageBytes) throw new IOException();
        chunk.AsSpan(start, length).CopyTo(message.GetSpan(length));
        message.Advance(length);
    }

    private async Task MonitorParentAsync(Process parent, CancellationToken token)
    {
        try
        {
            while (!token.IsCancellationRequested)
            {
                if (parent.HasExited) { _lifetime.Cancel(); return; }
                await Task.Delay(500, token);
            }
        }
        catch (InvalidOperationException) { _lifetime.Cancel(); }
    }

    private void ValidateClient(NamedPipeServerStream pipe)
    {
        if (!GetNamedPipeClientProcessId(pipe.SafePipeHandle.DangerousGetHandle(), out var id) || id != parentPid)
            throw new IOException();
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeClientProcessId(IntPtr pipe, out uint clientProcessId);

    private static WorkerEvent Error(string requestId, string code) => new()
    { RequestId = requestId, Kind = "error", ErrorCode = code, Message = WorkerOperations.ErrorMessage(code) };

    internal static WorkerHello CreateHello(string enginePath)
    {
        string version = "unknown";
        try
        {
            var info = FileVersionInfo.GetVersionInfo(enginePath);
            if (info.FileMajorPart > 0) version = $"{info.FileMajorPart}.{info.FileMinorPart:D2}";
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
        { SafeDiagnostics.Emit("startup", "metadata", "IoError", ex); }
        return new()
        {
            Capabilities = new()
            {
                BackendVersion = version,
                Backends = [
                    new() { BackendId = "7-Zip", BackendVersion = version,
                        Formats = ["7z", "zip", "rar", "tar", "gz", "bz2", "xz", "cab", "wim", "lzh", "zstd", "verifiedSfx"],
                        ReadMetadata = true, EncryptedArchives = true, UnknownExpandedSize = true,
                        VerificationScopes = ["archiveCrcAndStreamSha256", "salvagedWithCrcWarning"] },
                    new() { BackendId = "managed-lz4-frame", BackendVersion = "implementation-v1",
                        Formats = ["lz4IndependentSingleFrame"], ReadMetadata = true,
                        EncryptedArchives = false, UnknownExpandedSize = true,
                        VerificationScopes = ["frameValidationAndStreamSha256"] }
                ]
            }
        };
    }

    private static WorkerRequest IdentityOnly(WorkerRequest request) => new()
    { ProtocolVersion = request.ProtocolVersion, RequestId = request.RequestId,
      OperationId = request.OperationId, Generation = request.Generation };

    private static bool SameOperation(WorkerRequest left, WorkerRequest right) =>
        left.ProtocolVersion == right.ProtocolVersion && left.RequestId == right.RequestId
        && (left.ProtocolVersion == Protocol.VersionV1
            || (left.OperationId == right.OperationId && left.Generation == right.Generation));

    private static WorkerEvent ForRequest(WorkerEvent value, WorkerRequest request) => value with
    { ProtocolVersion = request.ProtocolVersion, RequestId = request.RequestId,
      OperationId = request.OperationId, Generation = request.Generation };

    private static void ZeroizeRequest(WorkerRequest? request, string? preserve = null)
    {
        if (request is null) return;
        if (request.Password is not null && !ReferenceEquals(request.Password, preserve))
        {
            CredentialSecurity.ZeroizeString(request.Password);
        }
        if (request.CandidatePasswords is not null)
        {
            foreach (var pwd in request.CandidatePasswords)
            {
                if (pwd is not null && !ReferenceEquals(pwd, preserve))
                {
                    CredentialSecurity.ZeroizeString(pwd);
                }
            }
            request.CandidatePasswords.Clear();
        }
        if (request.Candidates is not null)
        {
            foreach (var candidate in request.Candidates)
            {
                if (candidate?.Secret is not null && !ReferenceEquals(candidate.Secret, preserve))
                {
                    CredentialSecurity.ZeroizeString(candidate.Secret);
                }
            }
            request.Candidates.Clear();
        }
    }

    private static void ApplyProcessPriority(string? priority)
    {
        if (string.IsNullOrWhiteSpace(priority)) return;
        try
        {
            var p = Process.GetCurrentProcess();
            var target = priority switch
            {
                "idle" or "low" => ProcessPriorityClass.Idle,
                "belowNormal" => ProcessPriorityClass.BelowNormal,
                "normal" => ProcessPriorityClass.Normal,
                "aboveNormal" => ProcessPriorityClass.AboveNormal,
                "high" => ProcessPriorityClass.High,
                _ => ProcessPriorityClass.BelowNormal
            };
            if (p.PriorityClass != target) p.PriorityClass = target;
        }
        catch { /* Non-critical if insufficient privilege */ }
    }
}
