use crate::error::DomainError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Queued,
    Running,
    Paused,
    Stopping,
    Completed,
    Salvaged,
    Partial,
    NeedsPassword,
    WaitingResource,
    Failed,
    Cancelled,
    Skipped,
    Interrupted,
    PublishPending,
    CleanupPending,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Completed => "completed",
            Self::Salvaged => "salvaged",
            Self::Partial => "partial",
            Self::NeedsPassword => "needsPassword",
            Self::WaitingResource => "waitingResource",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
            Self::Interrupted => "interrupted",
            Self::PublishPending => "publishPending",
            Self::CleanupPending => "cleanupPending",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "paused" => Some(Self::Paused),
            "stopping" => Some(Self::Stopping),
            "completed" => Some(Self::Completed),
            "salvaged" => Some(Self::Salvaged),
            "partial" => Some(Self::Partial),
            "needsPassword" => Some(Self::NeedsPassword),
            "waitingResource" => Some(Self::WaitingResource),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "skipped" => Some(Self::Skipped),
            "interrupted" => Some(Self::Interrupted),
            "publishPending" => Some(Self::PublishPending),
            "cleanupPending" => Some(Self::CleanupPending),
            _ => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Salvaged
                | Self::Failed
                | Self::Cancelled
                | Self::Skipped
                | Self::Partial
        )
    }

    pub fn can_transition_to(&self, next: Self) -> bool {
        if *self == next {
            return true;
        }
        match self {
            Self::Queued => matches!(
                next,
                Self::Running | Self::Cancelled | Self::Paused | Self::WaitingResource
            ),
            Self::Running => matches!(
                next,
                Self::Completed
                    | Self::Salvaged
                    | Self::Partial
                    | Self::Failed
                    | Self::Cancelled
                    | Self::Paused
                    | Self::Stopping
                    | Self::NeedsPassword
                    | Self::WaitingResource
                    | Self::Interrupted
                    | Self::PublishPending
                    | Self::CleanupPending
            ),
            Self::Paused => matches!(next, Self::Running | Self::Cancelled),
            Self::Stopping => matches!(
                next,
                Self::Cancelled | Self::Failed | Self::Completed | Self::Salvaged | Self::Partial
            ),
            Self::NeedsPassword => matches!(
                next,
                Self::Queued | Self::Running | Self::Cancelled | Self::Failed
            ),
            Self::WaitingResource => matches!(next, Self::Queued | Self::Running | Self::Cancelled),
            Self::PublishPending => matches!(
                next,
                Self::Completed
                    | Self::Salvaged
                    | Self::Partial
                    | Self::Failed
                    | Self::CleanupPending
            ),
            Self::CleanupPending => matches!(
                next,
                Self::Completed | Self::Salvaged | Self::Partial | Self::Failed
            ),
            Self::Interrupted => matches!(
                next,
                Self::Queued | Self::Running | Self::Failed | Self::Cancelled
            ),
            Self::Completed
            | Self::Salvaged
            | Self::Partial
            | Self::Failed
            | Self::Cancelled
            | Self::Skipped => {
                // 终态允许显式重试转入 Queued
                matches!(next, Self::Queued)
            }
        }
    }

    pub fn transition_to(&mut self, next: Self) -> Result<(), DomainError> {
        if self.can_transition_to(next) {
            *self = next;
            Ok(())
        } else {
            Err(DomainError::InvalidStateTransition {
                from: self.as_str().to_string(),
                to: next.as_str().to_string(),
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionStage {
    Queued,
    Checking,
    Caching,
    Extracting,
    Nested,
    Planning,
    Copying,
    Verifying,
    Publishing,
    Cleanup,
    Finished,
    Waiting,
}

impl ExecutionStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Checking => "checking",
            Self::Caching => "caching",
            Self::Extracting => "extracting",
            Self::Nested => "nested",
            Self::Planning => "planning",
            Self::Copying => "copying",
            Self::Verifying => "verifying",
            Self::Publishing => "publishing",
            Self::Cleanup => "cleanup",
            Self::Finished => "finished",
            Self::Waiting => "waiting",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "queued" => Some(Self::Queued),
            "checking" => Some(Self::Checking),
            "caching" => Some(Self::Caching),
            "extracting" => Some(Self::Extracting),
            "nested" => Some(Self::Nested),
            "planning" => Some(Self::Planning),
            "copying" => Some(Self::Copying),
            "verifying" => Some(Self::Verifying),
            "publishing" => Some(Self::Publishing),
            "cleanup" => Some(Self::Cleanup),
            "finished" => Some(Self::Finished),
            "waiting" => Some(Self::Waiting),
            _ => None,
        }
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    #[test]
    fn explicit_retry_requeues_password_and_resource_waiters() {
        for mut status in [JobStatus::NeedsPassword, JobStatus::WaitingResource] {
            status.transition_to(JobStatus::Queued).unwrap();
            assert_eq!(status, JobStatus::Queued);
        }
        assert!(!JobStatus::Running.can_transition_to(JobStatus::Queued));
    }
}
