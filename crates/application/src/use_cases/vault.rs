use crate::error::ApplicationError;
use crate::ports::{PasswordSuccessEvent, VaultStore};
use openunpack_domain::DomainCandidate;
use openunpack_protocol::{ImportPreview, ImportResult, PasswordInput, PasswordView};
use std::path::Path;
use std::sync::Arc;

pub struct VaultUseCases {
    store: Arc<dyn VaultStore>,
}

impl VaultUseCases {
    pub fn new(store: Arc<dyn VaultStore>) -> Self {
        Self { store }
    }

    pub fn list_passwords(&self) -> Result<Vec<PasswordView>, ApplicationError> {
        self.store.list_entries()
    }

    pub fn add_password(
        &self,
        label: &str,
        secret: &str,
    ) -> Result<PasswordView, ApplicationError> {
        if secret.is_empty() {
            return Err(ApplicationError::InvalidInput("密码不能为空".into()));
        }
        self.store.add_entry(label, secret)
    }

    pub fn delete_password(&self, id: &str) -> Result<(), ApplicationError> {
        self.store.delete_entry(id)
    }

    pub fn reveal_password(&self, id: &str) -> Result<String, ApplicationError> {
        self.store.reveal_entry(id)
    }

    pub fn import_passwords(&self, list: &[String]) -> Result<usize, ApplicationError> {
        self.store.import_passwords(list)
    }

    /// 准备用于尝试解密的候选集（按命中频次排序并去重）
    pub fn prepare_candidates_for_job(
        &self,
        hint_candidates: Vec<DomainCandidate>,
    ) -> Result<Vec<DomainCandidate>, ApplicationError> {
        let mut vault_candidates = self.store.load_decrypt_candidates()?;
        vault_candidates.extend(hint_candidates);
        Ok(openunpack_domain::apply_candidate_mutations(
            vault_candidates,
        ))
    }

    /// 记录某候选密码命中解密成功
    pub fn mark_candidate_success(
        &self,
        event: &PasswordSuccessEvent,
    ) -> Result<(), ApplicationError> {
        self.store.record_success(event)
    }

    pub fn upsert_password(&self, input: PasswordInput) -> Result<PasswordView, ApplicationError> {
        self.store.upsert_entry(input)
    }
    pub fn groups(&self) -> Result<Vec<String>, ApplicationError> {
        self.store.groups()
    }
    pub fn bulk_action(
        &self,
        ids: &[String],
        action: &str,
        groups: &[String],
    ) -> Result<(), ApplicationError> {
        self.store.bulk_action(ids, action, groups)
    }
    pub fn preview_import(
        &self,
        text: &str,
        format: &str,
        group: &str,
    ) -> Result<ImportPreview, ApplicationError> {
        self.store.preview_import(text, format, group)
    }
    pub fn apply_import(
        &self,
        token: &str,
        text: &str,
        format: &str,
        group: &str,
    ) -> Result<ImportResult, ApplicationError> {
        self.store.apply_import(token, text, format, group)
    }
    pub fn export_backup(&self, path: &Path, passphrase: &str) -> Result<(), ApplicationError> {
        self.store.export_backup(path, passphrase)
    }
    pub fn preview_restore_backup(
        &self,
        path: &Path,
        passphrase: &str,
    ) -> Result<ImportPreview, ApplicationError> {
        self.store.preview_restore_backup(path, passphrase)
    }
    pub fn confirm_restore_backup(
        &self,
        path: &Path,
        passphrase: &str,
        token: &str,
    ) -> Result<ImportResult, ApplicationError> {
        self.store.confirm_restore_backup(path, passphrase, token)
    }
}
