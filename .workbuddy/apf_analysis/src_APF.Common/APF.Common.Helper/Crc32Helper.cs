using System;
using System.Runtime.CompilerServices;

namespace APF.Common.Helper;

internal class Crc32Helper
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public Crc32Helper()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static Crc32Helper()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
