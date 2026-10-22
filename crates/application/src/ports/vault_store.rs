use crate::error::ApplicationError;
use openunpack_domain::{CandidateOrigin, DomainCandidate};
use openunpack_protocol::{ImportPreview, ImportResult, PasswordInput, PasswordView};
use std::path::Path;

/// Stable delivery event. The same event is safe to replay after an outbox retry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PasswordSuccessEvent {
    pub event_id: String,
    pub candidate_id: String,
    pub candidate_revision: u64,
    pub origin: CandidateOrigin,
    pub fingerprint: Option<String>,
}

fn unsupported() -> ApplicationError {
    ApplicationError::VaultError("此密码库适配器不支持该操作".into())
}

/// 密码存储库端口（DPAPI 加密存储契约）
pub trait VaultStore: Send + Sync {
    /// 列出所有密码元数据视图（不包含明文）
    fn list_entries(&self) -> Result<Vec<PasswordView>, ApplicationError>;

    /// 添加一条新密码
    fn add_entry(&self, label: &str, secret: &str) -> Result<PasswordView, ApplicationError>;

    /// 删除指定密码
    fn delete_entry(&self, id: &str) -> Result<(), ApplicationError>;

    /// 获取所有可用于尝试解密的候选密码（drop 时清零内存）
    fn load_decrypt_candidates(&self) -> Result<Vec<DomainCandidate>, ApplicationError>;

    /// 解压成功后记录命中，按事件 ID 幂等并验证版本与来源
    fn record_success(&self, event: &PasswordSuccessEvent) -> Result<(), ApplicationError>;

    /// 批量导入文本密码列表
    fn import_passwords(&self, passwords: &[String]) -> Result<usize, ApplicationError>;

    fn upsert_entry(&self, _input: PasswordInput) -> Result<PasswordView, ApplicationError> {
        Err(unsupported())
    }
    fn groups(&self) -> Result<Vec<String>, ApplicationError> {
        Err(unsupported())
    }
    fn bulk_action(
        &self,
        _ids: &[String],
        _action: &str,
        _groups: &[String],
    ) -> Result<(), ApplicationError> {
        Err(unsupported())
    }
    fn preview_import(
        &self,
        _text: &str,
        _format: &str,
        _group: &str,
    ) -> Result<ImportPreview, ApplicationError> {
        Err(unsupported())
    }
    fn apply_import(
        &self,
        _token: &str,
        _text: &str,
        _format: &str,
        _group: &str,
    ) -> Result<ImportResult, ApplicationError> {
        Err(unsupported())
    }
    fn export_backup(&self, _path: &Path, _passphrase: &str) -> Result<(), ApplicationError> {
        Err(unsupported())
    }
    fn preview_restore_backup(
        &self,
        _path: &Path,
        _passphrase: &str,
    ) -> Result<ImportPreview, ApplicationError> {
        Err(unsupported())
    }
    fn confirm_restore_backup(
        &self,
        _path: &Path,
        _passphrase: &str,
        _token: &str,
    ) -> Result<ImportResult, ApplicationError> {
        Err(unsupported())
    }

    /// 查看指定密码条目的明文（按需解密）
    fn reveal_entry(&self, _id: &str) -> Result<String, ApplicationError> {
        Err(unsupported())
    }
}
