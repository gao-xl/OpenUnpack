//! OpenUnpack Core - 任务编排与批量解压核心引擎。
//!
//! 作为唯一权威业务状态机与调度核心，负责队列持久化、分卷识别、密码匹配、
//! 目录扫描、原子发布、目录工具箱及文件监听自动入队。

pub mod coordinator;
pub mod error;
pub use error::UnpackError;
pub mod dedup {
    pub use openunpack_infrastructure::dedup::*;
}
pub mod intake_guard;
pub mod paths;
pub mod repository;
pub mod route;
mod saferecycle;
pub mod scanner;
pub mod toolbox;
mod transfer_progress;
pub mod types;
pub mod vault;
pub mod watcher;
pub mod worker;
pub mod zip_encoding;
