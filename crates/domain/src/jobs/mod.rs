//! 任务领域模型：聚合根、执行选项与状态机。

pub mod model;
pub mod options;
pub mod state_machine;

pub use model::*;
pub use options::*;
pub use state_machine::*;
