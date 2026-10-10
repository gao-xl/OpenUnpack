using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Threading.Tasks;
using ARchivePasswordFinder.Core;
using Schematrix;

namespace ARchivePasswordFinder.Extractors;

internal class UnrarExtractor : BaseExtractor
{
	[CompilerGenerated]
	private sealed class _003CExtract_003Ed__9 : IAsyncStateMachine
	{
		public int _003C_003E1__state;

		public AsyncTaskMethodBuilder<bool> _003C_003Et__builder;

		public string password;

		public UnrarExtractor _003C_003E4__this;

		private IntPtr _003Cd7e_003E5__1;

		private IntPtr _003C_003Es__2;

		private IntPtr _003C_003Es__3;

		private object _003Ce_003E5__4;

		private IntPtr _003CrightPwd_003E5__5;

		private object _003Ce_003E5__6;

		private IntPtr _003Ctest_003E5__7;

		private double _003Cs_003E5__8;

		private long _003Csize_003E5__9;

		private object _003Cfiles_003E5__10;

		private IntPtr _003Css_003E5__11;

		private IntPtr _003C_003Es__12;

		private IntPtr _003C_003Es__13;

		private IntPtr _003C_003Es__14;

		private object _003Ce_003E5__15;

		private IntPtr _003CexeErr_003E5__16;

		private IEnumerable<string> _003CrootFiles_003E5__17;

		private IntPtr _003Cdec_003E5__18;

		private IntPtr _003C_003Es__19;

		private IntPtr _003Ctem_003E5__20;

		private IntPtr _003C_003Es__21;

		private object _003Cfiles_003E5__22;

		private List<string> _003Cfileinforder_003E5__23;

		private IEnumerable<string> _003Crealdirs_003E5__24;

		private object _003CsingleForderPath_003E5__25;

		private object _003Ctmpdir_003E5__26;

		private IntPtr _003Cextnum_003E5__27;

		private object _003Cpath_003E5__28;

		private IntPtr _003Ccount_003E5__29;

		private object _003C_003Es__30;

		private IntPtr _003C_003Es__31;

		private object _003C_003E8__32;

		private object _003Ce_003E5__33;

		private IntPtr _003C_003Es__34;

		private object _003Ce_003E5__35;

		private TaskAwaiter<bool> _003C_003Eu__1;

		[MethodImpl(MethodImplOptions.NoInlining)]
		public _003CExtract_003Ed__9()
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
		private sealed void 鸀()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		private sealed void 鼀()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		private sealed void ꀀ()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		static _003CExtract_003Ed__9()
		{
			_003CModule_003E.ZYXDNGuarder.Startup();
		}
	}

	private object unrar;

	private object sw;

	public override string InfoText
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		get
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	public override int Priority => 5;

	[MethodImpl(MethodImplOptions.NoInlining)]
	public override bool Initialize(ArchiveItem archive)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	private void Unrar_NewVolume(object sender, NewVolumeEventArgs e)
	{
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void unrar_PasswordRequired(object sender, object e)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	[DebuggerStepThrough]
	[AsyncStateMachine(typeof(_003CExtract_003Ed__9))]
	public override Task<bool> Extract(string password)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void Unrar_AllExtractionProgress(object sender, object e)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static string ToHexString(byte[] bytes)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	private void Unrar_ExtractionProgress1(object sender, ExtractionProgressEventArgs e)
	{
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void Unrar_MissingVolume(object sender, object e)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public override void Close()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public override bool IsArchiveFormatSupported(ArchiveItem archive)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public UnrarExtractor()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00_FF00ሀ()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00_FF00ጀ()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00_FF00_1400()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal byte _FF00_FF00_FF00ᔀ(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal byte _FF00_FF00_FF00ᘀ(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00_1800(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static object _FF00_FF00_FF00ᤀ(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private Task<bool> _FF00_FF00_FF00ᨀ(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private Task<bool> _FF00_FF00_FF00_1B00(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal byte _FF00_FF00_FF00ᰀ(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal Task<bool> _FF00_FF00_FF00ᴀ(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal Task<bool> _FF00_FF00_FF00Ḁ(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal Task<bool> _FF00_FF00_FF00ἀ(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void _FF00_FF00_FF00_2000(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal object _FF00_FF00_FF00_2200()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void _FF00_FF00_FF00_2500(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void _FF00_FF00_FF00_2600(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void _FF00_FF00_FF00_2700(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void _FF00_FF00_FF00_2800(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void _FF00_FF00_FF00_2900()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void _FF00_FF00_FF00_2A00()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static UnrarExtractor()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
