use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::Utc;
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
        let guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
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
        Ok(list)
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
        let now = Utc::now().to_rfc3339();
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
                if new_secret != item.secret {
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
        item.updated_at = Utc::now().to_rfc3339();
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
            item.last_success = Some(Utc::now().to_rfc3339());
            item.updated_at = Utc::now().to_rfc3339();

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
        let now = Utc::now().to_rfc3339();
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

    pub fn export_backup(&self, destination: &Path, passphrase: &str) -> Result<(), String> {
        validate_backup_passphrase(passphrase)?;
        if destination == self.path
            || (destination.exists()
                && fs::canonicalize(destination).ok() == fs::canonicalize(&self.path).ok())
        {
            return Err("Backup cannot overwrite the active vault".to_string());
        }
        let payload = {
            let guard = self
                .data
                .lock()
                .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
            if active_item_count(&guard) > MAX_VAULT_ITEMS {
                return Err(vault_capacity_message());
            }
            // Old versions could persist metadata that current restore rejects.
            // Keep such stores openable for explicit repair, but never export an
            // otherwise authenticated backup whose contents cannot be restored.
            for item in guard.items.values().filter(|item| !item.deleted) {
                validate_secret(&item.secret)
                    .and_then(|_| validate_metadata(&item.name, &item.groups, &item.notes))
                    .map_err(|_| {
                        "密码库存在无效条目，请先修正其密码、名称、分组或备注后再导出备份。"
                            .to_string()
                    })?;
            }
            let mut backup_items = Vec::new();
            for item in guard.items.values() {
                if !item.deleted {
                    backup_items.push(BackupItem {
                        name: item.name.clone(),
                        secret: item.secret.clone(),
                        groups: item.groups.clone(),
                        notes: item.notes.clone(),
                        enabled: item.enabled,
                        success_count: item.success_count,
                        last_success: item.last_success.clone(),
                    });
                }
            }
            BackupPayload {
                version: 1,
                items: backup_items,
            }
        };

        let raw_payload = Zeroizing::new(
            serde_json::to_vec(&payload).map_err(|_| ERR_VAULT_CORRUPT.to_string())?,
        );
        if raw_payload.len() > MAX_BACKUP_PAYLOAD_SIZE {
            return Err(ERR_BACKUP_TOO_LARGE.to_string());
        }
        validate_empty_restore_size(&payload)?;

        let mut salt = [0u8; 16];
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut salt);
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        let mut key = Zeroizing::new([0u8; 32]);
        pbkdf2::pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, BACKUP_DEFAULT_ITER, &mut *key);

        let cipher = Aes256Gcm::new_from_slice(&*key).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, raw_payload.as_slice())
            .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;

        key.zeroize();

        let envelope = BackupEnvelope {
            version: 1,
            kdf: "PBKDF2-SHA256".to_string(),
            iterations: BACKUP_DEFAULT_ITER,
            salt: BASE64.encode(salt),
            nonce: BASE64.encode(nonce_bytes),
            ciphertext: BASE64.encode(ciphertext),
        };

        let envelope_json =
            serde_json::to_vec_pretty(&envelope).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
        if envelope_json.len() > MAX_BACKUP_FILE_SIZE {
            return Err(ERR_BACKUP_TOO_LARGE.into());
        }
        atomic_write_file(destination, &envelope_json)
    }

    fn decode_backup(path: &Path, passphrase: &str) -> Result<(BackupPayload, String), String> {
        validate_backup_passphrase(passphrase).map_err(|_| ERR_INVALID_BACKUP)?;
        let envelope_bytes =
            read_bounded(path, MAX_BACKUP_FILE_SIZE).map_err(|_| ERR_INVALID_BACKUP.to_string())?;

        let envelope: BackupEnvelope =
            serde_json::from_slice(&envelope_bytes).map_err(|_| ERR_INVALID_BACKUP.to_string())?;

        if envelope.iterations < BACKUP_MIN_ITER || envelope.iterations > BACKUP_MAX_ITER {
            return Err(ERR_INVALID_BACKUP.to_string());
        }
        if envelope.kdf != "PBKDF2-SHA256" || envelope.version != 1 {
            return Err(ERR_INVALID_BACKUP.to_string());
        }

        let salt = BASE64
            .decode(&envelope.salt)
            .map_err(|_| ERR_INVALID_BACKUP.to_string())?;
        let nonce_bytes = BASE64
            .decode(&envelope.nonce)
            .map_err(|_| ERR_INVALID_BACKUP.to_string())?;
        let ciphertext = BASE64
            .decode(&envelope.ciphertext)
            .map_err(|_| ERR_INVALID_BACKUP.to_string())?;

        if nonce_bytes.len() != 12
            || salt.len() != 16
            || ciphertext.len() > MAX_BACKUP_PAYLOAD_SIZE + 16
        {
            return Err(ERR_INVALID_BACKUP.to_string());
        }

        let mut key = Zeroizing::new([0u8; 32]);
        pbkdf2::pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, envelope.iterations, &mut *key);

        let cipher =
            Aes256Gcm::new_from_slice(&*key).map_err(|_| ERR_INVALID_BACKUP.to_string())?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let decrypted = Zeroizing::new(
            cipher
                .decrypt(nonce, ciphertext.as_slice())
                .map_err(|_| ERR_INVALID_BACKUP.to_string())?,
        );

        key.zeroize();

        if decrypted.len() > MAX_BACKUP_PAYLOAD_SIZE {
            return Err(ERR_BACKUP_TOO_LARGE.to_string());
        }

        let backup_payload: BackupPayload =
            serde_json::from_slice(&decrypted).map_err(|_| ERR_INVALID_BACKUP.to_string())?;
        if backup_payload.version != 1 {
            return Err(ERR_INVALID_BACKUP.to_string());
        }
        if backup_payload.items.len() > MAX_VAULT_ITEMS {
            return Err(ERR_BACKUP_TOO_LARGE.into());
        }
        for item in &backup_payload.items {
            validate_secret(&item.secret).map_err(|_| ERR_INVALID_BACKUP)?;
            validate_metadata(&item.name, &item.groups, &item.notes)
                .map_err(|_| ERR_INVALID_BACKUP)?;
        }
        validate_empty_restore_size(&backup_payload)?;
        Ok((backup_payload, hex::encode(Sha256::digest(&envelope_bytes))))
    }

    pub fn preview_restore_backup(
        &self,
        path: &Path,
        passphrase: &str,
    ) -> Result<ImportPreview, String> {
        let (payload, hash) = Self::decode_backup(path, passphrase)?;
        let data = self.data.lock().map_err(|_| ERR_MUTEX_POISONED)?;
        let mut seen: BTreeSet<&str> = data
            .items
            .values()
            .filter(|i| !i.deleted)
            .map(|i| i.secret.as_str())
            .collect();
        let mut new_count = 0;
        let mut duplicate_count = 0;
        let mut error_count = 0;
        let item_count = active_item_count(&data);
        let mut rows = vec![];
        for (index, item) in payload.items.iter().enumerate() {
            let duplicate = !seen.insert(item.secret.as_str());
            if duplicate {
                duplicate_count += 1
            } else {
                if validate_new_item_capacity(item_count + new_count).is_err() {
                    seen.remove(item.secret.as_str());
                    error_count += 1;
                    rows.push(ImportRow {
                        line: index + 1,
                        name: item.name.clone(),
                        group: clean_groups(&item.groups).join("、"),
                        status: "error".into(),
                        message: vault_capacity_message(),
                    });
                    continue;
                }
                new_count += 1
            }
            rows.push(ImportRow {
                line: index + 1,
                name: item.name.clone(),
                group: clean_groups(&item.groups).join("、"),
                status: if duplicate { "duplicate" } else { "new" }.into(),
                message: if duplicate {
                    "合并分组，保留现有名称、备注与启用状态。"
                } else {
                    "新增密码记录。"
                }
                .into(),
            });
        }
        Ok(ImportPreview {
            token: compute_preview_token(&hash, "backup", "", data.generation, &self.preview_key),
            new_count,
            duplicate_count,
            error_count,
            rows,
        })
    }

    pub fn confirm_restore_backup(
        &self,
        path: &Path,
        passphrase: &str,
        token: &str,
    ) -> Result<ImportResult, String> {
        let (payload, hash) = Self::decode_backup(path, passphrase)?;
        self.merge_backup(payload, Some((&hash, token)))
    }

    pub fn restore_backup(&self, path: &Path, passphrase: &str) -> Result<ImportResult, String> {
        let (payload, _) = Self::decode_backup(path, passphrase)?;
        self.merge_backup(payload, None)
    }

    fn merge_backup(
        &self,
        backup_payload: BackupPayload,
        confirmation: Option<(&str, &str)>,
    ) -> Result<ImportResult, String> {
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
        if let Some((hash, token)) = confirmation {
            let expected =
                compute_preview_token(hash, "backup", "", cloned.generation, &self.preview_key);
            if !token_equal(&expected, token) {
                return Err(ERR_INVALID_TOKEN.into());
            }
        }

        let mut added = 0;
        let mut duplicates = 0;
        let now = Utc::now().to_rfc3339();
        let mut item_count = active_item_count(&cloned);

        for mut b_item in backup_payload.items {
            let mut matched_id = None;
            for item in cloned.items.values() {
                if !item.deleted && item.secret == b_item.secret {
                    matched_id = Some(item.id.clone());
                    break;
                }
            }

            if let Some(id) = matched_id {
                duplicates += 1;
                if let Some(item) = cloned.items.get_mut(&id) {
                    let mut grp_set: BTreeSet<String> = item.groups.iter().cloned().collect();
                    for g in &b_item.groups {
                        let trimmed = g.trim();
                        if !trimmed.is_empty() {
                            grp_set.insert(trimmed.to_string());
                        }
                    }
                    let merged_groups: Vec<String> = grp_set.into_iter().collect();
                    validate_metadata(&item.name, &merged_groups, &item.notes)?;
                    item.groups = merged_groups;
                    if b_item.success_count > item.success_count {
                        item.success_count = b_item.success_count;
                        item.last_success = b_item.last_success.clone();
                    }
                    item.updated_at = now.clone();
                }
            } else {
                validate_new_item_capacity(item_count)?;
                item_count += 1;
                added += 1;
                let new_id = Uuid::new_v4().to_string();
                let item = VaultItem {
                    id: new_id.clone(),
                    name: b_item.name.trim().to_string(),
                    secret: std::mem::take(&mut b_item.secret),
                    groups: clean_groups(&b_item.groups),
                    notes: std::mem::take(&mut b_item.notes),
                    enabled: b_item.enabled,
                    success_count: b_item.success_count,
                    last_success: b_item.last_success.take(),
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
        let timestamp = Utc::now().to_rfc3339();
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

    pub fn reveal(&self, id: &str) -> Result<String, String> {
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
        Ok(item.secret.clone())
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

fn clean_groups(groups: &[String]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for g in groups {
        let trimmed = g.trim();
        if !trimmed.is_empty() {
            set.insert(trimmed.to_string());
        }
    }
    set.into_iter().collect()
}

fn active_item_count(data: &VaultData) -> usize {
    data.items.values().filter(|item| !item.deleted).count()
}

fn vault_capacity_message() -> String {
    format!("密码库最多支持 {MAX_VAULT_ITEMS} 条有效记录，请先删除不需要的记录。")
}

fn validate_new_item_capacity(current: usize) -> Result<(), String> {
    if current >= MAX_VAULT_ITEMS {
        Err(vault_capacity_message())
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct SerializedSize(usize);

impl Write for SerializedSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("Serialized vault size overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn serialized_size(value: &impl Serialize) -> Result<usize, String> {
    let mut size = SerializedSize::default();
    serde_json::to_writer(&mut size, value).map_err(|_| ERR_BACKUP_TOO_LARGE.to_string())?;
    Ok(size.0)
}

// A portable backup has fewer fields than local storage. Bound the size of an
// empty-vault restore as well, so a successful export cannot exceed the local
// reader after UUIDs and timestamps have been added. Duplicates/normalization
// only reduce this upper bound; it needs no encryption or second KDF.
fn validate_empty_restore_size(payload: &BackupPayload) -> Result<(), String> {
    let mut size = serialized_size(&VaultData::default())?;
    const RESTORED_ID: &str = "00000000-0000-4000-8000-000000000000";
    const LONGEST_TIMESTAMP: &str = "2000-01-01T00:00:00.000000000+00:00";
    for (index, item) in payload.items.iter().enumerate() {
        let restored = VaultItem {
            id: RESTORED_ID.into(),
            name: item.name.clone(),
            secret: item.secret.clone(),
            groups: item.groups.clone(),
            notes: item.notes.clone(),
            enabled: item.enabled,
            success_count: item.success_count,
            last_success: item.last_success.clone(),
            revision: 1,
            deleted: false,
            created_at: LONGEST_TIMESTAMP.into(),
            updated_at: LONGEST_TIMESTAMP.into(),
        };
        size += serialized_size(&RESTORED_ID)? + 1 + serialized_size(&restored)?;
        if index != 0 {
            size += 1; // comma between map entries
        }
        if size > MAX_BACKUP_PAYLOAD_SIZE {
            return Err(ERR_BACKUP_TOO_LARGE.into());
        }
    }
    Ok(())
}

fn validate_import_format(format: &str) -> Result<(), String> {
    let lower = format.trim().to_ascii_lowercase();
    if lower == "txt" || lower == "tsv" || lower == "csv" || lower.starts_with("csv:") {
        Ok(())
    } else {
        Err("Unsupported import format".to_string())
    }
}

fn validate_import_input(text: &str, format: &str, group: &str) -> Result<(), String> {
    validate_import_format(format)?;
    if text.len() > 8 * 1024 * 1024
        || text.lines().take(MAX_VAULT_ITEMS + 1).count() > MAX_VAULT_ITEMS
    {
        return Err("导入内容超过 8 MiB 或 100000 行限制。".into());
    }
    validate_metadata("", &[group.into()], "")
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
    if file
        .metadata()
        .map_err(|_| ERR_VAULT_CORRUPT.to_string())?
        .len()
        > limit as u64
    {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    let mut content = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut content)
        .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
    if content.len() > limit {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    Ok(content)
}

fn compute_preview_token(
    text: &str,
    format: &str,
    group: &str,
    generation: u64,
    key: &[u8; 32],
) -> String {
    // HMAC-SHA256 with an ephemeral random key prevents forging confirmation tokens.
    let mut inner_pad = [0x36u8; 64];
    let mut outer_pad = [0x5cu8; 64];
    for (index, byte) in key.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    for field in [text, format, group] {
        inner.update((field.len() as u64).to_le_bytes());
        inner.update(field.as_bytes());
    }
    inner.update(generation.to_le_bytes());
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner.finalize());
    inner_pad.zeroize();
    outer_pad.zeroize();
    hex::encode(outer.finalize())
}

fn token_equal(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |sum, (x, y)| sum | (x ^ y))
        == 0
}
fn validate_backup_passphrase(passphrase: &str) -> Result<(), String> {
    if passphrase.is_empty() || passphrase.len() > MAX_BACKUP_PASSPHRASE_BYTES {
        Err("备份口令必须为1至4096字节；Unicode和前后空格会保留。".into())
    } else {
        Ok(())
    }
}
fn validate_secret(secret: &str) -> Result<(), String> {
    if secret.is_empty() || secret.len() > 4096 || secret.contains(['\0', '\r', '\n']) {
        Err("密码必须为1至4096字节的单行文本；前后空格会保留。".into())
    } else {
        Ok(())
    }
}
fn validate_metadata(name: &str, groups: &[String], notes: &str) -> Result<(), String> {
    if name.len() > 1024
        || groups.len() > 100
        || groups
            .iter()
            .any(|g| g.len() > 1024 || g.contains(['\0', '\r', '\n']))
        || notes.len() > 16384
        || name.contains(['\0', '\r', '\n'])
    {
        Err("密码名称、分组或备注超出限制。".into())
    } else {
        Ok(())
    }
}

struct ParsedRecord {
    line: usize,
    name: String,
    secret: String,
    groups: Vec<String>,
    notes: String,
    error: bool,
    message: String,
}

fn parse_import_text(text: &str, format: &str, default_group: &str) -> Vec<ParsedRecord> {
    let lower_format = format.trim().to_ascii_lowercase();
    let is_tsv = lower_format == "tsv";
    let is_csv = lower_format == "csv" || lower_format.starts_with("csv:");
    let def_grp = default_group.trim();

    let mut results = if is_csv {
        parse_csv_import(text, &lower_format, def_grp)
    } else {
        let mut rows = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            let line_num = idx + 1;
            let line_raw = line.trim_end_matches(['\r', '\n']);

            if is_tsv {
                let parts: Vec<&str> = line_raw.split('\t').collect();
                if parts.is_empty() || (parts.len() == 1 && parts[0].trim().is_empty()) {
                    continue;
                }

                if parts.len() != 4 {
                    rows.push(ParsedRecord {
                        line: line_num,
                        name: String::new(),
                        secret: String::new(),
                        groups: vec![],
                        notes: String::new(),
                        error: true,
                        message: "TSV 必须恰好为四列：名称、密码、分组、备注。".into(),
                    });
                    continue;
                }
                let (name, secret, grp_opt, notes) = (parts[0], parts[1], Some(parts[2]), parts[3]);

                if secret.is_empty() {
                    rows.push(ParsedRecord {
                        line: line_num,
                        name: name.trim().to_string(),
                        secret: String::new(),
                        groups: Vec::new(),
                        notes: notes.to_string(),
                        error: true,
                        message: "Empty secret".to_string(),
                    });
                    continue;
                }

                let mut grps = Vec::new();
                if let Some(g) = grp_opt {
                    for part in g.split([',', ';']) {
                        let t = part.trim();
                        if !t.is_empty() {
                            grps.push(t.to_string());
                        }
                    }
                }
                if grps.is_empty() && !def_grp.is_empty() {
                    grps.push(def_grp.to_string());
                }

                rows.push(ParsedRecord {
                    line: line_num,
                    name: name.trim().to_string(),
                    secret: secret.to_string(),
                    groups: grps,
                    notes: notes.to_string(),
                    error: false,
                    message: String::new(),
                });
            } else {
                // Plain text: first column/token is secret
                if line_raw.is_empty() {
                    continue;
                }
                let mut grps = Vec::new();
                if !def_grp.is_empty() {
                    grps.push(def_grp.to_string());
                }

                rows.push(ParsedRecord {
                    line: line_num,
                    name: format!("Line {}", line_num),
                    secret: line_raw.to_string(),
                    groups: grps,
                    notes: String::new(),
                    error: false,
                    message: String::new(),
                });
            }
        }
        rows
    };

    for record in &mut results {
        if !record.error
            && let Err(message) = validate_secret(&record.secret)
                .and_then(|_| validate_metadata(&record.name, &record.groups, &record.notes))
        {
            record.error = true;
            record.message = message;
            record.secret.zeroize();
            record.name.clear();
            record.groups.clear();
            record.notes.clear();
        }
    }
    results
}

#[derive(Default)]
struct CsvColumnMapping {
    name_idx: Option<usize>,
    secret_idx: Option<usize>,
    group_idx: Option<usize>,
    notes_idx: Option<usize>,
}

struct CsvRow {
    line: usize,
    columns: Vec<String>,
    invalid_quotes: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum CsvFieldState {
    Start,
    Unquoted,
    Quoted,
    ClosedQuote,
    Invalid,
}

fn parse_csv_rows(text: &str) -> Vec<CsvRow> {
    let mut rows = Vec::new();
    let mut current_row = Vec::new();
    let mut current_field = String::new();
    let mut state = CsvFieldState::Start;
    let mut invalid_quotes = false;
    let mut row_start_line = 1;
    let mut current_line = 1;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if state == CsvFieldState::Quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current_field.push('"');
                } else {
                    state = CsvFieldState::ClosedQuote;
                }
            } else {
                if ch == '\r' {
                    current_field.push(ch);
                    if chars.peek() == Some(&'\n') {
                        current_field.push(chars.next().unwrap());
                    }
                    current_line += 1;
                    continue;
                } else if ch == '\n' {
                    current_line += 1;
                }
                current_field.push(ch);
            }
        } else {
            match ch {
                '"' => {
                    if state == CsvFieldState::Start {
                        state = CsvFieldState::Quoted;
                    } else {
                        invalid_quotes = true;
                        state = CsvFieldState::Invalid;
                    }
                }
                ',' => {
                    current_row.push(std::mem::take(&mut current_field));
                    state = CsvFieldState::Start;
                }
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    current_row.push(std::mem::take(&mut current_field));
                    if invalid_quotes
                        || !(current_row.is_empty()
                            || current_row.len() == 1 && current_row[0].is_empty())
                    {
                        rows.push(CsvRow {
                            line: row_start_line,
                            columns: std::mem::take(&mut current_row),
                            invalid_quotes,
                        });
                    } else {
                        current_row.clear();
                    }
                    current_line += 1;
                    row_start_line = current_line;
                    state = CsvFieldState::Start;
                    invalid_quotes = false;
                }
                '\n' => {
                    current_row.push(std::mem::take(&mut current_field));
                    if invalid_quotes
                        || !(current_row.is_empty()
                            || current_row.len() == 1 && current_row[0].is_empty())
                    {
                        rows.push(CsvRow {
                            line: row_start_line,
                            columns: std::mem::take(&mut current_row),
                            invalid_quotes,
                        });
                    } else {
                        current_row.clear();
                    }
                    current_line += 1;
                    row_start_line = current_line;
                    state = CsvFieldState::Start;
                    invalid_quotes = false;
                }
                _ => {
                    if state == CsvFieldState::ClosedQuote {
                        invalid_quotes = true;
                        state = CsvFieldState::Invalid;
                    } else if state == CsvFieldState::Start {
                        state = CsvFieldState::Unquoted;
                    }
                    current_field.push(ch);
                }
            }
        }
    }

    invalid_quotes |= state == CsvFieldState::Quoted;
    if invalid_quotes
        || state == CsvFieldState::ClosedQuote
        || !current_field.is_empty()
        || !current_row.is_empty()
    {
        current_row.push(std::mem::take(&mut current_field));
        rows.push(CsvRow {
            line: row_start_line,
            columns: current_row,
            invalid_quotes,
        });
    }

    rows
}

fn parse_csv_import(text: &str, format: &str, def_grp: &str) -> Vec<ParsedRecord> {
    let rows = parse_csv_rows(text);
    if rows.is_empty() {
        return Vec::new();
    }

    let mut mapping = CsvColumnMapping::default();
    let mut data_start_idx = 0;

    // 检查显式列映射参数：csv:secret=1,name=0 或 csv:name,secret,group,notes
    if let Some(spec) = format.strip_prefix("csv:") {
        let spec_trimmed = spec.trim();
        if spec_trimmed.contains('=') {
            for kv in spec_trimmed.split(['&', ',']) {
                let mut parts = kv.split('=');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    let k = k.trim().to_ascii_lowercase();
                    let v_idx = v.trim().parse::<usize>().ok();
                    match k.as_str() {
                        "secret" | "password" | "pwd" => mapping.secret_idx = v_idx,
                        "name" | "title" => mapping.name_idx = v_idx,
                        "group" | "groups" => mapping.group_idx = v_idx,
                        "notes" | "note" => mapping.notes_idx = v_idx,
                        _ => {}
                    }
                }
            }
        } else if spec_trimmed.contains(',') {
            let cols: Vec<&str> = spec_trimmed.split(',').map(|s| s.trim()).collect();
            for (idx, col) in cols.iter().enumerate() {
                match col.to_ascii_lowercase().as_str() {
                    "secret" | "password" | "pwd" => mapping.secret_idx = Some(idx),
                    "name" | "title" => mapping.name_idx = Some(idx),
                    "group" | "groups" => mapping.group_idx = Some(idx),
                    "notes" | "note" => mapping.notes_idx = Some(idx),
                    _ => {}
                }
            }
        }
    }

    // 若无显式映射，探测第一行是否为表头
    if mapping.secret_idx.is_none() && !rows[0].invalid_quotes {
        let first_row = &rows[0].columns;
        let mut header_has_secret = false;
        let mut detected_map = CsvColumnMapping::default();

        for (idx, cell) in first_row.iter().enumerate() {
            let lower = cell.to_ascii_lowercase();
            if lower.contains("password")
                || lower.contains("secret")
                || lower.contains("pwd")
                || lower.contains("密码")
                || lower.contains("解压码")
                || lower.contains("提取码")
            {
                detected_map.secret_idx = Some(idx);
                header_has_secret = true;
            } else if lower.contains("name") || lower.contains("名称") || lower.contains("标题")
            {
                detected_map.name_idx = Some(idx);
            } else if lower.contains("group") || lower.contains("分组") || lower.contains("标签")
            {
                detected_map.group_idx = Some(idx);
            } else if lower.contains("notes")
                || lower.contains("note")
                || lower.contains("备注")
                || lower.contains("说明")
            {
                detected_map.notes_idx = Some(idx);
            }
        }

        if header_has_secret {
            mapping = detected_map;
            data_start_idx = 1;
        }
    }

    let mut results = Vec::new();
    for row in rows.into_iter().skip(data_start_idx) {
        let line_num = row.line;
        let cols = row.columns;
        if row.invalid_quotes {
            results.push(ParsedRecord {
                line: line_num,
                name: String::new(),
                secret: String::new(),
                groups: Vec::new(),
                notes: String::new(),
                error: true,
                message: "CSV 引号格式无效：引号字段必须完整闭合，且字段内引号必须写为两个双引号。"
                    .into(),
            });
            continue;
        }
        if cols.is_empty() || (cols.len() == 1 && cols[0].is_empty()) {
            continue;
        }

        let (name, secret, grp_opt, notes) = if mapping.secret_idx.is_some() {
            let secret = mapping
                .secret_idx
                .and_then(|i| cols.get(i))
                .cloned()
                .unwrap_or_default();
            let name = mapping
                .name_idx
                .and_then(|i| cols.get(i))
                .cloned()
                .unwrap_or_default();
            let grp = mapping.group_idx.and_then(|i| cols.get(i)).cloned();
            let notes = mapping
                .notes_idx
                .and_then(|i| cols.get(i))
                .cloned()
                .unwrap_or_default();
            (name, secret, grp, notes)
        } else {
            // 根据列数自动对齐
            match cols.len() {
                1 => (String::new(), cols[0].clone(), None, String::new()),
                2 => (cols[0].clone(), cols[1].clone(), None, String::new()),
                3 => (
                    cols[0].clone(),
                    cols[1].clone(),
                    Some(cols[2].clone()),
                    String::new(),
                ),
                _ => (
                    cols[0].clone(),
                    cols[1].clone(),
                    Some(cols[2].clone()),
                    cols[3..].join(" "),
                ),
            }
        };

        if secret.is_empty() {
            results.push(ParsedRecord {
                line: line_num,
                name: name.trim().to_string(),
                secret: String::new(),
                groups: Vec::new(),
                notes: notes.to_string(),
                error: true,
                message: "Empty secret".to_string(),
            });
            continue;
        }

        let mut grps = Vec::new();
        if let Some(g) = grp_opt {
            for part in g.split([',', ';']) {
                let t = part.trim();
                if !t.is_empty() {
                    grps.push(t.to_string());
                }
            }
        }
        if grps.is_empty() && !def_grp.is_empty() {
            grps.push(def_grp.to_string());
        }

        let display_name = if name.trim().is_empty() {
            format!("Row {}", line_num)
        } else {
            name.trim().to_string()
        };

        results.push(ParsedRecord {
            line: line_num,
            name: display_name,
            secret,
            groups: grps,
            notes,
            error: false,
            message: String::new(),
        });
    }

    results
}

