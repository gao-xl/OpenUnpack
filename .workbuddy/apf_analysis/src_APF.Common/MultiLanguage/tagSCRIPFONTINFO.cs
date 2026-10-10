using System.Runtime.InteropServices;

namespace MultiLanguage;

[StructLayout(LayoutKind.Sequential, Pack = 8)]
public struct tagSCRIPFONTINFO
{
	public long scripts;

	[MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)]
	public ushort[] wszFont;
}
