//! OpenUnpack 本机 HTTP/WebSocket API 服务器与路由装配。

use crate::{ApiClient, ApiCommand, ApiStore, Scope};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    middleware::{self as axum_middleware, Next},
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
    pub error_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<&'static str>,
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
            error_type: Some(error_type),
            title: Some(title),
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
            error_type: Some("https://openunpack.local/errors/validation-failed"),
            title: Some("Validation Failed"),
            invalid_fields: Some(errors),
            retry_after_secs: None,
        }
    }
    pub fn rate_limited(message: &str, retry_after: u32) -> Self {
        Self {
            status: 429,
            code: "busy".into(),
            message: message.into(),
            error_type: Some("https://openunpack.local/errors/rate-limit-exceeded"),
            title: Some("Too Many Requests"),
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
        .layer(axum_middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

mod assets;
mod middleware;
use assets::*;
use middleware::*;

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
mod tests;
#[cfg(test)]
use tests::rejected_body_tests;
