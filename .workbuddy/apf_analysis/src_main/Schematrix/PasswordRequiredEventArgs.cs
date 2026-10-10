using System;
using System.Runtime.CompilerServices;

namespace Schematrix;

public class PasswordRequiredEventArgs
{
	public string Password;

	public bool ContinueOperation;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public PasswordRequiredEventArgs()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static PasswordRequiredEventArgs()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
