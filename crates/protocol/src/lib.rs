//! OpenUnpack Protocol - 跨进程/跨层通信契约层。
//!
//! 定义 UI、local-api 与协调器之间共享的 DTO；
//! 字段命名使用 camelCase 序列化，是与前端的线格式契约。

pub mod desktop;
pub mod error;
pub mod worker;

pub use desktop::*;
pub use error::*;
pub use worker::*;

use serde::{Deserialize, Serialize};

/// 协议层密码候选。与领域层 `DomainCandidate` 的区别：
/// 无来源/统计字段，仅承载传输所需的最小集合。
///
/// `secret` 为明文，Drop 时 zeroize；Debug 输出永不渲染明文。
#[derive(Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub revision: u64,
    pub label: String,
    pub secret: String,
}

impl Drop for Candidate {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
}

impl std::fmt::Debug for Candidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the secret: Debug output may reach logs or panic messages.
        f.debug_struct("Candidate")
            .field("id", &self.id)
            .field("revision", &self.revision)
            .field("label", &self.label)
            .field(
                "secret",
                &if self.secret.is_empty() {
                    "empty"
                } else {
                    "[REDACTED]"
                },
            )
            .finish()
    }
}

/// 密码命中事件：某候选验证成功后回写密钥库的协议载荷。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessEvent {
    /// 事件唯一 ID，用于消费侧幂等去重。
    pub event_id: String,
    pub password_id: String,
    pub password_revision: u64,
    /// 归因指纹（如命中来源），用于统计而非反查明文。
    pub fingerprint: Option<String>,
}
