//! 压缩包领域规则：排除匹配、单根分组、输出命名。
//!
//! 三个子模块均为纯函数实现，是全仓库唯一的规则真源——
//! `core::scanner` 与 `infra::engine::path_safety` 只做委托。

pub mod exclusions;
pub mod grouping;
pub mod naming;

pub use exclusions::*;
pub use grouping::*;
pub use naming::*;
