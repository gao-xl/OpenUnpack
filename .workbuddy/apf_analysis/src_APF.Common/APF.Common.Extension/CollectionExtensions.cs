using System;
using System.Collections;
using System.Runtime.CompilerServices;

namespace APF.Common.Extension;

internal static class CollectionExtensions
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public static T[] ToArray<T>(this ICollection collection)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static CollectionExtensions()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
