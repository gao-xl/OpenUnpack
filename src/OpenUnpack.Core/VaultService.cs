using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace OpenUnpack.Core;

public sealed class VaultService
{
    private const int MaxPlaintextBytes = 32 * 1024 * 1024;
    private const int MaxVaultFileBytes = MaxPlaintextBytes + 64 * 1024;
    private const int MaxBackupFileBytes = 46 * 1024 * 1024;
    private const int BackupIterations = 600_000;
    private const string BackupFormat = "openunpack-backup-v1";
    private static readonly byte[] VaultEntropy = Encoding.UTF8.GetBytes("OpenUnpack/local-vault/v1");
    private static readonly byte[] BackupAdditionalData = Encoding.UTF8.GetBytes(BackupFormat);
    private static readonly JsonSerializerOptions JsonOptions = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
    private readonly string _dataDirectory;
    private readonly string _vaultPath;
    private readonly string _mutexName;
    private readonly object _sync = new();

    public VaultService(string dataDirectory)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(dataDirectory);
        _dataDirectory = Path.GetFullPath(dataDirectory);
        _vaultPath = Path.Combine(_dataDirectory, "vault.dat");
        _mutexName = "Local\\OpenUnpack.Vault." + Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(_vaultPath.ToUpperInvariant())));
    }

    public VaultSnapshot Load() => WithVaultLock(() =>
    {
        try { return ReadLocalVault(); }
        catch (FileNotFoundException) { return new VaultSnapshot(); }
        catch (DirectoryNotFoundException) { return new VaultSnapshot(); }
    });

    public void Save(VaultSnapshot snapshot) => WithVaultLock(() =>
    {
        var plaintext = SerializeSnapshot(snapshot);
        try
        {
            // Do not replace an unreadable or corrupt existing vault with an empty/new snapshot.
            try { _ = ReadLocalVault(); }
            catch (FileNotFoundException) { }
            catch (DirectoryNotFoundException) { }
            var protectedBytes = Protect(plaintext, decrypt: false);
            try { AtomicWrite(_vaultPath, protectedBytes, _vaultPath + ".bak"); }
            finally { CryptographicOperations.ZeroMemory(protectedBytes); }
        }
        finally { CryptographicOperations.ZeroMemory(plaintext); }
        return true;
    });

    public static IReadOnlyList<PasswordEntry> GetCandidates(VaultSnapshot snapshot, string fingerprint, string? group)
    {
        ArgumentNullException.ThrowIfNull(snapshot);
        ArgumentNullException.ThrowIfNull(fingerprint);
        snapshot.ArchiveMatches.TryGetValue(fingerprint, out var associatedId);
        var secrets = new HashSet<string>(StringComparer.Ordinal);
        return snapshot.Entries
            .Where(entry => entry.Enabled && entry.Secret.Length > 0 && (group is null || entry.Group == group))
            .OrderByDescending(entry => entry.Id == associatedId)
            .ThenByDescending(entry => entry.SuccessCount)
            .ThenByDescending(entry => entry.LastUsedUtc)
            .ThenBy(entry => entry.CreatedUtc)
            .Where(entry => secrets.Add(entry.Secret))
            .ToArray();
    }

    public static void RecordSuccess(VaultSnapshot snapshot, Guid id, string fingerprint)
    {
        ArgumentNullException.ThrowIfNull(snapshot);
        ArgumentException.ThrowIfNullOrWhiteSpace(fingerprint);
        if (fingerprint.Length > 256) throw new ArgumentException("压缩包指纹过长。", nameof(fingerprint));
        var index = snapshot.Entries.FindIndex(entry => entry.Id == id);
        if (index < 0) throw new ArgumentException("指定的密码记录不存在。", nameof(id));
        var entry = snapshot.Entries[index];
        snapshot.Entries[index] = entry with
        {
            SuccessCount = entry.SuccessCount == int.MaxValue ? int.MaxValue : entry.SuccessCount + 1,
            LastUsedUtc = DateTimeOffset.UtcNow
        };
        snapshot.ArchiveMatches[fingerprint] = id;
    }

    public static void ExportBackup(VaultSnapshot snapshot, string destination, string passphrase)
    {
        ValidatePassphrase(passphrase);
        ArgumentException.ThrowIfNullOrWhiteSpace(destination);
        var outputPath = Path.GetFullPath(destination);
        if (string.Equals(Path.GetFileName(outputPath), "vault.dat", StringComparison.OrdinalIgnoreCase) ||
            string.Equals(Path.GetFileName(outputPath), "vault.dat.bak", StringComparison.OrdinalIgnoreCase))
            throw new ArgumentException("备份目标不能覆盖本地密码库。", nameof(destination));
        var plaintext = SerializeSnapshot(snapshot);
        var salt = RandomNumberGenerator.GetBytes(32);
        var nonce = RandomNumberGenerator.GetBytes(12);
        var key = DeriveKey(passphrase, salt, BackupIterations);
        var ciphertext = new byte[plaintext.Length];
        var tag = new byte[16];
        try
        {
            using var aes = new AesGcm(key, tag.Length);
            aes.Encrypt(nonce, plaintext, ciphertext, tag, BackupAdditionalData);
            var envelope = new BackupEnvelope
            {
                Format = BackupFormat, Version = 1, Kdf = "PBKDF2-SHA256", Iterations = BackupIterations,
                Cipher = "AES-256-GCM", Salt = Convert.ToBase64String(salt), Nonce = Convert.ToBase64String(nonce),
                Tag = Convert.ToBase64String(tag), Ciphertext = Convert.ToBase64String(ciphertext)
            };
            AtomicWrite(outputPath, JsonSerializer.SerializeToUtf8Bytes(envelope, JsonOptions), backupPath: null);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(key);
            CryptographicOperations.ZeroMemory(plaintext);
        }
    }

    public static VaultSnapshot ImportBackup(string source, string passphrase)
    {
        ValidatePassphrase(passphrase);
        ArgumentException.ThrowIfNullOrWhiteSpace(source);
        BackupEnvelope envelope;
        try
        {
            envelope = JsonSerializer.Deserialize<BackupEnvelope>(ReadBoundedFile(source, MaxBackupFileBytes), JsonOptions)
                ?? throw new InvalidDataException("备份文件格式无效。");
        }
        catch (JsonException) { throw new InvalidDataException("备份文件格式无效。"); }
        if (envelope.Format != BackupFormat || envelope.Version != 1 || envelope.Kdf != "PBKDF2-SHA256" ||
            envelope.Cipher != "AES-256-GCM" || envelope.Iterations < BackupIterations || envelope.Iterations > 2_000_000)
            throw new InvalidDataException("备份版本、加密格式或参数不受支持。");
        var salt = Decode(envelope.Salt, 32, exact: true);
        var nonce = Decode(envelope.Nonce, 12, exact: true);
        var tag = Decode(envelope.Tag, 16, exact: true);
        var ciphertext = Decode(envelope.Ciphertext, MaxPlaintextBytes, exact: false);
        if (ciphertext.Length == 0) throw new InvalidDataException("备份文件内容为空。");
        var plaintext = new byte[ciphertext.Length];
        var key = DeriveKey(passphrase, salt, envelope.Iterations);
        try
        {
            using var aes = new AesGcm(key, tag.Length);
            try { aes.Decrypt(nonce, ciphertext, tag, plaintext, BackupAdditionalData); }
            catch (CryptographicException) { throw new InvalidDataException("备份口令不正确，或备份文件已损坏。"); }
            // Returning a snapshot cannot modify the current local vault.
            return DeserializeSnapshot(plaintext);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(key);
            CryptographicOperations.ZeroMemory(plaintext);
        }
    }

    private VaultSnapshot ReadLocalVault()
    {
        var protectedBytes = ReadBoundedFile(_vaultPath, MaxVaultFileBytes);
        var plaintext = Protect(protectedBytes, decrypt: true);
        try { return DeserializeSnapshot(plaintext); }
        finally { CryptographicOperations.ZeroMemory(plaintext); }
    }

    private T WithVaultLock<T>(Func<T> action)
    {
        if (!OperatingSystem.IsWindows()) throw new PlatformNotSupportedException("本地密码库需要 Windows 用户数据保护服务。");
        lock (_sync)
        {
            using var mutex = new Mutex(false, _mutexName);
            var acquired = false;
            try
            {
                try { acquired = mutex.WaitOne(TimeSpan.FromSeconds(15)); }
                catch (AbandonedMutexException) { acquired = true; }
                if (!acquired) throw new IOException("密码库正在被其他进程使用，请稍后重试。");
                return action();
            }
            finally { if (acquired) mutex.ReleaseMutex(); }
        }
    }

    private static byte[] SerializeSnapshot(VaultSnapshot snapshot)
    {
        ValidateSnapshot(snapshot);
        var bytes = JsonSerializer.SerializeToUtf8Bytes(snapshot, JsonOptions);
        if (bytes.Length <= MaxPlaintextBytes) return bytes;
        CryptographicOperations.ZeroMemory(bytes);
        throw new InvalidDataException("密码库内容过大，最多允许 32 MiB。");
    }

    private static VaultSnapshot DeserializeSnapshot(byte[] bytes)
    {
        if (bytes.Length == 0 || bytes.Length > MaxPlaintextBytes) throw new InvalidDataException("密码库大小无效。");
        VaultSnapshot snapshot;
        try { snapshot = JsonSerializer.Deserialize<VaultSnapshot>(bytes, JsonOptions) ?? throw new InvalidDataException("密码库格式无效。"); }
        catch (JsonException) { throw new InvalidDataException("密码库格式无效。"); }
        ValidateSnapshot(snapshot);
        return snapshot;
    }

    private static void ValidateSnapshot(VaultSnapshot snapshot)
    {
        ArgumentNullException.ThrowIfNull(snapshot);
        if (snapshot.Version != 1 || snapshot.Entries is null || snapshot.ArchiveMatches is null || snapshot.History is null)
            throw new InvalidDataException("密码库版本或结构无效。");
        if (snapshot.Entries.Count > 100_000 || snapshot.ArchiveMatches.Count > 100_000 || snapshot.History.Count > 10_000)
            throw new InvalidDataException("密码库记录数量超过限制。");
        var ids = new HashSet<Guid>();
        foreach (var entry in snapshot.Entries)
        {
            if (entry is null || entry.Id == Guid.Empty || !ids.Add(entry.Id) || string.IsNullOrEmpty(entry.Secret) ||
                entry.Secret.Length > 16_384 || entry.Label is null || entry.Label.Length > 4096 ||
                entry.Group is null || entry.Group.Length > 4096 || entry.Notes is null || entry.Notes.Length > 65_536 || entry.SuccessCount < 0)
                throw new InvalidDataException("密码记录格式或长度无效。");
        }
        foreach (var match in snapshot.ArchiveMatches)
            if (string.IsNullOrWhiteSpace(match.Key) || match.Key.Length > 256 || match.Value == Guid.Empty)
                throw new InvalidDataException("压缩包密码关联格式无效。");
        foreach (var entry in snapshot.History)
            if (entry is null || entry.FileName is null || entry.FileName.Length > 32_768 ||
                entry.Status is null || entry.Status.Length > 256 || entry.Message is null || entry.Message.Length > 65_536 ||
                (entry.OutputPath?.Length ?? 0) > 32_768)
                throw new InvalidDataException("任务历史格式或长度无效。");
    }

    private static void ValidatePassphrase(string passphrase)
    {
        ArgumentNullException.ThrowIfNull(passphrase);
        if (passphrase.Length < 8 || passphrase.Length > 1024)
            throw new ArgumentException("备份口令长度必须为 8 至 1024 个字符。", nameof(passphrase));
    }

    private static byte[] DeriveKey(string passphrase, byte[] salt, int iterations)
    {
        var passwordBytes = Encoding.UTF8.GetBytes(passphrase);
        try { return Rfc2898DeriveBytes.Pbkdf2(passwordBytes, salt, iterations, HashAlgorithmName.SHA256, 32); }
        finally { CryptographicOperations.ZeroMemory(passwordBytes); }
    }

    private static byte[] Decode(string? value, int maxBytes, bool exact)
    {
        if (value is null || value.Length > ((maxBytes + 2L) / 3) * 4)
            throw new InvalidDataException("备份加密数据长度无效。");
        byte[] bytes;
        try { bytes = Convert.FromBase64String(value); }
        catch (FormatException) { throw new InvalidDataException("备份加密数据格式无效。"); }
        if (bytes.Length > maxBytes || (exact && bytes.Length != maxBytes))
            throw new InvalidDataException("备份加密数据长度无效。");
        return bytes;
    }

    private static byte[] ReadBoundedFile(string path, int maximumBytes)
    {
        using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
        if (stream.Length == 0 || stream.Length > maximumBytes) throw new InvalidDataException("文件为空或超过大小限制。");
        var bytes = new byte[(int)stream.Length];
        stream.ReadExactly(bytes);
        return bytes;
    }

    private static void AtomicWrite(string path, byte[] bytes, string? backupPath)
    {
        var directory = Path.GetDirectoryName(Path.GetFullPath(path))!;
        Directory.CreateDirectory(directory);
        var temporaryPath = Path.Combine(directory, ".openunpack-" + Guid.NewGuid().ToString("N") + ".tmp");
        try
        {
            using (var stream = new FileStream(temporaryPath, FileMode.CreateNew, FileAccess.Write, FileShare.None,
                       64 * 1024, FileOptions.WriteThrough))
            {
                stream.Write(bytes);
                stream.Flush(flushToDisk: true);
            }
            if (File.Exists(path)) File.Replace(temporaryPath, path, backupPath, ignoreMetadataErrors: false);
            else File.Move(temporaryPath, path);
        }
        finally
        {
            try { File.Delete(temporaryPath); }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
        }
    }

    private static byte[] Protect(byte[] bytes, bool decrypt)
    {
        if (!OperatingSystem.IsWindows()) throw new PlatformNotSupportedException("本地密码库需要 Windows 用户数据保护服务。");
        var pinnedBytes = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        var pinnedEntropy = GCHandle.Alloc(VaultEntropy, GCHandleType.Pinned);
        var output = new DataBlob();
        try
        {
            var input = new DataBlob { Length = bytes.Length, Data = pinnedBytes.AddrOfPinnedObject() };
            var entropy = new DataBlob { Length = VaultEntropy.Length, Data = pinnedEntropy.AddrOfPinnedObject() };
            var success = decrypt
                ? CryptUnprotectData(ref input, IntPtr.Zero, ref entropy, IntPtr.Zero, IntPtr.Zero, 1, out output)
                : CryptProtectData(ref input, "OpenUnpack local vault", ref entropy, IntPtr.Zero, IntPtr.Zero, 1, out output);
            if (!success) throw new InvalidDataException(decrypt
                ? "密码库无法解密：文件已损坏，或不属于当前 Windows 用户。"
                : "Windows 用户数据保护服务无法加密密码库。");
            if (output.Length <= 0 || output.Length > MaxVaultFileBytes) throw new InvalidDataException("密码库保护数据长度无效。");
            var result = new byte[output.Length];
            Marshal.Copy(output.Data, result, 0, result.Length);
            return result;
        }
        finally
        {
            if (output.Data != IntPtr.Zero)
            {
                // Clear the native plaintext buffer before releasing DPAPI-owned memory.
                var zeros = new byte[4096];
                for (var offset = 0; offset < output.Length; offset += zeros.Length)
                    Marshal.Copy(zeros, 0, IntPtr.Add(output.Data, offset), Math.Min(zeros.Length, output.Length - offset));
                _ = LocalFree(output.Data);
            }
            pinnedEntropy.Free();
            pinnedBytes.Free();
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct DataBlob { public int Length; public IntPtr Data; }

    [DllImport("crypt32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CryptProtectData(ref DataBlob input, string description, ref DataBlob entropy,
        IntPtr reserved, IntPtr prompt, uint flags, out DataBlob output);

    [DllImport("crypt32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CryptUnprotectData(ref DataBlob input, IntPtr description, ref DataBlob entropy,
        IntPtr reserved, IntPtr prompt, uint flags, out DataBlob output);

    [DllImport("kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr memory);

    private sealed class BackupEnvelope
    {
        public string Format { get; init; } = "";
        public int Version { get; init; }
        public string Kdf { get; init; } = "";
        public int Iterations { get; init; }
        public string Cipher { get; init; } = "";
        public string Salt { get; init; } = "";
        public string Nonce { get; init; } = "";
        public string Tag { get; init; } = "";
        public string Ciphertext { get; init; } = "";
    }
}
