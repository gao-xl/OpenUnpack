use super::{ApiError, ApiResponse, ServerState, dispatch, success};
use crate::{ApiClient, ApiCommand, Scope};
use axum::{
    Extension, Json,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use futures_util::{SinkExt, StreamExt, stream};
use openunpack_protocol::{JobProgressUpdate, NeedPasswordEvent, StateInvalidated};
use rand::RngCore;
use serde_json::{Value, json};
use std::{
    convert::Infallible,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, broadcast};

const TICKET_TTL: Duration = Duration::from_secs(15);
const SEND_TIMEOUT: Duration = Duration::from_secs(2);
const FRAME_LIMIT: usize = 16 * 1024;
pub(super) struct SocketTicket {
    client_id: String,
    expires: Instant,
}

#[utoipa::path(post, path = "/api/v1/ws-ticket", security(("bearerAuth" = [])), responses((status = 200, body = ApiResponse), (status = 403, body = super::ApiErrorResponse), (status = 429, body = super::ApiErrorResponse)))]
pub(super) async fn ws_ticket(
    State(state): State<ServerState>,
    Extension(client): Extension<ApiClient>,
) -> Result<Json<ApiResponse>, ApiError> {
    let client = state
        .store
        .current_client(&client.id)
        .ok_or_else(ApiError::denied)?;
    if !client.scopes.contains(&Scope::JobsRead) {
        return Err(ApiError::denied());
    }
    let mut tickets = state.tickets.lock().map_err(|_| ApiError::internal())?;
    let now = Instant::now();
    tickets.retain(|_, ticket| ticket.expires > now);
    if tickets.len() >= 64
        || tickets
            .values()
            .filter(|ticket| ticket.client_id == client.id)
            .count()
            >= 2
    {
        return Err(ApiError::rate_limited("订阅票据过多，请稍后重试。", 1));
    }
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let ticket = hex::encode(bytes);
    tickets.insert(
        ticket.clone(),
        SocketTicket {
            client_id: client.id,
            expires: now + TICKET_TTL,
        },
    );
    Ok(success(
        json!({"ticket":ticket,"expiresInSeconds":TICKET_TTL.as_secs()}),
        Duration::ZERO,
    ))
}
pub(super) fn consume_ticket(state: &ServerState, ticket: &str) -> Option<ApiClient> {
    let ticket = state.tickets.lock().ok()?.remove(ticket)?;
    if ticket.expires <= Instant::now() {
        return None;
    }
    state.store.current_client(&ticket.client_id)
}
fn current(state: &ServerState, id: &str) -> Option<ApiClient> {
    if state.closing.load(Ordering::Acquire) {
        return None;
    }
    state
        .store
        .current_client(id)
        .filter(|client| client.scopes.contains(&Scope::JobsRead))
}
fn need_value(state: &ServerState, event: NeedPasswordEvent, client: &ApiClient) -> Option<Value> {
    let event = state.backend.project_need_input(event, client)?;
    let mut value = serde_json::to_value(event).ok()?;
    if value["archivePath"] == "" {
        value.as_object_mut()?.remove("archivePath");
    }
    Some(value)
}
struct Subscription {
    state: ServerState,
    id: String,
    _permit: OwnedSemaphorePermit,
    revisions: broadcast::Receiver<StateInvalidated>,
    progress: broadcast::Receiver<JobProgressUpdate>,
    needs: broadcast::Receiver<NeedPasswordEvent>,
    progress_open: bool,
    needs_open: bool,
    first: bool,
    tick: tokio::time::Interval,
}
impl Subscription {
    fn new(state: ServerState, id: String, permit: OwnedSemaphorePermit) -> Self {
        Self {
            revisions: state.backend.subscribe(),
            progress: state.backend.subscribe_progress(),
            needs: state.backend.subscribe_need_input(),
            state,
            id,
            _permit: permit,
            progress_open: true,
            needs_open: true,
            first: true,
            tick: tokio::time::interval(Duration::from_millis(250)),
        }
    }
}
#[utoipa::path(get, path = "/api/v1/events", security(("bearerAuth" = [])), responses((status = 200, description = "Authorized revision, progress and input notifications.", content_type = "text/event-stream", body = String), (status = 403, body = super::ApiErrorResponse), (status = 429, body = super::ApiErrorResponse)))]
pub(super) async fn events(
    State(state): State<ServerState>,
    Extension(client): Extension<ApiClient>,
) -> Result<impl IntoResponse, ApiError> {
    current(&state, &client.id).ok_or_else(ApiError::denied)?;
    let permit = state
        .events
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::rate_limited("状态订阅数量已达上限。", 1))?;
    let stream = stream::unfold(
        Subscription::new(state, client.id, permit),
        |mut sub| async move {
            loop {
                let client = current(&sub.state, &sub.id)?;
                if sub.first {
                    sub.first = false;
                    return Some((
                        Ok::<_, Infallible>(
                            Event::default()
                                .event("ready")
                                .data("{\"apiVersion\":1,\"resync\":true}"),
                        ),
                        sub,
                    ));
                }
                let event = tokio::select! {
                    _=sub.tick.tick()=>{continue;},
                    value=sub.revisions.recv()=>match value {
                        Ok(value)=>Event::default().event("state-changed").data(serde_json::to_string(&value).ok()?),
                        Err(broadcast::error::RecvError::Lagged(_))=>Event::default().event("state-changed").data("{\"resync\":true}"),
                        Err(broadcast::error::RecvError::Closed)=>return None,
                    },
                    value=sub.progress.recv(), if sub.progress_open=>match value {
                        Ok(value)=>{
                            let Some(value)=sub.state.backend.project_progress(value,&client) else {continue;};
                            Event::default().event("progress").data(serde_json::to_string(&value).ok()?)
                        },
                        Err(broadcast::error::RecvError::Closed)=>{sub.progress_open=false;continue;},
                        Err(broadcast::error::RecvError::Lagged(_))=>continue,
                    },
                    value=sub.needs.recv(), if sub.needs_open=>match value {
                        Ok(value)=>{
                            let Some(value)=need_value(&sub.state,value,&client) else {continue;};
                            Event::default().event("need-password").data(value.to_string())
                        },
                        Err(broadcast::error::RecvError::Closed)=>{sub.needs_open=false;continue;},
                        Err(broadcast::error::RecvError::Lagged(_))=>continue,
                    },
                };
                current(&sub.state, &sub.id)?;
                return Some((Ok(event), sub));
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::default().interval(Duration::from_secs(10))))
}
pub(super) async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
    Extension(client): Extension<ApiClient>,
) -> Result<Response, ApiError> {
    current(&state, &client.id).ok_or_else(ApiError::denied)?;
    let permit = state
        .events
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::rate_limited("状态订阅数量已达上限。", 1))?;
    Ok(ws
        .max_message_size(FRAME_LIMIT)
        .max_frame_size(FRAME_LIMIT)
        .on_upgrade(move |socket| handle_socket(socket, state, client.id, permit)))
}
fn notification(method: &str, event: &str, data: Value) -> Value {
    json!({"jsonrpc":"2.0","method":method,"params":data,"event":event,"data":data})
}
#[allow(clippy::result_large_err)]
fn input_command(value: &Value) -> Result<ApiCommand, ApiError> {
    let job = value
        .get("jobId")
        .or_else(|| value.get("job_id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let input = value
        .get("input")
        .or_else(|| value.get("password"))
        .or_else(|| value.get("value"))
        .and_then(Value::as_str)
        .unwrap_or("");
    Ok(ApiCommand::SubmitInput {
        job_id: job.into(),
        input_type: "password".into(),
        value: input.into(),
    })
}
async fn message_reply(state: ServerState, id: &str, value: Value) -> Option<Value> {
    let client = current(&state, id)?;
    let rpc = value.get("jsonrpc").and_then(Value::as_str) == Some("2.0");
    let req_id = value.get("id").cloned();
    let method = if rpc {
        value.get("method")
    } else {
        value.get("action")
    }
    .and_then(Value::as_str)
    .unwrap_or("");
    if matches!(method, "openunpack.ping" | "ping") {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        return if rpc {
            req_id.map(|id| json!({"jsonrpc":"2.0","id":id,"result":{"timestamp":now}}))
        } else {
            Some(notification(
                "openunpack.onPong",
                "pong",
                json!({"timestamp":now}),
            ))
        };
    }
    let args = if rpc {
        value.get("params").unwrap_or(&Value::Null)
    } else {
        &value
    };
    let command=match method {
        "openunpack.submitPassword"|"openunpack.submitInput"|"submit_input"|"submit_password"=>input_command(args),
        "openunpack.snapshot"=>Ok(ApiCommand::Snapshot),
        "openunpack.pause"=>Ok(ApiCommand::PauseJobs),
        "openunpack.resume"=>Ok(ApiCommand::StartJobs),
        _=>return req_id.map(|id|json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}})),
    };
    let input_job = command.as_ref().ok().and_then(|c| {
        if let ApiCommand::SubmitInput { job_id, .. } = c {
            Some(job_id.clone())
        } else {
            None
        }
    });
    let result = match command {
        Ok(command) => dispatch(state.clone(), client, command, Instant::now())
            .await
            .map(|Json(response)| response.data),
        Err(error) => Err(error),
    };
    current(&state, id)?;
    if rpc {
        return req_id.map(|id|match result {
        Ok(data)=>json!({"jsonrpc":"2.0","id":id,"result":data}),
        Err(error)=>json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":error.message,"data":error.code}}),
    });
    }
    input_job.map(|job| {
        notification(
            "openunpack.onInputResult",
            "input_result",
            json!({"jobId":job,"success":result.is_ok()}),
        )
    })
}
async fn handle_socket(
    socket: WebSocket,
    state: ServerState,
    id: String,
    permit: OwnedSemaphorePermit,
) {
    let (mut sender, mut receiver) = socket.split();
    // Register receivers before ready closes the initial subscription gap.
    let mut sub = Subscription::new(state.clone(), id.clone(), permit);
    let Some(client) = current(&state, &id) else {
        return;
    };
    let ready = notification(
        "openunpack.onReady",
        "ready",
        json!({"apiVersion":1,"resync":true,"clientId":client.id}),
    );
    if !tokio::time::timeout(
        SEND_TIMEOUT,
        sender.send(Message::Text(ready.to_string().into())),
    )
    .await
    .is_ok_and(|v| v.is_ok())
    {
        return;
    }
    let (replies, mut reply_rx) = tokio::sync::mpsc::channel::<Value>(8);
    let receive_state = state.clone();
    let receive_id = id.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(message)) = receiver.next().await {
            if current(&receive_state, &receive_id).is_none() {
                break;
            }
            match message {
                Message::Text(text) => {
                    let Ok(value) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    if let Some(reply) =
                        message_reply(receive_state.clone(), &receive_id, value).await
                        && !tokio::time::timeout(SEND_TIMEOUT, replies.send(reply))
                            .await
                            .is_ok_and(|v| v.is_ok())
                    {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });
    let mut send_task = tokio::spawn(async move {
        let mut ping = tokio::time::interval(Duration::from_secs(15));
        while let Some(client) = current(&sub.state, &sub.id) {
            let message = tokio::select! {
                _=sub.tick.tick()=>{continue;},                _=ping.tick()=>notification("openunpack.onPing","ping",json!({"timestamp":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|v|v.as_secs()).unwrap_or(0)})),
                Some(reply)=reply_rx.recv()=>reply,
                value=sub.revisions.recv()=>match value {
                    Ok(value)=>notification("openunpack.onStateChanged","state_changed",json!(value)),
                    Err(broadcast::error::RecvError::Lagged(_))=>notification("openunpack.onStateChanged","state_changed",json!({"resync":true})),
                    Err(broadcast::error::RecvError::Closed)=>break,
                },
                value=sub.progress.recv(), if sub.progress_open=>match value {
                    Ok(value)=>{
                        let Some(value)=sub.state.backend.project_progress(value,&client) else {continue;};
                        notification("openunpack.onJobProgress","job_progress",json!(value))
                    },
                    Err(broadcast::error::RecvError::Closed)=>{sub.progress_open=false;continue;},
                    Err(broadcast::error::RecvError::Lagged(_))=>continue,
                },
                value=sub.needs.recv(), if sub.needs_open=>match value {
                    Ok(value)=>{
                        let Some(value)=need_value(&sub.state,value,&client) else {continue;};
                        notification("openunpack.onNeedPassword","need_password",value)
                    },
                    Err(broadcast::error::RecvError::Closed)=>{sub.needs_open=false;continue;},
                    Err(broadcast::error::RecvError::Lagged(_))=>continue,
                },
            };
            if current(&sub.state, &sub.id).is_none() {
                break;
            }
            if !tokio::time::timeout(
                SEND_TIMEOUT,
                sender.send(Message::Text(message.to_string().into())),
            )
            .await
            .is_ok_and(|v| v.is_ok())
            {
                break;
            }
        }
        let _ = tokio::time::timeout(SEND_TIMEOUT, sender.close()).await;
    });
    tokio::select! {
        _=&mut recv_task=>{send_task.abort();let _=send_task.await;},
        _=&mut send_task=>{recv_task.abort();let _=recv_task.await;},
    }
}

#[cfg(test)]
mod ticket_tests {
    use super::*;
    #[test]
    fn expired_tickets_are_consumed_without_authenticating_their_owner() {
        let (_directory, state) = super::super::rejected_body_tests::fixture();
        state.tickets.lock().unwrap().insert(
            "synthetic-expired".into(),
            SocketTicket {
                client_id: "synthetic-owner".into(),
                expires: Instant::now() - Duration::from_secs(1),
            },
        );
        assert!(consume_ticket(&state, "synthetic-expired").is_none());
        assert!(
            !state
                .tickets
                .lock()
                .unwrap()
                .contains_key("synthetic-expired")
        );
    }
}
