using System;
using System.Runtime.CompilerServices;

namespace Schematrix;

public class NewVolumeEventArgs
{
	public string VolumeName;

	public bool ContinueOperation;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public NewVolumeEventArgs(string volumeName)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static NewVolumeEventArgs()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
