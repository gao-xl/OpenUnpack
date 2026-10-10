using System;
using System.Runtime.CompilerServices;

namespace CryptSharp.Internal;

internal static class Security
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void Clear(Array array)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static byte[] GenerateRandomBytes(int count)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void 䠀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void 䤀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 䬀(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 䰀(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static Security()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
