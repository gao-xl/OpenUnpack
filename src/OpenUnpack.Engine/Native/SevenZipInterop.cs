using System.Runtime.InteropServices;

namespace OpenUnpack.Engine.Native;

// Public property IDs from the pinned 7-Zip 26.04 CPP/7zip/PropID.h.
// kpidEncrypted is 15; 19 is kpidCRC and must never be read as a Boolean.
internal static class ArchiveProperty
{
    internal const uint Path = 3, Name = 4, IsDirectory = 6, Size = 7, Attributes = 9,
        Encrypted = 15, IsAnti = 21, PosixAttributes = 53, SymbolicLink = 54,
        IsAlternateStream = 63, ErrorFlags = 71, WarningFlags = 72, HardLink = 90;
}

internal static class ArchiveErrorFlags
{
    internal const ulong EncryptedHeadersError = 1UL << 2,
        UnsupportedMethod = 1UL << 7, UnsupportedFeature = 1UL << 8;
}

// ABI declarations follow the public 7-Zip SDK interfaces in IArchive.h, IStream.h and IPassword.h.
[ComVisible(true), Guid("23170F69-40C1-278A-0000-000300010000"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface ISequentialInStream
{
    [PreserveSig] int Read(IntPtr data, uint size, IntPtr processedSize);
}
[ComVisible(true), Guid("23170F69-40C1-278A-0000-000300030000"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IInStream : ISequentialInStream
{
    [PreserveSig] new int Read(IntPtr data, uint size, IntPtr processedSize);
    [PreserveSig] int Seek(long offset, uint seekOrigin, IntPtr newPosition);
}
[ComVisible(true), Guid("23170F69-40C1-278A-0000-000300020000"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface ISequentialOutStream
{
    [PreserveSig] int Write(IntPtr data, uint size, IntPtr processedSize);
}
[ComVisible(true), Guid("23170F69-40C1-278A-0000-000600100000"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IArchiveOpenCallback
{
    [PreserveSig] int SetTotal(IntPtr files, IntPtr bytes);
    [PreserveSig] int SetCompleted(IntPtr files, IntPtr bytes);
}
[ComVisible(true), Guid("23170F69-40C1-278A-0000-000600300000"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IArchiveOpenVolumeCallback
{
    [PreserveSig] int GetProperty(uint propertyId, out PropVariant value);
    [PreserveSig] int GetStream([MarshalAs(UnmanagedType.LPWStr)] string name, [MarshalAs(UnmanagedType.Interface)] out IInStream? stream);
}
[ComVisible(true), Guid("23170F69-40C1-278A-0000-000500100000"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface ICryptoGetTextPassword
{
    [PreserveSig] int CryptoGetTextPassword([MarshalAs(UnmanagedType.BStr)] out string? password);
}
[ComVisible(true), Guid("23170F69-40C1-278A-0000-000600200000"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IArchiveExtractCallback
{
    [PreserveSig] int SetTotal(ulong total);
    [PreserveSig] int SetCompleted(IntPtr completeValue);
    [PreserveSig] int GetStream(uint index, [MarshalAs(UnmanagedType.Interface)] out ISequentialOutStream? stream, int askExtractMode);
    [PreserveSig] int PrepareOperation(int askExtractMode);
    [PreserveSig] int SetOperationResult(int result);
}
[ComImport, Guid("23170F69-40C1-278A-0000-000600030000"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface ISetProperties
{
    [PreserveSig] int SetProperties([MarshalAs(UnmanagedType.LPArray, ArraySubType = UnmanagedType.LPWStr)] string[] names, [In] PropVariant[] values, int numProps);
}
[ComImport, Guid("23170F69-40C1-278A-0000-000600600000"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IInArchive
{
    [PreserveSig] int Open([MarshalAs(UnmanagedType.Interface)] IInStream stream, IntPtr maxCheckStartPosition, [MarshalAs(UnmanagedType.Interface)] IArchiveOpenCallback callback);
    [PreserveSig] int Close();
    [PreserveSig] int GetNumberOfItems(out uint numItems);
    [PreserveSig] int GetProperty(uint index, uint propertyId, out PropVariant value);
    [PreserveSig] int Extract(IntPtr indices, uint numItems, int testMode, [MarshalAs(UnmanagedType.Interface)] IArchiveExtractCallback callback);
    [PreserveSig] int GetArchiveProperty(uint propertyId, out PropVariant value);
    [PreserveSig] int GetNumberOfProperties(out uint numProps);
    [PreserveSig] int GetPropertyInfo(uint index, [MarshalAs(UnmanagedType.BStr)] out string name, out uint propertyId, out ushort varType);
    [PreserveSig] int GetNumberOfArchiveProperties(out uint numProps);
    [PreserveSig] int GetArchivePropertyInfo(uint index, [MarshalAs(UnmanagedType.BStr)] out string name, out uint propertyId, out ushort varType);
}

[StructLayout(LayoutKind.Explicit, Size = 24)]
public struct PropVariant
{
    [FieldOffset(0)] public ushort Type;
    [FieldOffset(8)] public IntPtr Pointer;
    [FieldOffset(8)] public ulong Unsigned;
    [FieldOffset(8)] public long Signed;
    [FieldOffset(8)] public short Boolean;
    public readonly string? Text => Type == 8 && Pointer != IntPtr.Zero ? Marshal.PtrToStringBSTR(Pointer) : null;
    public readonly bool Bool => Type == 11 && Boolean != 0;
    public readonly ulong Number => Type switch { 19 => (uint)Unsigned, 21 => Unsigned, 3 => (uint)Signed, _ => 0 };
    public void Clear() => PropVariantClear(ref this);
    public static PropVariant FromText(string text) => new() { Type = 8, Pointer = Marshal.StringToBSTR(text) };
    public static PropVariant FromNumber(ulong number) => new() { Type = 21, Unsigned = number };
    public static PropVariant FromUInt32(uint number) => new() { Type = 19, Unsigned = number };
    [DllImport("ole32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int PropVariantClear(ref PropVariant value);
}

internal static class NativeResult
{
    internal const int Ok = 0, False = 1, Abort = unchecked((int)0x80004004), Fail = unchecked((int)0x80004005);
    internal static int FromException(Exception exception) => exception is OperationCanceledException ? Abort : Fail;
}
