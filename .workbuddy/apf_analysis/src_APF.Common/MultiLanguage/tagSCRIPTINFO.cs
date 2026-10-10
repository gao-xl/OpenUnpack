using System.Runtime.InteropServices;

namespace MultiLanguage;

[StructLayout(LayoutKind.Sequential, Pack = 4)]
public struct tagSCRIPTINFO
{
	public byte ScriptId;

	public uint uiCodePage;

	[MarshalAs(UnmanagedType.ByValArray, SizeConst = 48)]
	public ushort[] wszDescription;

	[MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)]
	public ushort[] wszFixedWidthFont;

	[MarshalAs(UnmanagedType.ByValArray, SizeConst = 32)]
	public ushort[] wszProportionalFont;
}
