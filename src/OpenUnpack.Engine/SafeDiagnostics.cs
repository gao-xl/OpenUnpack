using System.Text.Json;

namespace OpenUnpack.Engine;

/// <summary>Bounded stderr schema v1; no messages, paths, identifiers or credentials.</summary>
public static class SafeDiagnostics
{
    private static int _emitted;
    public const int MaxEvents = 8;
    public const int MaxLineBytes = 512;

    public static void Emit(string operation, string phase, string code, Exception? error = null, int? nativeCode = null)
    {
        if (Interlocked.Increment(ref _emitted) > MaxEvents) return;
        var safeOperation = operation is "startup" or "extract" or "inspect" or "probe" or "copy" or "readback" ? operation : "unknown";
        var safePhase = phase is "request" or "metadata" or "decode" or "transfer" or "sourceZone" or "targetZone" or "mitigation" or "cleanup" ? phase : "unknown";
        var safeCode = code is "ProtocolError" or "ResourceLimit" or "Unsupported" or "Corrupt" or "MissingVolume"
            or "UnsafePath" or "IoError" or "Cancelled" or "HardeningFailed" or "HardeningUnavailable"
            or "NeedsPassword" or "PasswordMismatchOrCorrupt" ? code : "IoError";
        var type = error?.GetType().Name ?? "None";
        if (type.Length > 64 || type.Any(c => !char.IsAsciiLetterOrDigit(c) && c != '_')) type = "Exception";
        var json = JsonSerializer.Serialize(new
        {
            kind = "openunpackDiagnostic", version = 1, operation = safeOperation, phase = safePhase,
            errorCode = safeCode, exceptionType = type, hResult = error?.HResult, nativeCode
        });
        if (System.Text.Encoding.UTF8.GetByteCount(json) > MaxLineBytes) return;
        try { Console.Error.WriteLine(json); }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException) { }
    }
}
