use crate::{ApiClient, ApiCommand, ApiStore, Scope};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use openunpack_protocol::StateInvalidated;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    future::Future,
    net::Ipv4Addr,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, broadcast, oneshot},
};
use utoipa::{
    Modify, OpenApi, ToSchema,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};

mod events;
use events::{SocketTicket, events as event_stream, ws_handler, ws_ticket};

pub type BackendFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, ApiError>> + Send + 'a>>;
pub trait ApiBackend: Send + Sync + 'static {
    fn execute(&self, command: ApiCommand, client: ApiClient) -> BackendFuture<'_>;
    fn subscribe(&self) -> broadcast::Receiver<StateInvalidated>;
    fn subscribe_progress(&self) -> broadcast::Receiver<openunpack_protocol::JobProgressUpdate> {
        let (_, rx) = broadcast::channel(1);
        rx
    }
    fn subscribe_need_input(&self) -> broadcast::Receiver<openunpack_protocol::NeedPasswordEvent> {
        let (_, rx) = broadcast::channel(1);
        rx
    }
    /// Conservative defaults never disclose paths through a new transport.
    fn project_progress(
        &self,
        mut update: openunpack_protocol::JobProgressUpdate,
        _client: &ApiClient,
    ) -> Option<openunpack_protocol::JobProgressUpdate> {
        update.current_file = None;
        Some(update)
    }
    fn project_need_input(
        &self,
        mut event: openunpack_protocol::NeedPasswordEvent,
        _client: &ApiClient,
    ) -> Option<openunpack_protocol::NeedPasswordEvent> {
        event.archive_path.clear();
        Some(event)
    }
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip)]
    #[schema(ignore)]
    pub status: u16,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_fields: Option<Vec<crate::FieldError>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u32>,
}
impl ApiError {
    pub fn new(status: u16, code: &str, message: &str) -> Self {
        let (error_type, title) = match status {
            400 => (
                "https://openunpack.local/errors/invalid-request",
                "Invalid Request",
            ),
            401 => (
                "https://openunpack.local/errors/unauthorized",
                "Unauthorized",
            ),
            403 => (
                "https://openunpack.local/errors/permission-denied",
                "Permission Denied",
            ),
            404 => ("https://openunpack.local/errors/not-found", "Not Found"),
            405 => (
                "https://openunpack.local/errors/method-not-allowed",
                "Method Not Allowed",
            ),
            413 => (
                "https://openunpack.local/errors/request-too-large",
                "Request Too Large",
            ),
            429 => (
                "https://openunpack.local/errors/rate-limit-exceeded",
                "Too Many Requests",
            ),
            503 => (
                "https://openunpack.local/errors/service-unavailable",
                "Service Unavailable",
            ),
            _ => (
                "https://openunpack.local/errors/internal-error",
                "Internal Server Error",
            ),
        };
        Self {
            status,
            code: code.into(),
            message: message.into(),
            error_type: Some(error_type.into()),
            title: Some(title.into()),
            invalid_fields: None,
            retry_after_secs: None,
        }
    }
    pub fn validation_failed(errors: Vec<crate::FieldError>) -> Self {
        let summary = errors
            .first()
            .map(|e| format!("{}: {}", e.field, e.reason))
            .unwrap_or_else(|| "请求参数验证未通过。".into());
        Self {
            status: 400,
            code: "invalid_request".into(),
            message: summary,
            error_type: Some("https://openunpack.local/errors/validation-failed".into()),
            title: Some("Validation Failed".into()),
            invalid_fields: Some(errors),
            retry_after_secs: None,
        }
    }
    pub fn rate_limited(message: &str, retry_after: u32) -> Self {
        Self {
            status: 429,
            code: "busy".into(),
            message: message.into(),
            error_type: Some("https://openunpack.local/errors/rate-limit-exceeded".into()),
            title: Some("Too Many Requests".into()),
            invalid_fields: None,
            retry_after_secs: Some(retry_after),
        }
    }
    pub fn denied() -> Self {
        Self::new(
            403,
            "permission_denied",
            "此应用没有执行该操作或访问该目录的权限。",
        )
    }
    pub fn permission_denied(message: &str) -> Self {
        Self::new(403, "permission_denied", message)
    }
    pub fn operation(message: &str) -> Self {
        Self::new(400, "operation_failed", message)
    }
    pub fn internal() -> Self {
        Self::new(
            500,
            "internal_error",
            "操作暂时不可用，请在 OpenUnpack 中查看状态。",
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let retry_after = self.retry_after_secs;
        let mut response = (
            status,
            Json(ApiErrorResponse {
                request_id: uuid::Uuid::new_v4().to_string(),
                error: self,
            }),
        )
            .into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response.headers_mut().insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        if let Some(secs) = retry_after
            && let Ok(value) = HeaderValue::from_str(&secs.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorResponse {
    pub request_id: String,
    pub error: ApiError,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResponseMeta {
    pub timestamp: i64,
    pub duration_ms: f64,
    pub api_version: u32,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse {
    pub request_id: String,
    pub data: Value,
    pub meta: ResponseMeta,
}
fn success(data: Value, duration: Duration) -> Json<ApiResponse> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let duration_ms = (duration.as_secs_f64() * 1000.0 * 100.0).round() / 100.0;
    Json(ApiResponse {
        request_id: uuid::Uuid::new_v4().to_string(),
        data,
        meta: ResponseMeta {
            timestamp,
            duration_ms,
            api_version: 1,
        },
    })
}

#[derive(Clone)]
struct ServerState {
    backend: Arc<dyn ApiBackend>,
    store: Arc<ApiStore>,
    port: u16,
    work: Arc<Semaphore>,
    events: Arc<Semaphore>,
    rejected_bodies: Arc<Semaphore>,
    closing: Arc<AtomicBool>,
    tickets: Arc<Mutex<HashMap<String, SocketTicket>>>,
    bodies: Arc<Semaphore>,
}

pub struct LocalApiServer {
    pub port: u16,
    address: std::net::SocketAddr,
    closing: Arc<AtomicBool>,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}
impl LocalApiServer {
    pub async fn start(store: Arc<ApiStore>, backend: Arc<dyn ApiBackend>) -> Result<Self, String> {
        let settings = store.settings()?;
        if !settings.enabled {
            return Err("请先启用本机 API。".into());
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, settings.port))
            .await
            .map_err(|_| "本机 API 端口无法使用，可能已被占用，请更换端口。")?;
        Self::from_listener(listener, store, backend)
    }
    fn from_listener(
        listener: TcpListener,
        store: Arc<ApiStore>,
        backend: Arc<dyn ApiBackend>,
    ) -> Result<Self, String> {
        let address = listener.local_addr().map_err(|_| "API 地址不可用。")?;
        if address.ip() != std::net::IpAddr::V4(Ipv4Addr::LOCALHOST) {
            return Err("API 只能监听 127.0.0.1。".into());
        }
        let closing = Arc::new(AtomicBool::new(false));
        let state = ServerState {
            backend,
            store,
            port: address.port(),
            work: Arc::new(Semaphore::new(16)),
            events: Arc::new(Semaphore::new(32)),
            rejected_bodies: Arc::new(Semaphore::new(64)),
            closing: closing.clone(),
            tickets: Arc::new(Mutex::new(HashMap::new())),
            bodies: Arc::new(Semaphore::new(64)),
        };
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router(state))
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await;
        });
        Ok(Self {
            port: address.port(),
            address,
            closing,
            stop: Some(stop),
            task,
        })
    }
    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.address
    }
    /// Confirm listener shutdown before a settings change reuses its port.
    /// Business jobs already accepted by the Coordinator have their own lifetime.
    pub async fn shutdown(mut self) {
        self.closing.store(true, Ordering::Release);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if tokio::time::timeout(Duration::from_millis(250), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}
impl Drop for LocalApiServer {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Release);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.task.abort();
    }
}

