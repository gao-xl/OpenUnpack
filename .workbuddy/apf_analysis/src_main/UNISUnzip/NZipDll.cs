using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace UNISUnzip;

public class NZipDll
{
	public delegate string ZMUnzipGetPasswordCallBack(string path, IntPtr userdata);

	public delegate int ZMUnzipUpdateProgressCallBack(string path, IntPtr userdata, ulong currentSize, ulong totalSize);

	public delegate void ZMUnzipListResultCallBack(int resultCode, IntPtr listArchive);

	[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
	public unsafe delegate byte* GetPassWordCallBack(byte* path, IntPtr userdata);

	[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
	public delegate int ListNodeResultCb(int resultCode, IntPtr node);

	[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
	public delegate int ListSubNodesResultCb(IntPtr parentNode, int resultCode, int nodesCount, IntPtr nodes);

	[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
	public unsafe delegate int TotalUpdateProgressCallBack(byte* path, IntPtr userdata, ulong currentIndex, ulong totalIndex);

	[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
	public delegate int UnzipBatchResultCallBack(int number, IntPtr results);

	[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
	public unsafe delegate int UnzipResultCallBack(int resultCode, byte* resultFile);

	[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
	public unsafe delegate int UpdateProgressCallBack(byte* path, IntPtr userdata, ulong currentSize, ulong totalSize);

	public struct ZmCoreUnzipResultWrapper
	{
		public int Code;

		public int UsePwd;

		public unsafe byte* ResultPath;

		[MethodImpl(MethodImplOptions.NoInlining)]
		public string _ResultPath()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00㔀()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00㘀()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		static ZmCoreUnzipResultWrapper()
		{
			_003CModule_003E.ZYXDNGuarder.Startup();
		}
	}

	public struct ZMListArchiveFileModel
	{
		public IntPtr fileName;

		public IntPtr filePath;

		public IntPtr fileDate;

		public bool isDir;

		public ulong fileSize;

		public ulong allChildModelsCount;

		public IntPtr allChildModels;

		[MethodImpl(MethodImplOptions.NoInlining)]
		public ZMListArchiveFileModel[] GetAllChildModels()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		public string GetFileDate()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		public string GetFileName()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		public string GetFilePath()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00Ⰰ()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00ⴀ()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00_2E00()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00_2F00()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00_3000()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00_3100()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00_3200()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00_3300()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		[MethodImpl(MethodImplOptions.NoInlining)]
		internal object _FF00_FF00㐀()
		{
			throw new Exception("Error, DNGuard Runtime library not loaded!");
		}

		static ZMListArchiveFileModel()
		{
			_003CModule_003E.ZYXDNGuarder.Startup();
		}
	}

	static NZipDll()
	{
	}

	[DllImport("kernel32.dll", SetLastError = true)]
	private static extern bool SetDllDirectory(string lpPathName);

	[DllImport("kernel32.dll")]
	public static extern IntPtr LoadLibrary(string dllToLoad);

	[DllImport("ZMUnzipWrapper.dll", CallingConvention = CallingConvention.Cdecl)]
	public static extern void ZMUnzip_DeleteNode(IntPtr currentNode);

	[DllImport("ZMUnzipWrapper.dll", CallingConvention = CallingConvention.Cdecl)]
	public unsafe static extern int ZMUnzip_DZipProtectInterval(byte* dzipPath);

	[DllImport("ZMUnzipWrapper.dll", CallingConvention = CallingConvention.Cdecl)]
	public unsafe static extern void ZMUnzip_ExtractFile(byte* zipPath, byte* extractZipPath, byte* destinationFolder, GetPassWordCallBack passwordcb, UpdateProgressCallBack progresscb, UnzipResultCallBack resultscb);

	[DllImport("ZMUnzipWrapper.dll", CallingConvention = CallingConvention.Cdecl)]
	public static extern void ZMUnzip_GetSubNodes(IntPtr currentNode, ListSubNodesResultCb subNodes);

	[DllImport("ZMUnzipWrapper.dll", CallingConvention = CallingConvention.Cdecl)]
	public static extern void ZMUnzip_ListFile(string zipPath, ZMUnzipGetPasswordCallBack passwordcb, ZMUnzipUpdateProgressCallBack progresscb, ZMUnzipListResultCallBack resultcb);

	[DllImport("ZMUnzipWrapper.dll", CallingConvention = CallingConvention.Cdecl)]
	public unsafe static extern void ZMUnzip_UnzipFiles(byte** zipFiles, int normalZipFilesCount, byte* destinationFolder, GetPassWordCallBack passwordcb, UpdateProgressCallBack progresscb, TotalUpdateProgressCallBack totalProgresscb, UnzipBatchResultCallBack resultscb);

	[DllImport("ZMUnzipWrapper.dll", CallingConvention = CallingConvention.Cdecl)]
	public unsafe static extern void ZMUnzip_ZipFile(byte** files, int filesCount, byte* zipFormat, byte* destinationFolder, byte* password, int compresslevel, bool isVolume, long volumeSize, UpdateProgressCallBack progresscb, UnzipResultCallBack resultscb);

	[MethodImpl(MethodImplOptions.NoInlining)]
	public NZipDll()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}
}
