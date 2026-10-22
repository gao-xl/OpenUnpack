use crate::error::ApplicationError;
use openunpack_protocol::{WorkerEvent, WorkerRequest};
use std::sync::mpsc::{Receiver, Sender};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHandle {
    pub session_id: String,
    pub job_id: String,
}

/// Worker 进程生命周期与管道监督端口
pub trait WorkerSupervisor: Send + Sync {
    /// 是否存在可用于启动任务的 Worker 与引擎。未知适配器保守返回 false。
    fn is_available(&self) -> bool {
        false
    }

    /// 启动一个隔离的 Worker 会话，返回控制通道和事件接收通道
    fn start_session(
        &self,
        job_id: &str,
    ) -> Result<(SessionHandle, Sender<WorkerRequest>, Receiver<WorkerEvent>), ApplicationError>;

    /// 发送友好停止请求（进入 Stopping 状态，等待 Worker 确认）
    fn request_stop(&self, handle: &SessionHandle) -> Result<(), ApplicationError>;

    /// 停止排队操作、取消当前操作，并等待线程和子进程退出确认。
    /// 只有实际退出确认才可返回 Ok；失败时调用方必须保留工作区与会话。
    fn terminate(&self, handle: &SessionHandle) -> Result<(), ApplicationError>;
}
