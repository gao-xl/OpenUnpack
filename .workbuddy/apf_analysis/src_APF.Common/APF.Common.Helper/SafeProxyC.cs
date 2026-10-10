using System;
using System.Runtime.CompilerServices;

namespace APF.Common.Helper;

internal class SafeProxyC : SafeProxy
{
	private const uint Poly = 2197175160u;

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal SafeProxyC()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static SafeProxyC()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
