using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace OpenUnpack.Engine;

/// <summary>
/// Binds every directory component to a validated object until the operation finishes.
/// Directory handles deny deletion/rename sharing, while every child open/create uses its parent's
/// existing handle. Attribute-only reparse conversion cannot redirect these relative opens.
/// </summary>
public sealed class PathIdentityLease : IDisposable
{
    private const uint ReadAttributes = 0x80;
    private const uint ListDirectory = 0x1;
    private const uint Synchronize = 0x100000;
    private const uint GenericRead = 0x80000000;
    private const uint GenericWrite = 0x40000000;
    private const uint DeleteAccess = 0x10000;
    private const uint OpenExisting = 3;
    private const uint OpenReparsePoint = 0x00200000;
    private const uint BackupSemantics = 0x02000000;
    private const uint DirectoryAttribute = 0x10;
    private const uint ReparseAttribute = 0x400;
    private const int MaxPinnedDirectories = 100_000;
    private static readonly AsyncLocal<PathIdentityLease?> OperationScope = new();
    private readonly Dictionary<string, SafeFileHandle> _directories = new(StringComparer.OrdinalIgnoreCase);
    private readonly PathIdentityLease? _owner;
    private readonly PathIdentityLease? _previousScope;
    private readonly bool _isOperationScope;
    private bool _disposed;

    public PathIdentityLease()
    {
        _owner = OperationScope.Value;
        if (_owner is not null) _directories = _owner._directories;
    }

    private PathIdentityLease(bool operationScope)
    {
        _isOperationScope = operationScope;
        _previousScope = OperationScope.Value;
        OperationScope.Value = this;
    }

    public static PathIdentityLease EnterOperation() => new(operationScope: true);

