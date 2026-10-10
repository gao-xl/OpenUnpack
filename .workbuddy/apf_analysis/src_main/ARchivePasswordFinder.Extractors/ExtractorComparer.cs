using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.Extractors;

public class ExtractorComparer : IComparer<BaseExtractor>
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public int Compare(BaseExtractor x, BaseExtractor y)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public ExtractorComparer()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal sealed uint _FF00_FF00_FF00_F700(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal sealed uint _FF00_FF00_FF00_F800(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static ExtractorComparer()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
