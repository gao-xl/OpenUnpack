using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.Extensions;

public static class ConcurrentQueueExtensions
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void Add<T>(this ConcurrentQueue<T> queue, T item)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void Clear<T>(this ConcurrentQueue<T> queue)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void AddRange<T>(this ConcurrentQueue<T> queue, IEnumerable<T> itemsToAdd)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static ConcurrentQueueExtensions()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