#[derive(PartialEq, Eq, Hash)]
struct TemporarySecret(String);
impl Drop for TemporarySecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// 基于常见规则为已保存候选词派生受限变体（大小写、去修饰符、分隔符替换等）
pub fn derive_candidate_mutations(base_candidates: &[Candidate]) -> Vec<Candidate> {
    let mut existing: std::collections::HashSet<TemporarySecret> = base_candidates
        .iter()
        .map(|c| TemporarySecret(c.secret.clone()))
        .collect();
    let mut mutations = Vec::new();

    for base in base_candidates {
        let trimmed = base.secret.trim();
        if trimmed.is_empty() || trimmed.len() > 256 {
            continue;
        }

        let mut variants = Vec::new();

        // 1. 剥离括号与修饰符
        if let Some(inner) = strip_affixes_and_tags(trimmed)
            && !inner.is_empty()
            && inner != trimmed
        {
            variants.push(Zeroizing::new(inner));
        }

        // 2. 大小写变换
        if trimmed.chars().any(|c| c.is_ascii_uppercase()) {
            variants.push(Zeroizing::new(trimmed.to_ascii_lowercase()));
        }
        if trimmed.chars().any(|c| c.is_ascii_lowercase()) {
            variants.push(Zeroizing::new(trimmed.to_ascii_uppercase()));
        }

        // 3. 分隔符互换（_ 与 -）
        if trimmed.contains('_') && !trimmed.contains('-') {
            variants.push(Zeroizing::new(trimmed.replace('_', "-")));
        } else if trimmed.contains('-') && !trimmed.contains('_') {
            variants.push(Zeroizing::new(trimmed.replace('-', "_")));
        }

        // 4. 剥离内部空格（防止复制带入的杂质空白）
        if trimmed.contains(' ') {
            let no_space = trimmed.replace(' ', "");
            if !no_space.is_empty() {
                variants.push(Zeroizing::new(no_space));
            }
        }

        // 5. 域名互补派生（www. 补充）
        let lower = trimmed.to_ascii_lowercase();
        if !lower.starts_with("www.")
            && !lower.starts_with("http")
            && (lower.ends_with(".com")
                || lower.ends_with(".cn")
                || lower.ends_with(".net")
                || lower.ends_with(".org")
                || lower.ends_with(".cc")
                || lower.ends_with(".me")
                || lower.ends_with(".top"))
        {
            variants.push(Zeroizing::new(format!("www.{trimmed}")));
        }

        let mut count = 0;
        for mut var in variants {
            if count >= 3 {
                break;
            }
            if !var.is_empty()
                && var.len() <= 4096
                && existing.insert(TemporarySecret(var.to_string()))
            {
                count += 1;
                mutations.push(Candidate {
                    id: format!("{}#m{}", base.id, count),
                    revision: base.revision,
                    label: format!("{} (变体)", base.label),
                    secret: std::mem::take(&mut *var),
                });
            }
        }
    }

    mutations
}