struct BearerSecurity;
impl Modify for BearerSecurity {
    fn modify(&self, api: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = api.components.as_mut() {
            components.add_security_scheme(
                "bearerAuth",
                SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()),
            );
        }
    }
}
#[derive(OpenApi)]
#[openapi(
    paths(execute, capabilities, health, events::events, events::ws_ticket),
    components(schemas(
        ApiCommand,
        ApiResponse,
        ResponseMeta,
        ApiErrorResponse,
        ApiError,
        crate::FieldError
    )),
    modifiers(&BearerSecurity),
    info(
        title = "OpenUnpack Local API",
        version = "1.0.0",
        description = "Authenticated local desktop operations. Use the API settings panel to create a client key and approve directories. All existing desktop functions are available through the typed command request. Interactive desktop operations may display a native dialog."
    )
)]
struct ApiDoc;

fn router(state: ServerState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/capabilities", get(capabilities))
        .route("/api/v1/commands", post(execute))
        .route("/api/v1/commands/{command}", post(execute_named))
        .route("/api/v1/events", get(event_stream))
        .route("/api/v1/ws-ticket", post(ws_ticket))
        .route("/api/v1/ws", get(ws_handler))
        .route("/docs", get(|| async { Redirect::to("/docs/") }))
        .route(
            "/docs/",
            get(|| async { Html(include_str!("../assets/swagger-ui/index.html")) }),
        )
        .route("/docs/{asset}", get(swagger_asset))
        .route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
        .route("/", get(serve_index))
        .route("/index.html", get(serve_index))
        .route("/assets/{asset}", get(serve_asset))
        .route("/favicon.ico", get(serve_favicon))
        .fallback(|| async { ApiError::new(404, "not_found", "接口不存在。") })
        .method_not_allowed_fallback(|| async {
            ApiError::new(405, "method_not_allowed", "此接口不支持该请求方法。")
        })
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// Document roots the daemon may serve. Everything here is either an explicit
/// operator opt-in or a directory shipped next to the executable; the current
/// working directory is deliberately excluded so that an HTML file dropped in
/// the launch directory cannot inherit the API origin and reach the token.
fn web_root_candidates(
    env_root: Option<&str>,
    exe_dir: Option<&std::path::Path>,
) -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = env_root {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            candidates.push(std::path::PathBuf::from(trimmed));
        }
    }
    if let Some(parent) = exe_dir {
        candidates.push(parent.join("web"));
    }
    #[cfg(debug_assertions)]
    {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        candidates.push(manifest_dir.join("../../apps/desktop-ui/dist"));
    }
    candidates
}

