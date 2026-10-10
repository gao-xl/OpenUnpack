using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.Core;

public class PasswordComparer : IComparer<Password>
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public int Compare(Password x, Password y)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public PasswordComparer()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal sealed uint _FF00_FF00_FF00倀(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal sealed uint _FF00_FF00_FF00儀(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal sealed uint _FF00_FF00_FF00刀(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static PasswordComparer()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
