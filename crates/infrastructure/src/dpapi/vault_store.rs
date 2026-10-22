use super::vault::Vault;
use openunpack_application::{ApplicationError, PasswordSuccessEvent, VaultStore};
use openunpack_domain::{CandidateOrigin, DomainCandidate};
use openunpack_protocol::{ImportPreview, ImportResult, PasswordInput, PasswordView};
use std::collections::HashMap;
use std::path::Path;
use zeroize::Zeroizing;

/// Compatibility adapter over the production vault. Both stacks read and write the
/// same DPAPI format, atomic commits, import tokens, revisions and success ledger.
pub struct DpapiVaultStore {
    vault: Vault,
}

impl DpapiVaultStore {
    pub fn open(file_path: &Path) -> Result<Self, ApplicationError> {
        Ok(Self {
            vault: Vault::open(file_path).map_err(ApplicationError::VaultError)?,
        })
    }
}

impl VaultStore for DpapiVaultStore {
    fn list_entries(&self) -> Result<Vec<PasswordView>, ApplicationError> {
        self.vault.views().map_err(ApplicationError::VaultError)
    }

    fn add_entry(&self, label: &str, secret: &str) -> Result<PasswordView, ApplicationError> {
        self.upsert_entry(PasswordInput {
            id: None,
            name: label.to_string(),
            secret: Some(secret.to_string()),
            groups: vec![],
            notes: String::new(),
            enabled: true,
        })
    }

    fn delete_entry(&self, id: &str) -> Result<(), ApplicationError> {
        self.vault.delete(id).map_err(ApplicationError::VaultError)
    }

    fn load_decrypt_candidates(&self) -> Result<Vec<DomainCandidate>, ApplicationError> {
        let counts: HashMap<String, u64> = self
            .vault
            .views()
            .map_err(ApplicationError::VaultError)?
            .into_iter()
            .map(|view| (view.id, view.success_count))
            .collect();
        Ok(self
            .vault
            .candidates(None, None)
            .map_err(ApplicationError::VaultError)?
            .into_iter()
            .map(|candidate| DomainCandidate {
                id: candidate.id.clone(),
                revision: candidate.revision,
                origin: CandidateOrigin::VaultEntry,
                label: candidate.label.clone(),
                secret: candidate.secret.clone(),
                success_count: counts.get(&candidate.id).copied().unwrap_or_default(),
            })
            .collect())
    }

    fn record_success(&self, event: &PasswordSuccessEvent) -> Result<(), ApplicationError> {
        if event.origin != CandidateOrigin::VaultEntry {
            // Filename hints and explicit inputs must never mutate a vault record.
            return Ok(());
        }
        if event.event_id.is_empty() || event.candidate_id.is_empty() {
            return Err(ApplicationError::InvalidInput(
                "成功事件必须携带稳定的事件与候选 ID".into(),
            ));
        }
        self.vault
            .record_success(&openunpack_protocol::SuccessEvent {
                event_id: event.event_id.clone(),
                password_id: event.candidate_id.clone(),
                password_revision: event.candidate_revision,
                fingerprint: event.fingerprint.clone(),
            })
            .map_err(ApplicationError::VaultError)
    }

    fn import_passwords(&self, passwords: &[String]) -> Result<usize, ApplicationError> {
        if passwords.iter().any(|p| p.contains(['\r', '\n', '\0'])) {
            return Err(ApplicationError::InvalidInput(
                "列表中的每个密码必须是单行原文".into(),
            ));
        }
        let text = Zeroizing::new(passwords.join("\n"));
        let preview = self
            .vault
            .preview_import(&text, "txt", "")
            .map_err(ApplicationError::VaultError)?;
        if preview.error_count > 0 {
            return Err(ApplicationError::InvalidInput(
                "密码导入列表包含无效记录".into(),
            ));
        }
        self.vault
            .apply_import(&preview.token, &text, "txt", "")
            .map(|result| result.added)
            .map_err(ApplicationError::VaultError)
    }

    fn upsert_entry(&self, input: PasswordInput) -> Result<PasswordView, ApplicationError> {
        self.vault
            .upsert(input)
            .map_err(ApplicationError::VaultError)
    }
    fn groups(&self) -> Result<Vec<String>, ApplicationError> {
        self.vault.groups().map_err(ApplicationError::VaultError)
    }
    fn bulk_action(
        &self,
        ids: &[String],
        action: &str,
        groups: &[String],
    ) -> Result<(), ApplicationError> {
        self.vault
            .bulk_action(ids, action, groups)
            .map_err(ApplicationError::VaultError)
    }
    fn preview_import(
        &self,
        text: &str,
        format: &str,
        group: &str,
    ) -> Result<ImportPreview, ApplicationError> {
        self.vault
            .preview_import(text, format, group)
            .map_err(ApplicationError::VaultError)
    }
    fn apply_import(
        &self,
        token: &str,
        text: &str,
        format: &str,
        group: &str,
    ) -> Result<ImportResult, ApplicationError> {
        self.vault
            .apply_import(token, text, format, group)
            .map_err(ApplicationError::VaultError)
    }
    fn export_backup(&self, path: &Path, passphrase: &str) -> Result<(), ApplicationError> {
        self.vault
            .export_backup(path, passphrase)
            .map_err(ApplicationError::VaultError)
    }
    fn preview_restore_backup(
        &self,
        path: &Path,
        passphrase: &str,
    ) -> Result<ImportPreview, ApplicationError> {
        self.vault
            .preview_restore_backup(path, passphrase)
            .map_err(ApplicationError::VaultError)
    }
    fn confirm_restore_backup(
        &self,
        path: &Path,
        passphrase: &str,
        token: &str,
    ) -> Result<ImportResult, ApplicationError> {
        self.vault
            .confirm_restore_backup(path, passphrase, token)
            .map_err(ApplicationError::VaultError)
    }

    fn reveal_entry(&self, id: &str) -> Result<String, ApplicationError> {
        self.vault.reveal(id).map_err(ApplicationError::VaultError)
    }
}