fn resolve_web_root() -> Option<std::path::PathBuf> {
    let env_root = std::env::var("OPENUNPACK_WEB_ROOT").ok();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf));
    web_root_candidates(env_root.as_deref(), exe_dir.as_deref())
        .into_iter()
        .find(|candidate| candidate.join("index.html").is_file())
}

async fn serve_index() -> Result<Response, ApiError> {
    if let Some(root) = resolve_web_root() {
        let index_file = root.join("index.html");
        if let Ok(content) = std::fs::read_to_string(&index_file) {
            let content = content.replacen(
                "<head>",
                "<head><meta name=\"openunpack-web-console\" content=\"1\">",
                1,
            );
            return Ok((
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                content,
            )
                .into_response());
        }
    }

    let fallback_html = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>OpenUnpack Web Console</title>
  <style>
    :root { --bg: #0b0f19; --card: #151d30; --text: #f1f5f9; --muted: #94a3b8; --accent: #38bdf8; --border: #1e293b; --green: #10b981; }
    * { box-sizing: border-box; }
    body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: var(--bg); color: var(--text); margin: 0; padding: 2rem; display: flex; justify-content: center; }
    .container { max-width: 860px; width: 100%; }
    .header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 2rem; }
    .brand { display: flex; align-items: center; gap: 0.75rem; }
    .brand svg { width: 32px; height: 32px; fill: var(--accent); }
    h1 { font-size: 1.5rem; margin: 0; font-weight: 700; letter-spacing: -0.025em; }
    .status-badge { display: inline-flex; align-items: center; gap: 0.4rem; padding: 0.35rem 0.75rem; border-radius: 9999px; font-size: 0.8rem; font-weight: 600; background: rgba(16, 185, 129, 0.15); color: var(--green); border: 1px solid rgba(16, 185, 129, 0.3); }
    .status-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--green); box-shadow: 0 0 8px var(--green); }
    .card { background: var(--card); border: 1px solid var(--border); border-radius: 12px; padding: 1.5rem; margin-bottom: 1.5rem; box-shadow: 0 10px 15px -3px rgba(0,0,0,0.3); }
    .card-title { font-size: 1.1rem; font-weight: 600; margin-top: 0; margin-bottom: 0.5rem; color: #fff; }
    .card-desc { color: var(--muted); font-size: 0.9rem; margin-bottom: 1rem; line-height: 1.5; }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 1rem; margin-bottom: 1.5rem; }
    .nav-box { background: rgba(255,255,255,0.03); border: 1px solid var(--border); border-radius: 8px; padding: 1rem; text-decoration: none; color: inherit; transition: all 0.2s; display: block; }
    .nav-box:hover { border-color: var(--accent); transform: translateY(-2px); }
    .nav-box-title { font-weight: 600; color: var(--accent); margin-bottom: 0.25rem; }
    .nav-box-desc { font-size: 0.8rem; color: var(--muted); }
    .token-box { display: flex; gap: 0.5rem; margin-top: 1rem; }
    input[type="text"] { flex: 1; background: #0b0f19; border: 1px solid var(--border); border-radius: 6px; padding: 0.5rem 0.75rem; color: #fff; font-family: monospace; font-size: 0.85rem; }
    input[type="text"]:focus { outline: none; border-color: var(--accent); }
    button { background: var(--accent); color: #0b0f19; border: none; border-radius: 6px; padding: 0.5rem 1rem; font-weight: 600; cursor: pointer; transition: opacity 0.2s; }
    button:hover { opacity: 0.9; }
    pre { background: #070a12; border: 1px solid var(--border); border-radius: 8px; padding: 1rem; overflow-x: auto; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.82rem; color: #7dd3fc; margin: 0; }
  </style>
</head>
<body>
  <div class="container">
    <div class="header">
      <div class="brand">
        <svg viewBox="0 0 24 24"><path d="M21 16.5c0 .38-.21.71-.53.88l-7.9 4.44c-.16.12-.36.18-.57.18s-.41-.06-.57-.18l-7.9-4.44A.991.991 0 0 1 3 16.5v-9c0-.38.21-.71.53-.88l7.9-4.44c.16-.12.36-.18.57-.18s.41.06.57.18l7.9 4.44c.32.17.53.5.53.88v9zM12 4.15L6.04 7.5 12 10.85l5.96-3.35L12 4.15z"/></svg>
        <h1>OpenUnpack Daemon</h1>
      </div>
      <div class="status-badge"><span class="status-dot"></span><span>Daemon Running</span></div>
    </div>
    <div class="card">
      <div class="card-title">开发者与第三方接入生态</div>
      <div class="card-desc">OpenUnpack Daemon 提供完整的无头后台服务。您可以通过 REST API、WebSocket 双向通信或 CLI 远程调度解压集群。</div>
      <div class="grid">
        <a class="nav-box" href="/docs/">
          <div class="nav-box-title">Swagger API 文档</div>
          <div class="nav-box-desc">实时交互式 OpenAPI 接口在线调试与参数预览。</div>
        </a>
        <a class="nav-box" href="/openapi.json">
          <div class="nav-box-title">OpenAPI 规范</div>
          <div class="nav-box-desc">标准 JSON 契约文件，一键导入 Postman 或生成 SDK。</div>
        </a>
        <a class="nav-box" href="/api/v1/health">
          <div class="nav-box-title">服务健康检查</div>
          <div class="nav-box-desc">GET /api/v1/health 快速探测服务存活状态。</div>
        </a>
      </div>
    </div>
    <div class="card">
      <div class="card-title">实时状态探查 (Snapshot)</div>
      <div class="card-desc">输入您的 API Client Token，即可在线读取解压队列与系统状态：</div>
      <div class="token-box">
        <input type="password" autocomplete="off" id="tokenInput" placeholder="输入 API Token (如 openunpack-token-...)">
        <button onclick="fetchSnapshot()">查询状态</button>
      </div>
      <div style="margin-top: 1rem;">
        <pre id="output">// 点击“查询状态”读取当前活动任务...</pre>
      </div>
    </div>
  </div>
  <script>
    const saved = sessionStorage.getItem('openunpack_token');
    if (saved) document.getElementById('tokenInput').value = saved;
    async function fetchSnapshot() {
      const token = document.getElementById('tokenInput').value.trim();
      if (token) sessionStorage.setItem('openunpack_token', token);
      const out = document.getElementById('output');
      out.textContent = '正在获取状态...';
      try {
        const headers = token ? { 'Authorization': 'Bearer ' + token } : {};
        const res = await fetch('/api/v1/commands', {
          method: 'POST',
          redirect: 'error',
          headers: { ...headers, 'Content-Type': 'application/json' },
          body: JSON.stringify({ command: 'queue_snapshot' })
        });
        const data = await res.json();
        out.textContent = JSON.stringify(data, null, 2);
      } catch (e) {
        out.textContent = '请求失败: ' + e.message;
      }
    }
  </script>
</body>
</html>"#;

    Ok((
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        fallback_html.as_bytes(),
    )
        .into_response())
}

async fn serve_asset(Path(asset): Path<String>) -> Result<Response, ApiError> {
    if asset.contains("..") || asset.contains('/') || asset.contains('\\') || asset.contains(':') {
        return Err(ApiError::new(400, "invalid_path", "非法的资源路径。"));
    }

    if let Some(root) = resolve_web_root() {
        let file_path = root.join("assets").join(&asset);
        if file_path.is_file()
            && let Ok(bytes) = std::fs::read(&file_path)
        {
            let mime = match asset.rsplit('.').next() {
                Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
                Some("css") => "text/css; charset=utf-8",
                Some("svg") => "image/svg+xml",
                Some("png") => "image/png",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("ico") => "image/x-icon",
                Some("woff") => "font/woff",
                Some("woff2") => "font/woff2",
                Some("json") => "application/json; charset=utf-8",
                _ => "application/octet-stream",
            };
            return Ok((
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                bytes,
            )
                .into_response());
        }
    }

    Err(ApiError::new(404, "not_found", "资源文件不存在。"))
}

async fn serve_favicon() -> Result<Response, ApiError> {
    if let Some(root) = resolve_web_root() {
        let ico = root.join("favicon.ico");
        if ico.is_file()
            && let Ok(bytes) = std::fs::read(&ico)
        {
            return Ok(([(header::CONTENT_TYPE, "image/x-icon")], bytes).into_response());
        }
    }
    const EMPTY_ICO: &[u8] = &[
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x20, 0x00, 0x68,
        0x04, 0x00, 0x00, 0x16, 0x00, 0x00, 0x00,
    ];
    Ok(([(header::CONTENT_TYPE, "image/x-icon")], EMPTY_ICO).into_response())
}

async fn swagger_asset(Path(asset): Path<String>) -> Result<Response, ApiError> {
    let (mime, bytes): (&str, &'static [u8]) = match asset.as_str() {
        "swagger-ui.css" => (
            "text/css; charset=utf-8",
            include_bytes!("../assets/swagger-ui/swagger-ui.css"),
        ),
        "swagger-ui-bundle.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../assets/swagger-ui/swagger-ui-bundle.js"),
        ),
        "swagger-ui-standalone-preset.js" => (
            "text/javascript; charset=utf-8",
            include_bytes!("../assets/swagger-ui/swagger-ui-standalone-preset.js"),
        ),
        _ => return Err(ApiError::new(404, "not_found", "文档资源不存在。")),
    };
    Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response())
}

