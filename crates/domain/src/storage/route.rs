//! 跨盘解压路由决策：依据源/目标物理介质与缓存偏好选择落盘路径。

use serde::{Deserialize, Serialize};

/// 目标卷的物理介质类型（由调用方探测后传入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriveMedium {
    Ssd,
    Hdd,
    /// 探测失败的兜底值，按保守策略处理。
    Unknown,
}

/// 解压数据落盘的三条路由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtractionRoute {
    /// 直接写入目标盘（无缓存中转）。
    DirectToTarget,
    /// 先把源压缩包复制到 SSD 缓存，再从缓存解压到目标。
    CacheInputThenExtract,
    /// 先解压到 SSD 缓存，校验后再整体移动到目标盘。
    CacheExtractThenMove,
}

/// 依据缓存开关、源/目标介质与输入缓存偏好，选出解压路由。
///
/// 决策要点：
/// - 缓存关闭时永远直写目标盘；
/// - HDD→HDD 时二选一：缓存输入可平滑慢源读取，缓存输出可减少目标盘寻道；
/// - HDD→SSD 时默认直写（目标已足够快），仅在显式要求缓存输入时中转；
/// - 其余组合（源为 SSD）直接解压，无需中转。
pub fn decide_route(
    cache_enabled: bool,
    source_medium: DriveMedium,
    target_medium: DriveMedium,
    cache_input_pref: bool,
) -> ExtractionRoute {
    if !cache_enabled {
        return ExtractionRoute::DirectToTarget;
    }
    match (source_medium, target_medium) {
        (DriveMedium::Hdd, DriveMedium::Hdd) => {
            if cache_input_pref {
                ExtractionRoute::CacheInputThenExtract
            } else {
                ExtractionRoute::CacheExtractThenMove
            }
        }
        (DriveMedium::Hdd, DriveMedium::Ssd) => {
            if cache_input_pref {
                ExtractionRoute::CacheInputThenExtract
            } else {
                ExtractionRoute::DirectToTarget
            }
        }
        _ => ExtractionRoute::DirectToTarget,
    }
}
