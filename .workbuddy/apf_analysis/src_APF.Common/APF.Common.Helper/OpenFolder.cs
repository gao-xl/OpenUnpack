using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

namespace APF.Common.Helper;

public static class OpenFolder
{
	[Flags]
	private enum SHCONT : ushort
	{
		SHCONTF_CHECKING_FOR_CHILDREN = 0x10,
		SHCONTF_FOLDERS = 0x20,
		SHCONTF_NONFOLDERS = 0x40,
		SHCONTF_INCLUDEHIDDEN = 0x80,
		SHCONTF_INIT_ON_FIRST_NEXT = 0x100,
		SHCONTF_NETPRINTERSRCH = 0x200,
		SHCONTF_SHAREABLE = 0x400,
		SHCONTF_STORAGE = 0x800,
		SHCONTF_NAVIGATION_ENUM = 0x1000,
		SHCONTF_FASTITEMS = 0x2000,
		SHCONTF_FLATLIST = 0x4000,
		SHCONTF_ENABLE_ASYNC = 0x8000
	}

	[ComImport]
	[Guid("000214E6-0000-0000-C000-000000000046")]
	[ComConversionLoss]
	[InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
	private interface IShellFolder
	{
		[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		void ParseDisplayName(IntPtr hwnd, [In][MarshalAs(UnmanagedType.Interface)] IBindCtx pbc, [In][MarshalAs(UnmanagedType.LPWStr)] string pszDisplayName, out uint pchEaten, out IntPtr ppidl, [In][Out] ref uint pdwAttributes);

		[MethodImpl(MethodImplOptions.PreserveSig | MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		int EnumObjects([In] IntPtr hwnd, [In] SHCONT grfFlags, [MarshalAs(UnmanagedType.Interface)] out IEnumIDList ppenumIDList);

		[MethodImpl(MethodImplOptions.PreserveSig | MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		int BindToObject([In] IntPtr pidl, [In][MarshalAs(UnmanagedType.Interface)] IBindCtx pbc, [In] ref Guid riid, [MarshalAs(UnmanagedType.Interface)] out IShellFolder ppv);

		[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		void BindToStorage([In] ref IntPtr pidl, [In][MarshalAs(UnmanagedType.Interface)] IBindCtx pbc, [In] ref Guid riid, out IntPtr ppv);

		[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		void CompareIDs([In] IntPtr lParam, [In] ref IntPtr pidl1, [In] ref IntPtr pidl2);

		[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		void CreateViewObject([In] IntPtr hwndOwner, [In] ref Guid riid, out IntPtr ppv);

		[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		void GetAttributesOf([In] uint cidl, [In] IntPtr apidl, [In][Out] ref uint rgfInOut);

		[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		void GetUIObjectOf([In] IntPtr hwndOwner, [In] uint cidl, [In] IntPtr apidl, [In] ref Guid riid, [In][Out] ref uint rgfReserved, out IntPtr ppv);

		[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		void GetDisplayNameOf([In] ref IntPtr pidl, [In] uint uFlags, out IntPtr pName);

		[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		void SetNameOf([In] IntPtr hwnd, [In] ref IntPtr pidl, [In][MarshalAs(UnmanagedType.LPWStr)] string pszName, [In] uint uFlags, [Out] IntPtr ppidlOut);
	}

	[ComImport]
	[InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
	[Guid("000214F2-0000-0000-C000-000000000046")]
	private interface IEnumIDList
	{
		[MethodImpl(MethodImplOptions.PreserveSig | MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		int Next(uint celt, IntPtr rgelt, out uint pceltFetched);

		[MethodImpl(MethodImplOptions.PreserveSig | MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		int Skip([In] uint celt);

		[MethodImpl(MethodImplOptions.PreserveSig | MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		int Reset();

		[MethodImpl(MethodImplOptions.PreserveSig | MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
		int Clone([MarshalAs(UnmanagedType.Interface)] out IEnumIDList ppenum);
	}

	private static class NativeMethods
	{
		[DllImport("shell32.dll", CharSet = CharSet.Unicode, EntryPoint = "SHGetDesktopFolder", SetLastError = true)]
		private static extern int SHGetDesktopFolder_([MarshalAs(UnmanagedType.Interface)] out IShellFolder ppshf);

		[MethodImpl(MethodImplOptions.NoInlining)]
		public static IShellFolder SHGetDesktopFolder()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[DllImport("shell32.dll", EntryPoint = "SHOpenFolderAndSelectItems")]
		private static extern int SHOpenFolderAndSelectItems_([In] IntPtr pidlFolder, uint cidl, [Optional][In][MarshalAs(UnmanagedType.LPArray)] IntPtr[] apidl, int dwFlags);

		[MethodImpl(MethodImplOptions.NoInlining)]
		public static void SHOpenFolderAndSelectItems(IntPtr pidlFolder, IntPtr[] apidl, int dwFlags)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[DllImport("shell32.dll")]
		public static extern void ILFree([In] IntPtr pidl);

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static object _1800()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static object ᤀ()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static object ᨀ()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static void _1B00(IntPtr P_0, object P_1, uint P_2)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static void ᰀ(IntPtr P_0, object P_1, uint P_2)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal static void ᴀ(IntPtr P_0, object P_1, uint P_2)
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		static NativeMethods()
		{
			_003CModule_003E.ZYXDNGuarder.Startup();
		}
	}

	private static Guid IID_IShellFolder = typeof(IShellFolder).GUID;

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static IntPtr GetShellFolderChildrenRelativePIDL(object parentFolder, object displayName)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static IntPtr PathToAbsolutePIDL(object path)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object PIDLToShellFolder(object parent, IntPtr pidl)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object PIDLToShellFolder(IntPtr pidl)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void SHOpenFolderAndSelectItems(IntPtr pidlFolder, object apidl, byte edit)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void FileOrFolder(string path, bool edit = false)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void FilesOrFolders(string parentDirectory, IEnumerable<string> filenames)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	public static void FilesOrFolders(string parentDirectory, params string[] paths)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object _FF00_FF00_EF00(object P_0, IntPtr P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object _FF00_FF00_F000(object P_0, IntPtr P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object _FF00_FF00_F100(object P_0, IntPtr P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static IntPtr _FF00_FF00_F200(object P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_F300(object P_0, byte P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_F400(object P_0, byte P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_F700(object P_0, IEnumerable<string> P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void _FF00_FF00_F800(IntPtr P_0, object P_1, byte P_2)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void _FF00_FF00豈(IntPtr P_0, object P_1, byte P_2)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object _FF00_FF00切(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static object _FF00_FF00ﬀ(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static IntPtr _FF00_FF00ﰀ(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static IntPtr _FF00_FF00ﴀ(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FE00(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	internal static void _FF00_FF00_FF00(object P_0, object P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}
}
