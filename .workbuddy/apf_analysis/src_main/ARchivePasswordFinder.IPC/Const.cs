using System;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.IPC;

internal class Const
{
	internal const string _PipeName = "ARFNamedPipe7";

	[MethodImpl(MethodImplOptions.NoInlining)]
	public Const()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static Const()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