fn allowed_origin(origin: &str, port: u16) -> bool {
    origin == format!("http://127.0.0.1:{port}")
        || origin == format!("http://localhost:{port}")
        || matches!(
            origin,
            "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost"
        )
}
fn response_headers(response: &mut Response, origin: Option<&str>) {
    let headers = response.headers_mut();
    if !headers.contains_key(header::CACHE_CONTROL) {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"));
    if let Some(origin) = origin.and_then(|v| HeaderValue::from_str(v).ok()) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
}
async fn guard(State(state): State<ServerState>, mut request: Request, next: Next) -> Response {
    if state.closing.load(Ordering::Acquire) {
        return reject_request(
            &state,
            request,
            ApiError::new(503, "unavailable", "API 正在停止，请稍后重试。"),
        )
        .await;
    }
    let path = request.uri().path().to_string();
    let is_ws = path == "/api/v1/ws";
    let address = format!("127.0.0.1:{}", state.port);
    let localhost = format!("localhost:{}", state.port);
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok());
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if request.headers().get_all(header::HOST).iter().count() != 1
        || !matches!(host, Some(v) if v == address || v == localhost)
        || request.headers().get_all(header::ORIGIN).iter().count() > 1
        || request.headers().contains_key(header::ORIGIN)
            && origin
                .as_deref()
                .is_none_or(|v| !allowed_origin(v, state.port))
    {
        return reject_request(&state, request, ApiError::denied()).await;
    }
    if request.method() == axum::http::Method::OPTIONS {
        let requested = request
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_METHOD)
            .and_then(|v| v.to_str().ok());
        if !matches!(requested, Some("GET" | "POST")) {
            return reject_request(&state, request, ApiError::denied()).await;
        }
        let mut response = StatusCode::NO_CONTENT.into_response();
        response_headers(&mut response, origin.as_deref());
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, OPTIONS"),
        );
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization, Content-Type"),
        );
        return response;
    }
    let public = request.method() == axum::http::Method::GET
        && matches!(
            path.as_str(),
            "/api/v1/health" | "/openapi.json" | "/docs" | "/" | "/index.html" | "/favicon.ico"
        )
        || request.method() == axum::http::Method::GET
            && (path.starts_with("/docs/") || path.starts_with("/assets/"));
    if !public {
        if request
            .headers()
            .get_all(header::AUTHORIZATION)
            .iter()
            .count()
            > 1
        {
            return reject_request(
                &state,
                request,
                ApiError::new(401, "unauthorized", "请提供唯一有效的 API 应用密钥。"),
            )
            .await;
        }
        let client = if let Some(query) = request.uri().query() {
            if !is_ws || request.headers().contains_key(header::AUTHORIZATION) {
                return reject_request(
                    &state,
                    request,
                    ApiError::new(
                        400,
                        "invalid_request",
                        "密钥必须通过 Authorization 请求头传递；浏览器订阅须使用一次性票据。",
                    ),
                )
                .await;
            }
            let Some(ticket) = query
                .strip_prefix("ticket=")
                .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                return reject_request(
                    &state,
                    request,
                    ApiError::new(400, "invalid_request", "WebSocket 票据无效。"),
                )
                .await;
            };
            events::consume_ticket(&state, ticket)
        } else {
            if let Some(delay) = state.store.throttle_delay() {
                tokio::time::sleep(delay).await;
            }
            request
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| {
                    v.strip_prefix("Bearer ")
                        .or_else(|| v.strip_prefix("bearer "))
                })
                .and_then(|v| state.store.authenticate(v))
        };
        let Some(client) = client else {
            return reject_request(
                &state,
                request,
                ApiError::new(401, "unauthorized", "请提供有效的 API 应用密钥或订阅票据。"),
            )
            .await;
        };
        request.extensions_mut().insert(client);
    }
    // Reserve a bounded aggregate byte budget before collecting authenticated JSON.
    // Small control requests retain capacity while ordinary commands are in flight.
    let mut body_budget = None;
    if request.method() == axum::http::Method::POST && path.starts_with("/api/v1/commands") {
        const MAX: usize = 20 * 1024 * 1024;
        let declared = request
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok());
        if declared.is_some_and(|n| n > MAX) {
            return reject_request(
                &state,
                request,
                ApiError::new(413, "request_too_large", "请求正文过大。"),
            )
            .await;
        }
        let units = declared.unwrap_or(MAX).max(1).div_ceil(1024 * 1024) as u32;
        let Ok(permit) = state.bodies.clone().try_acquire_many_owned(units) else {
            return reject_request(
                &state,
                request,
                ApiError::rate_limited("请求正文缓冲已达上限，请稍后重试。", 1),
            )
            .await;
        };
        body_budget = Some(permit);
        let (parts, body) = request.into_parts();
        request =
            match tokio::time::timeout(Duration::from_secs(2), axum::body::to_bytes(body, MAX))
                .await
            {
                Ok(Ok(bytes)) => Request::from_parts(parts, axum::body::Body::from(bytes)),
                result => {
                    let error = if result.is_err() {
                        ApiError::new(408, "request_timeout", "请求正文读取超时。")
                    } else {
                        ApiError::new(413, "request_too_large", "请求正文过大或无效。")
                    };
                    let mut response = error.into_response();
                    response
                        .headers_mut()
                        .insert(header::CONNECTION, HeaderValue::from_static("close"));
                    response_headers(&mut response, origin.as_deref());
                    return response;
                }
            };
    }
    let mut response = next.run(request).await;
    drop(body_budget);
    response_headers(&mut response, origin.as_deref());
    // The console shares the API origin, so pin what that origin may load. A
    // planted or injected document can then neither pull remote code nor be
    // framed by another local page.
    let is_html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"));
    if is_html {
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; \
                 script-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; \
                 base-uri 'none'; form-action 'none'",
            ),
        );
        response.headers_mut().insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        response
            .headers_mut()
            .insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    }
    response
}

