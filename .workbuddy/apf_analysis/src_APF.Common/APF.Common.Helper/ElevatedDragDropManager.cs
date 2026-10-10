using System;
using System.Diagnostics;
using System.Drawing;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;
using System.Windows.Forms;

namespace APF.Common.Helper;

public class ElevatedDragDropManager : IMessageFilter
{
	private struct POINT
	{
		public int X;

		public int Y;

		[MethodImpl(MethodImplOptions.NoInlining)]
		public POINT(int newX, int newY)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		public static implicit operator Point(POINT p)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		public static implicit operator POINT(Point p)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static Point 㠀(POINT P_0)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static Point 㤀(POINT P_0)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static Point 㨀(POINT P_0)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static POINT 㬀(Point P_0)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static POINT 㰀(Point P_0)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static POINT 㴀(Point P_0)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		static POINT()
		{
			_003CModule_003E.ZYXDNGuarder.Startup();
		}
	}

	private enum MessageFilterInfo : uint
	{
		None,
		AlreadyAllowed,
		AlreadyDisAllowed,
		AllowedHigher
	}

	private enum ChangeWindowMessageFilterExAction : uint
	{
		Reset,
		Allow,
		Disallow
	}

	private enum ChangeWindowMessageFilterFlags : uint
	{
		Add = 1u,
		Remove
	}

	private struct CHANGEFILTERSTRUCT
	{
		public uint cbSize;

		public MessageFilterInfo ExtStatus;
	}

	public static ElevatedDragDropManager Instance = new ElevatedDragDropManager();

	[CompilerGenerated]
	[DebuggerBrowsable(DebuggerBrowsableState.Never)]
	private EventHandler<ElevatedDragDropArgs> m_ElevatedDragDrop;

	private const uint WM_DROPFILES = 563u;

	private const uint WM_COPYDATA = 74u;

	private const uint WM_COPYGLOBALDATA = 73u;

	private readonly IntPtr IsVistaOrHigher;

	private readonly IntPtr Is7OrHigher;

	public event EventHandler<ElevatedDragDropArgs> ElevatedDragDrop
	{
		[MethodImpl(MethodImplOptions.NoInlining)]
		[CompilerGenerated]
		add
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
		[MethodImpl(MethodImplOptions.NoInlining)]
		[CompilerGenerated]
		remove
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	[DllImport("user32.dll")]
	[return: MarshalAs(UnmanagedType.Bool)]
	private static extern bool ChangeWindowMessageFilterEx(IntPtr hWnd, uint msg, ChangeWindowMessageFilterExAction action, ref CHANGEFILTERSTRUCT changeInfo);

	[DllImport("user32.dll")]
	[return: MarshalAs(UnmanagedType.Bool)]
	private static extern bool ChangeWindowMessageFilter(uint msg, ChangeWindowMessageFilterFlags flags);

	[DllImport("shell32.dll")]
	private static extern void DragAcceptFiles(IntPtr hwnd, bool fAccept);

	[DllImport("shell32.dll")]
	private static extern uint DragQueryFileW(IntPtr hDrop, uint iFile, [Out][MarshalAs(UnmanagedType.LPWStr)] StringBuilder lpszFile, uint cch);

	[DllImport("shell32.dll")]
	private static extern bool DragQueryPoint(IntPtr hDrop, ref POINT lppt);

	[DllImport("shell32.dll")]
	private static extern void DragFinish(IntPtr hDrop);

	[MethodImpl(MethodImplOptions.NoInlining)]
	public ElevatedDragDropManager()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public void EnableDragDrop(IntPtr hWnd)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public bool PreFilterMessage(ref Message m)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void HandleDragDropMessage(Message m)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00縀(EventHandler<ElevatedDragDropArgs> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00缀(EventHandler<ElevatedDragDropArgs> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00耀(EventHandler<ElevatedDragDropArgs> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00脀(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00舀(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00茀(EventHandler<ElevatedDragDropArgs> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00萀(EventHandler<ElevatedDragDropArgs> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal void _FF00_FF00蔀(EventHandler<ElevatedDragDropArgs> P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void _FF00_FF00蠀(Message P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private void _FF00_FF00褀(Message P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal sealed byte _FF00_FF00言(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}
}
