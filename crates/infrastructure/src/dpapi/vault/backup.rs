//! 密码库加密备份导出、恢复预览与恢复确认。

use super::*;

impl Vault {
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

    pub(super) fn decode_backup(
        path: &Path,
        passphrase: &str,
    ) -> Result<(BackupPayload, String), String> {
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

    pub(super) fn merge_backup(
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
        let now = now_rfc3339();
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
}
