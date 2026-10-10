using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Runtime.CompilerServices;
using ARchivePasswordFinder.Model;

namespace ARchivePasswordFinder.Helper;

public class CultureUtils
{
	[CompilerGenerated]
	private sealed class _003C_003Ec__DisplayClass1_0
	{
		public CultureInfo item;

		[MethodImpl(MethodImplOptions.NoInlining)]
		public _003C_003Ec__DisplayClass1_0()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal bool _003C_002Ecctor_003Eb__0(EZLanguage l)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal byte _FF00_FF00_0900(object P_0)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal byte _FF00_FF00_0A00(object P_0)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		static _003C_003Ec__DisplayClass1_0()
		{
			_003CModule_003E.ZYXDNGuarder.Startup();
		}
	}

	public static List<EZLanguage> SupportedLanguages;

	static CultureUtils()
	{
		SupportedLanguages = new List<EZLanguage>();
		try
		{
			DirectoryInfo directoryInfo = new DirectoryInfo((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00_FF00_FF00匀());
			if (!directoryInfo.Exists)
			{
				return;
			}
			FileInfo[] files = directoryInfo.GetFiles((string)_003CModule_003E.ZYXDNGuarder._FF00_FF00_FF00_FF00_FF00_FF00吀());
			for (int i = 0; i < files.Length; i++)
			{
				string fileNameWithoutExtension = Path.GetFileNameWithoutExtension(files[i].Name);
				try
				{
					_003C_003Ec__DisplayClass1_0 CS_0024_003C_003E8__locals4 = new _003C_003Ec__DisplayClass1_0();
					CS_0024_003C_003E8__locals4.item = new CultureInfo(fileNameWithoutExtension);
					if (!SupportedLanguages.Any([MethodImpl(MethodImplOptions.NoInlining)] (EZLanguage l) =>
					{
						throw new Exception("Error, DNGuard Runtime library not loaded!");
					}))
					{
						SupportedLanguages.Add(new EZLanguage(CS_0024_003C_003E8__locals4.item.TwoLetterISOLanguageName, CS_0024_003C_003E8__locals4.item.Name, CS_0024_003C_003E8__locals4.item.NativeName));
					}
				}
				catch (Exception value)
				{
					Console.WriteLine(value);
				}
			}
		}
		catch (Exception value2)
		{
			Console.WriteLine(value2);
		}
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static CultureInfo GetCurrentCulture()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public CultureUtils()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00唀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_FF00_FF00_FF00嘀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}
}
