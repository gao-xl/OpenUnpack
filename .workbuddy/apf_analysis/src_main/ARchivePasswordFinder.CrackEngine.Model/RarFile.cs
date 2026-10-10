using System;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.CrackEngine.Model;

public class RarFile : CompressFileBase
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public RarFile(string filename)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static RarFile()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