    public void EnsureDirectory(string path, bool create = false)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        ObjectDisposedException.ThrowIf(_owner?._disposed == true, _owner!);
        var fullPath = Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));
        var root = Path.GetPathRoot(fullPath) ?? throw new ArchiveException(ArchiveFailure.UnsafePath, "路径缺少卷根目录。");
        // Count once before allocating ancestor substrings or opening/creating anything.
        // The extra absolute allowance covers an existing output root plus 128 entry components.
        int depth = 0;
        bool componentStart = true;
        foreach (var character in fullPath.AsSpan(Math.Min(root.Length, fullPath.Length)))
        {
            if (character is '\\' or '/') { componentStart = true; continue; }
            if (componentStart && ++depth > 256)
                throw new ArchiveException(ArchiveFailure.ResourceLimit, "操作路径层级超过安全上限。");
            componentStart = false;
        }
        var chain = new Stack<string>();
        var current = fullPath;
        while (!_directories.ContainsKey(current))
        {
            chain.Push(current);
            if (current.Equals(Path.TrimEndingDirectorySeparator(root), StringComparison.OrdinalIgnoreCase)) break;
            current = Path.GetDirectoryName(current) ?? throw new ArchiveException(ArchiveFailure.UnsafePath, "路径的祖先目录无效。");
        }
        while (chain.TryPop(out var directory))
        {
            if (_directories.Count >= MaxPinnedDirectories)
                throw new ArchiveException(ArchiveFailure.ResourceLimit, "锁定目录数量超过安全上限。");
            if (!OperatingSystem.IsWindows())
                throw new ArchiveException(ArchiveFailure.Unsupported, "当前平台不支持文件系统身份保护。");
            // FILE_READ_ATTRIBUTES alone does not participate in Windows share-state
            // accounting. FILE_LIST_DIRECTORY is essential to deny later delete opens.
            // Write sharing permits the filesystem's internal target-directory opens
            // during handle-based rename. Relative object lookup supplies reparse safety.
            SafeFileHandle handle;
            var parent = Path.GetDirectoryName(directory);
            if (directory.Equals(Path.TrimEndingDirectorySeparator(root), StringComparison.OrdinalIgnoreCase))
                handle = Open(directory, ReadAttributes | ListDirectory, FileShare.ReadWrite, OpenExisting);
            else
                handle = OpenRelative(_directories[parent!], Path.GetFileName(directory), ReadAttributes | ListDirectory | Synchronize,
                    FileShare.ReadWrite, 1, directory: true, allowMissing: create);
            if (handle.IsInvalid)
            {
                int error = Marshal.GetLastWin32Error();
                handle.Dispose();
                // Only a missing child may be created; its already pinned parent cannot be redirected.
                if (!create || error is not (2 or 3) || directory.Equals(root, StringComparison.OrdinalIgnoreCase))
                    throw IoFailure("无法锁定操作目录。", error);
                WriteScopeGuard.DemandWriteAccess(directory);
                handle = OpenRelative(_directories[parent!], Path.GetFileName(directory), ReadAttributes | ListDirectory | Synchronize,
                    FileShare.ReadWrite, 3, directory: true);
                if (handle.IsInvalid)
                {
                    error = Marshal.GetLastWin32Error(); handle.Dispose();
                    throw IoFailure("无法锁定新建操作目录。", error);
                }
            }
            try { DemandType(handle, directory: true); _directories.Add(directory, handle); }
            catch { handle.Dispose(); throw; }
        }
    }

    public FileStream OpenRead(string path)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        var full = Path.GetFullPath(path);
        EnsureDirectory(Path.GetDirectoryName(full)!);
        var handle = OpenRelative(_directories[Path.TrimEndingDirectorySeparator(Path.GetDirectoryName(full)!)],
            Path.GetFileName(full), GenericRead, FileShare.Read, 1, directory: false);
        if (handle.IsInvalid)
        { int error = Marshal.GetLastWin32Error(); handle.Dispose(); throw IoFailure("无法锁定来源文件。", error); }
        try { DemandType(handle, directory: false); return new FileStream(handle, FileAccess.Read); }
        catch { handle.Dispose(); throw; }
    }

    public CreatedOutputFile CreateFile(string path)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        var full = Path.GetFullPath(path);
        WriteScopeGuard.DemandWriteAccess(full);
        EnsureDirectory(Path.GetDirectoryName(full)!, create: true);
        // CREATE_NEW never follows/replaces an existing leaf. DELETE permission permits
        // failure cleanup through this exact object rather than reopening its pathname.
        var handle = OpenRelative(_directories[Path.TrimEndingDirectorySeparator(Path.GetDirectoryName(full)!)],
            Path.GetFileName(full), GenericWrite | DeleteAccess, FileShare.None, 2, directory: false);
        if (handle.IsInvalid)
        { int error = Marshal.GetLastWin32Error(); handle.Dispose(); throw IoFailure("无法独占创建目标文件。", error); }
        try { DemandType(handle, directory: false); return new CreatedOutputFile(new FileStream(handle, FileAccess.Write)); }
        catch { DeleteCreatedObject(handle); handle.Dispose(); throw; }
    }

    public FileStream OpenAlternateStream(string basePath, bool write)
    {
        var full = Path.GetFullPath(basePath);
        if (write) WriteScopeGuard.DemandWriteAccess(full);
        EnsureDirectory(Path.GetDirectoryName(full)!);
        var handle = OpenRelative(_directories[Path.TrimEndingDirectorySeparator(Path.GetDirectoryName(full)!)],
            Path.GetFileName(full) + ZoneIdentifier.StreamSuffix, write ? GenericWrite : GenericRead,
            write ? FileShare.ReadWrite | FileShare.Delete : FileShare.Read, write ? 5u : 1u, directory: false);
        try { DemandType(handle, directory: false); return new FileStream(handle, write ? FileAccess.Write : FileAccess.Read); }
        catch { handle.Dispose(); throw; }
    }

    public void Publish(CreatedOutputFile output, string path)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        var full = Path.GetFullPath(path);
        WriteScopeGuard.DemandWriteAccess(full);
        EnsureDirectory(Path.GetDirectoryName(full)!);
        // FileRenameInfo uses a HANDLE-aligned root-directory field; its variable
        // name begins immediately after the DWORD length (20 bytes on Windows x64).
        var parent = _directories[Path.TrimEndingDirectorySeparator(Path.GetDirectoryName(full)!)];
        DemandType(parent, directory: true);
        var name = Encoding.Unicode.GetBytes(Path.GetFileName(full));
        int rootOffset = IntPtr.Size == 8 ? 8 : 4;
        int lengthOffset = rootOffset + IntPtr.Size;
        int nameOffset = lengthOffset + sizeof(uint);
        var buffer = Marshal.AllocHGlobal(nameOffset + name.Length);
        try
        {
            Marshal.Copy(new byte[nameOffset], 0, buffer, nameOffset);
            Marshal.WriteInt32(buffer, 0, 0); // ReplaceIfExists=false.
            Marshal.WriteIntPtr(buffer, rootOffset, parent.DangerousGetHandle());
            Marshal.WriteInt32(buffer, lengthOffset, name.Length);
            Marshal.Copy(name, 0, IntPtr.Add(buffer, nameOffset), name.Length);
            int status = NtSetInformationFile(output.Stream.SafeFileHandle, out _, buffer, (uint)(nameOffset + name.Length), 10);
            if (status < 0)
                throw IoFailure("无法原子发布目标文件。", unchecked((int)RtlNtStatusToDosError(status)));
        }
        finally { Marshal.FreeHGlobal(buffer); }
    }

    private static void DemandType(SafeFileHandle handle, bool directory)
    {
        if (!GetFileInformationByHandleEx(handle, 9, out FileAttributeTagInfo information, 8))
            throw IoFailure("无法验证文件系统对象。", Marshal.GetLastWin32Error());
        if ((information.Attributes & ReparseAttribute) != 0 || ((information.Attributes & DirectoryAttribute) != 0) != directory)
            throw new ArchiveException(ArchiveFailure.UnsafePath, "路径包含文件系统链接或对象类型发生变化，已拒绝操作。");
    }

    private static SafeFileHandle Open(string path, uint access, FileShare share, uint disposition) =>
        CreateFile(path, access, (uint)share, IntPtr.Zero, disposition, OpenReparsePoint | BackupSemantics, IntPtr.Zero);

    private static SafeFileHandle OpenRelative(SafeFileHandle parent, string name, uint access, FileShare share,
        uint disposition, bool directory, bool allowMissing = false)
    {
        DemandType(parent, directory: true);
        var nameBuffer = Marshal.StringToHGlobalUni(name);
        var unicodeBuffer = Marshal.AllocHGlobal(Marshal.SizeOf<UnicodeString>());
        try
        {
            var unicode = new UnicodeString { Length = checked((ushort)(name.Length * 2)), MaximumLength = checked((ushort)(name.Length * 2 + 2)), Buffer = nameBuffer };
            Marshal.StructureToPtr(unicode, unicodeBuffer, false);
            var attributes = new ObjectAttributes
            {
                Length = (uint)Marshal.SizeOf<ObjectAttributes>(), RootDirectory = parent.DangerousGetHandle(),
                ObjectName = unicodeBuffer, Attributes = 0x40 | 0x1000 // CASE_INSENSITIVE | DONT_REPARSE.
            };
            uint options = 0x00200000 | 0x20 | (directory ? 0x1u : 0x40u); // OPEN_REPARSE_POINT | SYNCHRONOUS_IO_NONALERT | type.
            int status = NtCreateFile(out var handle, access | Synchronize | ReadAttributes, ref attributes, out _, IntPtr.Zero,
                0, (uint)share, disposition, options, IntPtr.Zero, 0);
            if (status >= 0) return handle;
            int error = unchecked((int)RtlNtStatusToDosError(status));
            handle?.Dispose();
            if (allowMissing && error is 2 or 3)
            { Marshal.SetLastPInvokeError(error); return new SafeFileHandle(new IntPtr(-1), ownsHandle: true); }
            if (unchecked((uint)status) == 0xC000050B || error is 4390 or 4393 or 1920)
                throw new ArchiveException(ArchiveFailure.UnsafePath, "路径包含文件系统链接，已拒绝操作。");
            throw IoFailure("无法通过已锁定目录访问对象。", error);
        }
        finally { Marshal.FreeHGlobal(unicodeBuffer); Marshal.FreeHGlobal(nameBuffer); }
    }

    private static ArchiveException IoFailure(string message, int error) =>
        new(ArchiveFailure.IoError, message, new Win32Exception(error));

    private static void DeleteCreatedObject(SafeFileHandle handle)
    {
        int delete = 1;
        if (!SetFileInformationByHandle(handle, 4, ref delete, sizeof(int)))
            SafeDiagnostics.Emit("extract", "cleanup", "IoError", new Win32Exception(Marshal.GetLastWin32Error()));
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        if (_owner is not null) return; // The request retains borrowed directory identities.
        foreach (var handle in _directories.Values.Reverse()) handle.Dispose();
        _directories.Clear();
        if (_isOperationScope) OperationScope.Value = _previousScope;
    }

    public sealed class CreatedOutputFile : IDisposable
    {
        private bool _keep;
        private bool _disposed;
        internal CreatedOutputFile(FileStream stream) => Stream = stream;
        public FileStream Stream { get; }
        public void Keep() => _keep = true;
        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            try
            {
                if (!_keep)
                {
                    DeleteCreatedObject(Stream.SafeFileHandle);
                }
            }
            finally { Stream.Dispose(); }
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct FileAttributeTagInfo { public uint Attributes; public uint ReparseTag; }
    [StructLayout(LayoutKind.Sequential)]
    private struct UnicodeString { public ushort Length; public ushort MaximumLength; public IntPtr Buffer; }
    [StructLayout(LayoutKind.Sequential)]
    private struct ObjectAttributes
    {
        public uint Length; public IntPtr RootDirectory; public IntPtr ObjectName; public uint Attributes;
        public IntPtr SecurityDescriptor; public IntPtr SecurityQualityOfService;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct IoStatusBlock { public IntPtr Status; public UIntPtr Information; }
    [DllImport("kernel32.dll", EntryPoint = "CreateFileW", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern SafeFileHandle CreateFile(string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandleEx(SafeFileHandle handle, int informationClass, out FileAttributeTagInfo information, uint size);
    [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetFileInformationByHandle(SafeFileHandle handle, int informationClass, ref int information, uint size);
    [DllImport("ntdll.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int NtCreateFile(out SafeFileHandle handle, uint access, ref ObjectAttributes attributes,
        out IoStatusBlock ioStatus, IntPtr allocationSize, uint fileAttributes, uint share, uint disposition, uint options, IntPtr ea, uint eaLength);
    [DllImport("ntdll.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern uint RtlNtStatusToDosError(int status);
    [DllImport("ntdll.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int NtSetInformationFile(SafeFileHandle handle, out IoStatusBlock ioStatus, IntPtr information, uint size, int informationClass);
}
