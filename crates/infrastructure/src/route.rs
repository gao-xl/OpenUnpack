use openunpack_protocol::{MAX_CONCURRENT_JOBS, Settings};
use std::path::{Path, PathBuf};

/// 解压路径决策结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionRoute {
    /// 直接解压：直接在目标盘解压，避免经 SSD 中转产生二次复制与额外写入磨损
    Direct { target_dir: PathBuf, reason: String },
    /// SSD 缓存加速：先在 SSD 缓存区解压，处理嵌套压缩包后移至目标目录
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

/// 物理/逻辑存储设备端点标识
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum DeviceEndpoint {
    /// 物理设备编号（如 PhysicalDrive0, PhysicalDrive1）
    Physical(u32),
    /// 驱动器逻辑盘符（如 'C', 'D'）
    Volume(char),
    /// 备用回退标识（如网络共享 UNC 或非标准路径）
    Fallback(String),
}

impl DeviceEndpoint {
    pub fn from_path(path: &Path, device_num: Option<u32>) -> Self {
        if let Some(dev) = device_num {
            Self::Physical(dev)
        } else if let Some(letter) = crate::paths::drive_letter_of(path) {
            Self::Volume(letter)
        } else {
            let share = path.components().next().and_then(|component| {
                if let std::path::Component::Prefix(prefix) = component {
                    match prefix.kind() {
                        std::path::Prefix::UNC(server, share)
                        | std::path::Prefix::VerbatimUNC(server, share) => Some(format!(
                            "unc:{}/{}",
                            server.to_string_lossy().to_lowercase(),
                            share.to_string_lossy().to_lowercase()
                        )),
                        _ => None,
                    }
                } else {
                    None
                }
            });
            // Unknown endpoints share one conservative scheduling identity.
            Self::Fallback(share.unwrap_or_else(|| "unknown-storage".into()))
        }
    }
}

/// 多物理盘独立并发调度仲裁器
#[derive(Debug, Clone, Default)]
pub struct MultiDeviceScheduler {
    pub active_loads: std::collections::HashMap<DeviceEndpoint, usize>,
    pub hdd_endpoints: std::collections::HashSet<DeviceEndpoint>,
    pub max_nvme_concurrency: usize,
}

impl MultiDeviceScheduler {
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            active_loads: std::collections::HashMap::new(),
            hdd_endpoints: std::collections::HashSet::new(),
            max_nvme_concurrency: max_concurrency.clamp(1, MAX_CONCURRENT_JOBS as usize),
        }
    }

    pub fn register_active_job(
        &mut self,
        src_endpoint: DeviceEndpoint,
        src_is_hdd: bool,
        dst_endpoint: DeviceEndpoint,
        dst_is_hdd: bool,
    ) {
        if src_is_hdd {
            self.hdd_endpoints.insert(src_endpoint.clone());
        }
        if dst_is_hdd {
            self.hdd_endpoints.insert(dst_endpoint.clone());
        }
        *self.active_loads.entry(src_endpoint.clone()).or_insert(0) += 1;
        if src_endpoint != dst_endpoint {
            *self.active_loads.entry(dst_endpoint).or_insert(0) += 1;
        }
    }

    pub fn can_schedule(
        &self,
        src_endpoint: &DeviceEndpoint,
        src_is_hdd: bool,
        dst_endpoint: &DeviceEndpoint,
        dst_is_hdd: bool,
    ) -> bool {
        // Keep the stricter classification if this endpoint was already probed
        // as HDD/unknown for another active job during the same arbitration.
        let src_is_hdd = src_is_hdd || self.hdd_endpoints.contains(src_endpoint);
        let dst_is_hdd = dst_is_hdd || self.hdd_endpoints.contains(dst_endpoint);
        // 1. 机械硬盘：排他单任务顺序 I/O，已有活动任务则禁止并发
        let src_load = self.active_loads.get(src_endpoint).copied().unwrap_or(0);
        if src_is_hdd && src_load >= 1 {
            return false;
        }
        if !src_is_hdd && src_load >= self.max_nvme_concurrency {
            return false;
        }

        // 2. 目标设备检查
        let dst_load = self.active_loads.get(dst_endpoint).copied().unwrap_or(0);
        if dst_is_hdd && dst_load >= 1 {
            return false;
        }
        if !dst_is_hdd && dst_load >= self.max_nvme_concurrency {
            return false;
        }

        true
    }
}

pub struct RouteDecider;

impl RouteDecider {
    /// 综合评估解压路径（达到 30 分则启用 SSD 缓存加速）
    pub const CACHE_TRIGGER_THRESHOLD: i32 = 30;

