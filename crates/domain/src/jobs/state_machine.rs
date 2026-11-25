//! 任务状态机：状态全集、转移矩阵与执行阶段定义。

use crate::error::DomainError;
use serde::{Deserialize, Serialize};

/// 任务生命周期状态。
///
/// 转移矩阵由 [`JobStatus::can_transition_to`] 唯一定义；
/// 终态允许显式重试回到 `Queued`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    /// 已入队，等待调度。
    Queued,
    /// 解压进行中。
    Running,
    /// 用户暂停。
    Paused,
    /// 收到停止请求，等待协作式退出。
    Stopping,
    /// 成功完成（终态）。
    Completed,
    /// 经残卷还原/降级救援后完成（终态）。
    Salvaged,
    /// 部分条目成功（终态）。
    Partial,
    /// 等待用户提交密码候选。
    NeedsPassword,
    /// 等待资源预算释放（并发/磁盘）。
    WaitingResource,
    /// 失败（终态）。
    Failed,
    /// 用户取消（终态）。
    Cancelled,
    /// 调度前被跳过（终态）。
    Skipped,
    /// 进程中断后恢复的中间态。
    Interrupted,
    /// 解压完成，等待发布（校验/落盘确认）。
    PublishPending,
    /// 发布完成，等待工作区清理。
    CleanupPending,
}

impl JobStatus {
    /// 返回用于持久化与协议传输的稳定小驼峰短名。
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

    /// 从持久化/协议短名还原状态，未知值返回 `None`。
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

    /// 是否为终态。终态不再被协调器推进，只允许显式重试转入 `Queued`。
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

    /// 判断当前状态到 `next` 的跳变是否被转移矩阵允许。
    ///
    /// 相同状态视为合法（幂等）；关键规则：
    /// - `Running` 可进入全部执行中间态与终态；
    /// - 等待类状态（`NeedsPassword` / `WaitingResource` / `Interrupted`）
    ///   可回到 `Queued` 以支持显式重试；
    /// - 终态只能通过重试进入 `Queued`，不能直接复活为 `Running`。
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

    /// 执行状态转移；被矩阵拒绝时返回
    /// [`DomainError::InvalidStateTransition`] 且不改变自身。
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

/// 任务执行阶段：比状态更细的进度划分，仅用于 UI 展示与统计，不参与状态机校验。
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
    /// 返回用于持久化与协议传输的稳定短名。
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

    /// 从持久化/协议短名还原阶段，未知值返回 `None`。
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
