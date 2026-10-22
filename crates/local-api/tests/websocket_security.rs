#[allow(dead_code)]
mod support;
use futures_util::{SinkExt, StreamExt};
use openunpack_local_api::{
    ApiBackend, ApiClient, ApiClientInput, ApiCommand, ApiSettings, ApiStore, BackendFuture,
    LocalApiServer, Scope,
};
use openunpack_protocol::{JobProgressUpdate, NeedPasswordEvent, StateInvalidated};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, broadcast};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
struct Backend {
    calls: Mutex<Vec<&'static str>>,
    revisions: broadcast::Sender<StateInvalidated>,
    progress: broadcast::Sender<JobProgressUpdate>,
    needs: broadcast::Sender<NeedPasswordEvent>,
    gate: Option<Arc<Semaphore>>,
}
impl ApiBackend for Backend {
    fn execute(&self, command: ApiCommand, _: ApiClient) -> BackendFuture<'_> {
        Box::pin(async move {
            let kind = match command {
                ApiCommand::QueueSnapshot => "queue",
                ApiCommand::JobDetails { .. } => "target",
                ApiCommand::PauseJobs => "pause",
                ApiCommand::SubmitInput { .. } => "input",
                _ => "other",
            };
            self.calls.lock().unwrap().push(kind);
            if kind == "queue"
                && let Some(gate) = &self.gate
            {
                gate.acquire().await.unwrap().forget();
            }
            Ok(json!({"kind":kind}))
        })
    }
    fn subscribe(&self) -> broadcast::Receiver<StateInvalidated> {
        self.revisions.subscribe()
    }
    fn subscribe_progress(&self) -> broadcast::Receiver<JobProgressUpdate> {
        self.progress.subscribe()
    }
    fn subscribe_need_input(&self) -> broadcast::Receiver<NeedPasswordEvent> {
        self.needs.subscribe()
    }
}
struct Fixture {
    _directory: tempfile::TempDir,
    store: Arc<ApiStore>,
    backend: Arc<Backend>,
    server: LocalApiServer,
}
impl Fixture {
    async fn new(block: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(ApiStore::open(&directory.path().join("api.json")).unwrap());
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        store
            .save_settings(ApiSettings {
                enabled: true,
                port,
            })
            .unwrap();
        let backend = Arc::new(Backend {
            calls: Mutex::new(vec![]),
            revisions: broadcast::channel(16).0,
            progress: broadcast::channel(16).0,
            needs: broadcast::channel(16).0,
            gate: block.then(|| Arc::new(Semaphore::new(0))),
        });
        let server = LocalApiServer::start(store.clone(), backend.clone())
            .await
            .unwrap();
        Self {
            _directory: directory,
            store,
            backend,
            server,
        }
    }
    fn client(
        &self,
        scopes: &[Scope],
        expiry: Option<String>,
    ) -> openunpack_local_api::CreatedClient {
        self.store
            .create_client(ApiClientInput {
                name: "synthetic security client".into(),
                scopes: scopes.iter().copied().collect(),
                read_roots: vec![],
                write_roots: vec![],
                expires_at: expiry,
            })
            .unwrap()
    }
    async fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Value,
    ) -> support::Response {
        request(self.server.port, method, path, token, body).await
    }
    async fn socket(&self, token: &str) -> Socket {
        let mut request = format!("ws://127.0.0.1:{}/api/v1/ws", self.server.port)
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse().unwrap());
        tokio_tungstenite::connect_async(request)
            .await
            .expect("loopback websocket connection")
            .0
    }
}
async fn request(
    port: u16,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> support::Response {
    let body = if method == "POST" {
        serde_json::to_vec(&body).unwrap()
    } else {
        vec![]
    };
    let auth = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut bytes = head.into_bytes();
    bytes.extend(body);
    support::exchange(port, bytes).await
}
async fn next(socket: &mut Socket, event: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(message) = socket.next().await {
            if let Ok(Message::Text(text)) = message {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["event"] == event {
                    return value;
                }
            }
        }
        panic!("subscription closed before synthetic event");
    })
    .await
    .expect("synthetic event deadline")
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn readonly_websocket_cannot_mutate_through_either_protocol() {
    let f = Fixture::new(false).await;
    let c = f.client(&[Scope::JobsRead], None);
    let mut ws = f.socket(&c.token).await;
    next(&mut ws, "ready").await;
    ws.send(Message::Text(
        json!({"action":"submit_password","jobId":"synthetic-job","password":"synthetic input"})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    assert_eq!(
        next(&mut ws, "input_result").await["data"]["success"],
        false
    );
    ws.send(Message::Text(
        json!({"jsonrpc":"2.0","id":1,"method":"openunpack.pause"})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(Ok(Message::Text(text))) = ws.next().await {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["id"] == 1 {
                    return value;
                }
            }
        }
    })
    .await
    .unwrap();
    assert!(reply.get("error").is_some());
    assert!(f.backend.calls.lock().unwrap().is_empty());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoked_socket_never_dispatches_a_new_mutation() {
    let f = Fixture::new(false).await;
    let c = f.client(
        &[Scope::JobsRead, Scope::JobsWrite, Scope::SettingsWrite],
        None,
    );
    let mut ws = f.socket(&c.token).await;
    next(&mut ws, "ready").await;
    next(&mut ws, "ping").await;
    f.store.revoke_client(&c.client.id).unwrap();
    let _ = ws
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":1,"method":"openunpack.pause"})
                .to_string()
                .into(),
        ))
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(f.backend.calls.lock().unwrap().is_empty());
    let closed = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            }
        }
    })
    .await;
    assert!(
        closed.is_ok(),
        "revoked websocket must close without waiting for an application event"
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_registered_before_ready_never_disclose_unapproved_paths() {
    let f = Fixture::new(false).await;
    let c = f.client(&[Scope::JobsRead], None);
    let mut ws = f.socket(&c.token).await;
    next(&mut ws, "ready").await;
    assert!(
        f.backend.needs.receiver_count() > 0,
        "ready requires receivers already registered"
    );
    f.backend
        .needs
        .send(NeedPasswordEvent {
            job_id: "synthetic-job".into(),
            archive_path: r"C:\SyntheticOutsideGrant\fixture.7z".into(),
            timeout_seconds: 30,
            input_type: "password".into(),
        })
        .unwrap();
    let value = next(&mut ws, "need_password").await;
    assert!(value["data"].get("archivePath").is_none());
    assert!(!value.to_string().contains("SyntheticOutsideGrant"));
    f.backend
        .progress
        .send(JobProgressUpdate {
            job_id: "synthetic-job".into(),
            stage: "extracting".into(),
            progress: 0.5,
            completed_bytes: 1,
            total_bytes: 2,
            bytes_per_second: 1.0,
            remaining_seconds: Some(1.0),
            current_file: Some(r"C:\SyntheticOutsideGrant\secret.txt".into()),
        })
        .unwrap();
    let value = next(&mut ws, "job_progress").await;
    assert!(value["data"].get("currentFile").is_none());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_tickets_are_one_time_and_revocation_invalidates_unused_tickets() {
    let f = Fixture::new(false).await;
    let c = f.client(&[Scope::JobsRead], None);
    let response = f
        .request("POST", "/api/v1/ws-ticket", Some(&c.token), Value::Null)
        .await;
    assert_eq!(response.status, 200);
    let value = response.json();
    let ticket = value["data"]["ticket"].as_str().unwrap();
    assert_eq!(ticket.len(), 64);
    assert_eq!(value["data"]["expiresInSeconds"], 15);
    let url = format!("ws://127.0.0.1:{}/api/v1/ws?ticket={ticket}", f.server.port);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("first ticket use");
    next(&mut ws, "ready").await;
    assert!(
        matches!(tokio_tungstenite::connect_async(&url).await,Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status()==401)
    );
    let response = f
        .request("POST", "/api/v1/ws-ticket", Some(&c.token), Value::Null)
        .await;
    let value = response.json();
    let ticket = value["data"]["ticket"].as_str().unwrap();
    f.store.revoke_client(&c.client.id).unwrap();
    let response = f
        .request(
            "GET",
            &format!("/api/v1/ws?ticket={ticket}"),
            None,
            Value::Null,
        )
        .await;
    assert_eq!(response.status, 401);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bearer_query_is_rejected_and_web_console_is_public_without_public_business_data() {
    let f = Fixture::new(false).await;
    let response = f
        .request(
            "GET",
            "/api/v1/ws?token=synthetic-secret",
            None,
            Value::Null,
        )
        .await;
    assert_eq!(response.status, 400);
    assert!(!response.text().contains("synthetic-secret"));
    let response = f.request("GET", "/", None, Value::Null).await;
    assert_eq!(response.status, 200);
    assert!(
        response.text().contains("openunpack-web-console")
            || response.text().contains("OpenUnpack Daemon")
    );
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/commands",
            None,
            json!({"command":"queue_snapshot"})
        )
        .await
        .status,
        401
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_websocket_input_is_closed_before_backend_dispatch() {
    let f = Fixture::new(false).await;
    let c = f.client(&[Scope::JobsRead, Scope::JobsWrite], None);
    let mut ws = f.socket(&c.token).await;
    next(&mut ws, "ready").await;
    let _ = ws
        .send(Message::Text(
            json!({"action":"submit_input","jobId":"synthetic-job","value":"x".repeat(17*1024)})
                .to_string()
                .into(),
        ))
        .await;
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok());
    assert!(f.backend.calls.lock().unwrap().is_empty());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authorization_is_refreshed_after_waiting_in_the_work_queue() {
    let f = Fixture::new(true).await;
    let c = f.client(&[Scope::JobsRead], None);
    let target = f.client(&[Scope::JobsRead], None);
    let mut tasks = vec![];
    for _ in 0..16 {
        let token = c.token.clone();
        let port = f.server.port;
        tasks.push(tokio::spawn(async move {
            request(
                port,
                "POST",
                "/api/v1/commands",
                Some(&token),
                json!({"command":"queue_snapshot"}),
            )
            .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.backend.calls.lock().unwrap().len() != 16 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let token = target.token.clone();
    let port = f.server.port;
    let queued = tokio::spawn(async move {
        request(
            port,
            "POST",
            "/api/v1/commands",
            Some(&token),
            json!({"command":"job_details","args":{"jobId":"queued-target"}}),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    f.store.revoke_client(&target.client.id).unwrap();
    f.backend.gate.as_ref().unwrap().add_permits(16);
    assert_eq!(queued.await.unwrap().status, 403);
    for task in tasks {
        assert_eq!(task.await.unwrap().status, 200);
    }
    assert_eq!(f.backend.calls.lock().unwrap().len(), 16);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expiry_closes_existing_socket_and_rejects_new_requests() {
    let f = Fixture::new(false).await;
    let deadline = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3;
    let c = f.client(&[Scope::JobsRead], Some(deadline.to_string()));
    let mut ws = f.socket(&c.token).await;
    next(&mut ws, "ready").await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            }
        }
    })
    .await
    .expect("expired socket closes on authorization timer");
    assert!(!f.store.active_client(&c.client.id));
    assert!(f.store.authenticate(&c.token).is_none());
}
