using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

namespace APF.Common.Helper;

public static class Pathing
{
	[DllImport("mpr.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	public static extern int WNetGetConnection([MarshalAs(UnmanagedType.LPTStr)] string localName, [MarshalAs(UnmanagedType.LPTStr)] StringBuilder remoteName, ref int length);

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static string GetUNCPath(string originalPath)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 簀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 紀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object 縀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static Pathing()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
