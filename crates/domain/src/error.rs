use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    #[error("非法状态转换：从 {from} 到 {to}")]
    InvalidStateTransition { from: String, to: String },
    #[error("节点未找到：{0}")]
    NodeNotFound(String),
    #[error("超出资源预算限制：{0}")]
    BudgetExceeded(String),
    #[error("不安全的路径段或包含非法字符：{0}")]
    UnsafePathSegment(String),
    #[error("分卷编号不连续或缺失：{0}")]
    VolumeSequenceError(String),
}
