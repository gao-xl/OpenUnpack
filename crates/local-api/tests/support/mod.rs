use openunpack_local_api::{
    ApiBackend, ApiClient, ApiClientInput, ApiCommand, ApiSettings, ApiStore, BackendFuture,
    CreatedClient, LocalApiServer, Scope,
};
use openunpack_protocol::StateInvalidated;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::TempDir;
use tokio::sync::broadcast;

pub struct RecordingBackend {
    pub calls: Mutex<Vec<Value>>,
    pub invalidations: broadcast::Sender<StateInvalidated>,
}

impl RecordingBackend {
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            invalidations: broadcast::channel(16).0,
        }
    }

    pub fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl ApiBackend for RecordingBackend {
    fn execute(&self, command: ApiCommand, client: ApiClient) -> BackendFuture<'_> {
        Box::pin(async move {
            // Deliberately record no command secrets, tokens, paths or request bodies.
            let kind = match command {
                ApiCommand::QueueSnapshot => "queue_snapshot",
                ApiCommand::ReadSettings => "read_settings",
                ApiCommand::JobDetails { .. } => "job_details",
                ApiCommand::SaveSettings { .. } => "save_settings",
                ApiCommand::AddJobs { .. } => "add_jobs",
                ApiCommand::ExecuteToolboxOperation { .. } => "execute_toolbox_operation",
                _ => "other",
            };
            let value = json!({"kind":kind,"clientId":client.id});
            self.calls.lock().unwrap().push(value.clone());
            Ok(value)
        })
    }

    fn subscribe(&self) -> broadcast::Receiver<StateInvalidated> {
        self.invalidations.subscribe()
    }
}

pub struct Harness {
    pub directory: TempDir,
    pub store: Arc<ApiStore>,
    pub backend: Arc<RecordingBackend>,
    server: Option<LocalApiServer>,
    pub port: u16,
}

impl Harness {
    pub async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(ApiStore::open(&directory.path().join("local-api.json")).unwrap());
        let backend = Arc::new(RecordingBackend::new());
        // Port zero is deliberately invalid in persisted settings. The probe socket
        // is loopback only; retry the bounded race between releasing it and start().
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
                assert_eq!(server.port, port);
                assert!(server.is_running());
                return Self {
                    directory,
                    store,
                    backend,
                    server: Some(server),
                    port,
                };
            }
        }
        panic!("could not reserve a loopback API test port after 16 attempts");
    }

    pub fn client(&self, scopes: &[Scope]) -> CreatedClient {
        self.store
            .create_client(ApiClientInput {
                name: "synthetic integration client".into(),
                scopes: scopes.iter().copied().collect(),
                read_roots: vec![],
                write_roots: vec![],
                expires_at: None,
            })
            .unwrap()
    }

    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.server.as_ref().unwrap().local_addr()
    }

    pub async fn request(
        &self,
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Response {
        let mut request = format!(
            "{method} {target} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("host"))
        {
            request.push_str(&format!("Host: 127.0.0.1:{}\r\n", self.port));
        }
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        {
            request.push_str("Content-Type: application/json\r\n");
        }
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        let mut bytes = request.into_bytes();
        bytes.extend_from_slice(body);
        exchange(self.port, bytes).await
    }

    pub async fn command(&self, token: &str, value: Value) -> Response {
        self.request(
            "POST",
            "/api/v1/commands",
            &[("Authorization", &format!("Bearer {token}"))],
            &serde_json::to_vec(&value).unwrap(),
        )
        .await
    }

    pub async fn stop(mut self) {
        self.server.take().unwrap().shutdown().await;
    }
}

pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("expected JSON response")
    }
    pub fn text(&self) -> String {
        String::from_utf8(self.body.clone()).expect("expected UTF-8 response")
    }
    pub fn error(&self, status: u16, code: &str) {
        assert_eq!(self.status, status);
        let json = self.json();
        assert_eq!(json["error"]["code"], code);
        assert!(json["requestId"].as_str().is_some_and(|id| !id.is_empty()));
        assert_eq!(
            self.headers.get("cache-control").map(String::as_str),
            Some("no-store")
        );
        assert_eq!(
            self.headers
                .get("x-content-type-options")
                .map(String::as_str),
            Some("nosniff")
        );
    }
}

