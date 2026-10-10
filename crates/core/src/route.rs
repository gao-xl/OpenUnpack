use crate::types::Settings;
use std::path::{Path, PathBuf};

/// 解压路线决策结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionRoute {
    /// 直通模式：直接在目标盘的工作区提取，避免 SSD 无效写入放大与二次复制
    Direct {
        target_dir: PathBuf,
        reason: String,
    },
    /// SSD 加速中转模式：经由高速 SSD 缓存区提取、就地解压嵌套包后原子发布
    SsdCache {
        cache_workspace: PathBuf,
        target_dir: PathBuf,
        score: i32,
        reason: String,
    },
}

/// 归档探针与拓扑特征摘要（由探测探针与硬件探查提供）
#[derive(Debug, Clone)]
pub struct ArchiveProbeSummary {
    /// 归档压缩包总字节数
    pub total_size_bytes: u64,
    /// 预估解压后总字节数（通常为压缩包体积的 1.5 ~ 3 倍）
    pub estimated_extracted_bytes: u64,
    /// 包含文件/条目数量
    pub entry_count: u64,
    /// 是否包含内层嵌套压缩包（如 .zip / .rar / .7z）
    pub has_nested_archives: bool,
    /// 源压缩包所在驱动器是否为机械硬盘（HDD）
    pub is_source_on_hdd: bool,
    /// 目标输出目录所在驱动器是否为机械硬盘（HDD）
    pub is_target_on_hdd: bool,
    /// 源路径与目标路径是否位于同一物理磁盘
    pub is_same_physical_disk: bool,
}

/// 物理磁盘拓扑结构
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskTopology {
    pub source_is_hdd: bool,
    pub target_is_hdd: bool,
    pub is_same_physical_disk: bool,
    pub source_device_number: Option<u32>,
    pub target_device_number: Option<u32>,
}

pub struct RouteDecider;

impl RouteDecider {
    /// 判定阈值基线（达到 30 分则激活 SSD 缓存中转加速）
    pub const CACHE_TRIGGER_THRESHOLD: i32 = 30;

