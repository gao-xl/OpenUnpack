using System;
using System.Runtime.CompilerServices;

namespace CryptSharp;

public static class SCrypt
{
	private const int hLen = 32;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static byte[] ComputeDerivedKey(byte[] key, byte[] salt, int cost, int blockSize, int parallel, int? maxThreads, int derivedKeyLength)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static byte[] GetEffectivePbkdf2Salt(byte[] key, byte[] salt, int cost, int blockSize, int parallel, int? maxThreads)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static Pbkdf2 GetStream(byte[] key, byte[] salt, int cost, int blockSize, int parallel, int? maxThreads)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object MFcrypt(object P, object S, uint cost, uint blockSize, uint parallel, int? maxThreads)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void ThreadSMixCalls(object B0, uint MFLen, uint cost, uint blockSize, uint parallel, uint maxThreads)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void SMix(object B, uint Boffset, object Bp, uint Bpoffset, uint N, uint r)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void BlockMix(object B, uint Boffset, object Bp, uint Bpoffset, object x, object y, object scratch, uint r)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void 蔀(object P_0, uint P_1, uint P_2, uint P_3, uint P_4, uint P_5)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void 蘀(object P_0, uint P_1, uint P_2, uint P_3, uint P_4, uint P_5)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 蠀(object P_0, object P_1, uint P_2, uint P_3, uint P_4, int? P_5, uint P_6)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 褀(object P_0, object P_1, uint P_2, uint P_3, uint P_4, int? P_5, uint P_6)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 言(object P_0, object P_1, uint P_2, uint P_3, uint P_4, int? P_5, uint P_6)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object 退(object P_0, object P_1, uint P_2, uint P_3, uint P_4, int? P_5)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object 鄀(object P_0, object P_1, uint P_2, uint P_3, uint P_4, int? P_5)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void 鈀(object P_0, uint P_1, object P_2, uint P_3, uint P_4, uint P_5)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void 錀(object P_0, uint P_1, object P_2, uint P_3, uint P_4, uint P_5)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void 鐀(object P_0, uint P_1, object P_2, uint P_3, uint P_4, uint P_5)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void 销(object P_0, uint P_1, object P_2, uint P_3, object P_4, object P_5, object P_6, uint P_7)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void 阀(object P_0, uint P_1, object P_2, uint P_3, object P_4, object P_5, object P_6, uint P_7)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void 需(object P_0, uint P_1, object P_2, uint P_3, object P_4, object P_5, object P_6, uint P_7)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 頀(object P_0, object P_1, uint P_2, uint P_3, uint P_4, int? P_5)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 餀(object P_0, object P_1, uint P_2, uint P_3, uint P_4, int? P_5)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 鰀(object P_0, object P_1, uint P_2, uint P_3, uint P_4, int? P_5)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static SCrypt()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