async fn reject_request(state: &ServerState, request: Request, error: ApiError) -> Response {
    const DRAIN_LIMIT: u64 = 64 * 1024;
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .filter(|v| allowed_origin(v, state.port));
    let mut response = error.into_response();
    response_headers(&mut response, origin);
    let length = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if length == Some(0)
        || length.is_none() && !request.headers().contains_key(header::TRANSFER_ENCODING)
    {
        return response;
    }
    // The request guard has already rejected access. Only discard a bounded small body;
    // never parse it or hold ordinary operation/SSE permits. Dropping an unread
    // HTTP/1 body makes Hyper close immediately, racing split header/body sends.
    // Expect clients wait for a response before uploading, so reject immediately.
    let permit = if request.headers().contains_key(header::EXPECT)
        || length.is_some_and(|v| v > DRAIN_LIMIT)
    {
        None
    } else {
        state.rejected_bodies.try_acquire().ok()
    };
    let drained = if permit.is_none() {
        false
    } else {
        let mut body = request.into_body().into_data_stream();
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut received = 0u64;
            while let Some(chunk) = body.next().await {
                let chunk = chunk.map_err(|_| ())?;
                received = received.saturating_add(chunk.len() as u64);
                if received > DRAIN_LIMIT {
                    return Err(());
                }
            }
            Ok::<_, ()>(())
        })
        .await
        .is_ok_and(|result| result.is_ok())
    };
    if !drained {
        response
            .headers_mut()
            .insert(header::CONNECTION, HeaderValue::from_static("close"));
    }
    response
}

