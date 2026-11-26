//! 密码库（Vault）加密存储、条目管理与导入调度。

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use openunpack_protocol::{
    Candidate, ImportPreview, ImportResult, ImportRow, PasswordInput, PasswordView, SuccessEvent,
};

pub const VAULT_MAGIC: &[u8; 8] = b"OPENVLT1";
pub const VAULT_PORTABLE_DEFAULT_ITER: u32 = 100_000;
const ERR_MUTEX_POISONED: &str = "Vault mutex poisoned";
const ERR_VAULT_CORRUPT: &str = "Vault storage is corrupted or unreadable";
/// Stable machine-readable prefix so the UI can turn this into actionable copy.
pub const PORTABLE_VAULT_DISABLED_CODE: &str = "PORTABLE_VAULT_DISABLED";
/// Portable mode must be requested explicitly on Windows; a key file that merely
/// exists next to the vault may have been dropped there by another process.
const ERR_PORTABLE_NOT_ENABLED: &str = "PORTABLE_VAULT_DISABLED: portable vault detected but portable mode is disabled; set OPENUNPACK_PORTABLE_VAULT=1 to open it.";
const ERR_RECORD_NOT_FOUND: &str = "Password entry not found";
const ERR_INVALID_BACKUP: &str = "Invalid backup archive or incorrect passphrase";
const ERR_INVALID_TOKEN: &str = "Invalid or expired import preview token";
const ERR_BACKUP_TOO_LARGE: &str = "Backup payload exceeds maximum allowed size";
/// A key that could not be persisted must never be used: the next launch would
/// generate a different one and the whole vault would become undecryptable.
const ERR_VAULT_KEY_UNAVAILABLE: &str = "便携密钥文件无法创建或写入，已停止加密以避免密码库永久不可恢复；请检查密码库目录的写入权限与磁盘空间。";
const VAULT_KEY_FILE: &str = ".vault.key";
const VAULT_KEY_LEN: usize = 32;
/// Generated keys are 32 bytes; the cap only bounds a hostile or corrupted file.
const MAX_VAULT_KEY_FILE_SIZE: u64 = 4096;

const BACKUP_MIN_ITER: u32 = 100_000;
const BACKUP_MAX_ITER: u32 = 2_000_000;
const BACKUP_DEFAULT_ITER: u32 = 600_000;
const MAX_BACKUP_PASSPHRASE_BYTES: usize = 4096;
const MAX_BACKUP_PAYLOAD_SIZE: usize = 64 * 1024 * 1024; // 64 MiB
const MAX_VAULT_ITEMS: usize = 100_000;
// DPAPI adds an envelope to the serialized vault. Check its actual size before
// writing, and use the same bound when reopening, including old 64 MiB stores.
const MAX_VAULT_FILE_SIZE: usize = MAX_BACKUP_PAYLOAD_SIZE + 64 * 1024;
const MAX_BACKUP_FILE_SIZE: usize = MAX_BACKUP_PAYLOAD_SIZE * 2;

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn ct_bytes_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    let max_len = a.len().max(b.len());
    for i in 0..max_len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= (x ^ y) as usize;
    }
    diff == 0
}

#[derive(Clone, Serialize, Deserialize)]
struct VaultItem {
    id: String,
    name: String,
    secret: String,
    groups: Vec<String>,
    notes: String,
    enabled: bool,
    success_count: u64,
    last_success: Option<String>,
    revision: u64,
    #[serde(default)]
    deleted: bool,
    created_at: String,
    updated_at: String,
}

impl Drop for VaultItem {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VaultData {
    generation: u64,
    items: BTreeMap<String, VaultItem>,
    seen_events: BTreeSet<String>,
    fingerprint_order: BTreeMap<String, Vec<String>>,
}

#[derive(Serialize, Deserialize)]
struct BackupEnvelope {
    version: u32,
    kdf: String,
    iterations: u32,
    salt: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Serialize, Deserialize)]
struct BackupPayload {
    version: u32,
    items: Vec<BackupItem>,
}

#[derive(Serialize, Deserialize)]
struct BackupItem {
    name: String,
    secret: String,
    groups: Vec<String>,
    notes: String,
    enabled: bool,
    success_count: u64,
    last_success: Option<String>,
}