    /// 综合评估决策解压路线
    pub fn decide(
        probe: &ArchiveProbeSummary,
        settings: &Settings,
        target_output_dir: &Path,
        ssd_free_bytes: u64,
    ) -> ExtractionRoute {
        // 1. 用户显式禁用缓存配置或未配置路径
        if !settings.cache_enabled || settings.cache_root.trim().is_empty() {
            return ExtractionRoute::Direct {
                target_dir: target_output_dir.to_path_buf(),
                reason: "用户设置未启用 SSD 缓存或未配置缓存路径".into(),
            };
        }

        let cache_root = PathBuf::from(&settings.cache_root);

        // 2. 空间安全预算熔断检查 (安全底线：保留 min_free_bytes + 1.25 倍预估膨胀体积)
        let safety_factor = 1.25;
        let required_bytes = ((probe.estimated_extracted_bytes as f64) * safety_factor) as u64
            + settings.min_free_bytes;
        if ssd_free_bytes < required_bytes {
            return ExtractionRoute::Direct {
                target_dir: target_output_dir.to_path_buf(),
                reason: format!(
                    "SSD 可用空间 ({} MB) 不足安全预算 ({} MB)，自动熔断直通",
                    ssd_free_bytes / (1024 * 1024),
                    required_bytes / (1024 * 1024)
                ),
            };
        }

        // 3. 巨型单文件绕行机制 (Bypass Huge Sequential Files)
        // 单个文件超大且条目极少时，纯顺序读写无随机寻道瓶颈，跨盘直接解压避免 SSD 双倍写入寿命损耗
        let is_huge_sequential = probe.entry_count <= 3
            && probe.total_size_bytes >= 2 * 1024 * 1024 * 1024
            && !probe.has_nested_archives;

        if is_huge_sequential {
            // 例外：若源盘与目标盘在同一物理机械盘，磁头仍会严重争用，此时走 SSD 依然具有解耦价值
            let hdd_head_contention = probe.is_same_physical_disk && probe.is_source_on_hdd;
            if !hdd_head_contention {
                return ExtractionRoute::Direct {
                    target_dir: target_output_dir.to_path_buf(),
                    reason: "大文件且条目极少（纯顺序大块 I/O），直通绕过 SSD 避免二次中转磨损".into(),
                };
            }
        }

        // 4. 多维度智能加权评分体系
        let mut score: i32 = 0;
        let mut reasons: Vec<String> = Vec::new();

        // 维度 A：双层嵌套包（强需求：+50 分）
        if probe.has_nested_archives {
            score += 50;
            reasons.push("检测到双层/多层嵌套归档 (+50分)".into());
        }

        // 维度 B：同物理机械盘读写冲突避让（+40 分）
        if probe.is_same_physical_disk && probe.is_source_on_hdd {
            score += 40;
            reasons.push("源与目标处于同一物理 HDD，避开磁头严重寻道争用 (+40分)".into());
        } else if probe.is_source_on_hdd || probe.is_target_on_hdd {
            score += 20;
            reasons.push("涉及机械硬盘流转 (+20分)".into());
        }

        // 维度 C：海量零碎小文件高随机写场景
        if probe.entry_count > 500 {
            score += 35;
            reasons.push(format!("海量小文件集群 ({} 项，+35分)", probe.entry_count));
        } else if probe.entry_count > 50 {
            score += 20;
            reasons.push(format!("中等文件聚集 ({} 项，+20分)", probe.entry_count));
        } else if probe.entry_count > 10 {
            score += 10;
            reasons.push(format!("多文件分布 ({} 项，+10分)", probe.entry_count));
        }

        // 维度 D：小平均文件大小 (< 1MB) 额外加权
        if probe.entry_count > 0 {
            let avg_size = probe.total_size_bytes / probe.entry_count;
            if avg_size < 1024 * 1024 && probe.entry_count >= 20 {
                score += 15;
                reasons.push(format!("文件平均体积较小 ({} KB，+15分)", avg_size / 1024));
            }
        }

        // 5. 最终判定输出
        if score >= Self::CACHE_TRIGGER_THRESHOLD {
            ExtractionRoute::SsdCache {
                cache_workspace: cache_root.join("OpenUnpack-cache"),
                target_dir: target_output_dir.to_path_buf(),
                score,
                reason: reasons.join("; "),
            }
        } else {
            ExtractionRoute::Direct {
                target_dir: target_output_dir.to_path_buf(),
                reason: format!(
                    "收益评分低 ({} 分 < 阈值 {} 分)，直通更优",
                    score, Self::CACHE_TRIGGER_THRESHOLD
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_settings() -> Settings {
        Settings {
            cache_enabled: true,
            cache_root: "C:\\cache".into(),
            min_free_bytes: 1024 * 1024 * 1024, // 1GB
            ..Default::default()
        }
    }

    #[test]
    fn test_route_disabled_cache() {
        let mut settings = sample_settings();
        settings.cache_enabled = false;
        let probe = ArchiveProbeSummary {
            total_size_bytes: 100 * 1024 * 1024,
            estimated_extracted_bytes: 200 * 1024 * 1024,
            entry_count: 50,
            has_nested_archives: true,
            is_source_on_hdd: false,
            is_target_on_hdd: false,
            is_same_physical_disk: false,
        };
        let target = Path::new("D:\\output");
        let route = RouteDecider::decide(&probe, &settings, target, 100 * 1024 * 1024 * 1024);
        assert!(matches!(route, ExtractionRoute::Direct { .. }));
    }

    #[test]
    fn test_route_space_budget_hard_gate() {
        let settings = sample_settings();
        let probe = ArchiveProbeSummary {
            total_size_bytes: 5 * 1024 * 1024 * 1024,
            estimated_extracted_bytes: 10 * 1024 * 1024 * 1024,
            entry_count: 1000,
            has_nested_archives: true,
            is_source_on_hdd: true,
            is_target_on_hdd: true,
            is_same_physical_disk: true,
        };
        let target = Path::new("D:\\output");
        // SSD free only 2GB, needs > 13.5GB
        let route = RouteDecider::decide(&probe, &settings, target, 2 * 1024 * 1024 * 1024);
        match route {
            ExtractionRoute::Direct { reason, .. } => {
                assert!(reason.contains("不足安全预算"));
            }
            _ => panic!("Expected Direct fallback on low space"),
        }
    }

    #[test]
    fn test_route_huge_sequential_bypass() {
        let settings = sample_settings();
        let probe = ArchiveProbeSummary {
            total_size_bytes: 20 * 1024 * 1024 * 1024,
            estimated_extracted_bytes: 20 * 1024 * 1024 * 1024,
            entry_count: 1, // Single 20GB ISO
            has_nested_archives: false,
            is_source_on_hdd: true,
            is_target_on_hdd: false, // Different drives
            is_same_physical_disk: false,
        };
        let target = Path::new("E:\\output");
        let route = RouteDecider::decide(&probe, &settings, target, 50 * 1024 * 1024 * 1024);
        match route {
            ExtractionRoute::Direct { reason, .. } => {
                assert!(reason.contains("纯顺序大块"));
            }
            _ => panic!("Expected Direct bypass for huge sequential file"),
        }
    }

    #[test]
    fn test_route_nested_archive_triggers_cache() {
        let settings = sample_settings();
        let probe = ArchiveProbeSummary {
            total_size_bytes: 500 * 1024 * 1024,
            estimated_extracted_bytes: 1024 * 1024 * 1024,
            entry_count: 10,
            has_nested_archives: true,
            is_source_on_hdd: false,
            is_target_on_hdd: false,
            is_same_physical_disk: false,
        };
        let target = Path::new("D:\\output");
        let route = RouteDecider::decide(&probe, &settings, target, 50 * 1024 * 1024 * 1024);
        match route {
            ExtractionRoute::SsdCache { score, .. } => {
                assert!(score >= 50);
            }
            _ => panic!("Expected SsdCache for nested archive"),
        }
    }

    #[test]
    fn test_route_same_hdd_triggers_cache() {
        let settings = sample_settings();
        let probe = ArchiveProbeSummary {
            total_size_bytes: 500 * 1024 * 1024,
            estimated_extracted_bytes: 1024 * 1024 * 1024,
            entry_count: 10,
            has_nested_archives: false,
            is_source_on_hdd: true,
            is_target_on_hdd: true,
            is_same_physical_disk: true,
        };
        let target = Path::new("D:\\output");
        let route = RouteDecider::decide(&probe, &settings, target, 50 * 1024 * 1024 * 1024);
        match route {
            ExtractionRoute::SsdCache { score, .. } => {
                assert!(score >= 40);
            }
            _ => panic!("Expected SsdCache for same HDD contention"),
        }
    }
}