#[utoipa::path(get, path = "/api/v1/health", responses((status = 200, body = ApiResponse)))]
async fn health() -> Json<ApiResponse> {
    success(
        json!({"service":"OpenUnpack","apiVersion":1}),
        Duration::ZERO,
    )
}

#[utoipa::path(get, path = "/api/v1/capabilities", security(("bearerAuth" = [])), responses((status = 200, body = ApiResponse), (status = 401, body = ApiErrorResponse)))]
async fn capabilities() -> Json<ApiResponse> {
    success(
        json!({"apiVersion":1,"commandEndpoint":"/api/v1/commands","namedCommandEndpoint":"/api/v1/commands/{command}","commands":ApiCommand::NAMES,"eventsEndpoint":"/api/v1/events","openapi":"/openapi.json","docs":"/docs","requestBodyLimitBytes":20*1024*1024,"permissions":Scope::ALL.into_iter().map(|scope|json!({"scope":scope,"label":scope.label()})).collect::<Vec<_>>()}),
        Duration::ZERO,
    )
}

#[utoipa::path(post, path = "/api/v1/commands", security(("bearerAuth" = [])), request_body = ApiCommand, responses((status = 200, body = ApiResponse), (status = 400, body = ApiErrorResponse), (status = 401, body = ApiErrorResponse), (status = 403, body = ApiErrorResponse), (status = 429, body = ApiErrorResponse)))]
async fn execute(
    State(state): State<ServerState>,
    axum::Extension(client): axum::Extension<ApiClient>,
    body: Result<Json<ApiCommand>, JsonRejection>,
) -> Result<Json<ApiResponse>, ApiError> {
    let start = std::time::Instant::now();
    let Json(command) = body.map_err(json_error)?;
    dispatch(state, client, command, start).await
}
async fn execute_named(
    State(state): State<ServerState>,
    axum::Extension(client): axum::Extension<ApiClient>,
    Path(command): Path<String>,
    body: Result<Json<Value>, JsonRejection>,
) -> Result<Json<ApiResponse>, ApiError> {
    let start = std::time::Instant::now();
    let Json(arguments) = body.map_err(json_error)?;
    let candidate = if arguments.as_object().is_some_and(|args| args.is_empty())
        && ApiCommand::UNIT_NAMES.contains(&command.as_str())
    {
        json!({"command":command})
    } else {
        json!({"command":command,"args":arguments})
    };
    let command = serde_json::from_value(candidate)
        .map_err(|_| ApiError::new(400, "invalid_request", "命令或参数与 OpenAPI 定义不符。"))?;
    dispatch(state, client, command, start).await
}
fn json_error(error: JsonRejection) -> ApiError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::new(413, "request_too_large", "请求正文超过 20 MiB。")
    } else {
        ApiError::new(
            400,
            "invalid_request",
            "请提供符合 OpenAPI 定义的 application/json 正文。",
        )
    }
}
/// Single authorization gate shared by the HTTP dispatcher and the WebSocket
/// control channel. Keeping it in one place prevents a transport from silently
/// skipping scope checks.
#[allow(clippy::result_large_err)]
fn authorize_command(command: &ApiCommand, client: &ApiClient) -> Result<(), ApiError> {
    if command
        .scopes()
        .iter()
        .any(|scope| !client.scopes.contains(scope))
        || command.requires_file_delete() && !client.scopes.contains(&Scope::FilesDelete)
    {
        return Err(ApiError::denied());
    }
    command.validate().map_err(ApiError::validation_failed)?;
    Ok(())
}
async fn dispatch(
    state: ServerState,
    client: ApiClient,
    command: ApiCommand,
    start: std::time::Instant,
) -> Result<Json<ApiResponse>, ApiError> {
    let client = state
        .store
        .current_client(&client.id)
        .ok_or_else(ApiError::denied)?;
    authorize_command(&command, &client)?;
    let control = matches!(
        command,
        ApiCommand::PauseJobs
            | ApiCommand::CancelJobs { .. }
            | ApiCommand::CancelScan { .. }
            | ApiCommand::CancelInspectArchive { .. }
            | ApiCommand::CancelPreviewExclusions { .. }
    );
    let _permit = if control {
        None
    } else {
        match tokio::time::timeout(Duration::from_millis(500), state.work.acquire()).await {
            Ok(Ok(permit)) => Some(permit),
            _ => {
                return Err(ApiError::rate_limited(
                    "API 正在处理较多请求，排队超时，请稍后重试。",
                    1,
                ));
            }
        }
    };
    let client = state
        .store
        .current_client(&client.id)
        .ok_or_else(ApiError::denied)?;
    authorize_command(&command, &client)?;
    let data = state.backend.execute(command, client).await?;
    Ok(success(data, start.elapsed()))
}

