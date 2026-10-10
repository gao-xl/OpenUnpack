using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.CompilerServices;
using APF.Data;
using ARchivePasswordFinder.Config;
using ARchivePasswordFinder.Core;
using ARchivePasswordFinder.Extractors;

namespace ARchivePasswordFinder;

internal class GlobalVal
{
	internal static List<string> SupportExt = new List<string>
	{
		(string)_003CModule_003E.ZYXDNGuarder._FF00밀(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00됀(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00봀(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00鸀(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00鼀(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00븀(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00뼀(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00쀀(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00섀(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00숀(),
		(string)_003CModule_003E.ZYXDNGuarder._FF00쌀()
	};

	private static List<BaseExtractor> extractorList;

	private static object passwords;

	private static object taskManager;

	private static object versionInfo;

	[DebuggerBrowsable(DebuggerBrowsableState.Never)]
	[CompilerGenerated]
	private static object GlobleDataContext__BackingField = new DataContext();

	public static DataContext GlobleDataContext
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

	public static TaskManager GTaskManager
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	public static PasswordList Passwords
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	public static ApplicationSettings Settings
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	public static List<BaseExtractor> ExtractorList
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	public static FileVersionInfo VersionInfo
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public GlobalVal()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00_2F00()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00_3000()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00_3100()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00_3200()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00_3300()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00㐀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<BaseExtractor> _FF00_FF00_FF00_FF00_FF00_FF00㔀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<BaseExtractor> _FF00_FF00_FF00_FF00_FF00_FF00㘀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<BaseExtractor> _FF00_FF00_FF00_FF00_FF00_FF00㜀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00_FF00_FF00㠀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00_FF00_FF00㤀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00㨀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00㬀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00㰀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00㴀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}
}
