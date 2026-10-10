using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;

namespace Crypt;

public class WinCrypt
{
	private object _cryptography;

	public DeStep Step;

	public float Progress
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public WinCrypt()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public Dictionary<string, long> DecryptFileTable(string filename, string key, string IVFilename = null)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public void Decrypt(string decFilename, string password, string ivFilename, string output, List<string> filesToDecrypt)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public void Cancel()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal float _0600()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal float _0700()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void ࠀ(object P_0, object P_1, object P_2, object P_3, List<string> P_4)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _0900(object P_0, object P_1, object P_2, object P_3, List<string> P_4)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _0A00(object P_0, object P_1, object P_2, object P_3, List<string> P_4)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _0C00()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _0D00()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _0E00()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal Dictionary<string, long> ༀ(object P_0, object P_1, object P_2)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal Dictionary<string, long> က(object P_0, object P_1, object P_2)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static WinCrypt()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
