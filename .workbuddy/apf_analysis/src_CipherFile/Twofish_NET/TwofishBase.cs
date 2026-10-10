using System;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;

namespace Twofish_NET;

internal class TwofishBase
{
	public enum EncryptionDirection
	{
		Encrypting,
		Decrypting
	}

	protected int inputBlockSize;

	protected int outputBlockSize;

	private object numRounds;

	protected uint[] sboxKeys;

	protected uint[] subKeys;

	protected uint[] Key;

	protected uint[] IV;

	private IntPtr keyLength;

	private IntPtr rounds;

	protected CipherMode cipherMode;

	private static readonly IntPtr BLOCK_SIZE = (IntPtr)128;

	private static readonly IntPtr MAX_ROUNDS = (IntPtr)16;

	private static readonly IntPtr ROUNDS_128 = (IntPtr)16;

	private static readonly IntPtr ROUNDS_192 = (IntPtr)16;

	private static readonly IntPtr ROUNDS_256 = (IntPtr)16;

	private static readonly IntPtr MAX_KEY_BITS = (IntPtr)256;

	private static readonly IntPtr MIN_KEY_BITS = (IntPtr)128;

	private static readonly IntPtr INPUT_WHITEN = (IntPtr)0;

	private static readonly IntPtr OUTPUT_WHITEN = (nint)INPUT_WHITEN + (nint)BLOCK_SIZE / 32;

	private static readonly IntPtr ROUND_SUBKEYS = (nint)OUTPUT_WHITEN + (nint)BLOCK_SIZE / 32;

	private static readonly IntPtr TOTAL_SUBKEYS = (nint)ROUND_SUBKEYS + 2 * (nint)MAX_ROUNDS;

	private static readonly uint SK_STEP = 33686018u;

	private static readonly uint SK_BUMP = 16843009u;

	private static readonly IntPtr SK_ROTL = (IntPtr)9;

	private static readonly uint RS_GF_FDBK = 333u;

	private static readonly IntPtr MDS_GF_FDBK = (IntPtr)361;

	private static readonly IntPtr P_00 = (IntPtr)1;

	private static readonly IntPtr P_01 = (IntPtr)0;

	private static readonly IntPtr P_02 = (IntPtr)0;

	private static readonly IntPtr P_03 = (nint)P_01 ^ 1;

	private static readonly IntPtr P_04 = (IntPtr)1;

	private static readonly IntPtr P_10 = (IntPtr)0;

	private static readonly IntPtr P_11 = (IntPtr)0;

	private static readonly IntPtr P_12 = (IntPtr)1;

	private static readonly IntPtr P_13 = (nint)P_11 ^ 1;

	private static readonly IntPtr P_14 = (IntPtr)0;

	private static readonly IntPtr P_20 = (IntPtr)1;

	private static readonly IntPtr P_21 = (IntPtr)1;

	private static readonly IntPtr P_22 = (IntPtr)0;

	private static readonly IntPtr P_23 = (nint)P_21 ^ 1;

	private static readonly IntPtr P_24 = (IntPtr)0;

	private static readonly IntPtr P_30 = (IntPtr)0;

	private static readonly IntPtr P_31 = (IntPtr)1;

	private static readonly IntPtr P_32 = (IntPtr)1;

	private static readonly IntPtr P_33 = (nint)P_31 ^ 1;

	private static readonly IntPtr P_34 = (IntPtr)1;

