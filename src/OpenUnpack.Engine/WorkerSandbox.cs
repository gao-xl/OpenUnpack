using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

namespace OpenUnpack.Engine;

/// <summary>
/// Worker 进程沙箱管理与文件系统写权限范围强制守卫。
/// 包含 Windows 令牌特权剥离、Process Mitigation 策略应用以及细粒度 WriteScopeGuard。
/// </summary>
public static class WorkerSandbox
{
    private const uint TOKEN_QUERY = 0x0008;
    private const uint TOKEN_ADJUST_PRIVILEGES = 0x0020;
    private const uint SE_PRIVILEGE_REMOVED = 0x00000004;

    [StructLayout(LayoutKind.Sequential)]
    private struct LUID
    {
        public uint LowPart;
        public int HighPart;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct LUID_AND_ATTRIBUTES
    {
        public LUID Luid;
        public uint Attributes;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PROCESS_MITIGATION_IMAGE_LOAD_POLICY
    {
        public uint Flags;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY
    {
        public uint Flags;
    }

    [DllImport("advapi32.dll", SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool OpenProcessToken(IntPtr processHandle, uint desiredAccess, out IntPtr tokenHandle);

    [DllImport("advapi32.dll", EntryPoint = "LookupPrivilegeValueW", CharSet = CharSet.Unicode, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool LookupPrivilegeValue(string? lpSystemName, string lpName, out LUID lpLuid);

    [DllImport("advapi32.dll", SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetTokenInformation(IntPtr tokenHandle, int tokenInformationClass, IntPtr tokenInformation, uint tokenInformationLength, out uint returnLength);

    [DllImport("advapi32.dll", SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool AdjustTokenPrivileges(IntPtr tokenHandle, [MarshalAs(UnmanagedType.Bool)] bool disableAllPrivileges, IntPtr newPrivileges, uint bufferLength, IntPtr previousPrivileges, IntPtr returnLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetProcessMitigationPolicy(int mitigationPolicy, ref PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY lpBuffer, nuint dwLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetProcessMitigationPolicy(IntPtr hProcess, int mitigationPolicy, ref PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY lpBuffer, nuint dwLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetProcessMitigationPolicy(int mitigationPolicy, ref PROCESS_MITIGATION_IMAGE_LOAD_POLICY lpBuffer, nuint dwLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetProcessMitigationPolicy(IntPtr hProcess, int mitigationPolicy, ref PROCESS_MITIGATION_IMAGE_LOAD_POLICY lpBuffer, nuint dwLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr hObject);

    /// <summary>
    /// 初始化 Worker 进程级沙箱环境：剥离危险特权，设置映像加载与注入防御缓解策略。
    /// </summary>
    public static void InitializeProcessSandbox()
    {
        if (!OperatingSystem.IsWindows()) return;
        if (!StripTokenPrivileges()) FailHardening(Marshal.GetLastWin32Error());
        ApplyProcessMitigations();
    }

    /// <summary>
    /// 永久剥离除 SeChangeNotifyPrivilege 外的所有 Windows 令牌特权（SE_PRIVILEGE_REMOVED）。
    /// </summary>
    public static bool StripTokenPrivileges()
    {
        if (!OperatingSystem.IsWindows()) return false;
        using var process = Process.GetCurrentProcess();
        var hProcess = process.Handle;
        if (!OpenProcessToken(hProcess, TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, out var hToken))
        {
            return false;
        }
        try
        {
            if (!LookupPrivilegeValue(null, "SeChangeNotifyPrivilege", out var changeNotifyLuid)) return false;
            _ = GetTokenInformation(hToken, 3 /* TokenPrivileges */, IntPtr.Zero, 0, out var requiredLength);
            if (requiredLength is < 4 or > 1024 * 1024) return false;

            var buffer = Marshal.AllocHGlobal((int)requiredLength);
            try
            {
                if (!GetTokenInformation(hToken, 3, buffer, requiredLength, out _))
                {
                    return false;
                }
                int privilegeCount = Marshal.ReadInt32(buffer);
                var luidPtr = IntPtr.Add(buffer, 4);
                int itemSize = Marshal.SizeOf<LUID_AND_ATTRIBUTES>();
                if (privilegeCount < 0 || (long)privilegeCount * itemSize + 4 > requiredLength) return false;

                for (int i = 0; i < privilegeCount; i++)
                {
                    var itemPtr = IntPtr.Add(luidPtr, i * itemSize);
                    var item = Marshal.PtrToStructure<LUID_AND_ATTRIBUTES>(itemPtr);
                    if (item.Luid.LowPart == changeNotifyLuid.LowPart && item.Luid.HighPart == changeNotifyLuid.HighPart)
                    {
                        continue; // 保留文件系统遍历与路径解析所需的 SeChangeNotifyPrivilege
                    }
                    item.Attributes = SE_PRIVILEGE_REMOVED;
                    Marshal.StructureToPtr(item, itemPtr, false);
                }

                if (!AdjustTokenPrivileges(hToken, false, buffer, requiredLength, IntPtr.Zero, IntPtr.Zero)
                    || Marshal.GetLastWin32Error() != 0) return false;
                // AdjustTokenPrivileges may report success while some privileges were not assigned.
                if (!GetTokenInformation(hToken, 3, buffer, requiredLength, out _)) return false;
                int remaining = Marshal.ReadInt32(buffer);
                if (remaining < 0 || (long)remaining * itemSize + 4 > requiredLength) return false;
                for (int i = 0; i < remaining; i++)
                {
                    var item = Marshal.PtrToStructure<LUID_AND_ATTRIBUTES>(IntPtr.Add(buffer, 4 + i * itemSize));
                    if (item.Luid.LowPart != changeNotifyLuid.LowPart || item.Luid.HighPart != changeNotifyLuid.HighPart) return false;
                }
                return true;
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }
        finally
        {
            CloseHandle(hToken);
        }
    }

    public enum ProcessMitigationPolicy
    {
        ProcessDEPPolicy = 0,
        ProcessASLRPolicy = 1,
        ProcessDynamicCodePolicy = 2,
        ProcessStrictHandleCheckPolicy = 3,
        ProcessSystemCallDisablePolicy = 4,
        ProcessMitigationOptionsMask = 5,
        ProcessExtensionPointDisablePolicy = 6,
        ProcessControlFlowGuardPolicy = 7,
        ProcessSignaturePolicy = 8,
        ProcessFontDisablePolicy = 9,
        ProcessImageLoadPolicy = 10,
    }

    /// <summary>
    /// 应用 Windows 进程安全缓解策略：阻断远程 UNC 镜像加载与 DLL 扩展点注入。
    /// </summary>
    public static void ApplyProcessMitigations()
    {
        if (!OperatingSystem.IsWindows()) return;
        if (!OperatingSystem.IsWindowsVersionAtLeast(6, 2))
        {
            SafeDiagnostics.Emit("startup", "mitigation", "HardeningUnavailable", nativeCode: 50);
            return;
        }
        using var process = Process.GetCurrentProcess();
        // 1. 禁用全局钩子与 AppInit_DLLs 注入 (ProcessExtensionPointDisablePolicy = 6, DisableExtensionPoints = 1)
        var extPolicy = new PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY { Flags = 1 };
        if (!SetProcessMitigationPolicy(
            (int)ProcessMitigationPolicy.ProcessExtensionPointDisablePolicy,
            ref extPolicy,
            (nuint)Marshal.SizeOf<PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY>())) FailHardening(Marshal.GetLastWin32Error());
        extPolicy.Flags = 0;
        if (!GetProcessMitigationPolicy(process.Handle, (int)ProcessMitigationPolicy.ProcessExtensionPointDisablePolicy,
            ref extPolicy, (nuint)Marshal.SizeOf<PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY>())) FailHardening(Marshal.GetLastWin32Error());
        if ((extPolicy.Flags & 1) == 0) FailHardening(0);

        if (!OperatingSystem.IsWindowsVersionAtLeast(10))
        {
            SafeDiagnostics.Emit("startup", "mitigation", "HardeningUnavailable", nativeCode: 50);
            return;
        }

        // 2. 禁止从网络共享加载可执行镜像 (ProcessImageLoadPolicy = 10, NoRemoteImages = 1)
        var imgPolicy = new PROCESS_MITIGATION_IMAGE_LOAD_POLICY { Flags = 1 };
        if (!SetProcessMitigationPolicy(
            (int)ProcessMitigationPolicy.ProcessImageLoadPolicy,
            ref imgPolicy,
            (nuint)Marshal.SizeOf<PROCESS_MITIGATION_IMAGE_LOAD_POLICY>())) FailHardening(Marshal.GetLastWin32Error());
        imgPolicy.Flags = 0;
        if (!GetProcessMitigationPolicy(process.Handle, (int)ProcessMitigationPolicy.ProcessImageLoadPolicy,
            ref imgPolicy, (nuint)Marshal.SizeOf<PROCESS_MITIGATION_IMAGE_LOAD_POLICY>())) FailHardening(Marshal.GetLastWin32Error());
        if ((imgPolicy.Flags & 1) == 0) FailHardening(0);
    }

    private static void FailHardening(int code)
    {
        SafeDiagnostics.Emit("startup", "mitigation", "HardeningFailed", nativeCode: code);
        throw new ArchiveException(ArchiveFailure.IoError, "无法启用所需的进程安全策略。");
    }

    public static bool IsNoRemoteImagesPolicyEnabled()
    {
        if (!OperatingSystem.IsWindowsVersionAtLeast(10)) return false;
        using var process = Process.GetCurrentProcess();
        var policy = new PROCESS_MITIGATION_IMAGE_LOAD_POLICY();
        return GetProcessMitigationPolicy(process.Handle, (int)ProcessMitigationPolicy.ProcessImageLoadPolicy,
            ref policy, (nuint)Marshal.SizeOf<PROCESS_MITIGATION_IMAGE_LOAD_POLICY>()) && (policy.Flags & 1) != 0;
    }

    /// <summary>
    /// 查询当前进程是否成功启用了扩展点禁用缓解策略 (用于安全自检与测试验证)。
    /// </summary>
    public static bool IsExtensionPointDisablePolicyEnabled()
    {
        if (!OperatingSystem.IsWindows()) return false;
        try
        {
            using var process = Process.GetCurrentProcess();
            var policy = new PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY();
            if (GetProcessMitigationPolicy(
                process.Handle,
                (int)ProcessMitigationPolicy.ProcessExtensionPointDisablePolicy,
                ref policy,
                (nuint)Marshal.SizeOf<PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY>()))
            {
                return (policy.Flags & 1) != 0;
            }
        }
        catch { }
        return false;
    }

    /// <summary>
    /// 查询当前进程访问令牌中剩余的特权数量（用于沙箱降权自检与自动化验收测试）。
    /// </summary>
    public static int GetTokenPrivilegeCount()
    {
        if (!OperatingSystem.IsWindows()) return 0;
        using var process = Process.GetCurrentProcess();
        var hProcess = process.Handle;
        if (!OpenProcessToken(hProcess, TOKEN_QUERY, out var hToken))
            return -1;
        try
        {
            _ = GetTokenInformation(hToken, 3 /* TokenPrivileges */, IntPtr.Zero, 0, out var requiredLength);
            if (requiredLength == 0) return -1;
            var buffer = Marshal.AllocHGlobal((int)requiredLength);
            try
            {
                if (!GetTokenInformation(hToken, 3, buffer, requiredLength, out _))
                    return -1;
                return Marshal.ReadInt32(buffer);
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }
        finally
        {
            CloseHandle(hToken);
        }
    }
}

/// <summary>
/// 线程/异步上下文绑定的强制文件系统写权限范围守卫。
/// 严格限制解压与复制操作只能向已授权的 workspace 写入，并拦截对源归档、系统目录与范围外目录的篡改。
/// </summary>
public sealed class WriteScopeGuard : IDisposable
{
    private static readonly AsyncLocal<WriteScopeGuard?> CurrentScope = new();

    public IReadOnlyList<string> AllowedWriteRoots { get; }
    public IReadOnlyList<string> AllowedWriteFiles { get; }
    public IReadOnlyList<string> AllowedManifestDirectories { get; }
    public IReadOnlyList<string> ForbiddenPaths { get; }
    public bool IsReadOnly { get; }

    public static WriteScopeGuard? ActiveScope => CurrentScope.Value;

    private readonly WriteScopeGuard? _parentScope;
    private readonly HashSet<string> _forbiddenExactSet;
    private readonly HashSet<string> _allowedWriteFilesSet;
    private readonly string[] _allowedRootsWithSep;
    private readonly string[] _allowedRootsWithoutSep;
    private readonly string[] _manifestDirsWithSep;
    private readonly string[] _manifestDirsWithoutSep;
    private readonly string[] _systemForbiddenDirsWithSep;
    private readonly string[] _systemForbiddenDirsWithoutSep;
    private string? _lastSafeParent;
    private bool _disposed;

    private WriteScopeGuard(
        IEnumerable<string> allowedRoots,
        IEnumerable<string> allowedFiles,
        IEnumerable<string> allowedManifestDirectories,
        IEnumerable<string> forbiddenPaths,
        bool isReadOnly)
    {
        AllowedWriteRoots = allowedRoots.Where(p => !string.IsNullOrWhiteSpace(p)).Select(Path.GetFullPath).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        AllowedWriteFiles = allowedFiles.Where(p => !string.IsNullOrWhiteSpace(p)).Select(Path.GetFullPath).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        AllowedManifestDirectories = allowedManifestDirectories.Where(p => !string.IsNullOrWhiteSpace(p)).Select(Path.GetFullPath).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        ForbiddenPaths = forbiddenPaths.Where(p => !string.IsNullOrWhiteSpace(p)).Select(Path.GetFullPath).Distinct(StringComparer.OrdinalIgnoreCase).ToArray();
        IsReadOnly = isReadOnly;

        _forbiddenExactSet = new HashSet<string>(ForbiddenPaths, StringComparer.OrdinalIgnoreCase);
        _allowedWriteFilesSet = new HashSet<string>(AllowedWriteFiles, StringComparer.OrdinalIgnoreCase);
        foreach (var file in AllowedWriteFiles)
        {
            _allowedWriteFilesSet.Add(file + ".tmp");
        }
        _allowedRootsWithoutSep = AllowedWriteRoots
            .Select(r => r.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar))
            .ToArray();
        _allowedRootsWithSep = _allowedRootsWithoutSep
            .Select(r => r + Path.DirectorySeparatorChar)
            .ToArray();
        _manifestDirsWithoutSep = AllowedManifestDirectories
            .Select(r => r.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar))
            .ToArray();
        _manifestDirsWithSep = _manifestDirsWithoutSep
            .Select(r => r + Path.DirectorySeparatorChar)
            .ToArray();

        var sysDirs = new List<string>();
        AddSystemForbiddenPaths(sysDirs);
        _systemForbiddenDirsWithoutSep = sysDirs
            .Where(p => !string.IsNullOrWhiteSpace(p))
            .Select(r => Path.GetFullPath(r).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();
        _systemForbiddenDirsWithSep = _systemForbiddenDirsWithoutSep
            .Select(r => r + Path.DirectorySeparatorChar)
            .ToArray();

        _parentScope = CurrentScope.Value;
        CurrentScope.Value = this;
    }

    /// <summary>
    /// 进入限制性写范围上下文。
    /// </summary>
    public static WriteScopeGuard Enter(
        IEnumerable<string>? allowedRoots = null,
        IEnumerable<string>? allowedFiles = null,
        IEnumerable<string>? forbiddenPaths = null,
        IEnumerable<string>? allowedManifestDirectories = null,
        bool isReadOnly = false)
    {
        var forbiddenList = new List<string>(forbiddenPaths ?? []);
        AddSystemForbiddenPaths(forbiddenList);
        return new WriteScopeGuard(
            allowedRoots ?? [],
            allowedFiles ?? [],
            allowedManifestDirectories ?? [],
            forbiddenList,
            isReadOnly);
    }

    /// <summary>
    /// 进入严格只读上下文（禁止向任何常规目录写出，仅允许写出显式指定的文件或指定目录下的溢出清单）。
    /// </summary>
    public static WriteScopeGuard EnterReadOnly(
        IEnumerable<string>? forbiddenPaths = null,
        IEnumerable<string>? allowedFiles = null,
        IEnumerable<string>? allowedManifestDirectories = null)
    {
        return Enter(
            allowedRoots: null,
            allowedFiles: allowedFiles,
            forbiddenPaths: forbiddenPaths,
            allowedManifestDirectories: allowedManifestDirectories,
            isReadOnly: true);
    }

    private static void AddSystemForbiddenPaths(List<string> list)
    {
        try
        {
            var win = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
            if (!string.IsNullOrEmpty(win)) list.Add(win);
            var prog = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
            if (!string.IsNullOrEmpty(prog)) list.Add(prog);
            var progX86 = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86);
            if (!string.IsNullOrEmpty(progX86)) list.Add(progX86);
            var sys = Environment.GetFolderPath(Environment.SpecialFolder.System);
            if (!string.IsNullOrEmpty(sys)) list.Add(sys);
        }
        catch { }
    }

    /// <summary>
    /// 在每次进行文件创建、覆盖或目录创建前，验证目标路径是否在当前沙箱允许的写范围之内。
    /// </summary>
    public static void DemandWriteAccess(string targetPath)
    {
        var current = CurrentScope.Value;
        if (current is null) return;

        if (string.IsNullOrWhiteSpace(targetPath))
            throw new ArchiveException(ArchiveFailure.UnsafePath, "目标写入路径为空，沙箱已拒绝操作。");

        var fullTarget = Path.GetFullPath(targetPath);

        // 1. 严格检查绝对禁止路径与祖先目录（源包、分卷、保护目录等，利用 HashSet O(1) 查找）
        if (current._forbiddenExactSet.Contains(fullTarget))
        {
            throw new ArchiveException(ArchiveFailure.UnsafePath, $"沙箱已阻止向受保护路径写入: {fullTarget}");
        }

        var parent = Path.GetDirectoryName(fullTarget);
        if (!string.IsNullOrEmpty(parent) && !string.Equals(parent, current._lastSafeParent, StringComparison.OrdinalIgnoreCase))
        {
            var p = parent;
            while (!string.IsNullOrEmpty(p))
            {
                if (current._forbiddenExactSet.Contains(p))
                {
                    throw new ArchiveException(ArchiveFailure.UnsafePath, $"沙箱已阻止向受保护路径写入: {fullTarget}");
                }
                p = Path.GetDirectoryName(p);
            }
            current._lastSafeParent = parent;
        }

        // 2. 检查系统目录前缀
        for (var i = 0; i < current._systemForbiddenDirsWithSep.Length; i++)
        {
            var sysDirWithSep = current._systemForbiddenDirsWithSep[i];
            var sysDirNoSep = current._systemForbiddenDirsWithoutSep[i];
            if (fullTarget.StartsWith(sysDirWithSep, StringComparison.OrdinalIgnoreCase) ||
                fullTarget.Equals(sysDirNoSep, StringComparison.OrdinalIgnoreCase))
            {
                throw new ArchiveException(ArchiveFailure.UnsafePath, $"沙箱已阻止向系统目录写入: {fullTarget}");
            }
        }

        // 3. 检查是否为已授权目录下的合法溢出清单文件 (.openunpack-*-manifest.json / .tmp)
        var fileName = Path.GetFileName(fullTarget);
        if (fileName.StartsWith(".openunpack-", StringComparison.OrdinalIgnoreCase) &&
            (fileName.EndsWith("-manifest.json", StringComparison.OrdinalIgnoreCase) ||
             fileName.EndsWith("-manifest.json.tmp", StringComparison.OrdinalIgnoreCase)))
        {
            for (var i = 0; i < current._manifestDirsWithSep.Length; i++)
            {
                if (fullTarget.StartsWith(current._manifestDirsWithSep[i], StringComparison.OrdinalIgnoreCase) ||
                    fullTarget.Equals(current._manifestDirsWithoutSep[i], StringComparison.OrdinalIgnoreCase))
                {
                    return;
                }
            }
        }

        // 4. 显式白名单文件放行 (O(1))
        if (current._allowedWriteFilesSet.Contains(fullTarget))
        {
            return;
        }

        // 5. 只读模式下，拦截一切非白名单写入
        if (current.IsReadOnly)
        {
            throw new ArchiveException(ArchiveFailure.UnsafePath, $"当前任务处于只读沙箱模式，已拦截对磁盘的写操作 ({fullTarget})。");
        }

        // 6. 允许的写根目录范围校验 (预格式化前缀匹配，无临时字符串分配)
        for (var i = 0; i < current._allowedRootsWithSep.Length; i++)
        {
            if (fullTarget.StartsWith(current._allowedRootsWithSep[i], StringComparison.OrdinalIgnoreCase) ||
                fullTarget.Equals(current._allowedRootsWithoutSep[i], StringComparison.OrdinalIgnoreCase))
            {
                return;
            }
        }

        throw new ArchiveException(ArchiveFailure.UnsafePath, $"沙箱已拒绝写操作：目标路径不在已授权的写范围目录内 ({fullTarget})。");
    }

    private static bool IsSubpathOrSame(string parent, string child)
    {
        var cleanParent = parent.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        var cleanChild = child.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        if (cleanChild.Equals(cleanParent, StringComparison.OrdinalIgnoreCase)) return true;
        return cleanChild.StartsWith(cleanParent + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase);
    }

    public void Dispose()
    {
        if (!_disposed)
        {
            CurrentScope.Value = _parentScope;
            _disposed = true;
        }
    }
}
