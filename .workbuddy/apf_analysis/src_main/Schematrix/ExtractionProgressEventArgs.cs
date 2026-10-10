using System;
using System.Runtime.CompilerServices;

namespace Schematrix;

public class ExtractionProgressEventArgs
{
	public string FileName;

	public long FileSize;

	public long BytesExtracted;

	public double PercentComplete;

	public bool ContinueOperation;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public ExtractionProgressEventArgs()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static ExtractionProgressEventArgs()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
