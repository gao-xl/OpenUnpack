using System;
using System.Collections;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

internal class _003CModule_003E
{
	internal sealed class ZYXDNGuarder
	{
		internal delegate IntPtr d(object P_0, int P_1, int P_2, IntPtr P_3, int P_4);

		internal static bool a;

		internal static IntPtr a;

		internal static IntPtr a;

		internal static int a;

		internal static object a;

		internal static object a;

		[MethodImpl(MethodImplOptions.Synchronized | MethodImplOptions.NoInlining)]
		internal static string CheckString(int P_0, int P_1)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		internal static string CheckString(int P_0, int P_1)
		{
			return ZYXDNGuarder.CheckString(P_0, P_1);
		}

		static ZYXDNGuarder()
		{
			ZYXDNGuarder.a = new Hashtable();
			ZYXDNGuarder.a = false;
			ZYXDNGuarder.a = Environment.Version.Major;
		}

		[DllImport("APFRuntime.dll", EntryPoint = "VMRuntime")]
		private static extern bool CheckRuntime(int P_0);

		[DllImport("APFRuntime64.dll", EntryPoint = "VMRuntime")]
		private static extern bool CheckRuntime64(int P_0);

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static bool RunHVM()
		{
			_ = 0;
			/*Error near IL_0002: Read out of bounds.*/;
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static bool RunHVM()
		{
			_ = 0;
			/*Error near IL_0002: Read out of bounds.*/;
		}

		internal static void Startup()
		{
			if (ZYXDNGuarder.a)
			{
				return;
			}
			try
			{
				ZYXDNGuarder.a = ZYXDNGuarder.RunHVM();
				return;
			}
			catch
			{
			}
			try
			{
				int num = ZYXDNGuarder.a;
				if (IntPtr.Size != 4)
				{
					CheckRuntime64(num);
				}
				else
					CheckRuntime(num);
			}
			catch
			{
			}
			try
			{
				ZYXDNGuarder.a = ZYXDNGuarder.RunHVM();
			}
			catch (Exception)
			{
				throw new Exception("*=Load DNGuard Runtime library failed, runtime library not found or Access Denied.=*[APF.Data.dll]");
			}
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static void dvm()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static object Ѐ()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static object Ԁ()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static object _0600()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static object 吀()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static object 唀()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static object 嘀()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static object 圀()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}
	}

	static _003CModule_003E()
	{
		ZYXDNGuarder.Startup();
	}
}
