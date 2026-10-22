//! OpenUnpack Infrastructure - 外部适配器实现层
//!
//! 统一承载生产文件系统安全 IO、DPAPI、Worker、路由与 Application 端口适配器。

pub mod dedup;
pub mod dpapi;
pub mod engine;
pub mod paths;
mod recycle_shell;
pub mod route;
pub mod saferecycle;
pub mod sqlite;
pub mod system;
pub mod worker;

pub use dpapi::vault_store::DpapiVaultStore;
pub use system::environment::WindowsSystemEnvironment;
pub use worker::supervisor::LocalWorkerSupervisor;
