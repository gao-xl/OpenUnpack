using System;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.CrackEngine.Model;

public class ZipFileCustom : CompressFileBase
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public ZipFileCustom(string filename)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static ZipFileCustom()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
