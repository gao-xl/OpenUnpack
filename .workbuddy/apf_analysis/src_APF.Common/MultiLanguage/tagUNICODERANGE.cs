using System.Runtime.InteropServices;

namespace MultiLanguage;

[StructLayout(LayoutKind.Sequential, Pack = 2)]
public struct tagUNICODERANGE
{
	public ushort wcFrom;

	public ushort wcTo;
}
