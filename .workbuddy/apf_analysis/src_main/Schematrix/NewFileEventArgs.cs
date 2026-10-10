using System;
using System.Runtime.CompilerServices;

namespace Schematrix;

public class NewFileEventArgs
{
	public RARFileInfo fileInfo;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public NewFileEventArgs(RARFileInfo fileInfo)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static NewFileEventArgs()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
