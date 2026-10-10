using System.Buffers;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Threading.Channels;

namespace OpenUnpack.Worker;

internal sealed class WorkerHost(string prefix, int parentPid, string engine)
{
    private readonly CancellationTokenSource _lifetime = new();
    private readonly Channel<WorkerEvent> _events = Channel.CreateBounded<WorkerEvent>(new BoundedChannelOptions(32)
    { SingleReader = true, SingleWriter = false, FullMode = BoundedChannelFullMode.Wait });
    private readonly object _state = new();
    private CancellationTokenSource? _activeCancellation;
    private string? _activeId;
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
        Emit(new() { Kind = "ready", Message = "作业进程已就绪。" });
        try
        {
            await foreach (var bytes in ReadLinesAsync(control, _lifetime.Token))
            {
                WorkerRequest? request;
                try { request = JsonSerializer.Deserialize<WorkerRequest>(bytes, Protocol.Json); }
                catch (JsonException) { Emit(Error("", "ProtocolError")); continue; }
                if (request is null || request.ProtocolVersion != Protocol.Version || string.IsNullOrWhiteSpace(request.RequestId)
                    || request.RequestId.Length > 200 || request.Operation is null)
                { Emit(Error(request?.RequestId ?? "", "ProtocolError")); continue; }
                if (request.Operation == "exit") { _lifetime.Cancel(); break; }
                if (request.Operation == "ping")
                { Emit(new() { RequestId = request.RequestId, Kind = "heartbeat", Message = "作业进程可响应。" }); continue; }
                if (request.Operation == "cancel")
                {
                    lock (_state)
                        if (_activeId == request.RequestId) _activeCancellation?.Cancel();
                    continue;
                }
                if (request.Operation is not ("extract" or "copy" or "readback" or "probe"))
                { Emit(Error(request.RequestId, "ProtocolError")); continue; }
                lock (_state)
                {
                    if (_activeId is not null) { Emit(Error(request.RequestId, "ProtocolError")); continue; }
                    _activeId = request.RequestId;
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
        WorkerEvent result;
        try
        {
            result = await new WorkerOperations(engine).ExecuteAsync(request,
                value => Interlocked.Exchange(ref _progress, value), cancellation.Token);
        }
        catch (OperationCanceledException)
        { result = new() { RequestId = request.RequestId, Kind = "cancelled", ErrorCode = "Cancelled", Message = "作业已取消。" }; }
        catch (Exception ex) { result = Error(request.RequestId, WorkerOperations.ErrorCode(ex, request)); }
        Interlocked.Exchange(ref _progress, null);
        Emit(result);
        lock (_state)
        {
            _activeId = null;
            _activeCancellation = null;
            cancellation.Dispose();
        }
    }

    private void Emit(WorkerEvent value)
    {
        // Terminal/control traffic has a separate bounded queue. On flooding, disconnect,
        // rather than waiting under a state lock or starving cancellation.
        if (!_events.Writer.TryWrite(value)) _lifetime.Cancel();
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
                    string? active;
                    lock (_state) active = _activeId;
                    if (active is not null) await WriteEventAsync(pipe, new() { RequestId = active, Kind = "heartbeat" }, token);
                    lastHeartbeat = Stopwatch.GetTimestamp();
                }
                await Task.Delay(100, token);
            }
        }
        catch (Exception ex) when (ex is IOException or OperationCanceledException) { _lifetime.Cancel(); }
    }

    private static async Task WriteEventAsync(Stream pipe, WorkerEvent value, CancellationToken token)
    {
        var bytes = JsonSerializer.SerializeToUtf8Bytes(value, Protocol.Json);
        if (bytes.Length > Protocol.MaxMessageBytes) throw new IOException();
        await pipe.WriteAsync(bytes, token);
        await pipe.WriteAsync(new byte[] { (byte)'\n' }, token);
        await pipe.FlushAsync(token);
    }

    private static async IAsyncEnumerable<byte[]> ReadLinesAsync(Stream pipe,
        [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken token)
    {
        var chunk = new byte[4096];
        var message = new ArrayBufferWriter<byte>();
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
                message.Clear();
                start = i + 1;
            }
            Append(message, chunk, start, count - start);
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
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeClientProcessId(IntPtr pipe, out uint clientProcessId);

    private static WorkerEvent Error(string requestId, string code) => new()
    { RequestId = requestId, Kind = "error", ErrorCode = code, Message = WorkerOperations.ErrorMessage(code) };
}