fn strip_affixes_and_tags(secret: &str) -> Option<String> {
    let mut s = secret.trim();
    let mut changed = false;

    if ((s.starts_with('[') && s.ends_with(']'))
        || (s.starts_with('【') && s.ends_with('】'))
        || (s.starts_with('(') && s.ends_with(')'))
        || (s.starts_with('（') && s.ends_with('）')))
        && let (Some(first), Some(last)) = (s.chars().next(), s.chars().next_back())
    {
        s = s[first.len_utf8()..s.len() - last.len_utf8()].trim();
        changed = true;
    }

    let prefixes = [
        "密码:",
        "密码：",
        "密码=",
        "解压密码:",
        "解压密码：",
        "解压密码=",
        "pwd:",
        "pwd：",
        "pwd=",
        "pass:",
        "pass：",
        "pass=",
        "code:",
        "code：",
        "code=",
        "解压码:",
        "解压码：",
        "解压码=",
        "提取码:",
        "提取码：",
        "提取码=",
        "提取:",
        "提取：",
        "提取=",
    ];
    for p in prefixes {
        if let Some(stripped) = s.strip_prefix(p) {
            s = stripped.trim();
            changed = true;
            break;
        }
        if s.get(..p.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(p))
        {
            s = s[p.len()..].trim();
            changed = true;
            break;
        }
    }

    if let Some(rest) = s.strip_prefix("https://") {
        s = rest.trim_start_matches("www.").trim();
        changed = true;
    } else if let Some(rest) = s.strip_prefix("http://") {
        s = rest.trim_start_matches("www.").trim();
        changed = true;
    } else if let Some(rest) = s.strip_prefix("www.") {
        s = rest.trim();
        changed = true;
    }

    let trimmed_punct = s
        .trim_end_matches(|c: char| {
            c == '.'
                || c == ','
                || c == '!'
                || c == '?'
                || c == ';'
                || c == ':'
                || c == '。'
                || c == '！'
                || c == '；'
                || c == '：'
        })
        .trim();
    if trimmed_punct != s {
        s = trimmed_punct;
        changed = true;
    }

    if changed && !s.is_empty() {
        Some(s.to_string())
    } else {
        None
    }
}

