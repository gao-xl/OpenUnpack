using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.CompilerServices;
using Newtonsoft.Json;

namespace ARchivePasswordFinder.Config;

public class ExclusionFile
{
	[DebuggerBrowsable(DebuggerBrowsableState.Never)]
	[CompilerGenerated]
	private static List<string> ExclusionFileNames__BackingField;

	[DebuggerBrowsable(DebuggerBrowsableState.Never)]
	[CompilerGenerated]
	private static List<string> ExclusionFileTypes__BackingField;

	private static object exFilenamesconfname;

	private static object exFileTypesconfname;

	public static List<string> ExclusionFileNames
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		[CompilerGenerated]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
		[MethodImpl(MethodImplOptions.NoInlining)]
		[CompilerGenerated]
		set
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	public static List<string> ExclusionFileTypes
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		[CompilerGenerated]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
		[MethodImpl(MethodImplOptions.NoInlining)]
		[CompilerGenerated]
		set
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	static ExclusionFile()
	{
		ExclusionFileNames__BackingField = new List<string>();
		ExclusionFileTypes__BackingField = new List<string>();
		exFilenamesconfname = _003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00혀();
		exFileTypesconfname = _003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00휀();
		if (!Directory.Exists((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00謀()))
		{
			Directory.CreateDirectory((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00謀());
		}
		if (File.Exists((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00耀() + (string)exFilenamesconfname))
		{
			ExclusionFileNames = JsonConvert.DeserializeObject<List<string>>(File.ReadAllText((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00耀() + (string)exFilenamesconfname));
		}
		if (File.Exists((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00耀() + (string)exFileTypesconfname))
		{
			ExclusionFileTypes = JsonConvert.DeserializeObject<List<string>>(File.ReadAllText((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00耀() + (string)exFileTypesconfname));
		}
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void Save()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public ExclusionFile()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<string> _FF00_FF00_FF00_FF00찀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<string> _FF00_FF00_FF00_FF00촀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<string> _FF00_FF00_FF00_FF00츀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<string> _FF00_FF00_FF00_FF00케()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<string> _FF00_FF00_FF00_FF00퀀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<string> _FF00_FF00_FF00_FF00턀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00툀(List<string> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00팀(List<string> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00퐀(List<string> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00픀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}
}
