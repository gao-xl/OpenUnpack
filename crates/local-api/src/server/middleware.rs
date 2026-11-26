//! 本机 API Origin 校验、Bearer 鉴权、限流与统一拒绝响应中间件。

use super::*;

pub(super) fn allowed_origin(origin: &str, port: u16) -> bool {
    origin == format!("http://127.0.0.1:{port}")
        || origin == format!("http://localhost:{port}")
        || matches!(
            origin,
            "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost"
        )
}
pub(super) fn response_headers(response: &mut Response, origin: Option<&str>) {
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
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"));
    if let Some(origin) = origin.and_then(|v| HeaderValue::from_str(v).ok()) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
}
pub(super) async fn guard(
    State(state): State<ServerState>,
    mut request: Request,
    next: Next,
) -> Response {
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
                 script-src 'self'; connect-src 'self'; frame-ancestors 'none'; \
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

pub(super) async fn reject_request(
    state: &ServerState,
    request: Request,
    error: ApiError,
) -> Response {
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