fn atomic_write_file(destination: &Path, content: &[u8]) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "Invalid destination path".to_string())?;
    fs::create_dir_all(parent).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;

    let tmp_name = format!(".tmp_{}_{}", std::process::id(), Uuid::new_v4().simple());
    let tmp_path = parent.join(tmp_name);

    let write_result: Result<(), String> = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
        f.write_all(content)
            .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
        f.sync_all().map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut wide_src: Vec<u16> = tmp_path.as_os_str().encode_wide().collect();
        wide_src.push(0);
        let mut wide_dst: Vec<u16> = destination.as_os_str().encode_wide().collect();
        wide_dst.push(0);

        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };

        let ret = unsafe {
            MoveFileExW(
                wide_src.as_ptr(),
                wide_dst.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };

        if ret == 0 {
            let _ = fs::remove_file(&tmp_path);
            return Err("Failed to atomic replace file".to_string());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        fs::rename(&tmp_path, destination).map_err(|_| {
            let _ = fs::remove_file(&tmp_path);
            ERR_VAULT_CORRUPT.to_string()
        })
    }
}

#[cfg(windows)]
fn dpapi_encrypt(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi_transform(data, true)
}

#[cfg(windows)]
fn dpapi_decrypt(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi_transform(data, false)
}

#[cfg(windows)]
fn dpapi_transform(data: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    if data.len() > u32::MAX as usize {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let success = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if success == 0 || output.pbData.is_null() {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        LocalFree(output.pbData as *mut std::ffi::c_void);
    }
    Ok(result)
}

/// Windows portable mode is opt-in: an environment switch or an explicit key.
/// On other platforms portable storage is the only implementation.
#[cfg(windows)]
fn portable_vault_opted_in() -> bool {
    std::env::var("OPENUNPACK_PORTABLE_VAULT").is_ok()
        || std::env::var("OPENUNPACK_VAULT_KEY").is_ok()
}
#[cfg(not(windows))]
fn portable_vault_opted_in() -> bool {
    true
}

fn resolve_vault_key(path: &Path) -> Result<Zeroizing<Vec<u8>>, String> {
    if let Ok(env_key) = std::env::var("OPENUNPACK_VAULT_KEY") {
        let trimmed = env_key.trim();
        if !trimmed.is_empty() {
            return Ok(Zeroizing::new(trimmed.as_bytes().to_vec()));
        }
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let key_file = parent.join(".vault.key");
    if key_file.exists() {
        if !portable_vault_opted_in() {
            return Err(ERR_PORTABLE_NOT_ENABLED.to_string());
        }
        let metadata =
            fs::symlink_metadata(&key_file).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
        if !metadata.file_type().is_file() {
            return Err("Vault key must be a regular file".into());
        }
        if let Ok(bytes) = fs::read(&key_file)
            && !bytes.is_empty()
        {
            return Ok(Zeroizing::new(bytes));
        }
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    // Auto-generate key file
    let mut key_bytes = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key_bytes);
    let _ = fs::create_dir_all(parent);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut f) = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&key_file)
        {
            let _ = f.write_all(&key_bytes);
        }
    }
    #[cfg(not(unix))]
    {
        if let Ok(mut f) = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&key_file)
        {
            let _ = f.write_all(&key_bytes);
        }
    }
    Ok(Zeroizing::new(key_bytes))
}

pub fn portable_encrypt(data: &[u8], path: &Path) -> Result<Vec<u8>, String> {
    let master_key = resolve_vault_key(path)?;
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let mut derived_key = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2_hmac::<Sha256>(
        &master_key,
        &salt,
        VAULT_PORTABLE_DEFAULT_ITER,
        &mut *derived_key,
    );

    let cipher =
        Aes256Gcm::new_from_slice(&*derived_key).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;

    let mut output = Vec::with_capacity(8 + 4 + 16 + 12 + ciphertext.len());
    output.extend_from_slice(VAULT_MAGIC);
    output.extend_from_slice(&VAULT_PORTABLE_DEFAULT_ITER.to_be_bytes());
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

pub fn portable_decrypt(data: &[u8], path: &Path) -> Result<Vec<u8>, String> {
    if data.len() < 8 + 4 + 16 + 12 + 16 {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    let iter = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    if !(BACKUP_MIN_ITER..=BACKUP_MAX_ITER).contains(&iter) {
        return Err(ERR_VAULT_CORRUPT.to_string());
    }
    let salt = &data[12..28];
    let nonce_bytes = &data[28..40];
    let ciphertext = &data[40..];

    let master_key = resolve_vault_key(path)?;
    let mut derived_key = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2_hmac::<Sha256>(&master_key, salt, iter, &mut *derived_key);

    let cipher =
        Aes256Gcm::new_from_slice(&*derived_key).map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| ERR_VAULT_CORRUPT.to_string())?;
    Ok(plaintext)
}

fn vault_encrypt(data: &[u8], path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        let use_portable = portable_vault_opted_in();
        if use_portable {
            portable_encrypt(data, path)
        } else {
            dpapi_encrypt(data)
        }
    }
    #[cfg(not(windows))]
    {
        portable_encrypt(data, path)
    }
}

