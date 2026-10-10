using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.CompilerServices;
using Newtonsoft.Json;

namespace ARchivePasswordFinder.Config;

public class Wordlists
{
	public class Wordlist
	{
		public int Order
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

		public string Text
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

		[MethodImpl(MethodImplOptions.NoInlining)]
		public Wordlist()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		public Wordlist(int order, string text)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object Ā()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object Ȁ()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _0300()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal uint Ѐ()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal uint Ԁ()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal void _0600(object P_0)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal void _0700(uint P_0)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		static Wordlist()
		{
			_003CModule_003E.ZYXDNGuarder.Startup();
		}
	}

	[CompilerGenerated]
	[DebuggerBrowsable(DebuggerBrowsableState.Never)]
	private static List<Wordlist> WordlistFileNames__BackingField;

	private static object listfilename;

	public static List<Wordlist> WordlistFileNames
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

	static Wordlists()
	{
		WordlistFileNames__BackingField = new List<Wordlist>();
		listfilename = _003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00_FF00ᜀ();
		if (!Directory.Exists((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00謀()))
		{
			Directory.CreateDirectory((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00謀());
		}
		if (File.Exists((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00耀() + (string)listfilename))
		{
			WordlistFileNames = JsonConvert.DeserializeObject<List<Wordlist>>(File.ReadAllText((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00耀() + (string)listfilename));
		}
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void Save()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public Wordlists()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00_FF00ᔀ()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00_FF00ᘀ()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<Wordlist> _FF00_FF00_FF00_FF00_FF00_1800()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static List<Wordlist> _FF00_FF00_FF00_FF00_FF00ᤀ()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00_FF00ᨀ(List<Wordlist> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00_FF00_1B00(List<Wordlist> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_FF00_FF00ᰀ(List<Wordlist> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}
}
