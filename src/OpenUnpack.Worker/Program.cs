using System.Runtime.InteropServices;
using System.Text.Json;

namespace OpenUnpack.Worker;

internal static class Program
{
    [DllImport("kernel32.dll", EntryPoint = "SetDllDirectoryW", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetDllDirectory(string lpPathName);

    private static async Task<int> Main(string[] args)
    {
        try
        {
            if (OperatingSystem.IsWindows())
            {
                if (!SetDllDirectory(string.Empty))
                    throw new OpenUnpack.Engine.ArchiveException(OpenUnpack.Engine.ArchiveFailure.IoError,
                        "无法设置安全的模块搜索路径。");
                OpenUnpack.Engine.WorkerSandbox.InitializeProcessSandbox();
            }
            if (args is ["--preview-output-rules"])
            {
                // Pure rule evaluation: no archives, credentials, engine load or file writes.
                using var input = Console.OpenStandardInput();
                using var buffer = new MemoryStream();
                var chunk = new byte[8192];
                int read;
                while ((read = await input.ReadAsync(chunk)) > 0)
                {
                    if (buffer.Length + read > 1024 * 1024) return 2;
                    buffer.Write(chunk, 0, read);
                }
                using var json = System.Text.Json.JsonDocument.Parse(buffer.ToArray());
                var paths = json.RootElement.GetProperty("paths").Deserialize<string[]>();
                var patterns = json.RootElement.GetProperty("patterns").Deserialize<string[]>();
                if (paths is null || patterns is null) return 2;
                var result = OpenUnpack.Engine.Native.SevenZipEngine.PreviewOutputExclusions(paths, patterns);
                await Console.Out.WriteAsync(System.Text.Json.JsonSerializer.Serialize(result));
                return 0;
            }
            if (args.Length != 6)
            {
                return 1;
            }

            string? pipePrefix = null;
            int? parentPid = null;
            string? enginePath = null;

            for (int i = 0; i < args.Length; i += 2)
            {
                string key = args[i];
                string val = args[i + 1];

                switch (key)
                {
                    case "--pipe-prefix":
                        if (pipePrefix is not null)
                        {
                            return 1;
                        }
                        pipePrefix = val;
                        break;
                    case "--parent-pid":
                        if (parentPid is not null)
                        {
                            return 1;
                        }
                        if (!int.TryParse(val, out int pid) || pid <= 0)
                        {
                            return 1;
                        }
                        parentPid = pid;
                        break;
                    case "--engine":
                        if (enginePath is not null)
                        {
                            return 1;
                        }
                        enginePath = val;
                        break;
                    default:
                        return 1;
                }
            }

            if (pipePrefix is null || parentPid is null || enginePath is null)
            {
                return 1;
            }

            if (pipePrefix.Length < 1 || pipePrefix.Length > 100)
            {
                return 1;
            }

            foreach (char c in pipePrefix)
            {
                if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '-'))
                {
                    return 1;
                }
            }

            if (!Path.IsPathFullyQualified(enginePath) || !File.Exists(enginePath))
            {
                return 1;
            }

            try
            {
                OpenUnpack.Engine.Native.SevenZipEngine.EnsureNoReparsePoint(enginePath);
            }
            catch
            {
                return 1;
            }

            await new WorkerHost(pipePrefix, parentPid.Value, enginePath).RunAsync();
            return 0;
        }
        catch (Exception ex)
        {
            // One diagnostic line for field support: type and HResult only. No stack and no
            // message body — exception text can embed paths, and stdout stays reserved for the
            // wire protocol consumed by the host process.
            OpenUnpack.Engine.SafeDiagnostics.Emit("startup", "request", "IoError", ex);
            return 1;
        }
    }
}