fn vault_decrypt(data: &[u8], path: &Path) -> Result<Vec<u8>, String> {
    if data.starts_with(VAULT_MAGIC) {
        #[cfg(windows)]
        if !portable_vault_opted_in() {
            return Err(ERR_PORTABLE_NOT_ENABLED.to_string());
        }
        portable_decrypt(data, path)
    } else {
        #[cfg(windows)]
        {
            dpapi_decrypt(data)
        }
        #[cfg(not(windows))]
        {
            Err(ERR_VAULT_CORRUPT.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn csv_quote_grammar_reports_rows_without_reinterpreting_passwords() {
        for malformed in [
            "Entry,\"unclosed",
            "Entry,ab\"cd",
            "Entry,\"abc\"tail",
            "Entry,\"abc\" \n",
            "\"name\"tail,password\nEntry,valid",
        ] {
            let records = parse_import_text(malformed, "csv:name,secret", "");
            assert!(records[0].error, "malformed quoting must be reported");
            assert!(records[0].secret.is_empty());
            assert_eq!(records[0].line, 1);
        }
        // CRLF in quoted notes is content, and physical line numbers remain
        // useful after a multiline row and an invalid row.
        let records = parse_import_text(
            "name,password,notes\r\nEntry,\"  a\"\"b,🔑  \",\"first\r\nsecond\"\r\nBad,ab\"cd,ignored\r\nGood,  raw  ,tail",
            "csv",
            "",
        );
        assert_eq!(records.len(), 3);
        assert!(!records[0].error);
        assert_eq!(records[0].secret, "  a\"b,🔑  ");
        assert_eq!(records[0].notes, "first\r\nsecond");
        assert_eq!(records[0].line, 2);
        assert!(records[1].error);
        assert_eq!(records[1].line, 4);
        assert!(!records[2].error);
        assert_eq!(records[2].secret, "  raw  ");
        assert_eq!(records[2].line, 5);
        let tsv = parse_import_text("Entry\tab\"cd\tGroup\tNote", "tsv", "");
        assert!(
            !tsv[0].error,
            "CSV quoting rules must not alter TSV literals"
        );
        assert_eq!(tsv[0].secret, "ab\"cd");
    }

    #[cfg(windows)]
    #[test]
    fn malformed_csv_confirmation_preserves_existing_vault_and_preview_generation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("csv.dat");
        let vault = Vault::open(&path).unwrap();
        let existing = vault
            .upsert(input(None, Some("synthetic-existing")))
            .unwrap();
        let memory = release_audit_state(&vault);
        let disk = fs::read(&path).unwrap();
        for malformed in ["\"unclosed", "ab\"cd", "\"abc\"tail"] {
            let csv = format!("name,password\nValid,synthetic-valid\nBad,{malformed}");
            let preview = vault.preview_import(&csv, "csv", "").unwrap();
            assert_eq!(preview.new_count, 1);
            assert_eq!(preview.error_count, 1);
            assert_eq!(preview.rows[1].line, 3);
            assert!(vault.apply_import(&preview.token, &csv, "csv", "").is_err());
            assert!(release_audit_state(&vault) == memory);
            assert!(fs::read(&path).unwrap() == disk);
        }
        drop(vault);
        let reopened = Vault::open(&path).unwrap();
        assert_eq!(reopened.views().unwrap().len(), 1);
        assert_eq!(reopened.reveal(&existing.id).unwrap(), "synthetic-existing");
        let csv = "name,password,notes\nValid,\"  a\"\"b,🔑  \",\"line 1\nline 2\"";
        let preview = reopened.preview_import(csv, "csv", "").unwrap();
        assert_eq!(preview.error_count, 0);
        assert_eq!(
            reopened
                .apply_import(&preview.token, csv, "csv", "")
                .unwrap()
                .added,
            1
        );
        drop(reopened);
        let reopened = Vault::open(&path).unwrap();
        let added = reopened
            .views()
            .unwrap()
            .into_iter()
            .find(|i| i.name == "Valid")
            .unwrap();
        assert_eq!(reopened.reveal(&added.id).unwrap(), "  a\"b,🔑  ");
        assert_eq!(added.notes, "line 1\nline 2");
    }

    #[cfg(windows)]
    #[test]
    fn vault_item_limit_is_shared_by_public_mutations_and_backup_reader_writer() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy-count.dat");
        let mut legacy = VaultData::default();
        for index in 0..=MAX_VAULT_ITEMS {
            let id = format!("{index:036}");
            legacy.items.insert(
                id.clone(),
                VaultItem {
                    id,
                    name: "Synthetic".into(),
                    secret: format!("synthetic-count-{index}"),
                    groups: vec![],
                    notes: String::new(),
                    enabled: true,
                    success_count: 0,
                    last_success: None,
                    revision: 1,
                    deleted: false,
                    created_at: "2000-01-01T00:00:00+00:00".into(),
                    updated_at: "2000-01-01T00:00:00+00:00".into(),
                },
            );
        }
        // Construct a schema-valid legacy store directly; do not make 100001
        // encrypted commits just to establish a boundary fixture.
        let raw = Zeroizing::new(serde_json::to_vec(&legacy).unwrap());
        fs::write(&path, dpapi_encrypt(&raw).unwrap()).unwrap();
        drop(raw);
        drop(legacy);
        let vault = Vault::open(&path).unwrap();
        assert_eq!(vault.views().unwrap().len(), MAX_VAULT_ITEMS + 1);
        let destination = dir.path().join("existing.json");
        fs::write(&destination, b"retain synthetic destination").unwrap();
        assert!(
            vault
                .export_backup(&destination, "synthetic-passphrase")
                .is_err()
        );
        assert_eq!(
            fs::read(&destination).unwrap(),
            b"retain synthetic destination"
        );
        let missing = dir.path().join("missing.json");
        assert!(
            vault
                .export_backup(&missing, "synthetic-passphrase")
                .is_err()
        );
        assert!(!missing.exists());
        let original_disk = fs::read(&path).unwrap();
        let original_memory = release_audit_state(&vault);
        assert!(
            vault
                .upsert(input(None, Some("synthetic-overflow")))
                .is_err()
        );
        assert!(fs::read(&path).unwrap() == original_disk);
        assert!(release_audit_state(&vault) == original_memory);
        drop(original_disk);
        drop(original_memory);
        // An old over-limit store remains editable and can be reduced without
        // silently discarding records on open.
        let first = format!("{:036}", 0);
        vault.upsert(input(Some(first.clone()), None)).unwrap();
        vault.delete(&first).unwrap();
        assert_eq!(vault.views().unwrap().len(), MAX_VAULT_ITEMS);
        assert!(
            vault
                .upsert(input(None, Some("synthetic-overflow")))
                .is_err()
        );
        let before = fs::read(&path).unwrap();
        let preview = vault
            .preview_import("synthetic-overflow", "txt", "")
            .unwrap();
        assert_eq!(preview.error_count, 1);
        assert!(
            vault
                .apply_import(&preview.token, "synthetic-overflow", "txt", "")
                .is_err()
        );
        assert!(fs::read(&path).unwrap() == before);
        // Existing secrets can still merge at capacity.
        drop(before);
        let duplicate = vault
            .preview_import("synthetic-count-1", "txt", "")
            .unwrap();
        assert_eq!(duplicate.duplicate_count, 1);
        assert_eq!(
            vault
                .apply_import(&duplicate.token, "synthetic-count-1", "txt", "")
                .unwrap()
                .added,
            0
        );
        let small = Vault::open(&dir.path().join("small.dat")).unwrap();
        small
            .upsert(input(None, Some("synthetic-restore-overflow")))
            .unwrap();
        let small_backup = dir.path().join("small.json");
        small
            .export_backup(&small_backup, "synthetic-passphrase")
            .unwrap();
        let restore_preview = vault
            .preview_restore_backup(&small_backup, "synthetic-passphrase")
            .unwrap();
        assert_eq!(restore_preview.error_count, 1);
        let before = fs::read(&path).unwrap();
        let memory = release_audit_state(&vault);
        assert!(
            vault
                .confirm_restore_backup(
                    &small_backup,
                    "synthetic-passphrase",
                    &restore_preview.token
                )
                .is_err()
        );
        assert!(fs::read(&path).unwrap() == before);
        assert!(release_audit_state(&vault) == memory);
        drop(before);
        drop(memory);
        // Deletion tombstones do not consume active-item capacity.
        vault.delete(&format!("{:036}", 1)).unwrap();
        vault
            .upsert(input(None, Some("synthetic-replacement")))
            .unwrap();
        vault
            .export_backup(&destination, "synthetic-passphrase")
            .unwrap();
        let (decoded, _) = Vault::decode_backup(&destination, "synthetic-passphrase").unwrap();
        assert_eq!(decoded.items.len(), MAX_VAULT_ITEMS);
        drop(decoded);
        drop(vault);
        assert_eq!(
            Vault::open(&path).unwrap().views().unwrap().len(),
            MAX_VAULT_ITEMS
        );
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_envelope_at_plaintext_limit_reopens_and_oversize_mutation_is_atomic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("size.dat");
        let vault = Vault::open(&path).unwrap();
        let mut data = VaultData::default();
        data.seen_events.insert(String::new());
        let overhead = serialized_size(&data).unwrap();
        data.seen_events.clear();
        data.seen_events
            .insert("x".repeat(MAX_BACKUP_PAYLOAD_SIZE - overhead));
        assert_eq!(serialized_size(&data).unwrap(), MAX_BACKUP_PAYLOAD_SIZE);
        vault.persist_internal(&data).unwrap();
        let file_size = fs::metadata(&path).unwrap().len() as usize;
        assert!(
            file_size > MAX_BACKUP_PAYLOAD_SIZE,
            "fixture must exercise DPAPI expansion"
        );
        assert!(file_size <= MAX_VAULT_FILE_SIZE);
        drop(data);
        drop(vault);
        let reopened = Vault::open(&path).unwrap();
        let before = Sha256::digest(fs::read(&path).unwrap());
        assert!(
            reopened
                .record_success(&SuccessEvent {
                    event_id: "one-more-event".into(),
                    password_id: "missing".into(),
                    password_revision: 1,
                    fingerprint: None,
                })
                .is_err()
        );
        assert_eq!(Sha256::digest(fs::read(&path).unwrap()), before);
        assert_eq!(reopened.data.lock().unwrap().seen_events.len(), 1);
        let mut too_large = reopened.data.lock().unwrap().clone();
        let value = too_large.seen_events.pop_first().unwrap() + "x";
        too_large.seen_events.insert(value);
        assert_eq!(
            serialized_size(&too_large).unwrap(),
            MAX_BACKUP_PAYLOAD_SIZE + 1
        );
        assert!(reopened.persist_internal(&too_large).is_err());
        assert_eq!(Sha256::digest(fs::read(&path).unwrap()), before);
    }

    #[test]
    fn backup_size_validation_accounts_for_local_restore_metadata() {
        let mut payload = BackupPayload {
            version: 1,
            items: (0..4040)
                .map(|index| BackupItem {
                    name: "n".into(),
                    secret: format!("s{index}"),
                    groups: vec![],
                    notes: "x".repeat(16384),
                    enabled: true,
                    success_count: 0,
                    last_success: None,
                })
                .collect(),
        };
        assert!(serialized_size(&payload).unwrap() < MAX_BACKUP_PAYLOAD_SIZE);
        assert!(
            validate_empty_restore_size(&payload).is_err(),
            "portable payload fits, restored local representation does not"
        );
        for item in &mut payload.items {
            item.notes.truncate(16000);
        }
        assert!(validate_empty_restore_size(&payload).is_ok());
    }

    #[cfg(windows)]
    fn release_audit_state(vault: &Vault) -> Vec<u8> {
        serde_json::to_vec(&*vault.data.lock().unwrap()).unwrap()
    }

    #[cfg(windows)]
    fn release_audit_groups(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("分组-{index:03}")).collect()
    }

    #[cfg(windows)]
    #[test]
    fn release_audit_backup_export_rejects_overlong_passphrase_atomically() {
        let dir = tempdir().unwrap();
        let vault_path = dir.path().join("vault.dat");
        let vault = Vault::open(&vault_path).unwrap();
        vault
            .upsert(input(None, Some("  synthetic-导出口令🔑  ")))
            .unwrap();
        let memory = release_audit_state(&vault);
        let disk = fs::read(&vault_path).unwrap();
        let destination = dir.path().join("backup.json");
        let retained = b"existing synthetic destination";
        fs::write(&destination, retained).unwrap();
        for passphrase in ["x".repeat(4097), "密".repeat(1366)] {
            assert!(
                vault.export_backup(&destination, &passphrase).is_err(),
                "export must reject a passphrase that decode_backup cannot accept"
            );
            assert!(
                fs::read(&destination).unwrap() == retained,
                "rejected export must preserve an existing destination"
            );
            let missing = dir.path().join("must-not-create.json");
            assert!(vault.export_backup(&missing, &passphrase).is_err());
            assert!(!missing.exists());
            assert!(release_audit_state(&vault) == memory);
            assert!(fs::read(&vault_path).unwrap() == disk);
        }
    }

    #[cfg(windows)]
    #[test]
    fn release_audit_import_duplicate_group_union_is_atomic_at_100_plus_1() {
        let dir = tempdir().unwrap();
        let vault_path = dir.path().join("vault.dat");
        let vault = Vault::open(&vault_path).unwrap();
        vault
            .upsert(input(None, Some("synthetic-existing-unrelated")))
            .unwrap();
        let groups = release_audit_groups(100).join(",");
        // Each row is valid by itself; duplicate rows together exceed metadata limits.
        let text = format!(
            "staged\t  synthetic-new-before-failure🔑  \t合法组\t保留备注\nfirst\t  synthetic-shared🔑  \t{groups}\t原始备注\nsecond\t  synthetic-shared🔑  \t分组-100\t"
        );
        let preview = vault.preview_import(&text, "tsv", "").unwrap();
        assert_eq!(preview.error_count, 0);
        let memory = release_audit_state(&vault);
        let disk = fs::read(&vault_path).unwrap();
        assert!(
            vault
                .apply_import(&preview.token, &text, "tsv", "")
                .is_err(),
            "valid individual rows must not commit an invalid 101-group union"
        );
        assert!(
            release_audit_state(&vault) == memory,
            "failed import must preserve all in-memory state"
        );
        assert!(
            fs::read(&vault_path).unwrap() == disk,
            "failed import must preserve encrypted storage bytes"
        );
        assert_eq!(Vault::open(&vault_path).unwrap().views().unwrap().len(), 1);

        // The rejected preview still belongs to the same unchanged generation.
        assert!(
            vault
                .apply_import(&preview.token, &text, "tsv", "")
                .is_err()
        );
        assert!(release_audit_state(&vault) == memory);
    }

    #[cfg(windows)]
    #[test]
    fn release_audit_two_backups_group_union_rejects_100_plus_1_atomically() {
        let dir = tempdir().unwrap();
        let first = Vault::open(&dir.path().join("first.dat")).unwrap();
        let mut first_input = input(None, Some("  synthetic-备份共享🔑  "));
        first_input.groups = release_audit_groups(100);
        first.upsert(first_input).unwrap();
        let first_backup = dir.path().join("first.json");
        first
            .export_backup(&first_backup, "  synthetic-备份口令🔑  ")
            .unwrap();

        let second = Vault::open(&dir.path().join("second.dat")).unwrap();
        let mut second_input = input(None, Some("  synthetic-备份共享🔑  "));
        second_input.groups = vec!["分组-100".into()];
        second.upsert(second_input).unwrap();
        second
            .upsert(input(None, Some("synthetic-staged-backup-item")))
            .unwrap();
        let second_backup = dir.path().join("second.json");
        second
            .export_backup(&second_backup, "  synthetic-备份口令🔑  ")
            .unwrap();

        let target_path = dir.path().join("target.dat");
        let target = Vault::open(&target_path).unwrap();
        assert_eq!(
            target
                .restore_backup(&first_backup, "  synthetic-备份口令🔑  ")
                .unwrap()
                .added,
            1
        );
        let memory = release_audit_state(&target);
        let disk = fs::read(&target_path).unwrap();
        let preview = target
            .preview_restore_backup(&second_backup, "  synthetic-备份口令🔑  ")
            .unwrap();
        assert_eq!(
            (
                preview.new_count,
                preview.duplicate_count,
                preview.error_count
            ),
            (1, 1, 0)
        );
        assert!(
            target
                .confirm_restore_backup(&second_backup, "  synthetic-备份口令🔑  ", &preview.token)
                .is_err(),
            "two valid backups must not commit an invalid merged metadata union"
        );
        assert!(release_audit_state(&target) == memory);
        assert!(fs::read(&target_path).unwrap() == disk);
        let reopened = Vault::open(&target_path).unwrap();
        assert_eq!(reopened.views().unwrap()[0].groups.len(), 100);
        assert_eq!(
            reopened.candidates(None, None).unwrap()[0].secret,
            "  synthetic-备份共享🔑  "
        );
    }

    #[cfg(windows)]
    #[test]
    fn release_audit_legal_overlapping_groups_and_byte_limit_passphrase_restore() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vault.dat");
        let vault = Vault::open(&path).unwrap();
        let mut original = input(None, Some("  synthetic-合法重叠🔑  "));
        original.name = "保留名称".into();
        original.notes = "  保留备注\n第二行  ".into();
        original.groups = release_audit_groups(100);
        let stored = vault.upsert(original).unwrap();
        let text = "替换名称\t  synthetic-合法重叠🔑  \t分组-000,分组-099,分组-099\t不替换备注";
        let preview = vault.preview_import(text, "tsv", "").unwrap();
        let result = vault.apply_import(&preview.token, text, "tsv", "").unwrap();
        assert_eq!((result.added, result.duplicates), (0, 1));
        let current = vault.views().unwrap().remove(0);
        assert_eq!(current.groups, stored.groups);
        assert_eq!(current.name, stored.name);
        assert_eq!(current.notes, stored.notes);

        // Exactly 4096 UTF-8 bytes, including Unicode and untrimmed outer spaces.
        let passphrase = format!(" {}ab ", "密".repeat(1364));
        assert_eq!(passphrase.len(), 4096);
        let backup = dir.path().join("legal.json");
        vault.export_backup(&backup, &passphrase).unwrap();
        let result = vault.restore_backup(&backup, &passphrase).unwrap();
        assert_eq!((result.added, result.duplicates), (0, 1));
        assert_eq!(vault.views().unwrap()[0].groups, stored.groups);
        let restored = Vault::open(&dir.path().join("restored.dat")).unwrap();
        assert_eq!(
            restored.restore_backup(&backup, &passphrase).unwrap().added,
            1
        );
        assert_eq!(
            restored.candidates(None, None).unwrap()[0].secret,
            "  synthetic-合法重叠🔑  "
        );
        assert_eq!(restored.views().unwrap()[0].notes, stored.notes);
    }

    #[cfg(windows)]
    #[test]
    fn release_audit_legacy_overlimit_item_export_is_rejected_until_explicit_repair() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy.dat");
        let initial = Vault::open(&path).unwrap();
        let mut original = input(None, Some("  synthetic-旧库保留🔑  "));
        original.groups = release_audit_groups(100);
        original.notes = "  旧版备注原文\n保留  ".into();
        let item = initial.upsert(original).unwrap();

        // Reproduce the old version's persisted result, including real DPAPI storage.
        // Opening this old file must remain supported; export must not create an
        // encrypted backup that the existing decoder cannot restore.
        let mut legacy = initial.data.lock().unwrap().clone();
        legacy
            .items
            .get_mut(&item.id)
            .unwrap()
            .groups
            .push("分组-100".into());
        initial.persist_internal(&legacy).unwrap();
        drop(initial);
        let reopened = Vault::open(&path).unwrap();
        assert_eq!(reopened.views().unwrap()[0].groups.len(), 101);
        assert_eq!(
            reopened.reveal(&item.id).unwrap(),
            "  synthetic-旧库保留🔑  "
        );
        let memory = release_audit_state(&reopened);
        let disk = fs::read(&path).unwrap();
        let backup = dir.path().join("existing-backup.json");
        let retained = b"retain existing synthetic backup destination";
        fs::write(&backup, retained).unwrap();
        let error = reopened
            .export_backup(&backup, "synthetic-legacy-backup")
            .expect_err("export must reject legacy metadata that decode_backup cannot restore");
        assert!(
            error.contains("条目") && error.contains("修正"),
            "export should explain how to repair the invalid item"
        );
        assert!(fs::read(&backup).unwrap() == retained);
        let missing = dir.path().join("must-not-create-legacy.json");
        assert!(
            reopened
                .export_backup(&missing, "synthetic-legacy-backup")
                .is_err()
        );
        assert!(!missing.exists());
        assert!(release_audit_state(&reopened) == memory);
        assert!(fs::read(&path).unwrap() == disk);

        // Repair is explicit through the supported group replacement API, without
        // silently discarding or truncating old data on open or export.
        reopened
            .bulk_action(
                std::slice::from_ref(&item.id),
                "group",
                &release_audit_groups(100),
            )
            .unwrap();
        assert_eq!(reopened.views().unwrap()[0].groups, item.groups);
        assert_eq!(reopened.views().unwrap()[0].notes, item.notes);
        assert_eq!(
            reopened.reveal(&item.id).unwrap(),
            "  synthetic-旧库保留🔑  "
        );
        reopened
            .export_backup(&backup, "synthetic-legacy-backup")
            .unwrap();
        let restored = Vault::open(&dir.path().join("legacy-restored.dat")).unwrap();
        assert_eq!(
            restored
                .restore_backup(&backup, "synthetic-legacy-backup")
                .unwrap()
                .added,
            1
        );
        assert_eq!(restored.views().unwrap()[0].groups, item.groups);
        assert_eq!(restored.views().unwrap()[0].notes, item.notes);
        assert_eq!(
            restored.candidates(None, None).unwrap()[0].secret,
            "  synthetic-旧库保留🔑  "
        );
    }

    #[test]
    fn test_parse_tsv_and_preview_token() {
        let tsv = "label1\tsecret_123\tgrp1, grp2\tnotes here\n\tsecond_secret\t\t";
        let records = parse_import_text(tsv, "tsv", "default");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "label1");
        assert_eq!(records[0].secret, "secret_123");
        assert_eq!(
            records[0].groups,
            vec!["grp1".to_string(), "grp2".to_string()]
        );
        assert_eq!(records[1].secret, "second_secret");
        assert_eq!(records[1].groups, vec!["default".to_string()]);

        let token1 = compute_preview_token(tsv, "tsv", "default", 0, &[7u8; 32]);
        let token2 = compute_preview_token(tsv, "tsv", "default", 0, &[7u8; 32]);
        let token3 = compute_preview_token(tsv, "tsv", "default", 1, &[7u8; 32]);
        assert_eq!(token1, token2);
        assert_ne!(token1, token3);
    }

    #[test]
    fn test_backup_and_restore_crypto() {
        let payload = BackupPayload {
            version: 1,
            items: vec![BackupItem {
                name: "Test item".into(),
                secret: "super secret".into(),
                groups: vec!["g1".into()],
                notes: "note".into(),
                enabled: true,
                success_count: 5,
                last_success: None,
            }],
        };

        let raw = serde_json::to_vec(&payload).unwrap();
        let salt = [1u8; 16];
        let nonce_bytes = [2u8; 12];
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(b"password123", &salt, 100_000, &mut key);
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = cipher.encrypt(nonce, raw.as_slice()).unwrap();

        let env = BackupEnvelope {
            version: 1,
            kdf: "PBKDF2-SHA256".to_string(),
            iterations: 100_000,
            salt: BASE64.encode(salt),
            nonce: BASE64.encode(nonce_bytes),
            ciphertext: BASE64.encode(ct),
        };

        let env_json = serde_json::to_vec(&env).unwrap();
        let parsed_env: BackupEnvelope = serde_json::from_slice(&env_json).unwrap();
        assert_eq!(parsed_env.iterations, 100_000);

        let mut key_decrypt = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(b"password123", &salt, 100_000, &mut key_decrypt);
        let cipher_dec = Aes256Gcm::new_from_slice(&key_decrypt).unwrap();
        let pt = cipher_dec
            .decrypt(
                nonce,
                BASE64.decode(&parsed_env.ciphertext).unwrap().as_slice(),
            )
            .unwrap();
        assert_eq!(pt, raw);
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_vault_lifecycle() {
        let dir = tempdir().unwrap();
        let vault_file = dir.path().join("vault.dat");

        let vault = Vault::open(&vault_file).unwrap();
        let item = vault
            .upsert(PasswordInput {
                id: None,
                name: "Alpha".to_string(),
                secret: Some("MySecretUnicode🔑".to_string()),
                groups: vec!["Work".to_string()],
                notes: "None".to_string(),
                enabled: true,
            })
            .unwrap();

        assert_eq!(vault.views().unwrap().len(), 1);
        assert_eq!(vault.reveal(&item.id).unwrap(), "MySecretUnicode🔑");

        let candidates = vault.candidates(Some("Work"), None).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].secret, "MySecretUnicode🔑");

        vault.delete(&item.id).unwrap();
        assert_eq!(vault.views().unwrap().len(), 0);
        assert!(vault.reveal(&item.id).is_err());
    }

    #[cfg(windows)]
    fn input(id: Option<String>, secret: Option<&str>) -> PasswordInput {
        PasswordInput {
            id,
            name: "Synthetic entry".into(),
            secret: secret.map(str::to_string),
            groups: vec!["A".into()],
            notes: String::new(),
            enabled: true,
        }
    }
    #[cfg(windows)]
    #[test]
    fn corrupt_storage_and_failed_commit_never_replace_good_memory() {
        let dir = tempdir().unwrap();
        let empty = dir.path().join("empty.dat");
        fs::write(&empty, []).unwrap();
        assert!(Vault::open(&empty).is_err());
        assert_eq!(fs::read(&empty).unwrap().len(), 0);
        let path = dir.path().join("vault.dat");
        let vault = Vault::open(&path).unwrap();
        vault
            .upsert(input(None, Some("synthetic-original")))
            .unwrap();
        let good = fs::read(&path).unwrap();
        let retained = dir.path().join("retained.dat");
        fs::rename(&path, &retained).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(vault.upsert(input(None, Some("synthetic-failed"))).is_err());
        assert_eq!(vault.views().unwrap().len(), 1);
        assert_eq!(fs::read(&retained).unwrap(), good);
        let corrupt = dir.path().join("corrupt.dat");
        fs::write(&corrupt, b"invalid synthetic blob").unwrap();
        assert!(Vault::open(&corrupt).is_err());
        assert_eq!(fs::read(&corrupt).unwrap(), b"invalid synthetic blob");
    }
    #[cfg(windows)]
    #[test]
    fn success_events_obey_revision_dedupe_and_tombstones() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vault.dat");
        let vault = Vault::open(&path).unwrap();
        let item = vault.upsert(input(None, Some("first-secret"))).unwrap();
        let renamed = vault.upsert(input(Some(item.id.clone()), None)).unwrap();
        assert_eq!(renamed.revision, 1);
        let changed = vault
            .upsert(input(Some(item.id.clone()), Some("second-secret")))
            .unwrap();
        assert_eq!(changed.revision, 2);
        let mut ev = SuccessEvent {
            event_id: "old-event".into(),
            password_id: item.id.clone(),
            password_revision: 1,
            fingerprint: Some("old-file".into()),
        };
        vault.record_success(&ev).unwrap();
        assert_eq!(vault.views().unwrap()[0].success_count, 0);
        ev.event_id = "new-event".into();
        ev.password_revision = 2;
        ev.fingerprint = None;
        vault.record_success(&ev).unwrap();
        vault.record_success(&ev).unwrap();
        assert_eq!(vault.views().unwrap()[0].success_count, 1);
        vault.delete(&item.id).unwrap();
        ev.event_id = "after-delete".into();
        vault.record_success(&ev).unwrap();
        assert!(
            vault
                .upsert(input(Some(item.id.clone()), Some("resurrection")))
                .is_err()
        );
        drop(vault);
        assert!(Vault::open(&path).unwrap().views().unwrap().is_empty());
    }
    #[cfg(windows)]
    #[test]
    fn import_tokens_are_bound_and_unicode_whitespace_is_preserved() {
        let dir = tempdir().unwrap();
        let vault = Vault::open(&dir.path().join("vault.dat")).unwrap();
        let text = "  🔑秘密  \nrepeat\nrepeat";
        let preview = vault.preview_import(text, "txt", "A").unwrap();
        assert_eq!((preview.new_count, preview.duplicate_count), (2, 1));
        let view_json = serde_json::to_string(&preview).unwrap();
        assert!(!view_json.contains("秘密") && !view_json.contains("repeat"));
        assert!(
            vault
                .apply_import(&preview.token, "changed", "txt", "A")
                .is_err()
        );
        let result = vault
            .apply_import(&preview.token, text, "txt", "A")
            .unwrap();
        assert_eq!((result.added, result.duplicates), (2, 1));
        assert!(
            vault
                .apply_import(&preview.token, text, "txt", "A")
                .is_err()
        );
        assert!(
            vault
                .candidates(None, None)
                .unwrap()
                .iter()
                .any(|c| c.secret == "  🔑秘密  ")
        );
        let again = vault.preview_import("repeat", "txt", "B").unwrap();
        vault
            .apply_import(&again.token, "repeat", "txt", "B")
            .unwrap();
        let candidate = vault.candidates(Some("B"), None).unwrap().remove(0);
        assert_eq!(candidate.secret, "repeat");
        assert_eq!(
            vault
                .views()
                .unwrap()
                .into_iter()
                .find(|v| v.id == candidate.id)
                .unwrap()
                .groups,
            ["A", "B"]
        );
    }
    #[cfg(windows)]
    #[test]
    fn concurrent_writers_do_not_lose_commits() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vault.dat");
        let vault = std::sync::Arc::new(Vault::open(&path).unwrap());
        let threads: Vec<_> = (0..12)
            .map(|index| {
                let vault = vault.clone();
                std::thread::spawn(move || {
                    let item = vault
                        .upsert(input(None, Some(&format!("synthetic-{index}"))))
                        .unwrap();
                    let ev = SuccessEvent {
                        event_id: format!("event-{index}"),
                        password_id: item.id,
                        password_revision: item.revision,
                        fingerprint: None,
                    };
                    vault.record_success(&ev).unwrap();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(vault.views().unwrap().len(), 12);
        assert!(vault.views().unwrap().iter().all(|v| v.success_count == 1));
        drop(vault);
        assert_eq!(Vault::open(&path).unwrap().views().unwrap().len(), 12);
    }
    #[cfg(windows)]
    #[test]
    fn editing_a_password_removes_its_old_fingerprint_priority() {
        let dir = tempdir().unwrap();
        let vault = Vault::open(&dir.path().join("vault.dat")).unwrap();
        let item = vault.upsert(input(None, Some("original-secret"))).unwrap();
        vault
            .record_success(&SuccessEvent {
                event_id: "original-success".into(),
                password_id: item.id.clone(),
                password_revision: 1,
                fingerprint: Some("old-archive".into()),
            })
            .unwrap();
        let mut competitor = input(None, Some("other-secret"));
        competitor.name = "A competitor".into();
        let competitor = vault.upsert(competitor).unwrap();
        assert_eq!(
            vault.candidates(None, Some("old-archive")).unwrap()[0].id,
            item.id
        );
        let changed = vault
            .upsert(input(Some(item.id), Some("changed-secret")))
            .unwrap();
        assert_eq!(changed.success_count, 0);
        assert_eq!(changed.revision, 2);
        assert_eq!(
            vault.candidates(None, Some("old-archive")).unwrap()[0].id,
            competitor.id
        );
    }
    #[cfg(windows)]
    #[test]
    fn portable_backup_wrong_password_and_hostile_kdf_leave_vault_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("vault.dat");
        let backup_path = dir.path().join("backup.json");
        let vault = Vault::open(&path).unwrap();
        vault
            .upsert(input(None, Some("synthetic-backup-secret")))
            .unwrap();
        let before = fs::read(&path).unwrap();
        assert!(vault.export_backup(&path, "backup phrase").is_err());
        vault.export_backup(&backup_path, "backup phrase").unwrap();
        assert!(vault.restore_backup(&backup_path, "wrong phrase").is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        let restored = Vault::open(&dir.path().join("restored.dat")).unwrap();
        assert_eq!(
            restored
                .restore_backup(&backup_path, "backup phrase")
                .unwrap()
                .added,
            1
        );
        assert_eq!(
            restored.candidates(None, None).unwrap()[0].secret,
            "synthetic-backup-secret"
        );
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&fs::read(&backup_path).unwrap()).unwrap();
        envelope["iterations"] = serde_json::json!(u32::MAX);
        fs::write(&backup_path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(vault.restore_backup(&backup_path, "backup phrase").is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[cfg(windows)]
    #[test]
    fn csv_import_preserves_password_whitespace_through_preview_and_encrypted_save() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("csv-spaces.dat");
        let vault = Vault::open(&path).unwrap();
        let csv = "名称,密码,分组\r\nQuoted,\"  合法 密码  \", Group \r\nPlain,  raw secret  ,Group\r\nSpaces,\"   \",Group\r\nQuotes,\" a\"\"b \",Group";
        let preview = vault.preview_import(csv, "csv", "").unwrap();
        assert_eq!(preview.new_count, 4);
        assert_eq!(preview.error_count, 0);
        let result = vault.apply_import(&preview.token, csv, "csv", "").unwrap();
        assert_eq!(result.added, 4);
        drop(vault);
        let vault = Vault::open(&path).unwrap();
        let candidates = vault.candidates(None, None).unwrap();
        for expected in ["  合法 密码  ", "  raw secret  ", "   ", " a\"b "] {
            assert!(
                candidates
                    .iter()
                    .any(|candidate| candidate.secret == expected)
            );
        }
        assert!(
            vault
                .views()
                .unwrap()
                .iter()
                .all(|entry| entry.groups == ["Group"])
        );
        let custom = "Title,\"  mapped password  \",Group";
        let preview = vault
            .preview_import(custom, "csv:name,secret,group", "")
            .unwrap();
        vault
            .apply_import(&preview.token, custom, "csv:name,secret,group", "")
            .unwrap();
        assert!(
            vault
                .candidates(None, None)
                .unwrap()
                .iter()
                .any(|c| c.secret == "  mapped password  ")
        );
    }

    #[cfg(windows)]
    #[test]
    fn csv_import_supports_auto_headers_quoted_fields_and_custom_column_mapping() {
        let dir = tempdir().unwrap();
        let vault = Vault::open(&dir.path().join("vault.dat")).unwrap();

        // 1. 带表头与双引号逗号字段的 CSV
        let csv_with_header = "名称,密码,分组,备注\n\"Archive, A\",\"pass,123\",\"Game;Anime\",\"Note \"\"quoted\"\"\"\nSiteB,sec456,,Simple";
        let preview = vault
            .preview_import(csv_with_header, "csv", "DefaultGrp")
            .unwrap();
        assert_eq!(preview.new_count, 2);
        assert_eq!(preview.error_count, 0);
        let res = vault
            .apply_import(&preview.token, csv_with_header, "csv", "DefaultGrp")
            .unwrap();
        assert_eq!(res.added, 2);

        let cands = vault.candidates(None, None).unwrap();
        assert!(
            cands
                .iter()
                .any(|c| c.secret == "pass,123" && c.label == "Archive, A")
        );
        assert!(
            cands
                .iter()
                .any(|c| c.secret == "sec456" && c.label == "SiteB")
        );

        // 2. 自定义列映射 (csv:secret=0,name=2,group=1)
        let custom_csv = "custom_sec_1,VIP,ItemOne\ncustom_sec_2,VIP,ItemTwo";
        let preview2 = vault
            .preview_import(custom_csv, "csv:secret=0,name=2,group=1", "")
            .unwrap();
        assert_eq!(preview2.new_count, 2);
        vault
            .apply_import(
                &preview2.token,
                custom_csv,
                "csv:secret=0,name=2,group=1",
                "",
            )
            .unwrap();
        let vip_cands = vault.candidates(Some("VIP"), None).unwrap();
        assert_eq!(vip_cands.len(), 2);
        assert!(
            vip_cands
                .iter()
                .any(|c| c.secret == "custom_sec_1" && c.label == "ItemOne")
        );
    }

    #[cfg(windows)]
    #[test]
    fn candidate_mutations_derive_bounded_variants_and_attribute_success() {
        let dir = tempdir().unwrap();
        let vault = Vault::open(&dir.path().join("vault.dat")).unwrap();
        let item = vault.upsert(input(None, Some("[pwd:My_Secret]"))).unwrap();

        let base = vault.candidates(None, None).unwrap();
        let mutations = derive_candidate_mutations(&base);
        assert!(!mutations.is_empty());
        assert!(mutations.len() <= 3);
        assert!(mutations.iter().any(|m| m.secret == "My_Secret"));

        // 变体命中应归因回原始条目
        let variant_id = &mutations[0].id;
        vault
            .record_success(&SuccessEvent {
                event_id: "mut-hit-1".into(),
                password_id: variant_id.clone(),
                password_revision: item.revision,
                fingerprint: Some("fp-1".into()),
            })
            .unwrap();
        assert_eq!(vault.views().unwrap()[0].success_count, 1);
    }

    #[test]
    fn test_derive_candidate_mutations_pure_rules() {
        let base = vec![
            Candidate {
                id: "cand-1".into(),
                revision: 1,
                label: "Domain".into(),
                secret: "example.com".into(),
            },
            Candidate {
                id: "cand-2".into(),
                revision: 1,
                label: "Spaced".into(),
                secret: "123 456".into(),
            },
            Candidate {
                id: "cand-3".into(),
                revision: 1,
                label: "Tagged".into(),
                secret: "【解压密码: secret_code!】".into(),
            },
        ];
        let mutations = derive_candidate_mutations(&base);
        assert!(!mutations.is_empty());

        let secrets: Vec<&str> = mutations.iter().map(|m| m.secret.as_str()).collect();
        // Domain should derive www. variant
        assert!(secrets.contains(&"www.example.com"));
        // Spaced should derive space-stripped variant
        assert!(secrets.contains(&"123456"));
        // Tagged should derive stripped variant
        assert!(secrets.contains(&"secret_code"));
    }
}
