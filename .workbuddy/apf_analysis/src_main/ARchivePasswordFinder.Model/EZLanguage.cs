using System;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.Model;

public class EZLanguage
{
	public string NeutralCulture;

	public string Culture;

	public string Name;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public EZLanguage(string neutralCulture, string culture, string name)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static EZLanguage()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