    /// 评估是否值得使用 SSD 缓存加速
    pub fn decide(
        probe: &ArchiveProbeSummary,
        settings: &Settings,
        target_output_dir: &Path,
        ssd_free_bytes: u64,
    ) -> ExtractionRoute {
        // 1. 用户未启用缓存或未指定缓存目录
        if !settings.cache_enabled || settings.cache_root.trim().is_empty() {
            return ExtractionRoute::Direct {
                target_dir: target_output_dir.to_path_buf(),
                reason: "未启用 SSD 缓存或未配置缓存路径".into(),
            };
        }

        let cache_root = PathBuf::from(&settings.cache_root);

        // Use a widened integer budget: rounding must not lose bytes or wrap
        // when an untrusted/unknown size approaches u64::MAX.
        let required_bytes = (u128::from(probe.estimated_extracted_bytes) * 5).div_ceil(4)
            + u128::from(settings.min_free_bytes);
        if u128::from(ssd_free_bytes) < required_bytes {
            return ExtractionRoute::Direct {
                target_dir: target_output_dir.to_path_buf(),
                reason: format!(
                    "SSD 可用空间 ({} MB) 不足安全预算 ({} MB)，直接解压",
                    ssd_free_bytes / (1024 * 1024),
                    required_bytes / (1024 * 1024)
                ),
            };
        }

        if settings.cache_policy == "always" {
            return ExtractionRoute::SsdCache {
                cache_workspace: cache_root.join("OpenUnpack-cache"),
                target_dir: target_output_dir.to_path_buf(),
                score: 0,
                reason: "已选择总是使用中转缓存；空间和配额限制仍然生效".into(),
            };
        }

        // 3. 超大且条目极少的文件：顺序读写无明显寻道瓶颈，直接写入目标盘避免 SSD 寿命磨损
        let is_huge_sequential = probe.entry_count <= 3
            && probe.total_size_bytes >= 2 * 1024 * 1024 * 1024
            && !probe.has_nested_archives;

        if is_huge_sequential {
            // 若源与目标同在机械硬盘，读写争用较大，仍建议经 SSD 中转
            let hdd_head_contention = probe.is_same_physical_disk && probe.is_source_on_hdd;
            if !hdd_head_contention {
                return ExtractionRoute::Direct {
                    target_dir: target_output_dir.to_path_buf(),
                    reason: "大文件且条目极少（纯顺序大块 I/O），直接解压避免 SSD 二次写入磨损"
                        .into(),
                };
            }
        }

        // Source/target contention alone is insufficient evidence of a win:
        // staging adds a copy and final writeback. Unknown entry counts remain
        // unknown; enabling recursive extraction is not evidence of nesting.
        let small_file_workload = probe.entry_count >= 500
            && probe.estimated_extracted_bytes.div_ceil(probe.entry_count) < 256 * 1024;
        if !probe.has_nested_archives && !small_file_workload {
            return ExtractionRoute::Direct {
                target_dir: target_output_dir.to_path_buf(),
                reason: "自动选择：缺少已确认的嵌套或密集小文件信息，避免额外中转和回写".into(),
            };
        }

        // 4. 评估使用 SSD 缓存的加权收益
        let mut score: i32 = 0;
        let mut reasons: Vec<String> = Vec::new();

        // 嵌套压缩包适合在 SSD 处理中间文件：+50 分
        if probe.has_nested_archives {
            score += 50;
            reasons.push("检测到嵌套压缩包 (+50分)".into());
        }

        // 避免机械硬盘自身读写争用：+40 分；涉及机械硬盘：+20 分
        if probe.is_same_physical_disk && probe.is_source_on_hdd {
            score += 40;
            reasons.push("源文件与目标目录在同一机械硬盘 (+40分)".into());
        } else if probe.is_source_on_hdd || probe.is_target_on_hdd {
            score += 20;
            reasons.push("涉及机械硬盘读写 (+20分)".into());
        }

        // 文件数量较多时，SSD 随机写优势明显
        if probe.entry_count > 500 {
            score += 35;
            reasons.push(format!("文件数量较多 ({} 项，+35分)", probe.entry_count));
        } else if probe.entry_count > 50 {
            score += 20;
            reasons.push(format!("文件数量适中 ({} 项，+20分)", probe.entry_count));
        } else if probe.entry_count > 10 {
            score += 10;
            reasons.push(format!("包含多个文件 ({} 项，+10分)", probe.entry_count));
        }

        // 平均文件较小 (< 1MB) 额外加分
        if let Some(avg_size) = probe
            .estimated_extracted_bytes
            .checked_div(probe.entry_count)
            && avg_size < 1024 * 1024
            && probe.entry_count >= 20
        {
            score += 15;
            reasons.push(format!("小文件较多 (平均 {} KB，+15分)", avg_size / 1024));
        }

        // 5. 综合判定
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
                    "中转收益较低 ({} 分 < 阈值 {} 分)，直接解压更优",
                    score,
                    Self::CACHE_TRIGGER_THRESHOLD
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn load_scheduler_allows_five_ssd_jobs_but_no_sixth() {
        let mut scheduler = MultiDeviceScheduler::new(5);
        let disk = DeviceEndpoint::Physical(7);
        for _ in 0..4 {
            scheduler.register_active_job(disk.clone(), false, disk.clone(), false);
        }
        assert!(scheduler.can_schedule(&disk, false, &disk, false));
        scheduler.register_active_job(disk.clone(), false, disk.clone(), false);
        assert!(!scheduler.can_schedule(&disk, false, &disk, false));
    }

    #[test]
    fn load_scheduler_keeps_existing_serial_classification_when_a_probe_changes() {
        let mut scheduler = MultiDeviceScheduler::new(5);
        let serial_disk = DeviceEndpoint::Physical(7);
        let other_disk = DeviceEndpoint::Physical(8);
        scheduler.register_active_job(serial_disk.clone(), true, serial_disk.clone(), true);
        assert!(!scheduler.can_schedule(&serial_disk, false, &other_disk, false));
        assert!(!scheduler.can_schedule(&other_disk, false, &serial_disk, false));
        assert!(scheduler.can_schedule(&other_disk, false, &other_disk, false));
    }

    #[test]
    fn test_hdd_seek_storm_avoidance_scheduler() {
        let mut scheduler = MultiDeviceScheduler::new(4);
        let hdd_0 = DeviceEndpoint::Physical(0);
        let ssd_1 = DeviceEndpoint::Physical(1);
        let hdd_2 = DeviceEndpoint::Physical(2);

        // First job starts on HDD 0 (source HDD 0, target HDD 0)
        assert!(scheduler.can_schedule(&hdd_0, true, &hdd_0, true));
        scheduler.register_active_job(hdd_0.clone(), true, hdd_0.clone(), true);

        // Second job wanting to use HDD 0 must be BLOCKED to prevent seek storm
        assert!(!scheduler.can_schedule(&hdd_0, true, &hdd_0, true));
        assert!(!scheduler.can_schedule(&hdd_0, true, &ssd_1, false));
        assert!(!scheduler.can_schedule(&ssd_1, false, &hdd_0, true));

        // But concurrent job on SSD 1 or different physical HDD 2 can proceed
        assert!(scheduler.can_schedule(&ssd_1, false, &ssd_1, false));
        assert!(scheduler.can_schedule(&hdd_2, true, &hdd_2, true));
    }

    #[test]
    fn unc_aliases_and_unknown_paths_keep_stable_scheduling_identity() {
        assert_eq!(
            DeviceEndpoint::from_path(Path::new(r"\\server\share\a.zip"), None),
            DeviceEndpoint::from_path(Path::new(r"\\?\UNC\SERVER\SHARE\deep\b.zip"), None)
        );
        assert_ne!(
            DeviceEndpoint::from_path(Path::new(r"\\server\share\a.zip"), None),
            DeviceEndpoint::from_path(Path::new(r"\\server\other\b.zip"), None)
        );
        assert_eq!(
            DeviceEndpoint::from_path(Path::new("unknown-a"), None),
            DeviceEndpoint::from_path(Path::new("unknown-b"), None)
        );
        let topology =
            crate::paths::probe_disk_topology(Path::new("unknown-a"), Path::new("unknown-b"));
        assert!(topology.source_is_hdd && topology.target_is_hdd);
    }

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
    fn same_hdd_contention_without_archive_evidence_keeps_direct_route() {
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
        assert!(matches!(route, ExtractionRoute::Direct { .. }));
    }

    fn mixed_probe() -> ArchiveProbeSummary {
        ArchiveProbeSummary {
            total_size_bytes: 64 * 1024 * 1024 + 128 * 4096,
            estimated_extracted_bytes: 64 * 1024 * 1024 + 128 * 4096,
            entry_count: 129,
            has_nested_archives: false,
            is_source_on_hdd: true,
            is_target_on_hdd: true,
            is_same_physical_disk: true,
        }
    }

    #[test]
    fn automatic_cache_avoids_extra_writeback_for_measured_mixed_workload() {
        assert!(matches!(
            RouteDecider::decide(
                &mixed_probe(),
                &sample_settings(),
                Path::new("D:\\output"),
                u64::MAX
            ),
            ExtractionRoute::Direct { .. }
        ));
    }

    #[test]
    fn automatic_cache_does_not_infer_small_files_from_compressed_bytes() {
        let mut probe = mixed_probe();
        probe.entry_count = 1000;
        probe.total_size_bytes = 1024 * 1024;
        probe.estimated_extracted_bytes = 1024 * 1024 * 1024;
        assert!(matches!(
            RouteDecider::decide(
                &probe,
                &sample_settings(),
                Path::new("D:\\output"),
                u64::MAX
            ),
            ExtractionRoute::Direct { .. }
        ));
        probe.estimated_extracted_bytes = 100 * 1024 * 1024;
        assert!(matches!(
            RouteDecider::decide(
                &probe,
                &sample_settings(),
                Path::new("D:\\output"),
                u64::MAX
            ),
            ExtractionRoute::SsdCache { .. }
        ));
    }

    #[test]
    fn explicit_cache_retains_space_gate_and_disabled_cache_stays_direct() {
        let mut settings = sample_settings();
        settings.cache_policy = "always".into();
        let mut probe = mixed_probe();
        probe.entry_count = 0;
        assert!(matches!(
            RouteDecider::decide(&probe, &settings, Path::new("D:\\output"), u64::MAX),
            ExtractionRoute::SsdCache { .. }
        ));
        assert!(matches!(
            RouteDecider::decide(&probe, &settings, Path::new("D:\\output"), 1),
            ExtractionRoute::Direct { .. }
        ));
        settings.cache_enabled = false;
        assert!(matches!(
            RouteDecider::decide(&probe, &settings, Path::new("D:\\output"), u64::MAX),
            ExtractionRoute::Direct { .. }
        ));
    }

    #[test]
    fn cache_space_budget_cannot_overflow_or_round_down() {
        let mut probe = mixed_probe();
        let mut settings = sample_settings();
        settings.cache_policy = "always".into();
        probe.estimated_extracted_bytes = u64::MAX;
        assert!(matches!(
            RouteDecider::decide(&probe, &settings, Path::new("D:\\output"), u64::MAX),
            ExtractionRoute::Direct { .. }
        ));
        settings.min_free_bytes = 0;
        probe.estimated_extracted_bytes = 1;
        assert!(matches!(
            RouteDecider::decide(&probe, &settings, Path::new("D:\\output"), 1),
            ExtractionRoute::Direct { .. }
        ));
        assert!(matches!(
            RouteDecider::decide(&probe, &settings, Path::new("D:\\output"), 2),
            ExtractionRoute::SsdCache { .. }
        ));
    }

    #[test]
    fn test_multi_device_scheduler_isolation_and_concurrency() {
        let mut scheduler = MultiDeviceScheduler::new(4);
        let hdd_dev0 = DeviceEndpoint::Physical(0);
        let nvme_dev1 = DeviceEndpoint::Physical(1);

        // 1. 当机械硬盘 (dev0) 有 1 个任务运行时，阻断第二个同 HDD 任务
        scheduler.register_active_job(hdd_dev0.clone(), true, hdd_dev0.clone(), true);
        assert!(!scheduler.can_schedule(&hdd_dev0, true, &hdd_dev0, true));

        // 2. 但另一块高速 NVMe (dev1) 仍然能够被并行调度
        assert!(scheduler.can_schedule(&nvme_dev1, false, &nvme_dev1, false));

        // 3. NVMe 允许多任务流水线并发（此处配置为 4）
        scheduler.register_active_job(nvme_dev1.clone(), false, nvme_dev1.clone(), false);
        scheduler.register_active_job(nvme_dev1.clone(), false, nvme_dev1.clone(), false);
        assert!(scheduler.can_schedule(&nvme_dev1, false, &nvme_dev1, false));
    }

    #[test]
    fn test_device_endpoint_from_verbatim_and_standard_path() {
        let ep_std = DeviceEndpoint::from_path(Path::new(r"C:\Users\test\file.zip"), None);
        let ep_verb = DeviceEndpoint::from_path(Path::new(r"\\?\C:\Users\test\file.zip"), None);
        assert_eq!(ep_std, DeviceEndpoint::Volume('C'));
        assert_eq!(ep_verb, DeviceEndpoint::Volume('C'));
        assert_eq!(ep_std, ep_verb);
    }
}
