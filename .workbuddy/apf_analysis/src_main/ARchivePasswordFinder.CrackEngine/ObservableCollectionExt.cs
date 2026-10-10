using System;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.Runtime.CompilerServices;

namespace ARchivePasswordFinder.CrackEngine;

public class ObservableCollectionExt<T> : ObservableCollection<T>
{
	private bool IsUpdating;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public void BeginUpdate()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public void EndUpdate()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected override void OnCollectionChanged(NotifyCollectionChangedEventArgs e)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public ObservableCollectionExt()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static ObservableCollectionExt()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
