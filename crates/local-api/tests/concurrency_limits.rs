// This test binary reuses only the TCP client portion of the integration helpers.
#[allow(dead_code)]
mod support;

use openunpack_local_api::{
    ApiBackend, ApiClient, ApiClientInput, ApiCommand, ApiError, ApiSettings, ApiStore,
    BackendFuture, LocalApiServer, Scope,
};
use openunpack_protocol::StateInvalidated;
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use support::{EventStream, Response, event_json, exchange};
use tempfile::TempDir;
use tokio::sync::{Notify, Semaphore, broadcast};

struct BlockingBackend {
    started: AtomicUsize,
    finished: AtomicUsize,
    control_calls: AtomicUsize,
    started_changed: Notify,
    gate: Semaphore,
    invalidations: broadcast::Sender<StateInvalidated>,
}

impl BlockingBackend {
    fn new() -> Self {
        Self {
            started: AtomicUsize::new(0),
            finished: AtomicUsize::new(0),
            control_calls: AtomicUsize::new(0),
            started_changed: Notify::new(),
            gate: Semaphore::new(0),
            invalidations: broadcast::channel(16).0,
        }
    }

    async fn wait_for_started(&self, expected: usize) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let changed = self.started_changed.notified();
                if self.started.load(Ordering::SeqCst) >= expected {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("ordinary HTTP requests never reached the expected concurrency");
    }
}

impl ApiBackend for BlockingBackend {
    fn execute(&self, command: ApiCommand, _client: ApiClient) -> BackendFuture<'_> {
        Box::pin(async move {
            match command {
                ApiCommand::QueueSnapshot => {
                    self.started.fetch_add(1, Ordering::SeqCst);
                    self.started_changed.notify_one();
                    self.gate.acquire().await.unwrap().forget();
                    self.finished.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({"released": true}))
                }
                ApiCommand::PauseJobs => {
                    self.control_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({"paused": true}))
                }
                _ => Err(ApiError::denied()),
            }
        })
    }

    fn subscribe(&self) -> broadcast::Receiver<StateInvalidated> {
        self.invalidations.subscribe()
    }
}

struct Fixture {
    server: LocalApiServer,
    store: Arc<ApiStore>,
    _directory: TempDir,
    backend: Arc<BlockingBackend>,
    port: u16,
}

impl Fixture {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(ApiStore::open(&directory.path().join("api.json")).unwrap());
        let backend = Arc::new(BlockingBackend::new());
        for _ in 0..16 {
            let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
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
            if let Ok(server) = LocalApiServer::start(store.clone(), backend.clone()).await {
                return Self {
                    server,
                    store,
                    _directory: directory,
                    backend,
                    port,
                };
            }
        }
        panic!("could not start the loopback concurrency fixture");
    }

    fn token(&self, scopes: &[Scope]) -> String {
        self.store
            .create_client(ApiClientInput {
                name: "synthetic concurrency client".into(),
                scopes: scopes.iter().copied().collect(),
                read_roots: vec![],
                write_roots: vec![],
                expires_at: None,
            })
            .unwrap()
            .token
    }

    async fn stop(self) {
        self.server.shutdown().await;
    }
}

