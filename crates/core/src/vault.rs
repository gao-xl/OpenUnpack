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

use crate::types::{
    Candidate, ImportPreview, ImportResult, ImportRow, PasswordInput, PasswordView, SuccessEvent,
};

#[cfg(not(windows))]
const ERR_UNSUPPORTED: &str = "Unsupported platform";
const ERR_MUTEX_POISONED: &str = "Vault mutex poisoned";
const ERR_VAULT_CORRUPT: &str = "Vault storage is corrupted or unreadable";
const ERR_RECORD_NOT_FOUND: &str = "Password entry not found";
const ERR_INVALID_BACKUP: &str = "Invalid backup archive or incorrect passphrase";
const ERR_INVALID_TOKEN: &str = "Invalid or expired import preview token";
const ERR_BACKUP_TOO_LARGE: &str = "Backup payload exceeds maximum allowed size";

const BACKUP_MIN_ITER: u32 = 100_000;
const BACKUP_MAX_ITER: u32 = 2_000_000;
const BACKUP_DEFAULT_ITER: u32 = 600_000;
const MAX_BACKUP_PAYLOAD_SIZE: usize = 64 * 1024 * 1024; // 64 MiB

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

        let encrypted_bytes = read_bounded(path, MAX_BACKUP_PAYLOAD_SIZE)?;
        if encrypted_bytes.is_empty() {
            return Err(ERR_VAULT_CORRUPT.to_string());
        }

        let plaintext = Zeroizing::new(dpapi_decrypt(&encrypted_bytes)?);
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

    pub fn upsert(&self, input: PasswordInput) -> Result<PasswordView, String> {
        validate_metadata(&input.name,&input.groups,&input.notes)?;
        if let Some(secret)=&input.secret {validate_secret(secret)?;}
        let _writer = self
            .writer
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;
        let now = Utc::now().to_rfc3339();
        let trimmed_name = input.name.trim().to_string();
        let cleaned_groups = clean_groups(&input.groups);
        let cleaned_notes = input.notes;

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

            if let Some(mut new_secret) = input.secret {
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
            let secret_val = input
                .secret
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

        for ids in cloned.fingerprint_order.values_mut(){ids.retain(|entry|entry!=id);}

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
        let fp_rank:BTreeMap<&str,usize> = fp_priority.iter().enumerate().map(|(rank,id)|(id.as_str(),rank)).collect();

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

        if let Some(item) = cloned.items.get_mut(&event.password_id) {
            if !item.deleted && item.revision == event.password_revision {
                item.success_count = item.success_count.saturating_add(1);
                item.last_success = Some(Utc::now().to_rfc3339());
                item.updated_at = Utc::now().to_rfc3339();

                if let Some(ref fp) = event.fingerprint {
                    let list = cloned.fingerprint_order.entry(fp.clone()).or_default();
                    list.retain(|id| id != &event.password_id);
                    list.insert(0, event.password_id.clone());
                }
            }
        }

        cloned.seen_events.insert(event.event_id.clone());
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
        validate_import_input(text,format,group)?;
        let parsed_records = parse_import_text(text, format, group);
        let guard = self
            .data
            .lock()
            .map_err(|_| ERR_MUTEX_POISONED.to_string())?;

        let mut existing_secrets = BTreeSet::new();
        for item in guard.items.values() {
            if !item.deleted {
                existing_secrets.insert(item.secret.clone());
            }
        }

        let mut seen_in_batch = BTreeSet::new();
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

            if rec.error {
                error_count += 1;
                rows.push(ImportRow {
                    line: rec.line,
                    name: row_name,
                    group: rec.groups.first().cloned().unwrap_or_default(),
                    status: "error".to_string(),
                    message: rec.message,
                });
            } else if existing_secrets.contains(&rec.secret) || seen_in_batch.contains(&rec.secret)
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
                new_count += 1;
                seen_in_batch.insert(rec.secret);
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
        validate_import_input(text,format,group)?;
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
            if !token_equal(token,&expected_token) {
                return Err(ERR_INVALID_TOKEN.to_string());
            }
            guard.clone()
        };

        let parsed_records = parse_import_text(text, format, group);
        if parsed_records.iter().any(|record|record.error) {
            return Err("导入内容存在无效行，请先修正预览中的错误。".into());
        }
        let mut added = 0;
        let mut duplicates = 0;
        let now = Utc::now().to_rfc3339();

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
                    item.groups = grp_set.into_iter().collect();
                    item.updated_at = now.clone();
                }
            } else {
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
        if passphrase.is_empty() {
            return Err("Backup passphrase required".to_string());
        }
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
        atomic_write_file(destination, &envelope_json)
    }

    fn decode_backup(path: &Path, passphrase: &str) -> Result<(BackupPayload,String), String> {
        if passphrase.is_empty()||passphrase.len()>4096{return Err(ERR_INVALID_BACKUP.into());}
        let envelope_bytes = read_bounded(path, MAX_BACKUP_PAYLOAD_SIZE * 2)
            .map_err(|_| ERR_INVALID_BACKUP.to_string())?;

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
        if backup_payload.items.len()>100_000{return Err(ERR_BACKUP_TOO_LARGE.into());}
        for item in &backup_payload.items {validate_secret(&item.secret).map_err(|_|ERR_INVALID_BACKUP)?;validate_metadata(&item.name,&item.groups,&item.notes).map_err(|_|ERR_INVALID_BACKUP)?;}
        Ok((backup_payload,hex::encode(Sha256::digest(&envelope_bytes))))
    }

    pub fn preview_restore_backup(&self,path:&Path,passphrase:&str)->Result<ImportPreview,String>{
        let (payload,hash)=Self::decode_backup(path,passphrase)?;
        let data=self.data.lock().map_err(|_|ERR_MUTEX_POISONED)?;
        let mut seen:BTreeSet<&str>=data.items.values().filter(|i|!i.deleted).map(|i|i.secret.as_str()).collect();
        let mut new_count=0;let mut duplicate_count=0;let mut rows=vec![];
        for (index,item) in payload.items.iter().enumerate(){let duplicate=!seen.insert(item.secret.as_str());if duplicate{duplicate_count+=1}else{new_count+=1}rows.push(ImportRow{line:index+1,name:item.name.clone(),group:clean_groups(&item.groups).join("、"),status:if duplicate{"duplicate"}else{"new"}.into(),message:if duplicate{"合并分组，保留现有名称、备注与启用状态。"}else{"新增密码记录。"}.into()});}
        Ok(ImportPreview{token:compute_preview_token(&hash,"backup","",data.generation,&self.preview_key),new_count,duplicate_count,error_count:0,rows})
    }

    pub fn confirm_restore_backup(&self,path:&Path,passphrase:&str,token:&str)->Result<ImportResult,String>{
        let (payload,hash)=Self::decode_backup(path,passphrase)?;
        self.merge_backup(payload,Some((&hash,token)))
    }

    pub fn restore_backup(&self,path:&Path,passphrase:&str)->Result<ImportResult,String>{
        let (payload,_)=Self::decode_backup(path,passphrase)?;
        self.merge_backup(payload,None)
    }

    fn merge_backup(&self,backup_payload:BackupPayload,confirmation:Option<(&str,&str)>)->Result<ImportResult,String>{
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
        if let Some((hash,token))=confirmation {let expected=compute_preview_token(hash,"backup","",cloned.generation,&self.preview_key);if !token_equal(&expected,token){return Err(ERR_INVALID_TOKEN.into());}}

        let mut added = 0;
        let mut duplicates = 0;
        let now = Utc::now().to_rfc3339();

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
                    item.groups = grp_set.into_iter().collect();
                    if b_item.success_count > item.success_count {
                        item.success_count = b_item.success_count;
                        item.last_success = b_item.last_success.clone();
                    }
                    item.updated_at = now.clone();
                }
            } else {
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
    pub fn bulk_action(&self,ids:&[String],action:&str,groups:&[String])->Result<(),String>{
        if ids.is_empty()||ids.len()>100_000||!matches!(action,"enable"|"disable"|"delete"|"group"){return Err("批量密码操作无效。".into());}
        validate_metadata("",groups,"")?;
        let _writer=self.writer.lock().map_err(|_|ERR_MUTEX_POISONED)?;
        let mut data=self.data.lock().map_err(|_|ERR_MUTEX_POISONED)?.clone();
        for id in ids {if data.items.get(id).is_none_or(|i|i.deleted){return Err(ERR_RECORD_NOT_FOUND.into());}}
        let clean=clean_groups(groups);let timestamp=Utc::now().to_rfc3339();
        for id in ids {let item=data.items.get_mut(id).ok_or(ERR_RECORD_NOT_FOUND)?;match action{"enable"=>item.enabled=true,"disable"=>item.enabled=false,"delete"=>{item.deleted=true;item.secret.zeroize();},"group"=>item.groups=clean.clone(),_=>unreachable!()};item.updated_at=timestamp.clone();}
        if action=="delete"{for entries in data.fingerprint_order.values_mut(){entries.retain(|i|!ids.contains(i));}}
        data.generation=data.generation.checked_add(1).ok_or("密码库版本计数已耗尽。")?;
        self.persist_internal(&data)?;*self.data.lock().map_err(|_|ERR_MUTEX_POISONED)?=data;Ok(())
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
        let encrypted = dpapi_encrypt(&raw)?;
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

fn validate_import_format(format: &str) -> Result<(), String> {
    if format.eq_ignore_ascii_case("txt") || format.eq_ignore_ascii_case("tsv") {
        Ok(())
    } else {
        Err("Unsupported import format".to_string())
    }
}

fn validate_import_input(text:&str,format:&str,group:&str)->Result<(),String>{
    validate_import_format(format)?;
    if text.len()>8*1024*1024 || text.lines().take(100_001).count()>100_000 {
        return Err("导入内容超过 8 MiB 或 100000 行限制。".into());
    }
    validate_metadata("",&[group.into()],"")
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

fn token_equal(a:&str,b:&str)->bool {if a.len()!=b.len(){return false;}a.bytes().zip(b.bytes()).fold(0u8,|sum,(x,y)|sum|(x^y))==0}
fn validate_secret(secret:&str)->Result<(),String>{if secret.is_empty()||secret.len()>4096||secret.contains(['\0','\r','\n']){Err("密码必须为1至4096字节的单行文本；前后空格会保留。".into())}else{Ok(())}}
fn validate_metadata(name:&str,groups:&[String],notes:&str)->Result<(),String>{if name.len()>1024||groups.len()>100||groups.iter().any(|g|g.len()>1024||g.contains(['\0','\r','\n']))||notes.len()>16384||name.contains(['\0','\r','\n']){Err("密码名称、分组或备注超出限制。".into())}else{Ok(())}}

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
    let is_tsv = format.eq_ignore_ascii_case("tsv");
    let mut results = Vec::new();
    let def_grp = default_group.trim();

    for (idx, line) in text.lines().enumerate() {
        let line_num = idx + 1;
        let line_raw = line.trim_end_matches(['\r', '\n']);

        if is_tsv {
            let parts: Vec<&str> = line_raw.split('\t').collect();
            if parts.is_empty() || (parts.len() == 1 && parts[0].trim().is_empty()) {
                continue;
            }

            if parts.len()!=4 {
                results.push(ParsedRecord {line:line_num,name:String::new(),secret:String::new(),groups:vec![],notes:String::new(),error:true,message:"TSV 必须恰好为四列：名称、密码、分组、备注。".into()});
                continue;
            }
            let (name,secret,grp_opt,notes)=(parts[0],parts[1],Some(parts[2]),parts[3]);

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

            results.push(ParsedRecord {
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

            results.push(ParsedRecord {
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

    for record in &mut results {
        if !record.error {
            if let Err(message)=validate_secret(&record.secret).and_then(|_|validate_metadata(&record.name,&record.groups,&record.notes)) {
                record.error=true;record.message=message;
                record.secret.zeroize();record.name.clear();record.groups.clear();record.notes.clear();
            }
        }
    }
    results
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
#[cfg(not(windows))]
fn dpapi_encrypt(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err(ERR_UNSUPPORTED.to_string())
}

#[cfg(not(windows))]
fn dpapi_decrypt(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err(ERR_UNSUPPORTED.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
}
