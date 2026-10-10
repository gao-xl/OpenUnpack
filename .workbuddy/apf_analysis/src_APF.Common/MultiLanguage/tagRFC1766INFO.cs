using System.Runtime.InteropServices;

namespace MultiLanguage;

[StructLayout(LayoutKind.Sequential, Pack = 4)]
public struct tagRFC1766INFO
{
	public uint lcid;

	[MarshalAs(UnmanagedType.ByValArray, SizeConst = 6)]
	public ushort[] wszRfc1766;

	[MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)]
	public ushort[] wszLocaleName;
}
