//! 解压资源预算：条目数与字节量的统一上限。

use serde::{Deserialize, Serialize};

/// 一次解压作业允许消耗的资源上限集合。
///
/// 超出任一项即产生 [`crate::DomainError::BudgetExceeded`]，用于防御
/// 压缩炸弹（zip bomb）与超深目录嵌套。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceBudget {
    /// 解压产生的最大条目（文件 + 目录）数量。
    pub max_entries: u64,
    /// 压缩内容总字节上限（解压前体积）。
    pub max_total_bytes: u64,
    /// 实际写出字节上限（解压后体积）。
    pub max_written_bytes: u64,
    /// 目标盘必须保留的最小剩余空间。
    pub min_free_bytes: u64,
    /// 单个解码器允许占用的内存上限。
    pub decoder_memory_limit_bytes: u64,
    /// 单批密码候选的提交数量上限。
    pub max_batch_candidates: usize,
}

impl Default for ResourceBudget {
    fn default() -> Self {
        // 默认预算面向消费级单任务场景：10 万条目 / 100 GiB 双向体积 /
        // 1 GiB 保留空间 / 2 GiB 解码内存 / 128 个密码候选。
        Self {
            max_entries: 100_000,
            max_total_bytes: 100 * 1024 * 1024 * 1024,
            max_written_bytes: 100 * 1024 * 1024 * 1024,
            min_free_bytes: 1024 * 1024 * 1024,
            decoder_memory_limit_bytes: 2 * 1024 * 1024 * 1024,
            max_batch_candidates: 128,
        }
    }
}
