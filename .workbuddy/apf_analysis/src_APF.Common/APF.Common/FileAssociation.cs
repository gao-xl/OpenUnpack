using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

namespace APF.Common;

public static class FileAssociation
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public static bool DoesFileAssociationExists(string extension)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void CreateFileAssociation(string extension, string key, string description, string path)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void AssociateFileIcon(string extension, string progID, string icon)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[DllImport("Kernel32.dll")]
	private static extern uint GetShortPathName(string lpszLongPath, [Out] StringBuilder lpszShortPath, uint cchBuffer);

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object ToShortPathName(object longName)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object GetClasses()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object _FF00䴀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object _FF00一(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object _FF00伀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00儀(object P_0, object P_1, object P_2)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object _FF00刀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static byte _FF00匀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00圀(object P_0, object P_1, object P_2, object P_3)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00堀(object P_0, object P_1, object P_2, object P_3)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static FileAssociation()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