impl Drop for BackupItem {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

pub struct Vault {
    path: PathBuf,
    data: Mutex<VaultData>,
    writer: Mutex<()>,
    preview_key: [u8; 32],
}

impl Vault {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut preview_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut preview_key);
        if !path.exists() {
            let initial = VaultData::default();
            let vault = Vault {
                path: path.to_path_buf(),
                data: Mutex::new(initial),
                writer: Mutex::new(()),
                preview_key,
            };
            vault.persist_internal(&VaultData::default())?;
            return Ok(vault);
        }

        let encrypted_bytes = read_bounded(path, MAX_VAULT_FILE_SIZE)?;
        if encrypted_bytes.is_empty() {
            return Err(ERR_VAULT_CORRUPT.to_string());
        }

        let plaintext = Zeroizing::new(vault_decrypt(&encrypted_bytes, path)?);
        let loaded_data: VaultData =
            serde_json::from_slice(&plaintext).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;

        Ok(Vault {
            path: path.to_path_buf(),
            data: Mutex::new(loaded_data),
            writer: Mutex::new(()),
            preview_key,
        })
    }

    pub fn views(&self) -> Result<Vec<PasswordView>, String> {
        let (_, list) = self
            .views_if_changed(None)?
            .unwrap_or_else(|| (0, Vec::new()));
        Ok(list)
    }

    /// Returns `Some((generation, views))` when the in-memory vault generation differs
    /// from `cached_generation`, or `None` when the caller's projection is still fresh.
    pub fn views_if_changed(
        &self,
        cached_generation: Option<u64>,
    ) -> Result<Option<(u64, Vec<PasswordView>)>, String> {
        let guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        if cached_generation == Some(guard.generation) {
            return Ok(None);
        }
        let mut list = Vec::new();
        for item in guard.items.values() {
            if !item.deleted {
                list.push(PasswordView {
                    id: item.id.clone(),
                    name: item.name.clone(),
                    groups: item.groups.clone(),
                    notes: item.notes.clone(),
                    enabled: item.enabled,
                    success_count: item.success_count,
                    last_success: item.last_success.clone(),
                    revision: item.revision,
                });
            }
        }
        Ok(Some((guard.generation, list)))
    }

    pub fn groups(&self) -> Result<Vec<String>, String> {
        let guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        let mut group_set = BTreeSet::new();
        for item in guard.items.values() {
            if !item.deleted {
                for g in &item.groups {
                    let trimmed = g.trim();
                    if !trimmed.is_empty() {
                        group_set.insert(trimmed.to_string());
                    }
                }
            }
        }
        Ok(group_set.into_iter().collect())
    }

    pub fn upsert(&self, mut input: PasswordInput) -> Result<PasswordView, String> {
        validate_metadata(&input.name, &input.groups, &input.notes)?;
        if let Some(secret) = &input.secret {
            validate_secret(secret)?;
        }
        let _writer = self
            .writer
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        let now = now_rfc3339();
        let trimmed_name = input.name.trim().to_string();
        let cleaned_groups = clean_groups(&input.groups);
        let cleaned_notes = std::mem::take(&mut input.notes);

        let mut cloned = {
            let guard = self
                .data
                .lock()
                .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
            guard.clone()
        };

        let mut changed_id = None;
        let result_view = if let Some(ref id) = input.id {
            let item = cloned
                .items
                .get_mut(id)
                .ok_or_else(|| ERR_RECORD_NOT_FOUND.to_string())?;
            if item.deleted {
                return Err(ERR_RECORD_NOT_FOUND.to_string());
            }

            if let Some(mut new_secret) = input.secret.take() {
                if new_secret.is_empty() {
                    return Err("Password required".to_string());
                }
                if !ct_bytes_eq(new_secret.as_bytes(), item.secret.as_bytes()) {
                    item.secret.zeroize();
                    item.secret = new_secret;
                    item.revision = item.revision.saturating_add(1);
                    item.success_count = 0;
                    item.last_success = None;
                    changed_id = Some(id.clone());
                } else {
                    new_secret.zeroize();
                }
            }
            item.name = trimmed_name;
            item.groups = cleaned_groups;
            item.notes = cleaned_notes;
            item.enabled = input.enabled;
            item.updated_at = now;

            PasswordView {
                id: item.id.clone(),
                name: item.name.clone(),
                groups: item.groups.clone(),
                notes: item.notes.clone(),
                enabled: item.enabled,
                success_count: item.success_count,
                last_success: item.last_success.clone(),
                revision: item.revision,
            }
        } else {
            validate_new_item_capacity(active_item_count(&cloned))?;
            let secret_val = input
                .secret
                .take()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "Password required".to_string())?;
            let new_id = Uuid::new_v4().to_string();
            let new_item = VaultItem {
                id: new_id.clone(),
                name: trimmed_name,
                secret: secret_val,
                groups: cleaned_groups,
                notes: cleaned_notes,
                enabled: input.enabled,
                success_count: 0,
                last_success: None,
                revision: 1,
                deleted: false,
                created_at: now.clone(),
                updated_at: now,
            };

            let view = PasswordView {
                id: new_item.id.clone(),
                name: new_item.name.clone(),
                groups: new_item.groups.clone(),
                notes: new_item.notes.clone(),
                enabled: new_item.enabled,
                success_count: new_item.success_count,
                last_success: new_item.last_success.clone(),
                revision: new_item.revision,
            };
            cloned.items.insert(new_id, new_item);
            view
        };

        if let Some(id) = changed_id {
            for ids in cloned.fingerprint_order.values_mut() {
                ids.retain(|entry_id| entry_id != &id);
            }
        }
        cloned.generation = cloned.generation.wrapping_add(1);
        self.persist_internal(&cloned)?;

        let mut guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        *guard = cloned;

        Ok(result_view)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        let mut cloned = {
            let guard = self
                .data
                .lock()
                .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
            guard.clone()
        };

        let item = cloned
            .items
            .get_mut(id)
            .ok_or_else(|| ERR_RECORD_NOT_FOUND.to_string())?;
        if item.deleted {
            return Err(ERR_RECORD_NOT_FOUND.to_string());
        }

        item.deleted = true;
        item.secret.zeroize();
        item.updated_at = now_rfc3339();
        cloned.generation = cloned.generation.wrapping_add(1);

        for ids in cloned.fingerprint_order.values_mut() {
            ids.retain(|entry| entry != id);
        }

        self.persist_internal(&cloned)?;

        let mut guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        *guard = cloned;

        Ok(())
    }

    pub fn candidates(
        &self,
        group_filter: Option<&str>,
        fingerprint: Option<&str>,
    ) -> Result<Vec<Candidate>, String> {
        let guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;

        let fp_priority: Vec<String> = if let Some(fp) = fingerprint {
            guard.fingerprint_order.get(fp).cloned().unwrap_or_default()
        } else {
            Vec::new()
        };
        let fp_rank: BTreeMap<&str, usize> = fp_priority
            .iter()
            .enumerate()
            .map(|(rank, id)| (id.as_str(), rank))
            .collect();

        let mut active_items: Vec<&VaultItem> = guard
            .items
            .values()
            .filter(|it| !it.deleted && it.enabled)
            .filter(|it| {
                if let Some(target_group) = group_filter {
                    let tg = target_group.trim();
                    if tg.is_empty() {
                        true
                    } else {
                        it.groups.iter().any(|g| g.trim() == tg)
                    }
                } else {
                    true
                }
            })
            .collect();

        active_items.sort_by(|a, b| {
            let a_fp_pos = fp_rank.get(a.id.as_str()).copied();
            let b_fp_pos = fp_rank.get(b.id.as_str()).copied();

            match (a_fp_pos, b_fp_pos) {
                (Some(pa), Some(pb)) => pa.cmp(&pb),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b
                    .success_count
                    .cmp(&a.success_count)
                    .then_with(|| b.last_success.cmp(&a.last_success))
                    .then_with(|| a.name.cmp(&b.name))
                    .then_with(|| a.id.cmp(&b.id)),
            }
        });

        let candidates = active_items
            .into_iter()
            .map(|it| Candidate {
                id: it.id.clone(),
                revision: it.revision,
                label: if it.name.is_empty() {
                    "Untitled".to_string()
                } else {
                    it.name.clone()
                },
                secret: it.secret.clone(),
            })
            .collect();

        Ok(candidates)
    }

    pub fn record_success(&self, event: &SuccessEvent) -> Result<(), String> {
        let _writer = self
            .writer
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        let mut cloned = {
            let guard = self
                .data
                .lock()
                .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
            guard.clone()
        };

        if cloned.seen_events.contains(&event.event_id) {
            return Ok(());
        }

        let lookup_id = event
            .password_id
            .split('#')
            .next()
            .unwrap_or(&event.password_id);

        if let Some(item) = cloned.items.get_mut(lookup_id)
            && !item.deleted
            && item.revision == event.password_revision
        {
            item.success_count = item.success_count.saturating_add(1);
            item.last_success = Some(now_rfc3339());
            item.updated_at = now_rfc3339();

            if let Some(ref fp) = event.fingerprint {
                let list = cloned.fingerprint_order.entry(fp.clone()).or_default();
                list.retain(|id| id != lookup_id);
                list.insert(0, lookup_id.to_string());
            }
        }

        cloned.seen_events.insert(event.event_id.clone());
        while cloned.seen_events.len() > 10_000 {
            if let Some(first) = cloned.seen_events.iter().next().cloned() {
                cloned.seen_events.remove(&first);
            } else {
                break;
            }
        }
        cloned.generation = cloned.generation.wrapping_add(1);

        self.persist_internal(&cloned)?;

        let mut guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        *guard = cloned;

        Ok(())
    }

    pub fn preview_import(
        &self,
        text: &str,
        format: &str,
        group: &str,
    ) -> Result<ImportPreview, String> {
        validate_import_input(text, format, group)?;
        let parsed_records = parse_import_text(text, format, group);
        let guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;

        let secret_hash = |s: &str| -> [u8; 32] {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(s.as_bytes());
            hasher.finalize().into()
        };

        let mut existing_secret_hashes = BTreeSet::new();
        for item in guard.items.values() {
            if !item.deleted {
                existing_secret_hashes.insert(secret_hash(&item.secret));
            }
        }

        let mut seen_in_batch_hashes = BTreeSet::new();
        let item_count = active_item_count(&guard);
        let mut new_count = 0;
        let mut duplicate_count = 0;
        let mut error_count = 0;
        let mut rows = Vec::new();

        for rec in parsed_records {
            let row_name = if rec.name.is_empty() {
                format!("Row {}", rec.line)
            } else {
                rec.name
            };

            let rec_hash = secret_hash(&rec.secret);
            if rec.error {
                error_count += 1;
                rows.push(ImportRow {
                    line: rec.line,
                    name: row_name,
                    group: rec.groups.first().cloned().unwrap_or_default(),
                    status: "error".to_string(),
                    message: rec.message,
                });
            } else if existing_secret_hashes.contains(&rec_hash)
                || seen_in_batch_hashes.contains(&rec_hash)
            {
                duplicate_count += 1;
                rows.push(ImportRow {
                    line: rec.line,
                    name: row_name,
                    group: rec.groups.first().cloned().unwrap_or_default(),
                    status: "duplicate".to_string(),
                    message: "Duplicate secret".to_string(),
                });
            } else {
                if validate_new_item_capacity(item_count + new_count).is_err() {
                    error_count += 1;
                    rows.push(ImportRow {
                        line: rec.line,
                        name: row_name,
                        group: rec.groups.first().cloned().unwrap_or_default(),
                        status: "error".into(),
                        message: vault_capacity_message(),
                    });
                    continue;
                }
                new_count += 1;
                seen_in_batch_hashes.insert(rec_hash);
                rows.push(ImportRow {
                    line: rec.line,
                    name: row_name,
                    group: rec.groups.first().cloned().unwrap_or_default(),
                    status: "new".to_string(),
                    message: "Ready to import".to_string(),
                });
            }
        }

        let token = compute_preview_token(text, format, group, guard.generation, &self.preview_key);

        Ok(ImportPreview {
            token,
            new_count,
            duplicate_count,
            error_count,
            rows,
        })
    }

    pub fn apply_import(
        &self,
        token: &str,
        text: &str,
        format: &str,
        group: &str,
    ) -> Result<ImportResult, String> {
        validate_import_input(text, format, group)?;
        let _writer = self
            .writer
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        let mut cloned = {
            let guard = self
                .data
                .lock()
                .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
            let expected_token =
                compute_preview_token(text, format, group, guard.generation, &self.preview_key);
            if !token_equal(token, &expected_token) {
                return Err(ERR_INVALID_TOKEN.to_string());
            }
            guard.clone()
        };

        let parsed_records = parse_import_text(text, format, group);
        if parsed_records.iter().any(|record| record.error) {
            return Err("导入内容存在无效行，请先修正预览中的错误。".into());
        }
        let mut added = 0;
        let mut duplicates = 0;
        let now = now_rfc3339();
        let mut item_count = active_item_count(&cloned);

        for rec in parsed_records {
            let mut matched_id = None;
            for item in cloned.items.values() {
                if !item.deleted && item.secret == rec.secret {
                    matched_id = Some(item.id.clone());
                    break;
                }
            }

            if let Some(id) = matched_id {
                duplicates += 1;
                if let Some(item) = cloned.items.get_mut(&id) {
                    let mut grp_set: BTreeSet<String> = item.groups.iter().cloned().collect();
                    for g in rec.groups {
                        grp_set.insert(g);
                    }
                    let merged_groups: Vec<String> = grp_set.into_iter().collect();
                    validate_metadata(&item.name, &merged_groups, &item.notes)?;
                    item.groups = merged_groups;
                    item.updated_at = now.clone();
                }
            } else {
                validate_new_item_capacity(item_count)?;
                item_count += 1;
                added += 1;
                let new_id = Uuid::new_v4().to_string();
                let item = VaultItem {
                    id: new_id.clone(),
                    name: rec.name,
                    secret: rec.secret,
                    groups: rec.groups,
                    notes: rec.notes,
                    enabled: true,
                    success_count: 0,
                    last_success: None,
                    revision: 1,
                    deleted: false,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                };
                cloned.items.insert(new_id, item);
            }
        }

        cloned.generation = cloned.generation.wrapping_add(1);
        self.persist_internal(&cloned)?;

        let mut guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        *guard = cloned;

        Ok(ImportResult { added, duplicates })
    }

    /// Apply a selected batch in one encrypted-file commit, with no half-applied selection.
    pub fn bulk_action(
        &self,
        ids: &[String],
        action: &str,
        groups: &[String],
    ) -> Result<(), String> {
        if ids.is_empty()
            || ids.len() > MAX_VAULT_ITEMS
            || !matches!(action, "enable" | "disable" | "delete" | "group")
        {
            return Err("批量密码操作无效。".into());
        }
        validate_metadata("", groups, "")?;
        let _writer = self.writer.lock().map_err(|_| ERR_MUTEX_POISONED)?;
        let mut data = self.data.lock().map_err(|_| ERR_MUTEX_POISONED)?.clone();
        for id in ids {
            if data.items.get(id).is_none_or(|i| i.deleted) {
                return Err(ERR_RECORD_NOT_FOUND.into());
            }
        }
        let clean = clean_groups(groups);
        let timestamp = now_rfc3339();
        for id in ids {
            let item = data.items.get_mut(id).ok_or(ERR_RECORD_NOT_FOUND)?;
            match action {
                "enable" => item.enabled = true,
                "disable" => item.enabled = false,
                "delete" => {
                    item.deleted = true;
                    item.secret.zeroize();
                }
                "group" => item.groups = clean.clone(),
                _ => return Err("批量密码操作无效。".into()),
            };
            item.updated_at = timestamp.clone();
        }
        if action == "delete" {
            for entries in data.fingerprint_order.values_mut() {
                entries.retain(|i| !ids.contains(i));
            }
        }
        data.generation = data.generation.wrapping_add(1);
        self.persist_internal(&data)?;
        *self.data.lock().map_err(|_| ERR_MUTEX_POISONED)? = data;
        Ok(())
    }

    pub fn reveal(&self, id: &str) -> Result<Zeroizing<String>, String> {
        let guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        let item = guard
            .items
            .get(id)
            .ok_or_else(|| ERR_RECORD_NOT_FOUND.to_string())?;
        if item.deleted {
            return Err(ERR_RECORD_NOT_FOUND.to_string());
        }
        Ok(Zeroizing::new(item.secret.clone()))
    }

    fn persist_internal(&self, data: &VaultData) -> Result<(), String> {
        let raw =
            Zeroizing::new(serde_json::to_vec(data).map_err(|_| ERR_VAULT_CORRUPT.to_string())?);
        if raw.len() > MAX_BACKUP_PAYLOAD_SIZE {
            return Err(ERR_VAULT_CORRUPT.to_string());
        }
        let encrypted = vault_encrypt(&raw, &self.path)?;
        if encrypted.len() > MAX_VAULT_FILE_SIZE {
            return Err(ERR_VAULT_CORRUPT.into());
        }
        atomic_write_file(&self.path, &encrypted)
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        self.preview_key.zeroize();
    }
}

mod backup;
mod crypto;
mod import_export;
mod validation;

use crypto::*;
pub use crypto::{portable_decrypt, portable_encrypt};
pub use import_export::derive_candidate_mutations;
use import_export::*;
use validation::*;

#[cfg(test)]
mod tests;
