//! OpenUnpack Application - 用例编排与端口契约层
//!
//! 定义外部适配器 Port 接口，并提供业务用例编排与任务状态流转调度。

pub mod error;
pub mod ports;
pub mod use_cases;

pub use error::ApplicationError;
pub use ports::{
    PasswordSuccessEvent, SessionHandle, SystemEnvironment, VaultStore, WorkerSupervisor,
};
pub use use_cases::VaultUseCases;
