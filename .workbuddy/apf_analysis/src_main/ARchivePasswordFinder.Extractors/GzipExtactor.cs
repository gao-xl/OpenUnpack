using System;
using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Threading.Tasks;
using ARchivePasswordFinder.Core;

namespace ARchivePasswordFinder.Extractors;

internal class GzipExtactor : BaseExtractor
{
	[CompilerGenerated]
	private sealed class _003CExtract_003Ed__7 : IAsyncStateMachine
	{
		public int _003C_003E1__state;

		public AsyncTaskMethodBuilder<bool> _003C_003Et__builder;

		public string password;

		public GzipExtactor _003C_003E4__this;

		private object _003CgzipFile_003E5__1;

		private object _003CdestinationDir_003E5__2;

		private object _003CtarFile_003E5__3;

		private IntPtr _003Ctargzflag_003E5__4;

		private object _003Carchive_003E5__5;

		private object _003Cfiles_003E5__6;

		private IntPtr _003Calliteamcount_003E5__7;

		private IntPtr _003Ccount_003E5__8;

		private object _003C_003Es__9;

		private IntPtr _003C_003Es__10;

		private object _003Citem_003E5__11;

		private object _003Cdestforder_003E5__12;

		private object _003Czi_003E5__13;

		private object _003Cgza_003E5__14;

		private object _003Cexcept_003E5__15;

		[MethodImpl(MethodImplOptions.NoInlining)]
		public _003CExtract_003Ed__7()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		private void MoveNext()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		void IAsyncStateMachine.MoveNext()
		{
			//ILSpy generated this explicit interface implementation from .override directive in MoveNext
			this.MoveNext();
		}

		[DebuggerHidden]
		private void SetStateMachine(IAsyncStateMachine stateMachine)
		{
		}

		void IAsyncStateMachine.SetStateMachine(IAsyncStateMachine stateMachine)
		{
			//ILSpy generated this explicit interface implementation from .override directive in SetStateMachine
			this.SetStateMachine(stateMachine);
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		private sealed void _FFFD()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		private sealed void _FFFD()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		private sealed void _FFFD()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		static _003CExtract_003Ed__7()
		{
			_003CModule_003E.ZYXDNGuarder.Startup();
		}
	}

	private new ProgressInformation _info;

	public override string InfoText
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	public override int Priority => 10;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public override void Close()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public override bool Initialize(ArchiveItem archive)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	[DebuggerStepThrough]
	[AsyncStateMachine(typeof(_003CExtract_003Ed__7))]
	public override Task<bool> Extract(string password)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	public void OnItemProgression(string currentItemName, long itemProcessed, long totalItemCount, byte totalItemPercent, ref bool abort, object userParams)
	{
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public void OnByteProgression(string currentItemName, long currentItemsByteProcessed, long currentItemTotalBytes, byte currentItemPercent, long allItemsByteProcessed, long allItemsTotalBytes, byte allItemsPercent, object userParams)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public override bool IsArchiveFormatSupported(ArchiveItem archiveItem)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public GzipExtactor()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00_AB00(object P_0, long P_1, long P_2, byte P_3, long P_4, long P_5, byte P_6, object P_7)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00가(object P_0, long P_1, long P_2, byte P_3, long P_4, long P_5, byte P_6, object P_7)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal byte _FF00_FF00관(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal object _FF00_FF00꼀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal object _FF00_FF00뀀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00넀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00눀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00대()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal byte _FF00_FF00똀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal byte _FF00_FF00뜀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal Task<bool> _FF00_FF00렀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal Task<bool> _FF00_FF00뤀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal Task<bool> _FF00_FF00먀(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static GzipExtactor()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
