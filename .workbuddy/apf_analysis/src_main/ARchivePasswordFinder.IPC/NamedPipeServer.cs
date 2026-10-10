using System;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.IPC;

public class NamedPipeServer : IDisposable
{
	public void Dispose()
	{
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public byte[] Read()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public NamedPipeServer()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal object _FF00_FF00_FF00_FF00輀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal object _FF00_FF00_FF00_FF00退()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static NamedPipeServer()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
