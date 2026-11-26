use super::*;
use openunpack_infrastructure::paths::{
    path_eq_case_insensitive, path_starts_with_case_insensitive,
};
use openunpack_local_api::store::ScopeInfo;
use openunpack_local_api::{
    ApiBackend, ApiClient, ApiClientInput, ApiCommand, ApiError, ApiSettings, ApiStatus, ApiStore,
    BackendFuture, CreatedClient, LocalApiServer, Scope,
};
use serde_json::Value;

pub struct ApiManager {
    store: Result<Arc<ApiStore>, String>,
    server: tokio::sync::Mutex<Option<LocalApiServer>>,
    error: Mutex<Option<String>>,
}
impl ApiManager {
    pub fn open(data: &Path) -> Arc<Self> {
        let store = ApiStore::open(&data.join("local-api.json")).map(Arc::new);
        Arc::new(Self {
            store,
            server: tokio::sync::Mutex::new(None),
            error: Mutex::new(None),
        })
    }
    pub fn store(&self) -> Result<Arc<ApiStore>, String> {
        self.store.clone()
    }
    pub async fn status(&self) -> Result<ApiStatus, String> {
        let (settings, clients, error) = match &self.store {
            Ok(store) => (
                store.settings()?,
                store.clients()?,
                self.error.lock().map_err(|_| "API 状态不可用。")?.clone(),
            ),
            Err(error) => (ApiSettings::default(), vec![], Some(error.clone())),
        };
        let server = self.server.lock().await;
        let running = server.as_ref().is_some_and(LocalApiServer::is_running);
        let base = running.then(|| {
            format!(
                "http://127.0.0.1:{}",
                server.as_ref().expect("running server").port
            )
        });
        Ok(ApiStatus {
            settings,
            running,
            base_url: base.as_ref().map(|v| format!("{v}/api/v1")),
            docs_url: base.map(|v| format!("{v}/docs")),
            error,
            clients,
            permissions: ScopeInfo::all(),
        })
    }
    pub async fn configure(
        &self,
        app: tauri::AppHandle,
        settings: ApiSettings,
    ) -> Result<ApiStatus, String> {
        self.configure_internal(Some(app), None, settings).await
    }
    pub async fn configure_standalone(
        &self,
        state: Arc<AppState>,
        settings: ApiSettings,
    ) -> Result<ApiStatus, String> {
        self.configure_internal(None, Some(state), settings).await
    }
    async fn configure_internal(
        &self,
        app: Option<tauri::AppHandle>,
        standalone_state: Option<Arc<AppState>>,
        settings: ApiSettings,
    ) -> Result<ApiStatus, String> {
        let store = self.store()?;
        store.save_settings(settings.clone())?;
        let mut server = self.server.lock().await;
        if let Some(previous) = server.take() {
            previous.shutdown().await;
        }
        *self.error.lock().map_err(|_| "API 状态不可用。")? = None;
        if settings.enabled {
            let backend = Arc::new(DesktopBackend {
                app,
                standalone_state,
                store: store.clone(),
            });
            match LocalApiServer::start(store, backend).await {
                Ok(started) => *server = Some(started),
                Err(error) => {
                    *self.error.lock().map_err(|_| "API 状态不可用。")? = Some(error.clone());
                    return Err(error);
                }
            }
        }
        drop(server);
        self.status().await
    }
    pub async fn restore(&self, app: tauri::AppHandle) {
        if let Ok(store) = self.store()
            && let Ok(settings) = store.settings()
            && settings.enabled
        {
            let _ = self.configure(app, settings).await;
        }
    }
    pub async fn restore_standalone(&self, state: Arc<AppState>) {
        if let Ok(store) = self.store() {
            let mut settings = store.settings().unwrap_or_default();
            if !settings.enabled {
                settings.enabled = true;
                let _ = store.save_settings(settings.clone());
            }
            let _ = self.configure_standalone(state, settings).await;
        }
    }
}

#[tauri::command]
pub async fn api_status(
    window: WebviewWindow,
    manager: State<'_, Arc<ApiManager>>,
) -> Result<ApiStatus, String> {
    verify_frontend_window(&window)?;
    manager.status().await
}
#[tauri::command]
pub async fn api_configure(
    window: WebviewWindow,
    app: tauri::AppHandle,
    manager: State<'_, Arc<ApiManager>>,
    settings: ApiSettings,
) -> Result<ApiStatus, String> {
    verify_frontend_window(&window)?;
    manager.configure(app, settings).await
}
#[tauri::command]
pub async fn api_create_client(
    window: WebviewWindow,
    state: State<'_, AppState>,
    manager: State<'_, Arc<ApiManager>>,
    mut input: ApiClientInput,
) -> Result<CreatedClient, String> {
    verify_frontend_window(&window)?;
    if !openunpack_local_api::is_trusted_all_paths() {
        let access = state.access.lock().map_err(|_| "目录授权不可用。")?;
        let data = state
            .coordinator
            .data
            .canonicalize()
            .map_err(|_| "应用数据目录不可用。")?;
        for (paths, write) in [
            (&mut input.read_roots, false),
            (&mut input.write_roots, true),
        ] {
            for path in paths {
                let canonical = canonicalize_existing(path)?;
                if !canonical.is_dir()
                    || canonical.starts_with(&data)
                    || (write && !access.is_writable_dir(&canonical))
                    || (!write
                        && !access.is_readable(&canonical)
                        && !access.is_writable_dir(&canonical))
                {
                    return Err("请通过本机目录选择器批准 API 使用的目录。".into());
                }
                *path = canonical.to_string_lossy().into_owned();
            }
        }
    }
    manager.store()?.create_client(input)
}
#[tauri::command]
pub async fn api_revoke_client(
    window: WebviewWindow,
    manager: State<'_, Arc<ApiManager>>,
    id: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    manager.store()?.revoke_client(&id)
}
#[tauri::command]
pub async fn api_open_docs(
    window: WebviewWindow,
    manager: State<'_, Arc<ApiManager>>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let url = manager
        .status()
        .await?
        .docs_url
        .ok_or("请先启用本机 API。")?;
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .ok_or("系统目录不可用。")?;
    Command::new(root.join("explorer.exe"))
        .current_dir(&root)
        .arg(url)
        .spawn()
        .map_err(|_| "无法打开本机 API 文档。")?;
    Ok(())
}

mod backend;
mod policy;
mod projection;

use backend::*;
use policy::*;
use projection::*;

#[cfg(test)]
mod authorization_tests;

#[cfg(all(test, windows))]
mod native_tests;
