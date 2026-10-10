using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace APF.Common;

public class FileSystemUtils
{
	private static readonly object TEXT_EXTS = new string[22]
	{
		(string)_003CModule_003E.ZYXDNGuarder._FF00_E100(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_E200(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_E300(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_E400(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_E500(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_E600(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_E700(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_E800(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_E900(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_EA00(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_EB00(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_EC00(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_ED00(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_EE00(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_EF00(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_F000(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_F100(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_F200(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_F300(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_F400(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_F500(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_F600()
	};

	private static readonly object BINARY_EXTS = new string[17]
	{
		(string)_003CModule_003E.ZYXDNGuarder._FF00_F700(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_F800(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00豈(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00切(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00ﬀ(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00ﰀ(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00ﴀ(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FE00(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00Ā(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00Ȁ(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_0300(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00Ѐ(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00Ԁ(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_0600(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_0700(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00ࠀ()
	};

	private static readonly List<string> textExts = new List<string>((IEnumerable<string>)TEXT_EXTS);

	private static readonly List<string> bianryExts = new List<string>((IEnumerable<string>)BINARY_EXTS);

	[DllImport("kernel32.dll", CharSet = CharSet.Auto, SetLastError = true)]
	[return: MarshalAs(UnmanagedType.Bool)]
	public static extern bool GetDiskFreeSpaceEx(string lpDirectoryName, out ulong lpFreeBytesAvailable, out ulong lpTotalNumberOfBytes, out ulong lpTotalNumberOfFreeBytes);

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static bool DriveFreeBytes(string folderName, out ulong freespace)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static bool IsBinaryFile(string filePath, int sampleSize = 10240)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void CloneDirectory(string SourcePath, string DestinationPath, bool overwriteexisting, string ignore)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void CreateDirectory(string dirPath)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void CreateDirectory(DirectoryInfo dirinfo)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static string GetUtf8ValdPath(string defaultEncodingPath)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public FileSystemUtils()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FFFD(object P_0, object P_1, byte P_2, object P_3)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FFFD(object P_0, object P_1, byte P_2, object P_3)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_E000(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_0A00(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_0B00(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_0C00(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static byte _FF00_FF00_0E00(object P_0, IntPtr P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static byte _FF00_FF00ᄀ(object P_0, uint P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static byte _FF00_FF00ሀ(object P_0, uint P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00ጀ(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_1400(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}
}
