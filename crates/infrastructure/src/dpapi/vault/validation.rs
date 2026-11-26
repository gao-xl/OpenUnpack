//! 密码库容量、导入格式、预览令牌及元数据校验辅助函数。

use super::*;

pub(super) fn clean_groups(groups: &[String]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for g in groups {
        let trimmed = g.trim();
        if !trimmed.is_empty() {
            set.insert(trimmed.to_string());
        }
    }
    set.into_iter().collect()
}

pub(super) fn active_item_count(data: &VaultData) -> usize {
    data.items.values().filter(|item| !item.deleted).count()
}

pub(super) fn vault_capacity_message() -> String {
    format!("密码库最多支持 {MAX_VAULT_ITEMS} 条有效记录，请先删除不需要的记录。")
}

pub(super) fn validate_new_item_capacity(current: usize) -> Result<(), String> {
    if current >= MAX_VAULT_ITEMS {
        Err(vault_capacity_message())
    } else {
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct SerializedSize(usize);

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

pub(super) fn serialized_size(value: &impl Serialize) -> Result<usize, String> {
    let mut size = SerializedSize::default();
    serde_json::to_writer(&mut size, value).map_err(|_| ERR_BACKUP_TOO_LARGE.to_string())?;
    Ok(size.0)
}

// A portable backup has fewer fields than local storage. Bound the size of an
// empty-vault restore as well, so a successful export cannot exceed the local
// reader after UUIDs and timestamps have been added. Duplicates/normalization
// only reduce this upper bound; it needs no encryption or second KDF.
pub(super) fn validate_empty_restore_size(payload: &BackupPayload) -> Result<(), String> {
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

pub(super) fn validate_import_format(format: &str) -> Result<(), String> {
    let lower = format.trim().to_ascii_lowercase();
    if lower == "txt" || lower == "tsv" || lower == "csv" || lower.starts_with("csv:") {
        Ok(())
    } else {
        Err("Unsupported import format".to_string())
    }
}

pub(super) fn validate_import_input(text: &str, format: &str, group: &str) -> Result<(), String> {
    validate_import_format(format)?;
    if text.len() > 8 * 1024 * 1024
        || text.lines().take(MAX_VAULT_ITEMS + 1).count() > MAX_VAULT_ITEMS
    {
        return Err("导入内容超过 8 MiB 或 100000 行限制。".into());
    }
    validate_metadata("", &[group.into()], "")
}

pub(super) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
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

pub(super) fn compute_preview_token(
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

pub(super) fn token_equal(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |sum, (x, y)| sum | (x ^ y))
        == 0
}
pub(super) fn validate_backup_passphrase(passphrase: &str) -> Result<(), String> {
    if passphrase.is_empty() || passphrase.len() > MAX_BACKUP_PASSPHRASE_BYTES {
        Err("备份口令必须为1至4096字节；Unicode和前后空格会保留。".into())
    } else {
        Ok(())
    }
}
pub(super) fn validate_secret(secret: &str) -> Result<(), String> {
    if secret.is_empty() || secret.len() > 4096 || secret.contains(['\0', '\r', '\n']) {
        Err("密码必须为1至4096字节的单行文本；前后空格会保留。".into())
    } else {
        Ok(())
    }
}
pub(super) fn validate_metadata(name: &str, groups: &[String], notes: &str) -> Result<(), String> {
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
