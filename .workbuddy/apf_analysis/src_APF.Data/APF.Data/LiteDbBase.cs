using System;
using System.Runtime.CompilerServices;
using LiteDB;

namespace APF.Data;

public class LiteDbBase : IDisposable
{
	private readonly object DbFile;

	private object _liteDatabase;

	public LiteDatabase LiteDb
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public void Dispose()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public LiteDbBase()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal sealed void Ā()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal sealed void Ȁ()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal sealed void _0300()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal object _0700()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static LiteDbBase()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