async fn command(port: u16, token: &str, name: &str) -> Response {
    let body = serde_json::to_vec(&json!({"command":name})).unwrap();
    let mut request = format!("POST /api/v1/commands HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    request.extend_from_slice(&body);
    exchange(port, request).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_limit_rejects_the_17th_request_without_starving_pause_control() {
    let api = Fixture::start().await;
    let reader = api.token(&[Scope::JobsRead]);
    let controller = api.token(&[Scope::JobsWrite]);
    let mut requests = Vec::new();
    for _ in 0..16 {
        let token = reader.clone();
        let port = api.port;
        requests.push(tokio::spawn(async move {
            command(port, &token, "queue_snapshot").await
        }));
    }
    api.backend.wait_for_started(16).await;
    assert_eq!(api.backend.finished.load(Ordering::SeqCst), 0);
    assert_eq!(api.backend.gate.available_permits(), 0);

    let busy = tokio::time::timeout(
        Duration::from_secs(5),
        command(api.port, &reader, "queue_snapshot"),
    )
    .await
    .expect("the excess ordinary request queued instead of being rejected");
    busy.error(429, "busy");
    assert_eq!(api.backend.started.load(Ordering::SeqCst), 16);

    let pause = tokio::time::timeout(
        Duration::from_secs(5),
        command(api.port, &controller, "pause_jobs"),
    )
    .await
    .expect("pause was starved behind ordinary request slots");
    assert_eq!(pause.status, 200);
    assert_eq!(pause.json()["data"]["paused"], true);
    assert_eq!(api.backend.control_calls.load(Ordering::SeqCst), 1);
    assert_eq!(api.backend.finished.load(Ordering::SeqCst), 0);

    api.backend.gate.add_permits(16);
    tokio::time::timeout(Duration::from_secs(10), async {
        for request in requests {
            let response = request.await.unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(response.json()["data"]["released"], true);
        }
    })
    .await
    .expect("released ordinary requests did not all terminate");
    assert_eq!(api.backend.started.load(Ordering::SeqCst), 16);
    assert_eq!(api.backend.finished.load(Ordering::SeqCst), 16);
    api.stop().await;
}

async fn event_status_once(port: u16, token: String) -> u16 {
    tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect_timeout(
            &SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into(),
            Duration::from_secs(5),
        ).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(stream, "GET /api/v1/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n").unwrap();
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        assert_ne!(reader.read_line(&mut status).unwrap(), 0);
        let status: u16 = status.split_whitespace().nth(1).unwrap().parse().unwrap();
        let mut header_bytes = 0;
        loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            header_bytes += line.len();
            assert!(header_bytes <= 16 * 1024);
            if line == "\r\n" { break; }
        }
        // The probe closes even accepted subscriptions; it never leaves a
        // permit or an unbounded read waiting for the next invalidation.
        status
    }).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn event_limit_rejects_the_33rd_stream_and_releases_a_dropped_subscription() {
    let api = Fixture::start().await;
    let token = api.token(&[Scope::JobsRead]);
    let mut streams = Vec::new();
    for _ in 0..32 {
        let stream = EventStream::open(api.port, token.clone()).await;
        let (stream, ready) = stream.read_frame().await;
        assert!(
            ready.is_some(),
            "subscription closed before its ready frame"
        );
        streams.push(stream);
    }
    let busy = exchange(api.port, format!("GET /api/v1/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n", api.port).into_bytes()).await;
    busy.error(429, "busy");

    drop(streams.pop().unwrap());
    api.backend
        .invalidations
        .send(StateInvalidated {
            session_id: "synthetic-concurrency-session".into(),
            revision: "1".into(),
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match event_status_once(api.port, token.clone()).await {
                200 => break,
                429 => tokio::task::yield_now().await,
                _ => panic!("unexpected response while waiting for a released SSE slot"),
            }
        }
    })
    .await
    .expect("dropping an SSE subscription did not release its permit");
    assert_eq!(api.backend.started.load(Ordering::SeqCst), 0);
    assert_eq!(api.backend.control_calls.load(Ordering::SeqCst), 0);
    drop(streams);
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_closes_old_sse_while_enabled_and_restart_has_an_independent_lifetime() {
    let api = Fixture::start().await;
    let token = api.token(&[Scope::JobsRead]);
    let Fixture {
        server,
        store,
        _directory,
        backend,
        port,
    } = api;
    let old_stream = EventStream::open(port, token.clone()).await;
    let (old_stream, ready) = old_stream.read_frame().await;
    assert!(ready.is_some());

    server.shutdown().await;
    assert!(
        store.settings().unwrap().enabled,
        "shutdown must not require disabling saved settings"
    );
    assert!(store.authenticate(&token).is_some());
    // Restart immediately, before observing the old stream's EOF: the old
    // transport must close without also invalidating the new server's stream.
    let restarted = LocalApiServer::start(store.clone(), backend.clone())
        .await
        .unwrap();
    assert_eq!(restarted.port, port);
    let new_stream = EventStream::open(port, token).await;
    let (new_stream, ready) = new_stream.read_frame().await;
    assert!(ready.is_some());
    backend
        .invalidations
        .send(StateInvalidated {
            session_id: "synthetic-restarted-session".into(),
            revision: "2".into(),
        })
        .unwrap();
    let (old_stream, old_frame) = old_stream.read_frame().await;
    assert!(
        old_frame.is_none(),
        "the replaced server retained its enabled SSE subscription"
    );
    drop(old_stream);
    let (new_stream, update) = new_stream.read_frame().await;
    assert_eq!(
        event_json(&update.unwrap(), "state-changed"),
        json!({"sessionId":"synthetic-restarted-session","revision":"2"})
    );
    assert!(store.settings().unwrap().enabled);
    assert!(restarted.is_running());

    restarted.shutdown().await;
    let (new_stream, closed) = new_stream.read_frame().await;
    assert!(
        closed.is_none(),
        "restarted server did not close its stream on shutdown"
    );
    drop(new_stream);
    assert!(store.settings().unwrap().enabled);
}
