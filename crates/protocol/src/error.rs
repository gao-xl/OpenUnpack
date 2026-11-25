//! 线格式错误码：跨进程结果状态的稳定枚举。

use serde::{Deserialize, Serialize};

/// 跨进程结果错误码。
///
/// 序列化短名（`as_str`）是稳定的线格式契约，
/// 新增枚举值只能追加、不能改名或复用既有短名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WireErrorCode {
    /// 操作成功（空字符串同义）。
    Success,
    /// 压缩格式不受支持。
    UnsupportedFormat,
    /// 需要密码才能继续。
    PasswordRequired,
    /// 提交的密码不正确。
    PasswordIncorrect,
    /// 压缩包数据损坏。
    CorruptedArchive,
    /// 分卷缺失，无法按序解压。
    MissingVolume,
    /// 输出目标冲突（同名/占用等）。
    OutputConflict,
    /// 触及资源预算限制。
    ResourceLimit,
    /// 用户取消操作。
    OperationCancelled,
    /// 子进程异常终止。
    ProcessTerminated,
    /// 通用 IO 错误。
    IoError,
    /// 清单（manifest）数据非法。
    InvalidManifest,
    /// 协议版本不匹配。
    ProtocolMismatch,
    /// 内部错误（未归类）。
    InternalError,
    /// 未知短名的兜底承载。
    Custom(String),
}

impl WireErrorCode {
    /// 返回线格式稳定短名。
    pub fn as_str(&self) -> &str {
        match self {
            Self::Success => "SUCCESS",
            Self::UnsupportedFormat => "UNSUPPORTED_FORMAT",
            Self::PasswordRequired => "PASSWORD_REQUIRED",
            Self::PasswordIncorrect => "PASSWORD_INCORRECT",
            Self::CorruptedArchive => "CORRUPTED_ARCHIVE",
            Self::MissingVolume => "MISSING_VOLUME",
            Self::OutputConflict => "OUTPUT_CONFLICT",
            Self::ResourceLimit => "RESOURCE_LIMIT",
            Self::OperationCancelled => "OPERATION_CANCELLED",
            Self::ProcessTerminated => "PROCESS_TERMINATED",
            Self::IoError => "IO_ERROR",
            Self::InvalidManifest => "INVALID_MANIFEST",
            Self::ProtocolMismatch => "PROTOCOL_MISMATCH",
            Self::InternalError => "INTERNAL_ERROR",
            Self::Custom(s) => s.as_str(),
        }
    }

    /// 从线格式短名还原错误码；未知短名归入 [`WireErrorCode::Custom`]。
    pub fn from_raw(raw: &str) -> Self {
        match raw {
            "" | "SUCCESS" => Self::Success,
            "UNSUPPORTED_FORMAT" => Self::UnsupportedFormat,
            "PASSWORD_REQUIRED" => Self::PasswordRequired,
            "PASSWORD_INCORRECT" => Self::PasswordIncorrect,
            "CORRUPTED_ARCHIVE" => Self::CorruptedArchive,
            "MISSING_VOLUME" => Self::MissingVolume,
            "OUTPUT_CONFLICT" => Self::OutputConflict,
            "RESOURCE_LIMIT" => Self::ResourceLimit,
            "OPERATION_CANCELLED" => Self::OperationCancelled,
            "PROCESS_TERMINATED" => Self::ProcessTerminated,
            "IO_ERROR" => Self::IoError,
            "INVALID_MANIFEST" => Self::InvalidManifest,
            "PROTOCOL_MISMATCH" => Self::ProtocolMismatch,
            "INTERNAL_ERROR" => Self::InternalError,
            other => Self::Custom(other.to_string()),
        }
    }
}
