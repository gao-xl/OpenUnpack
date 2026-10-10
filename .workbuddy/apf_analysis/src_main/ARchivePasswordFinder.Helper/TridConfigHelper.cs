using System;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.Helper;

internal class TridConfigHelper
{
	private static IntPtr init;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void Init()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public TridConfigHelper()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00樀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00欀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static TridConfigHelper()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
