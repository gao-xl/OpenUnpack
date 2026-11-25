//! 领域层统一错误类型。
//!
//! 所有领域规则（状态机、资源预算、路径安全、分卷校验）的失败都以
//! [`DomainError`] 返回，不携带 IO 或基础设施细节。

use thiserror::Error;

/// 领域规则违反的统一错误类型。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    /// 任务状态机中出现了转移矩阵不允许的状态跳变。
    #[error("非法状态转换：从 {from} 到 {to}")]
    InvalidStateTransition { from: String, to: String },
    /// 引用的聚合节点（任务/目录树节点）不存在，通常指向上游 ID 失效。
    #[error("节点未找到：{0}")]
    NodeNotFound(String),
    /// 操作会突破资源预算上限（并发/磁盘/内存等），由预算模块判定。
    #[error("超出资源预算限制：{0}")]
    BudgetExceeded(String),
    /// 路径段包含非法字符或存在穿越风险（如绝对路径、`..`），拒绝展开。
    #[error("不安全的路径段或包含非法字符：{0}")]
    UnsafePathSegment(String),
    /// 分卷压缩包的卷号不连续或缺失，无法按序解压。
    #[error("分卷编号不连续或缺失：{0}")]
    VolumeSequenceError(String),
}
