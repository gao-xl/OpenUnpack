using System.Security.Cryptography;

namespace OpenUnpack.Engine;

public static class CredentialSecurity
{
    public static unsafe void ZeroizeString(string? value)
    {
        if (string.IsNullOrEmpty(value)) return;
        // Do not mutate interned string literals to protect AppDomain string literal pool
        if (ReferenceEquals(string.IsInterned(value), value)) return;
        fixed (char* ptr = value)
        {
            var byteSpan = new Span<byte>(ptr, checked(value.Length * sizeof(char)));
            CryptographicOperations.ZeroMemory(byteSpan);
        }
    }

    public static void ZeroizeBytes(Span<byte> bytes)
    {
        if (!bytes.IsEmpty)
        {
            CryptographicOperations.ZeroMemory(bytes);
        }
    }
}
