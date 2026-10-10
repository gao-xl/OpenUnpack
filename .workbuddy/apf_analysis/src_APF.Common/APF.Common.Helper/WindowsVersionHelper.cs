using System;
using System.Diagnostics;
using System.Runtime.CompilerServices;

namespace APF.Common.Helper;

internal class WindowsVersionHelper
{
	[DebuggerBrowsable(DebuggerBrowsableState.Never)]
	[CompilerGenerated]
	private static readonly IntPtr IsWindows10__BackingField;

	protected internal static bool IsWindows10
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		[CompilerGenerated]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	static WindowsVersionHelper()
	{
		IsWindows10__BackingField = (IntPtr)((Environment.OSVersion.Version.Major == 10) ? 1 : 0);
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public WindowsVersionHelper()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected internal static byte _FF00_FF00_FF00ꔀ()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected internal static byte _FF00_FF00_FF00ꘀ()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}
}
