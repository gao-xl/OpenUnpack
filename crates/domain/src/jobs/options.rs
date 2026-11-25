//! 任务创建时冻结的不可变选项快照。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn default_concurrency() -> u32 {
    1
}

/// 随任务一并持久化的不可变选项。不依赖任何 IO 或协议类型。
/// 未知字段通过 `extensions` 原样保留，保证版本升级时未来选项不被静默丢弃。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenJobOptions {
    /// 解压输出的根目录。
    pub output_root: String,
    /// 同名条目冲突策略（如 skip / overwrite / rename）。
    pub conflict_policy: String,
    /// 密码候选分组标识。
    pub password_group: String,
    /// 是否启用 SSD 缓存中转。
    pub cache_enabled: bool,
    /// 是否优先缓存输入侧（源压缩包）而非输出侧。
    pub cache_input: bool,
    /// 缓存目录根路径。
    pub cache_root: String,
    /// 缓存空间配额（字节）。
    pub cache_quota_bytes: u64,
    /// 目标盘最小保留空间（字节）。
    pub min_free_bytes: u64,
    /// 是否展开嵌套压缩包（压缩包中的压缩包）。
    pub expand_nested: bool,
    /// 嵌套展开的最大深度。
    pub max_depth: u32,
    /// 解压后校验策略（如 crc / none）。
    pub verification: String,
    /// 单任务允许的最大条目数（对应资源预算）。
    pub max_entries: u64,
    /// 单任务允许的最大解压体积（字节）。
    pub max_expanded_bytes: u64,
    /// 递归扫描输入目录子项。
    pub recursive_scan: bool,
    /// 输入扫描阶段的排除规则（通配符语法见 `archive::exclusions`）。
    pub input_exclusions: Vec<String>,
    /// 输出展开阶段的排除规则。
    pub output_exclusions: Vec<String>,
    /// 是否小文件优先调度（降低长时间等待的感知延迟）。
    pub small_first: bool,
    /// UI 主题名。
    pub theme: String,
    /// 历史记录保留条数上限。
    pub history_limit: usize,
    /// 窗口是否总置顶。
    pub always_on_top: bool,
    /// 完成后是否播放提示音。
    pub completion_sound: bool,
    /// 关闭时是否最小化到托盘。
    pub minimize_to_tray: bool,
    /// 单根目录是否自动剥除外层文件夹（智能解包）。
    pub unwrap_single_root: bool,
    /// 源压缩包清理策略（如 keep / delete / recycle）。
    pub source_cleanup: String,
    /// 并发解压任务数；旧版本数据无此字段时按 1 处理。
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
    /// 前向兼容桶：反序列化时收纳所有未知字段，升级后可原样读回。
    #[serde(flatten)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}
