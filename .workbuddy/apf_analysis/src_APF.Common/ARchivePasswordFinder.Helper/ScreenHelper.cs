using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace ARchivePasswordFinder.Helper;

public class ScreenHelper
{
	public struct RECT
	{
		public int Left;

		public int Top;

		public int Right;

		public int Bottom;
	}

	private IntPtr desktopHandle;

	private IntPtr shellHandle;

	[DllImport("user32.dll")]
	private static extern IntPtr GetForegroundWindow();

	[DllImport("user32.dll")]
	private static extern IntPtr GetDesktopWindow();

	[DllImport("user32.dll")]
	private static extern IntPtr GetShellWindow();

	[DllImport("user32.dll", SetLastError = true)]
	private static extern int GetWindowRect(IntPtr hwnd, out RECT rc);

	[MethodImpl(MethodImplOptions.NoInlining)]
	public ScreenHelper()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public bool DetectIsFullScreen()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal byte 吀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal byte 唀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal byte 嘀()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static ScreenHelper()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
