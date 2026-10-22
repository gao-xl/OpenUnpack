use openunpack_domain::DomainError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("领域规则错误: {0}")]
    Domain(#[from] DomainError),

    #[error("任务未找到: {0}")]
    JobNotFound(String),

    #[error("状态存储持久化异常: {0}")]
    StoreError(String),

    #[error("密码安全库错误: {0}")]
    VaultError(String),

    #[error("Worker 进程/管道通信异常: {0}")]
    WorkerError(String),

    #[error("Worker 不可用: {0}")]
    WorkerUnavailable(String),

    #[error("系统环境异常: {0}")]
    EnvironmentError(String),

    #[error("IO 错误: {0}")]
    IoError(#[from] std::io::Error),

    #[error("配置或参数无效: {0}")]
    InvalidInput(String),

    #[error("系统状态冲突: {0}")]
    Conflict(String),
}
