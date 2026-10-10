using System;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder;

internal class SystemCheck
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public static bool CheckCompoent()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public SystemCheck()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static byte _FF00_FF00_FF00_FF00_FF00騀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static byte _FF00_FF00_FF00_FF00_FF00鬀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static byte _FF00_FF00_FF00_FF00_FF00鰀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static SystemCheck()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
