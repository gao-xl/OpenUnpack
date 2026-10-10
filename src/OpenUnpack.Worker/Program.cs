namespace OpenUnpack.Worker;

internal static class Program
{
    private static async Task<int> Main(string[] args)
    {
        try
        {
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

            await new WorkerHost(pipePrefix, parentPid.Value, enginePath).RunAsync();
            return 0;
        }
        catch
        {
            return 1;
        }
    }
}
