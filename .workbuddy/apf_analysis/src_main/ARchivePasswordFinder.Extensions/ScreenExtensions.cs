using System;
using System.Drawing;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace ARchivePasswordFinder.Extensions;

public static class ScreenExtensions
{
	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void GetDpi(this Screen screen, DpiType dpiType, out uint dpiX, out uint dpiY)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[DllImport("User32.dll")]
	private static extern IntPtr MonitorFromPoint([In] Point pt, [In] uint dwFlags);

	[DllImport("Shcore.dll")]
	private static extern IntPtr GetDpiForMonitor([In] IntPtr hmonitor, [In] DpiType dpiType, out uint dpiX, out uint dpiY);

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_E400(object P_0, DpiType P_1, IntPtr P_2, IntPtr P_3)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00_E500(object P_0, DpiType P_1, IntPtr P_2, IntPtr P_3)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	static ScreenExtensions()
	{
		_003CModule_003E.ZYXDNGuarder.Startup();
	}
}
