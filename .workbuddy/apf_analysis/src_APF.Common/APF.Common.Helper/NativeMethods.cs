using System;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace APF.Common.Helper;

internal static class NativeMethods
{
	[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
	internal struct WIN32_FIND_DATA
	{
		public FileAttributes dwFileAttributes;

		public FILETIME ftCreationTime;

		public FILETIME ftLastAccessTime;

		public FILETIME ftLastWriteTime;

		public uint nFileSizeHigh;

		public uint nFileSizeLow;

		public uint dwReserved0;

		public uint dwReserved1;

		[MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
		public string cFileName;

		[MarshalAs(UnmanagedType.ByValTStr, SizeConst = 14)]
		public string cAlternate;
	}

	internal const int FILE_ATTRIBUTE_ARCHIVE = 32;

	internal const int INVALID_FILE_ATTRIBUTES = -1;

	internal const int FILE_READ_DATA = 1;

	internal const int FILE_WRITE_DATA = 2;

	internal const int FILE_APPEND_DATA = 4;

	internal const int FILE_READ_EA = 8;

	internal const int FILE_WRITE_EA = 16;

	internal const int FILE_READ_ATTRIBUTES = 128;

	internal const int FILE_WRITE_ATTRIBUTES = 256;

	internal const int FILE_SHARE_NONE = 0;

	internal const int FILE_SHARE_READ = 1;

	internal const int FILE_ATTRIBUTE_DIRECTORY = 16;

	internal const long FILE_GENERIC_WRITE = 1179926L;

	internal const long FILE_GENERIC_READ = 1179785L;

	internal const long READ_CONTROL = 131072L;

	internal const long STANDARD_RIGHTS_READ = 131072L;

	internal const long STANDARD_RIGHTS_WRITE = 131072L;

	internal const long SYNCHRONIZE = 1048576L;

	internal const int CREATE_NEW = 1;

	internal const int CREATE_ALWAYS = 2;

	internal const int OPEN_EXISTING = 3;

	internal const int MAX_PATH = 260;

	internal const int MAX_ALTERNATE = 14;

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern SafeFileHandle CreateFile(string lpFileName, int dwDesiredAccess, int dwShareMode, IntPtr lpSecurityAttributes, int dwCreationDisposition, int dwFlagsAndAttributes, IntPtr hTemplateFile);

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern bool CopyFileW(string lpExistingFileName, string lpNewFileName, bool bFailIfExists);

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern int GetFileAttributesW(string lpFileName);

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern bool DeleteFileW(string lpFileName);

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern bool MoveFileW(string lpExistingFileName, string lpNewFileName);

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern bool SetFileTime(SafeFileHandle hFile, ref long lpCreationTime, ref long lpLastAccessTime, ref long lpLastWriteTime);

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern bool GetFileTime(SafeFileHandle hFile, ref long lpCreationTime, ref long lpLastAccessTime, ref long lpLastWriteTime);

	[DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern IntPtr FindFirstFile(string lpFileName, out WIN32_FIND_DATA lpFindFileData);

	[DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern bool FindNextFile(IntPtr hFindFile, out WIN32_FIND_DATA lpFindFileData);

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern bool FindClose(IntPtr hFindFile);

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern bool RemoveDirectory(string path);

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern bool CreateDirectory(string lpPathName, IntPtr lpSecurityAttributes);

	[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
	internal static extern int SetFileAttributesW(string lpFileName, int fileAttributes);
}
