using System;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.CrackEngine;

public class ModelBase : NotifyPropertyChangedBase
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public ModelBase()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static ModelBase()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
