using System;
using System.Runtime.CompilerServices;

namespace Schematrix;

public class MissingVolumeEventArgs
{
	public string VolumeName;

	public bool ContinueOperation;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public MissingVolumeEventArgs(string volumeName)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static MissingVolumeEventArgs()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
