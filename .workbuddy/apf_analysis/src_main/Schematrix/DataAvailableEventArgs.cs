using System;
using System.Runtime.CompilerServices;

namespace Schematrix;

public class DataAvailableEventArgs
{
	public readonly byte[] Data;

	public bool ContinueOperation;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public DataAvailableEventArgs(byte[] data)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static DataAvailableEventArgs()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
