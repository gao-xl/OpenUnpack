using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace MultiLanguage;

[ComImport]
[Guid("4E5868AB-B157-4623-9ACC-6A1D9CAEBE04")]
[InterfaceType(1)]
public interface IMultiLanguage3 : IMultiLanguage2
{
	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void GetNumberOfCodePageInfo(out uint pcCodePage);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void GetCodePageInfo([In] uint uiCodePage, [In] ushort LangId, out tagMIMECPINFO pCodePageInfo);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void GetFamilyCodePage([In] uint uiCodePage, out uint puiFamilyCodePage);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void EnumCodePages([In] uint grfFlags, [In] ushort LangId, [MarshalAs(UnmanagedType.Interface)] out IEnumCodePage ppEnumCodePage);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void GetCharsetInfo([In][MarshalAs(UnmanagedType.BStr)] string Charset, out tagMIMECSETINFO pCharsetInfo);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void IsConvertible([In] uint dwSrcEncoding, [In] uint dwDstEncoding);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void ConvertString([In][Out] ref uint pdwMode, [In] uint dwSrcEncoding, [In] uint dwDstEncoding, [In] ref byte pSrcStr, [In][Out] ref uint pcSrcSize, [In] ref byte pDstStr, [In][Out] ref uint pcDstSize);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void ConvertStringToUnicode([In][Out] ref uint pdwMode, [In] uint dwEncoding, [In] ref sbyte pSrcStr, [In][Out] ref uint pcSrcSize, [In] ref ushort pDstStr, [In][Out] ref uint pcDstSize);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void ConvertStringFromUnicode([In][Out] ref uint pdwMode, [In] uint dwEncoding, [In] ref ushort pSrcStr, [In][Out] ref uint pcSrcSize, [In] ref sbyte pDstStr, [In][Out] ref uint pcDstSize);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void ConvertStringReset();

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void GetRfc1766FromLcid([In] uint locale, [MarshalAs(UnmanagedType.BStr)] out string pbstrRfc1766);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void GetLcidFromRfc1766(out uint plocale, [In][MarshalAs(UnmanagedType.BStr)] string bstrRfc1766);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void EnumRfc1766([In] ushort LangId, [MarshalAs(UnmanagedType.Interface)] out IEnumRfc1766 ppEnumRfc1766);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void GetRfc1766Info([In] uint locale, [In] ushort LangId, out tagRFC1766INFO pRfc1766Info);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void CreateConvertCharset([In] uint uiSrcCodePage, [In] uint uiDstCodePage, [In] uint dwProperty, [MarshalAs(UnmanagedType.Interface)] out CMLangConvertCharset ppMLangConvertCharset);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void ConvertStringInIStream([In][Out] ref uint pdwMode, [In] uint dwFlag, [In] ref ushort lpFallBack, [In] uint dwSrcEncoding, [In] uint dwDstEncoding, [In][MarshalAs(UnmanagedType.Interface)] IStream pstmIn, [In][MarshalAs(UnmanagedType.Interface)] IStream pstmOut);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void ConvertStringToUnicodeEx([In][Out] ref uint pdwMode, [In] uint dwEncoding, [In] ref sbyte pSrcStr, [In][Out] ref uint pcSrcSize, [In] ref ushort pDstStr, [In][Out] ref uint pcDstSize, [In] uint dwFlag, [In] ref ushort lpFallBack);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void ConvertStringFromUnicodeEx([In][Out] ref uint pdwMode, [In] uint dwEncoding, [In] ref ushort pSrcStr, [In][Out] ref uint pcSrcSize, [In] ref sbyte pDstStr, [In][Out] ref uint pcDstSize, [In] uint dwFlag, [In] ref ushort lpFallBack);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void DetectCodepageInIStream([In] MLDETECTCP flags, [In] uint dwPrefWinCodePage, [In][MarshalAs(UnmanagedType.Interface)] IStream pstmIn, [In][Out] ref DetectEncodingInfo lpEncoding, [In][Out] ref int pnScores);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void DetectInputCodepage([In] MLDETECTCP flags, [In] uint dwPrefWinCodePage, [In] ref byte pSrcStr, [In][Out] ref int pcSrcSize, [In][Out] ref DetectEncodingInfo lpEncoding, [In][Out] ref int pnScores);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void ValidateCodePage([In] uint uiCodePage, [In][ComAliasName("MultiLanguage.wireHWND")] ref _RemotableHandle hwnd);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void GetCodePageDescription([In] uint uiCodePage, [In] uint lcid, [In][Out][MarshalAs(UnmanagedType.LPWStr)] string lpWideCharStr, [In] int cchWideChar);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void IsCodePageInstallable([In] uint uiCodePage);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void SetMimeDBSource([In] tagMIMECONTF dwSource);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void GetNumberOfScripts(out uint pnScripts);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void EnumScripts([In] uint dwFlags, [In] ushort LangId, [MarshalAs(UnmanagedType.Interface)] out IEnumScript ppEnumScript);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	new void ValidateCodePageEx([In] uint uiCodePage, [In][ComAliasName("MultiLanguage.wireHWND")] ref _RemotableHandle hwnd, [In] uint dwfIODControl);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	void DetectOutboundCodePage([In] MLCPF dwFlags, [In][MarshalAs(UnmanagedType.LPWStr)] string lpWideCharStr, [In] uint cchWideChar, [In] IntPtr puiPreferredCodePages, [In] uint nPreferredCodePages, [In] IntPtr puiDetectedCodePages, [In][Out] ref uint pnDetectedCodePages, [In] ref ushort lpSpecialChar);

	[MethodImpl(MethodImplOptions.InternalCall, MethodCodeType = MethodCodeType.Runtime)]
	void DetectOutboundCodePageInIStream([In] uint dwFlags, [In][MarshalAs(UnmanagedType.Interface)] IStream pStrIn, [In] ref uint puiPreferredCodePages, [In] uint nPreferredCodePages, [In] ref uint puiDetectedCodePages, [In][Out] ref uint pnDetectedCodePages, [In] ref ushort lpSpecialChar);
}
