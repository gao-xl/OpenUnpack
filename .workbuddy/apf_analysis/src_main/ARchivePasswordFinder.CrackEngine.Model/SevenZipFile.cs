using System;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.CrackEngine.Model;

public class SevenZipFile : CompressFileBase
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public SevenZipFile(string filename)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static SevenZipFile()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
