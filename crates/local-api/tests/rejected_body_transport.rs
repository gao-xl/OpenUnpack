#[allow(dead_code)]
mod support;

use openunpack_local_api::Scope;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    time::Duration,
};
use support::{Harness, Response};

// Unlike the oversized-upload helper, this client never suppresses a failed
// write/reset and verifies the complete declared HTTP response body.
async fn split_request(port: u16, target: String, extra_headers: String) -> Response {
    split_payload_request(
        port,
        target,
        extra_headers,
        br#"{"command":"queue_snapshot"}"#.to_vec(),
        false,
    )
    .await
}

async fn split_payload_request(
    port: u16,
    target: String,
    extra_headers: String,
    body: Vec<u8>,
    chunked: bool,
) -> Response {
    tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect_timeout(
            &SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into(),
            Duration::from_secs(5),
        )
        .expect("split client connect");
        stream.set_nodelay(true).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        stream.set_read_timeout(Some(Duration::from_millis(80))).unwrap();
        let host = if extra_headers.lines().any(|line| line.to_ascii_lowercase().starts_with("host:")) {
            String::new()
        } else {
            format!("Host: 127.0.0.1:{port}\r\n")
        };
        let framing = if chunked { "Transfer-Encoding: chunked\r\n".into() } else { format!("Content-Length: {}\r\n", body.len()) };
        write!(stream, "POST {target} HTTP/1.1\r\n{host}Content-Type: application/json\r\n{framing}Connection: close\r\n{extra_headers}\r\n")
            .expect("split client header write failed");
        let mut byte = [0u8; 1];
        let premature_response = match stream.peek(&mut byte) {
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => false,
            Ok(count) => count > 0,
            Err(error) => panic!("split client header/body gap failed: {error}"),
        };
        let mut wire = if chunked { format!("{:x}\r\n", body.len()).into_bytes() } else { Vec::new() };
        wire.extend_from_slice(&body);
        if chunked { wire.extend_from_slice(b"\r\n0\r\n\r\n"); }
        stream.write_all(&wire[..8]).expect("split client first body write failed");
        std::thread::sleep(Duration::from_millis(10));
        stream.write_all(&wire[8..]).expect("split client final body write failed");
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let response = read_response(&mut stream);
        assert!(!premature_response, "server closed/rejected before a small split request body arrived; status {}", response.status);
        response
    }).await.unwrap()
}

fn read_response(stream: &mut TcpStream) -> Response {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let count = stream
            .read(&mut buffer)
            .expect("split client response read failed (resets are not suppressed)");
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() <= 64 * 1024, "unbounded HTTP error response");
    }
    strict_response(&bytes)
}

fn strict_response(bytes: &[u8]) -> Response {
    let split = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .expect("complete HTTP response headers");
    let mut lines = std::str::from_utf8(&bytes[..split]).unwrap().split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: BTreeMap<_, _> = lines
        .map(|line| {
            let (name, value) = line.split_once(':').expect("HTTP response header field");
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    let length: usize = headers
        .get("content-length")
        .expect("bounded JSON error must have Content-Length")
        .parse()
        .unwrap();
    let body = bytes[split + 4..].to_vec();
    assert_eq!(body.len(), length, "incomplete HTTP response body");
    Response {
        status,
        headers,
        body,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn small_rejected_bodies_can_arrive_after_headers_without_transport_abort() {
    let api = Harness::start().await;
    let revoked = api.client(&[Scope::JobsRead]);
    api.store.revoke_client(&revoked.client.id).unwrap();
    for target in [
        "/api/v1/commands",
        "/api/v1/commands/queue_snapshot",
        "/api/v1/commands/job_details",
    ] {
        split_request(api.port, target.into(), String::new())
            .await
            .error(401, "unauthorized");
        split_request(
            api.port,
            target.into(),
            "Authorization: Bearer SYNTHETIC_INVALID_TOKEN\r\n".into(),
        )
        .await
        .error(401, "unauthorized");
        split_request(
            api.port,
            target.into(),
            "Origin: https://untrusted.invalid\r\n".into(),
        )
        .await
        .error(403, "permission_denied");
        split_request(
            api.port,
            target.into(),
            "Host: untrusted.invalid\r\n".into(),
        )
        .await
        .error(403, "permission_denied");
        split_request(api.port, target.into(), "Authorization: Bearer SYNTHETIC_INVALID_TOKEN\r\nAuthorization: Bearer SYNTHETIC_INVALID_TOKEN\r\n".into()).await.error(401, "unauthorized");
        split_request(
            api.port,
            target.into(),
            format!("Authorization: Bearer {}\r\n", revoked.token),
        )
        .await
        .error(401, "unauthorized");
    }
    split_request(
        api.port,
        "/api/v1/commands?unrelated=1".into(),
        String::new(),
    )
    .await
    .error(400, "invalid_request");
    assert_eq!(
        api.backend.count(),
        0,
        "rejected bodies reached the backend"
    );
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn small_chunked_and_exact_budget_bodies_have_complete_transport_responses() {
    let api = Harness::start().await;
    for (body, chunked) in [
        (br#"{"synthetic":"small"}"#.to_vec(), true),
        (vec![b' '; 64 * 1024], false),
        (vec![b' '; 64 * 1024], true),
    ] {
        split_payload_request(
            api.port,
            "/api/v1/commands".into(),
            String::new(),
            body,
            chunked,
        )
        .await
        .error(401, "unauthorized");
    }
    assert_eq!(api.backend.count(), 0);
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_expect_large_and_stalled_uploads_finish_without_starving_control() {
    let api = Harness::start().await;
    for framing in [
        "Content-Length: 0\r\n",
        "Content-Length: 9\r\nExpect: 100-continue\r\n",
        "Content-Length: 65537\r\n",
    ] {
        let port = api.port;
        let response = tokio::time::timeout(Duration::from_secs(2), tokio::task::spawn_blocking(move || {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
            write!(stream, "POST /api/v1/commands HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{framing}Connection: close\r\n\r\n").expect("fallback header write");
            read_response(&mut stream)
        })).await.expect("immediate rejection stalled waiting for an upload").unwrap();
        response.error(401, "unauthorized");
    }
    let (sent, headers_sent) = tokio::sync::oneshot::channel();
    let port = api.port;
    let stalled = tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(stream, "POST /api/v1/commands HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 9\r\nConnection: close\r\n\r\n").unwrap();
        sent.send(()).unwrap();
        read_response(&mut stream)
    });
    headers_sent.await.unwrap();
    let controller = api.client(&[Scope::JobsWrite]);
    assert_eq!(
        api.request("GET", "/api/v1/health", &[], b"").await.status,
        200
    );
    assert_eq!(
        api.command(
            &controller.token,
            serde_json::json!({"command":"pause_jobs"})
        )
        .await
        .status,
        200
    );
    tokio::time::timeout(Duration::from_secs(5), stalled)
        .await
        .expect("stalled upload exceeded the rejection deadline")
        .unwrap()
        .error(401, "unauthorized");
    assert_eq!(
        api.backend.count(),
        1,
        "only authorized pause may reach the backend"
    );
    api.stop().await;
}