fn connect(port: u16) -> TcpStream {
    let stream = TcpStream::connect_timeout(
        &SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into(),
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(stream.peer_addr().unwrap().ip().is_loopback());
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
}

pub async fn exchange(port: u16, request: Vec<u8>) -> Response {
    tokio::task::spawn_blocking(move || {
        let mut stream = connect(port);
        // An early body-limit rejection can close the socket before all oversized
        // bytes are accepted. Still read and verify the actual HTTP response.
        let write_result = stream.write_all(&request);
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                Err(error)
                    if error.kind() == io::ErrorKind::ConnectionReset && !bytes.is_empty() =>
                {
                    break;
                }
                Err(error) => panic!("HTTP response read failed: {error}"),
            }
        }
        assert!(
            !bytes.is_empty(),
            "HTTP response missing; write succeeded: {}",
            write_result.is_ok()
        );
        let split = bytes
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .expect("HTTP header terminator");
        let (status, headers) = parse_headers(&bytes[..split]);
        let body = bytes[split + 4..].to_vec();
        let body = if headers
            .get("transfer-encoding")
            .is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
        {
            decode_chunks(&body)
        } else {
            body
        };
        Response {
            status,
            headers,
            body,
        }
    })
    .await
    .unwrap()
}

fn parse_headers(bytes: &[u8]) -> (u16, BTreeMap<String, String>) {
    let text = std::str::from_utf8(bytes).unwrap();
    let mut lines = text.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    (status, headers)
}

fn decode_chunks(mut bytes: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::new();
    loop {
        let end = bytes
            .windows(2)
            .position(|value| value == b"\r\n")
            .expect("chunk size terminator");
        let size = usize::from_str_radix(
            std::str::from_utf8(&bytes[..end])
                .unwrap()
                .split(';')
                .next()
                .unwrap(),
            16,
        )
        .unwrap();
        bytes = &bytes[end + 2..];
        if size == 0 {
            return decoded;
        }
        assert!(bytes.len() >= size + 2, "truncated HTTP body chunk");
        decoded.extend_from_slice(&bytes[..size]);
        assert_eq!(&bytes[size..size + 2], b"\r\n");
        bytes = &bytes[size + 2..];
    }
}

pub struct EventStream {
    reader: BufReader<TcpStream>,
    chunked: bool,
    remaining: usize,
    ended: bool,
}
impl EventStream {
    pub async fn open(port: u16, token: String) -> Self {
        tokio::task::spawn_blocking(move || {
            let mut stream = connect(port);
            write!(stream, "GET /api/v1/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n").unwrap();
            let mut reader = BufReader::new(stream);
            let mut header = String::new();
            loop {
                let mut line = String::new();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" { break; }
                header.push_str(&line);
            }
            let (status, headers) = parse_headers(header.trim_end().as_bytes());
            assert_eq!(status, 200);
            assert!(headers["content-type"].starts_with("text/event-stream"));
            let chunked = headers.get("transfer-encoding").is_some_and(|value| value.eq_ignore_ascii_case("chunked"));
            Self { reader, chunked, remaining: 0, ended: false }
        }).await.unwrap()
    }

    fn byte(&mut self) -> io::Result<Option<u8>> {
        if self.ended {
            return Ok(None);
        }
        if self.chunked && self.remaining == 0 {
            let mut size = String::new();
            if self.reader.read_line(&mut size)? == 0 {
                return Ok(None);
            }
            self.remaining = usize::from_str_radix(size.trim().split(';').next().unwrap(), 16)
                .map_err(io::Error::other)?;
            if self.remaining == 0 {
                self.ended = true;
                return Ok(None);
            }
        }
        let mut byte = [0u8; 1];
        if self.reader.read(&mut byte)? == 0 {
            return Ok(None);
        }
        if self.chunked {
            self.remaining -= 1;
            if self.remaining == 0 {
                let mut end = [0u8; 2];
                self.reader.read_exact(&mut end)?;
                if end != *b"\r\n" {
                    return Err(io::Error::other("invalid SSE chunk boundary"));
                }
            }
        }
        Ok(Some(byte[0]))
    }

    pub async fn read_frame(mut self) -> (Self, Option<String>) {
        tokio::task::spawn_blocking(move || {
            let mut bytes = Vec::new();
            loop {
                match self
                    .byte()
                    .expect("SSE stream read failed or did not close within 5 seconds")
                {
                    Some(value) => bytes.push(value),
                    None => {
                        assert!(bytes.is_empty(), "partial SSE frame at close");
                        return (self, None);
                    }
                }
                if bytes.ends_with(b"\n\n") || bytes.ends_with(b"\r\n\r\n") {
                    return (self, Some(String::from_utf8(bytes).unwrap()));
                }
                assert!(
                    bytes.len() < 16 * 1024,
                    "SSE unexpectedly contained a large payload"
                );
            }
        })
        .await
        .unwrap()
    }
}

pub fn event_json(frame: &str, event: &str) -> Value {
    assert!(
        frame
            .lines()
            .any(|line| line.trim() == format!("event: {event}"))
    );
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>()
        .join("\n");
    serde_json::from_str(&data).unwrap()
}
