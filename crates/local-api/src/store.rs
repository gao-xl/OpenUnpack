use crate::Scope;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use utoipa::ToSchema;
use zeroize::Zeroizing;

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiSettings {
    pub enabled: bool,
    pub port: u16,
}
impl Default for ApiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 47980,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiClientInput {
    pub name: String,
    pub scopes: BTreeSet<Scope>,
    #[serde(default)]
    pub read_roots: Vec<String>,
    #[serde(default)]
    pub write_roots: Vec<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiClient {
    pub id: String,
    pub name: String,
    pub scopes: BTreeSet<Scope>,
    pub read_roots: Vec<String>,
    pub write_roots: Vec<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
}
pub type ApiClientView = ApiClient;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedClient {
    pub client: ApiClientView,
    pub token: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiStatus {
    pub settings: ApiSettings,
    pub running: bool,
    pub base_url: Option<String>,
    pub docs_url: Option<String>,
    pub error: Option<String>,
    pub clients: Vec<ApiClientView>,
    pub permissions: Vec<ScopeInfo>,
}
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ScopeInfo {
    pub scope: Scope,
    pub label: String,
    pub sensitive: bool,
}
impl ScopeInfo {
    pub fn all() -> Vec<Self> {
        Scope::ALL
            .into_iter()
            .map(|scope| Self {
                scope,
                label: scope.label().into(),
                sensitive: matches!(
                    scope,
                    Scope::VaultReveal | Scope::FilesDelete | Scope::DesktopInteractive
                ),
            })
            .collect()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredClient {
    client: ApiClient,
    token_hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoreData {
    version: u32,
    settings: ApiSettings,
    clients: Vec<StoredClient>,
}

struct ThrottleState {
    failed_attempts: u32,
    last_failure_time: std::time::Instant,
}

pub struct ApiStore {
    path: PathBuf,
    data: Mutex<StoreData>,
    throttle: Mutex<ThrottleState>,
}
impl ApiStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        let parent = path.parent().ok_or("API 配置目录不可用。")?;
        if !parent.is_dir() {
            return Err("API 配置目录不存在。".into());
        }
        let data = if path.exists() {
            let metadata = fs::symlink_metadata(path).map_err(|_| "API 配置不可读取。")?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > 1024 * 1024
            {
                return Err("API 配置不是有效的普通文件。".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err("API 配置不能是重解析点。".into());
                }
            }
            let file = fs::File::open(path).map_err(|_| "API 配置不可读取。")?;
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "API 配置读取失败。")?;
            if bytes.len() > 1024 * 1024 {
                return Err("API 配置过大。".into());
            }
            let parsed: StoreData =
                serde_json::from_slice(&bytes).map_err(|_| "API 配置损坏，原文件已保留。")?;
            if parsed.version != 1 || parsed.settings.port == 0 || parsed.clients.len() > 32 {
                return Err("API 配置版本或内容无效。".into());
            }
            let mut ids = BTreeSet::new();
            for stored in &parsed.clients {
                if !valid_id(&stored.client.id)
                    || !ids.insert(&stored.client.id)
                    || stored.token_hash.len() != 64
                    || hex::decode(&stored.token_hash).is_err()
                {
                    return Err("API 应用授权内容无效。".into());
                }
                validate_input(&ApiClientInput {
                    name: stored.client.name.clone(),
                    scopes: stored.client.scopes.clone(),
                    read_roots: stored.client.read_roots.clone(),
                    write_roots: stored.client.write_roots.clone(),
                    expires_at: stored.client.expires_at.clone(),
                })?;
            }
            parsed
        } else {
            StoreData {
                version: 1,
                settings: ApiSettings::default(),
                clients: vec![],
            }
        };
        Ok(Self {
            path: path.to_owned(),
            data: Mutex::new(data),
            throttle: Mutex::new(ThrottleState {
                failed_attempts: 0,
                last_failure_time: std::time::Instant::now(),
            }),
        })
    }

    pub fn settings(&self) -> Result<ApiSettings, String> {
        Ok(self
            .data
            .lock()
            .map_err(|_| "API 配置不可用。")?
            .settings
            .clone())
    }
    pub fn clients(&self) -> Result<Vec<ApiClientView>, String> {
        Ok(self
            .data
            .lock()
            .map_err(|_| "API 配置不可用。")?
            .clients
            .iter()
            .map(|v| v.client.clone())
            .collect())
    }
    pub fn save_settings(&self, settings: ApiSettings) -> Result<(), String> {
        if settings.port == 0 {
            return Err("API 端口必须为 1 至 65535。".into());
        }
        let mut current = self.data.lock().map_err(|_| "API 配置不可用。")?;
        let mut next = current.clone();
        next.settings = settings;
        self.persist(&next)?;
        *current = next;
        Ok(())
    }
    pub fn create_client(&self, input: ApiClientInput) -> Result<CreatedClient, String> {
        validate_input(&input)?;
        let mut current = self.data.lock().map_err(|_| "API 配置不可用。")?;
        if current.clients.len() >= 32 {
            return Err("最多允许 32 个第三方应用，请先撤销不需要的应用。".into());
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let mut random = Zeroizing::new([0u8; 32]);
        rand::thread_rng().fill_bytes(&mut *random);
        let token = format!("oup_{id}_{}", hex::encode(random.as_slice()));
        let client = ApiClient {
            id,
            name: input.name.trim().into(),
            scopes: input.scopes,
            read_roots: input.read_roots,
            write_roots: input.write_roots,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "系统时间不可用。")?
                .as_secs()
                .to_string(),
            expires_at: input.expires_at,
            last_used_at: None,
        };
        let mut next = current.clone();
        next.clients.push(StoredClient {
            client: client.clone(),
            token_hash: digest(&token),
        });
        self.persist(&next)?;
        *current = next;
        Ok(CreatedClient { client, token })
    }
    pub fn revoke_client(&self, id: &str) -> Result<(), String> {
        let mut current = self.data.lock().map_err(|_| "API 配置不可用。")?;
        let mut next = current.clone();
        next.clients.retain(|v| v.client.id != id);
        if next.clients.len() == current.clients.len() {
            return Err("第三方应用不存在。".into());
        }
        self.persist(&next)?;
        *current = next;
        Ok(())
    }
    pub fn throttle_delay(&self) -> Option<std::time::Duration> {
        let th = self.throttle.lock().ok()?;
        if th.failed_attempts > 5 {
            let delay_ms = std::cmp::min((th.failed_attempts - 5) * 5, 200);
            Some(std::time::Duration::from_millis(delay_ms as u64))
        } else {
            None
        }
    }
    pub fn authenticate(&self, token: &str) -> Option<ApiClient> {
        let record_failure = || {
            if let Ok(mut th) = self.throttle.lock() {
                let now = std::time::Instant::now();
                if now.duration_since(th.last_failure_time).as_secs() > 10 {
                    th.failed_attempts = 1;
                } else {
                    th.failed_attempts = th.failed_attempts.saturating_add(1);
                }
                th.last_failure_time = now;
            }
        };

        if token.len() != 101 || !token.starts_with("oup_") {
            record_failure();
            return None;
        }
        let id = token.get(4..36)?;
        if token.as_bytes().get(36) != Some(&b'_')
            || !valid_id(id)
            || !token[37..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            record_failure();
            return None;
        }
        let mut guard = self.data.lock().ok()?;
        if !guard.settings.enabled {
            return None;
        }
        let stored = match guard.clients.iter_mut().find(|v| v.client.id == id) {
            Some(client) => client,
            None => {
                record_failure();
                return None;
            }
        };
        let now_sec = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        if !client_is_current(&stored.client, now_sec) {
            record_failure();
            return None;
        }
        let actual = digest(token);
        let difference = stored
            .token_hash
            .bytes()
            .zip(actual.bytes())
            .fold(0u8, |all, (left, right)| all | (left ^ right));
        if difference == 0 {
            if let Ok(mut th) = self.throttle.lock() {
                th.failed_attempts = 0;
            }
            stored.client.last_used_at = Some(now_sec.to_string());
            Some(stored.client.clone())
        } else {
            record_failure();
            None
        }
    }
    pub fn active_client(&self, id: &str) -> bool {
        self.current_client(id).is_some()
    }
    /// Refresh continuing authorization instead of trusting a handshake-time copy.
    pub fn current_client(&self, id: &str) -> Option<ApiClient> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        let data = self.data.lock().ok()?;
        if !data.settings.enabled {
            return None;
        }
        data.clients
            .iter()
            .find(|entry| entry.client.id == id && client_is_current(&entry.client, now))
            .map(|entry| entry.client.clone())
    }
    /// (Deprecated - SEC-02) Formerly used by native pickers. Direct grant inflation
    /// is discontinued to prevent scope escalation attacks.
    #[deprecated(note = "Picker grants are no longer automatically persisted to API token scopes")]
    pub fn add_picker_grants(
        &self,
        id: &str,
        reads: &[String],
        writes: &[String],
    ) -> Result<(), String> {
        let mut current = self.data.lock().map_err(|_| "API 配置不可用。")?;
        let mut next = current.clone();
        let client = &mut next
            .clients
            .iter_mut()
            .find(|v| v.client.id == id)
            .ok_or("应用授权已撤销。")?
            .client;
        for path in reads {
            if !client.read_roots.contains(path) {
                client.read_roots.push(path.clone());
            }
        }
        for path in writes {
            if !client.write_roots.contains(path) {
                client.write_roots.push(path.clone());
            }
        }
        validate_input(&ApiClientInput {
            name: client.name.clone(),
            scopes: client.scopes.clone(),
            read_roots: client.read_roots.clone(),
            write_roots: client.write_roots.clone(),
            expires_at: client.expires_at.clone(),
        })?;
        self.persist(&next)?;
        *current = next;
        Ok(())
    }
    fn persist(&self, value: &StoreData) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|_| "API 配置编码失败。")?;
        if bytes.len() > 1024 * 1024 {
            return Err("API 配置过大。".into());
        }
        let temporary = self
            .path
            .with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
            return Err("API 配置保存失败，原授权已保留。".into());
        }
        Ok(())
    }
}
fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|v| v.is_ascii_hexdigit())
}
pub fn validate_input(input: &ApiClientInput) -> Result<(), String> {
    if input.name.trim().is_empty()
        || input.name.len() > 128
        || input.name.chars().any(char::is_control)
        || input.scopes.is_empty()
        || input.read_roots.len() > 32
        || input.write_roots.len() > 32
        || input.read_roots.iter().chain(&input.write_roots).any(|v| {
            v.is_empty()
                || v.len() > 32768
                || v.chars().any(char::is_control)
                || !Path::new(v).is_absolute()
        })
    {
        return Err("请填写应用名称、权限及有效的本机目录。".into());
    }
    if let Some(exp) = &input.expires_at
        && (exp.trim().is_empty() || exp.parse::<u64>().is_err())
    {
        return Err("过期时间必须是有效的 Unix 秒级时间戳。".into());
    }
    Ok(())
}

fn client_is_current(client: &ApiClient, now: u64) -> bool {
    client
        .expires_at
        .as_ref()
        .is_none_or(|expires| expires.parse::<u64>().is_ok_and(|deadline| now < deadline))
}
