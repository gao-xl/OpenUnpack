using System;
using System.Runtime.CompilerServices;

namespace Schematrix;

public class RARFileInfo
{
	public string FileName;

	public bool ContinuedFromPrevious;

	public bool ContinuedOnNext;

	public bool IsDirectory;

	public long PackedSize;

	public long UnpackedSize;

	public int HostOS;

	public long FileCRC;

	public DateTime FileTime;

	public int VersionToUnpack;

	public int Method;

	public int FileAttributes;

	public long BytesExtracted;

	public FileFlag FileFlag;

	public double PercentComplete
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public RARFileInfo()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal double _FF00_FF00_FF00_FF00_FF00먀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal double _FF00_FF00_FF00_FF00_FF00묀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static RARFileInfo()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
