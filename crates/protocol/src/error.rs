use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WireErrorCode {
    Success,
    UnsupportedFormat,
    PasswordRequired,
    PasswordIncorrect,
    CorruptedArchive,
    MissingVolume,
    OutputConflict,
    ResourceLimit,
    OperationCancelled,
    ProcessTerminated,
    IoError,
    InvalidManifest,
    ProtocolMismatch,
    InternalError,
    Custom(String),
}

impl WireErrorCode {
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
