//! 密码候选来源分类。

use serde::{Deserialize, Serialize};

/// 密码候选的来源；排序器据此决定尝试优先级（显式输入 > 文件名提示 > 密钥库）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateOrigin {
    /// 来自本地密码库（vault）的存量条目。
    VaultEntry,
    /// 从压缩包文件名中提取的提示（如 `密码123.zip`）。
    FileNameHint,
    /// 用户在本次会话中手动输入的密码。
    ExplicitInput,
    /// 由基础候选派生的变体（去括号、去前缀等）。
    Derived,
}

impl CandidateOrigin {
    /// 返回用于持久化/展示的稳定短名。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::VaultEntry => "vault",
            Self::FileNameHint => "hint",
            Self::ExplicitInput => "input",
            Self::Derived => "derived",
        }
    }
}
