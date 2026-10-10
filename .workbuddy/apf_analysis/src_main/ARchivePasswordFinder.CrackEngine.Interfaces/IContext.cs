using System;

namespace ARchivePasswordFinder.CrackEngine.Interfaces;

public interface IContext
{
	bool IsSynchronized { get; }

	void Invoke(Action action);

	void BeginInvoke(Action action);
}
