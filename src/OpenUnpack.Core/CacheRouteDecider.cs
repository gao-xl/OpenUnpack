namespace OpenUnpack.Core;

public enum ExtractionRouteType
{
    Direct,
    SsdCache
}

public sealed record ExtractionRoute(
    ExtractionRouteType Type,
    string TargetDirectory,
    string? CacheWorkspace,
    int Score,
    string Reason);

public sealed record ArchiveProbeSummary(
    long TotalSizeBytes,
    long EstimatedExtractedBytes,
    int EntryCount,
    bool HasNestedArchives,
    bool IsSourceOnHdd,
    bool IsTargetOnHdd,
    bool IsSamePhysicalDisk);

public static class CacheRouteDecider
{
    public const int CacheTriggerThreshold = 30;

    public static ExtractionRoute Decide(
        ArchiveProbeSummary probe,
        BatchOptions options,
        string targetOutputDir,
        long ssdFreeBytes)
    {
        // 1. 用户未配置或禁用缓存路径
        if (string.IsNullOrWhiteSpace(options.CacheRoot))
        {
            return new ExtractionRoute(ExtractionRouteType.Direct, targetOutputDir, null, 0, "用户未配置 SSD 缓存路径");
        }

        var cacheWorkspace = Path.Combine(options.CacheRoot, "OpenUnpack-cache");

        // 2. 空间安全预算熔断检查 (安全底线：保留 MinFreeCacheBytes + 1.25 倍预估解压体积)
        var requiredBytes = (long)(probe.EstimatedExtractedBytes * 1.25) + options.MinFreeCacheBytes;
        if (ssdFreeBytes < requiredBytes)
        {
            return new ExtractionRoute(
                ExtractionRouteType.Direct,
                targetOutputDir,
                null,
                0,
                $"SSD 可用空间 ({ssdFreeBytes / (1024 * 1024)} MB) 不足安全预算 ({requiredBytes / (1024 * 1024)} MB)，自动熔断直通");
        }

        // 3. 巨型单文件绕行机制 (Bypass Huge Sequential Files)
        var isHugeSequential = probe.EntryCount <= 3
            && probe.TotalSizeBytes >= 2L * 1024 * 1024 * 1024
            && !probe.HasNestedArchives;

        if (isHugeSequential)
        {
            var hddHeadContention = probe.IsSamePhysicalDisk && probe.IsSourceOnHdd;
            if (!hddHeadContention)
            {
                return new ExtractionRoute(
                    ExtractionRouteType.Direct,
                    targetOutputDir,
                    null,
                    0,
                    "大文件且条目极少（纯顺序大块 I/O），直通绕过 SSD 避免二次中转磨损");
            }
        }

        // 4. 多维度智能加权评分体系
        var score = 0;
        var reasons = new List<string>();

        // 维度 A：双层嵌套包（强需求：+50 分）
        if (probe.HasNestedArchives)
        {
            score += 50;
            reasons.Add("检测到双层/多层嵌套归档 (+50分)");
        }

        // 维度 B：同物理机械盘读写冲突避让（+40 分）
        if (probe.IsSamePhysicalDisk && probe.IsSourceOnHdd)
        {
            score += 40;
            reasons.Add("源与目标处于同一物理 HDD，避开磁头严重寻道争用 (+40分)");
        }
        else if (probe.IsSourceOnHdd || probe.IsTargetOnHdd)
        {
            score += 20;
            reasons.Add("涉及机械硬盘流转 (+20分)");
        }

        // 维度 C：海量零碎小文件高随机写场景
        if (probe.EntryCount > 500)
        {
            score += 35;
            reasons.Add($"海量小文件集群 ({probe.EntryCount} 项，+35分)");
        }
        else if (probe.EntryCount > 50)
        {
            score += 20;
            reasons.Add($"中等文件聚集 ({probe.EntryCount} 项，+20分)");
        }
        else if (probe.EntryCount > 10)
        {
            score += 10;
            reasons.Add($"多文件分布 ({probe.EntryCount} 项，+10分)");
        }

        // 维度 D：小平均文件大小 (< 1MB) 额外加权
        if (probe.EntryCount > 0)
        {
            var avgSize = probe.TotalSizeBytes / probe.EntryCount;
            if (avgSize < 1024 * 1024 && probe.EntryCount >= 20)
            {
                score += 15;
                reasons.Add($"文件平均体积较小 ({avgSize / 1024} KB，+15分)");
            }
        }

        if (score >= CacheTriggerThreshold)
        {
            return new ExtractionRoute(
                ExtractionRouteType.SsdCache,
                targetOutputDir,
                cacheWorkspace,
                score,
                string.Join("; ", reasons));
        }

        return new ExtractionRoute(
            ExtractionRouteType.Direct,
            targetOutputDir,
            null,
            score,
            $"收益评分低 ({score} 分 < 阈值 {CacheTriggerThreshold} 分)，直通更优");
    }
}
