using System.Buffers.Binary;

namespace OpenUnpack.Engine;

/// <summary>
/// Implements one LZ4 frame with independent blocks and no preset dictionary.
/// Concatenated frames, skippable frames and trailing data are deliberately unsupported.
/// </summary>
public static class Lz4Decoder
{
    public const uint MagicNumber = 0x184D2204;

    public static bool IsLz4Frame(ReadOnlySpan<byte> header) =>
        header.Length >= 4 && BinaryPrimitives.ReadUInt32LittleEndian(header) == MagicNumber;

    public sealed record FrameInfo(long? ContentSize, int MaxBlockSize, bool BlockChecksum, bool ContentChecksum);

    // Reads at most 15 bytes, so inspection never decodes an unbounded payload.
    public static async Task<FrameInfo> ReadHeaderAsync(Stream input, long maxAllowedBytes, CancellationToken cancellationToken)
    {
        if (maxAllowedBytes < 0) throw new ArchiveException(ArchiveFailure.ResourceLimit, "LZ4 资源预算无效。");
        byte[] header = new byte[6];
        await input.ReadExactlyAsync(header, cancellationToken);
        if (!IsLz4Frame(header)) throw new InvalidDataException("Invalid LZ4 frame magic.");
        byte flg = header[4], bd = header[5];
        if ((flg >> 6) != 1 || (flg & 0x02) != 0 || (bd & 0x8F) != 0)
            throw new InvalidDataException("Invalid LZ4 frame descriptor.");
        int blockSize = ((bd >> 4) & 7) switch
        {
            4 => 64 * 1024, 5 => 256 * 1024, 6 => 1024 * 1024, 7 => 4 * 1024 * 1024,
            _ => throw new InvalidDataException("Invalid LZ4 maximum block size.")
        };
        if ((flg & 0x20) == 0 || (flg & 1) != 0)
            throw new ArchiveException(ArchiveFailure.Unsupported, "暂不支持依赖块或预设字典的 LZ4 帧。");
        byte[] descriptor = new byte[(flg & 8) != 0 ? 10 : 2];
        descriptor[0] = flg; descriptor[1] = bd;
        long? contentSize = null;
        if (descriptor.Length == 10)
        {
            await input.ReadExactlyAsync(descriptor.AsMemory(2, 8), cancellationToken);
            ulong value = BinaryPrimitives.ReadUInt64LittleEndian(descriptor.AsSpan(2));
            if (value > (ulong)maxAllowedBytes)
                throw new ArchiveException(ArchiveFailure.ResourceLimit, "LZ4 标称展开量超出资源预算。");
            contentSize = (long)value;
        }
        byte[] checksum = new byte[1];
        await input.ReadExactlyAsync(checksum, cancellationToken);
        if (checksum[0] != (byte)(XxHash32.Compute(descriptor) >> 8))
            throw new InvalidDataException("LZ4 header checksum mismatch.");
        return new(contentSize, blockSize, (flg & 0x10) != 0, (flg & 4) != 0);
    }

    public static Task<long> DecompressFrameAsync(Stream input, Stream output, CancellationToken cancellationToken) =>
        DecompressFrameAsync(input, output, long.MaxValue, 0, null, cancellationToken);