#[cfg(test)]
mod rejected_body_tests {
    use super::*;
    use axum::body::{Body, Bytes, to_bytes};
    use futures_util::stream;
    use std::convert::Infallible;
    use std::sync::atomic::AtomicUsize;

    struct UnusedBackend;
    impl ApiBackend for UnusedBackend {
        fn execute(&self, _: ApiCommand, _: ApiClient) -> BackendFuture<'_> {
            panic!("rejected body reached backend")
        }
        fn subscribe(&self) -> broadcast::Receiver<StateInvalidated> {
            panic!("rejected body subscribed to events")
        }
    }

    pub(super) fn fixture() -> (tempfile::TempDir, ServerState) {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(ApiStore::open(&directory.path().join("api.json")).unwrap());
        let state = ServerState {
            backend: Arc::new(UnusedBackend),
            store,
            port: 47980,
            work: Arc::new(Semaphore::new(16)),
            events: Arc::new(Semaphore::new(32)),
            rejected_bodies: Arc::new(Semaphore::new(16)),
            closing: Arc::new(AtomicBool::new(false)),
            tickets: Arc::new(Mutex::new(HashMap::new())),
            bodies: Arc::new(Semaphore::new(64)),
        };
        (directory, state)
    }

    async fn assert_error(response: Response, close: bool) {
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
        assert_eq!(response.headers().contains_key(header::CONNECTION), close);
        let bytes = to_bytes(response.into_body(), 2048).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], "unauthorized");
        assert!(value["requestId"].as_str().is_some());
    }

    fn error() -> ApiError {
        ApiError::new(401, "unauthorized", "请提供有效的 API 应用密钥。")
    }

    #[tokio::test]
    async fn rejected_small_body_waits_for_its_delayed_frame() {
        let (_directory, state) = fixture();
        let (polled, waiting) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let body = Body::from_stream(stream::once(async move {
            polled.send(()).unwrap();
            released.await.unwrap();
            Ok::<_, Infallible>(Bytes::from_static(b"synthetic"))
        }));
        let request = Request::builder()
            .header(header::CONTENT_LENGTH, 9)
            .body(body)
            .unwrap();
        let task = tokio::spawn(async move { reject_request(&state, request, error()).await });
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap();
        assert!(!task.is_finished(), "response discarded the delayed body");
        release.send(()).unwrap();
        assert_error(task.await.unwrap(), false).await;
    }

    #[tokio::test]
    async fn rejected_chunked_body_has_a_byte_budget_without_collecting_it() {
        let (_directory, state) = fixture();
        for (frames, close) in [(16, false), (18, true)] {
            let polls = Arc::new(AtomicUsize::new(0));
            let count = polls.clone();
            let body = Body::from_stream(stream::iter((0..frames).map(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok::<_, Infallible>(Bytes::from_static(&[b'x'; 4096]))
            })));
            let request = Request::builder()
                .header(header::TRANSFER_ENCODING, "chunked")
                .body(body)
                .unwrap();
            assert_error(reject_request(&state, request, error()).await, close).await;
            assert_eq!(polls.load(Ordering::SeqCst), frames.min(17));
        }
        let body = Body::from_stream(stream::once(async {
            Err::<Bytes, _>(std::io::Error::other("synthetic body error"))
        }));
        let request = Request::builder()
            .header(header::TRANSFER_ENCODING, "chunked")
            .body(body)
            .unwrap();
        assert_error(reject_request(&state, request, error()).await, true).await;
    }

    #[tokio::test]
    async fn zero_expect_oversize_and_saturated_rejections_do_not_poll_the_body() {
        let (_directory, state) = fixture();
        for (length, expect, saturated, close) in [
            (0, false, false, false),
            (9, true, false, true),
            (65537, false, false, true),
            (9, false, true, true),
        ] {
            let _permits = if saturated {
                Some(state.rejected_bodies.acquire_many(16).await.unwrap())
            } else {
                None
            };
            let body = Body::from_stream(stream::poll_fn(
                |_| -> std::task::Poll<Option<Result<Bytes, Infallible>>> {
                    panic!("fallback must not poll an untrusted upload")
                },
            ));
            let mut request = Request::builder().header(header::CONTENT_LENGTH, length);
            if expect {
                request = request.header(header::EXPECT, "100-continue");
            }
            assert_error(
                reject_request(&state, request.body(body).unwrap(), error()).await,
                close,
            )
            .await;
            assert_eq!(state.work.available_permits(), 16);
            assert_eq!(state.events.available_permits(), 32);
        }
    }

    #[tokio::test]
    async fn rejected_small_body_survives_a_descheduled_client() {
        let (_directory, state) = fixture();
        let body = Body::from_stream(stream::once(async {
            tokio::time::sleep(Duration::from_millis(450)).await;
            Ok::<_, Infallible>(Bytes::from_static(b"synthetic"))
        }));
        let request = Request::builder()
            .header(header::CONTENT_LENGTH, 9)
            .body(body)
            .unwrap();
        assert_error(reject_request(&state, request, error()).await, false).await;
        assert_eq!(state.rejected_bodies.available_permits(), 16);
    }

    #[tokio::test]
    async fn stalled_rejection_has_a_total_deadline_and_releases_its_own_permit() {
        let (_directory, state) = fixture();
        let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
        let request = Request::builder()
            .header(header::CONTENT_LENGTH, 9)
            .body(body)
            .unwrap();
        let response = tokio::time::timeout(
            Duration::from_secs(4),
            reject_request(&state, request, error()),
        )
        .await
        .expect("rejection exceeded its total time budget");
        assert_error(response, true).await;
        assert_eq!(state.rejected_bodies.available_permits(), 16);
        assert_eq!(state.work.available_permits(), 16);
        assert_eq!(state.events.available_permits(), 32);
    }
}