	private static object P8x8 = new byte[2, 256]
	{
		{
			169, 103, 179, 232, 4, 253, 163, 118, 154, 146,
			128, 120, 228, 221, 209, 56, 13, 198, 53, 152,
			24, 247, 236, 108, 67, 117, 55, 38, 250, 19,
			148, 72, 242, 208, 139, 48, 132, 84, 223, 35,
			25, 91, 61, 89, 243, 174, 162, 130, 99, 1,
			131, 46, 217, 81, 155, 124, 166, 235, 165, 190,
			22, 12, 227, 97, 192, 140, 58, 245, 115, 44,
			37, 11, 187, 78, 137, 107, 83, 106, 180, 241,
			225, 230, 189, 69, 226, 244, 182, 102, 204, 149,
			3, 86, 212, 28, 30, 215, 251, 195, 142, 181,
			233, 207, 191, 186, 234, 119, 57, 175, 51, 201,
			98, 113, 129, 121, 9, 173, 36, 205, 249, 216,
			229, 197, 185, 77, 68, 8, 134, 231, 161, 29,
			170, 237, 6, 112, 178, 210, 65, 123, 160, 17,
			49, 194, 39, 144, 32, 246, 96, 255, 150, 92,
			177, 171, 158, 156, 82, 27, 95, 147, 10, 239,
			145, 133, 73, 238, 45, 79, 143, 59, 71, 135,
			109, 70, 214, 62, 105, 100, 42, 206, 203, 47,
			252, 151, 5, 122, 172, 127, 213, 26, 75, 14,
			167, 90, 40, 20, 63, 41, 136, 60, 76, 2,
			184, 218, 176, 23, 85, 31, 138, 125, 87, 199,
			141, 116, 183, 196, 159, 114, 126, 21, 34, 18,
			88, 7, 153, 52, 110, 80, 222, 104, 101, 188,
			219, 248, 200, 168, 43, 64, 220, 254, 50, 164,
			202, 16, 33, 240, 211, 93, 15, 0, 111, 157,
			54, 66, 74, 94, 193, 224
		},
		{
			117, 243, 198, 244, 219, 123, 251, 200, 74, 211,
			230, 107, 69, 125, 232, 75, 214, 50, 216, 253,
			55, 113, 241, 225, 48, 15, 248, 27, 135, 250,
			6, 63, 94, 186, 174, 91, 138, 0, 188, 157,
			109, 193, 177, 14, 128, 93, 210, 213, 160, 132,
			7, 20, 181, 144, 44, 163, 178, 115, 76, 84,
			146, 116, 54, 81, 56, 176, 189, 90, 252, 96,
			98, 150, 108, 66, 247, 16, 124, 40, 39, 140,
			19, 149, 156, 199, 36, 70, 59, 112, 202, 227,
			133, 203, 17, 208, 147, 184, 166, 131, 32, 255,
			159, 119, 195, 204, 3, 111, 8, 191, 64, 231,
			43, 226, 121, 12, 170, 130, 65, 58, 234, 185,
			228, 154, 164, 151, 126, 218, 122, 23, 102, 148,
			161, 29, 61, 240, 222, 179, 11, 114, 167, 28,
			239, 209, 83, 62, 143, 51, 38, 95, 236, 118,
			42, 73, 129, 136, 238, 33, 196, 26, 235, 217,
			197, 57, 153, 205, 173, 49, 139, 1, 24, 35,
			221, 31, 78, 45, 249, 72, 79, 242, 101, 142,
			120, 92, 88, 25, 141, 229, 152, 87, 103, 127,
			5, 100, 175, 99, 182, 254, 245, 183, 60, 165,
			206, 233, 104, 68, 224, 77, 67, 105, 41, 46,
			172, 21, 89, 168, 10, 158, 110, 71, 223, 52,
			53, 106, 207, 220, 34, 201, 192, 155, 137, 212,
			237, 171, 18, 162, 13, 82, 187, 2, 47, 169,
			215, 97, 30, 180, 80, 4, 246, 194, 22, 37,
			134, 86, 85, 9, 190, 145
		}
	};

	[MethodImpl(MethodImplOptions.NoInlining)]
	public TwofishBase()
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint f32(uint x, IntPtr k32, uint keyLen)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected bool reKey(int keyLen, ref uint[] key32)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected void blockDecrypt(ref uint[] x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected void blockEncrypt(ref uint[] x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint RS_MDS_Encode(uint k0, uint k1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void RS_rem(IntPtr x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint LFSR1(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint LFSR2(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint Mx_1(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint Mx_X(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint Mx_Y(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M00(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M01(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M02(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M03(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M10(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M11(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M12(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M13(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M20(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M21(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M22(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M23(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M30(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M31(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M32(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint M33(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint Mul_1(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint Mul_X(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint Mul_Y(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint ROL(uint x, uint n)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint ROR(uint x, uint n)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte b0(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte b1(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte b2(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte b3(uint x)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte _FFFD(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte _FFFD(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FFFD(uint P_0, uint P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FFFD(uint P_0, uint P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FFFD(uint P_0, uint P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FFFD(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FFFD(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _E000(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected void _E100(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected void _E200(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected void _E300(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected void _E400(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected void _E500(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _E600(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _E700(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _E800(uint P_0, uint P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _E900(uint P_0, uint P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _EA00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _EB00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _EC00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _ED00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _EE00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _EF00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _F000(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _F100(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected byte _F200(uint P_0, IntPtr P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected byte _F300(uint P_0, IntPtr P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected byte _F400(uint P_0, IntPtr P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _F500(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _F600(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _F700(uint P_0, uint P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _F800(uint P_0, uint P_1)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint 豈(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint 切(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint ﬀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint ﰀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint ﴀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FE00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00Ā(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00Ȁ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00_0300(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00Ѐ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00Ԁ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void _FF00_0600(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static void _FF00_0700(IntPtr P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ࠀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00_0900(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00_0A00(uint P_0, IntPtr P_1, uint P_2)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00_0B00(uint P_0, IntPtr P_1, uint P_2)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte _FF00_0C00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte _FF00_0D00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte _FF00_0E00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ༀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00က(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ᄀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ሀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte _FF00ጀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte _FF00_1400(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ᔀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ᘀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ᜀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00_1800(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ᤀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ᨀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00_1B00(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ᰀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte _FF00ᴀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	protected static byte _FF00Ḁ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00ἀ(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00_2000(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00_2100(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00_2200(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}

	[MethodImpl(MethodImplOptions.NoInlining)]
	private static uint _FF00_2300(uint P_0)
	{
		throw new Exception("Error, DNGuard Runtime library not loaded!");
	}
}
