using System;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.Core;

public struct ProgressInformation
{
	public int Completed;

	public string CurrentItem;

	public int Total;

	public Guid taskGuid;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public ProgressInformation(Guid taskGuid)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public int GetPercentage()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal uint _FF00_FF00_FF00㐀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal uint _FF00_FF00_FF00㔀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static ProgressInformation()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