#[cfg(test)]
mod authorization_tests {
    use super::*;

    fn client_with(scopes: &[Scope]) -> ApiClient {
        ApiClient {
            id: "test-client".into(),
            name: "test".into(),
            scopes: scopes.iter().copied().collect(),
            read_roots: vec![],
            write_roots: vec![],
            created_at: "0".into(),
            expires_at: None,
            last_used_at: None,
        }
    }

    fn submit() -> ApiCommand {
        ApiCommand::SubmitInput {
            job_id: "job-1".into(),
            input_type: "password".into(),
            value: "secret".into(),
        }
    }

    #[test]
    fn jobs_read_alone_cannot_use_websocket_control_commands() {
        let read_only = client_with(&[Scope::JobsRead]);
        assert!(authorize_command(&ApiCommand::StartJobs, &read_only).is_err());
        assert!(authorize_command(&ApiCommand::PauseJobs, &read_only).is_err());
        assert!(authorize_command(&ApiCommand::Snapshot, &read_only).is_err());
        assert!(authorize_command(&submit(), &read_only).is_err());
    }

    #[test]
    fn matching_scopes_are_still_allowed() {
        let writer = client_with(&[Scope::JobsRead, Scope::JobsWrite]);
        assert!(authorize_command(&ApiCommand::StartJobs, &writer).is_ok());
        assert!(authorize_command(&submit(), &writer).is_ok());

        let reader = client_with(&[Scope::JobsRead, Scope::VaultRead, Scope::SettingsRead]);
        assert!(authorize_command(&ApiCommand::Snapshot, &reader).is_ok());
    }
}

#[cfg(test)]
mod web_root_policy {
    use super::web_root_candidates;

    #[test]
    fn launch_directory_is_never_a_document_root() {
        let exe = std::path::Path::new("C:/Program Files/OpenUnpack");
        let candidates = web_root_candidates(None, Some(exe));
        assert!(candidates.contains(&exe.join("web")));
        // A relative "./dist" candidate would let any file dropped in the
        // process working directory take over the API origin.
        assert!(candidates.iter().all(|path| path.is_absolute()));
    }

    #[test]
    fn explicit_root_outranks_the_install_directory() {
        let exe = std::path::Path::new("C:/Program Files/OpenUnpack");
        let candidates = web_root_candidates(Some(" D:/override "), Some(exe));
        assert_eq!(candidates[0], std::path::Path::new("D:/override"));
        assert_eq!(candidates[1], exe.join("web"));
    }

    #[test]
    fn blank_override_is_ignored() {
        let exe = std::path::Path::new("C:/Program Files/OpenUnpack");
        let candidates = web_root_candidates(Some("   "), Some(exe));
        assert_eq!(candidates[0], exe.join("web"));
    }
}
