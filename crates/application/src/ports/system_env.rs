use crate::error::ApplicationError;
use openunpack_domain::DriveMedium;

/// 宿主系统环境与资源探测端口
pub trait SystemEnvironment: Send + Sync {
    /// 探测指定路径所在卷的介质类型（SSD / HDD / Unknown）
    fn probe_drive_medium(&self, path: &str) -> DriveMedium;

    /// 查询可用磁盘空间（字节）
    fn available_disk_space(&self, path: &str) -> Result<u64, ApplicationError>;

    /// 按真实工作区计算占用；不支持时不得把未知值伪装为零。
    fn workspace_size(&self, _path: &str) -> Result<u64, ApplicationError> {
        Err(ApplicationError::EnvironmentError(
            "当前适配器不支持工作区统计".into(),
        ))
    }

    /// 系统防休眠锁定守卫
    fn acquire_sleep_lock(&self, reason: &str) -> Box<dyn Send>;
}
