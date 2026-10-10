using System;
using System.Runtime.CompilerServices;

namespace APF.Common.Helper;

internal class SafeProxy
{
	private const uint Poly = 3988292384u;

	private readonly object _table;

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal SafeProxy()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected void Init(uint poly)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public uint Append(uint crc, byte[] input, int offset, int length)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal uint _FF00_2100(uint P_0, object P_1, uint P_2, uint P_3)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal uint _FF00_2200(uint P_0, object P_1, uint P_2, uint P_3)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected void _FF00_2300(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected void _FF00_2400(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static SafeProxy()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