    public static async Task<long> DecompressFrameAsync(
        Stream input,
        Stream output,
        long maxAllowedBytes = long.MaxValue,
        long minFreeBytes = 0,
        string? destinationDirectory = null,
        CancellationToken cancellationToken = default)
    {
        if (minFreeBytes > 0 && !string.IsNullOrEmpty(destinationDirectory))
        {
            Native.SevenZipEngine.EnsureFreeSpace(destinationDirectory, minFreeBytes);
        }

        var info = await ReadHeaderAsync(input, maxAllowedBytes, cancellationToken);
        bool blockChecksumFlag = info.BlockChecksum;
        bool contentChecksumFlag = info.ContentChecksum;
        int maxBlockSize = info.MaxBlockSize;

        byte[] inBuffer = new byte[maxBlockSize];
        byte[] outBuffer = new byte[maxBlockSize];
        long totalWritten = 0;
        long bytesSinceLastSpaceCheck = 0;
        IncrementalXxHash32? contentHasher = contentChecksumFlag ? new IncrementalXxHash32(0) : null;
        bool reachedEndMark = false;

        byte[] blockSizeBuf = new byte[4];
        while (true)
        {
            cancellationToken.ThrowIfCancellationRequested();
            int bytesRead = await input.ReadAtLeastAsync(blockSizeBuf, 4, throwOnEndOfStream: false, cancellationToken);
            if (bytesRead == 0)
            {
                throw new InvalidDataException("Unexpected end of stream: LZ4 frame ended before EndMark.");
            }
            if (bytesRead < 4) throw new InvalidDataException("Unexpected end of stream reading LZ4 block size.");

            uint rawSize = BinaryPrimitives.ReadUInt32LittleEndian(blockSizeBuf);
            if (rawSize == 0)
            {
                reachedEndMark = true;
                break; // End of frame marker
            }

            bool uncompressed = (rawSize & 0x80000000) != 0;
            int blockSize = (int)(rawSize & 0x7FFFFFFF);
            if (blockSize == 0 || blockSize > maxBlockSize) throw new InvalidDataException("LZ4 block size exceeds the frame descriptor.");

            if (inBuffer.Length < blockSize) inBuffer = new byte[blockSize];
            await input.ReadExactlyAsync(inBuffer.AsMemory(0, blockSize), cancellationToken);

            if (blockChecksumFlag)
            {
                byte[] blockCrcBuf = new byte[4];
                await input.ReadExactlyAsync(blockCrcBuf, cancellationToken);
                uint expectedBlockCrc = XxHash32.Compute(inBuffer.AsSpan(0, blockSize), 0);
                uint actualBlockCrc = BinaryPrimitives.ReadUInt32LittleEndian(blockCrcBuf);
                if (expectedBlockCrc != actualBlockCrc)
                {
                    throw new InvalidDataException($"LZ4 block checksum mismatch: expected 0x{expectedBlockCrc:X8}, got 0x{actualBlockCrc:X8}.");
                }
            }

            int chunkBytes;
            if (uncompressed)
            {
                chunkBytes = blockSize;
                if (chunkBytes > maxAllowedBytes - totalWritten)
                    throw new ArchiveException(ArchiveFailure.ResourceLimit, $"LZ4 解压数据量超过设定安全上限 ({maxAllowedBytes} 字节)。");

                contentHasher?.Update(inBuffer.AsSpan(0, chunkBytes));
                await output.WriteAsync(inBuffer.AsMemory(0, blockSize), cancellationToken);
            }
            else
            {
                chunkBytes = DecompressBlock(inBuffer.AsSpan(0, blockSize), outBuffer);
                if (chunkBytes > maxAllowedBytes - totalWritten)
                    throw new ArchiveException(ArchiveFailure.ResourceLimit, $"LZ4 解压数据量超过设定安全上限 ({maxAllowedBytes} 字节)。");

                contentHasher?.Update(outBuffer.AsSpan(0, chunkBytes));
                await output.WriteAsync(outBuffer.AsMemory(0, chunkBytes), cancellationToken);
            }
            totalWritten += chunkBytes;

            if (minFreeBytes > 0 && !string.IsNullOrEmpty(destinationDirectory))
            {
                bytesSinceLastSpaceCheck += chunkBytes;
                if (bytesSinceLastSpaceCheck >= 64 * 1024 * 1024)
                {
                    Native.SevenZipEngine.EnsureFreeSpace(destinationDirectory, minFreeBytes);
                    bytesSinceLastSpaceCheck = 0;
                }
            }
        }

        if (!reachedEndMark)
        {
            throw new InvalidDataException("LZ4 frame truncated: missing EndMark.");
        }

        if (contentChecksumFlag)
        {
            byte[] checksumBuf = new byte[4];
            await input.ReadExactlyAsync(checksumBuf, cancellationToken);
            uint actualContentCrc = BinaryPrimitives.ReadUInt32LittleEndian(checksumBuf);
            uint expectedContentCrc = contentHasher!.GetHash();
            if (actualContentCrc != expectedContentCrc)
            {
                throw new InvalidDataException($"LZ4 content checksum mismatch: expected 0x{expectedContentCrc:X8}, got 0x{actualContentCrc:X8}.");
            }
        }

        if (info.ContentSize.HasValue)
        {
            if (totalWritten != info.ContentSize.Value)
            {
                throw new InvalidDataException("LZ4 decompressed size mismatch.");
            }
        }

        byte[] trailing = new byte[1];
        if (await input.ReadAsync(trailing, cancellationToken) != 0)
            throw new ArchiveException(ArchiveFailure.Unsupported, "暂不支持 LZ4 拼接帧、可跳过帧或尾随数据。");

        return totalWritten;
    }

    public static int DecompressBlock(ReadOnlySpan<byte> input, Span<byte> output)
    {
        int inPos = 0;
        int outPos = 0;

        while (inPos < input.Length)
        {
            byte token = input[inPos++];
            int literalLength = token >> 4;
            if (literalLength == 15)
            {
                byte extra;
                do
                {
                    if (inPos >= input.Length) throw new InvalidDataException("Unexpected end of block in literals.");
                    extra = input[inPos++];
                    literalLength += extra;
                } while (extra == 255);
            }

            if (literalLength > 0)
            {
                if (inPos + literalLength > input.Length || outPos + literalLength > output.Length)
                    throw new InvalidDataException("LZ4 literal copy bounds exceeded.");
                input.Slice(inPos, literalLength).CopyTo(output.Slice(outPos, literalLength));
                inPos += literalLength;
                outPos += literalLength;
            }

            if (inPos >= input.Length) break;

            if (inPos + 2 > input.Length) throw new InvalidDataException("Unexpected end of block reading match offset.");
            int offset = input[inPos] | (input[inPos + 1] << 8);
            inPos += 2;
            if (offset == 0) throw new InvalidDataException("LZ4 invalid match offset 0.");

            int matchLength = (token & 0x0F) + 4;
            if (matchLength == 19)
            {
                byte extra;
                do
                {
                    if (inPos >= input.Length) throw new InvalidDataException("Unexpected end of block in match length.");
                    extra = input[inPos++];
                    matchLength += extra;
                } while (extra == 255);
            }

            int matchStart = outPos - offset;
            if (matchStart < 0) throw new InvalidDataException("LZ4 match offset extends before output buffer start.");

            for (int i = 0; i < matchLength; i++)
            {
                if (outPos >= output.Length) throw new InvalidDataException("LZ4 output buffer overflow.");
                output[outPos++] = output[matchStart + i];
            }
        }

        return outPos;
    }
}
