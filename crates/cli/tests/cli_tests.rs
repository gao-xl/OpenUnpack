use openunpack_cli::client::{DaemonConnection, default_data_dir};

#[test]
fn test_default_data_dir_resolution() {
    let dir = default_data_dir();
    assert!(
        dir.is_ok(),
        "Default data dir should resolve on standard OS"
    );
    let p = dir.unwrap();
    assert!(!p.to_string_lossy().is_empty());
}

#[tokio::test]
async fn test_offline_daemon_connection_error_handled() {
    // Pick an unlikely bound port
    let conn = DaemonConnection {
        base_url: "http://127.0.0.1:59999".into(),
        token: "fake-token".into(),
        port: 59999,
    };

    let res = conn.get_status().await;
    assert!(res.is_err(), "Offline daemon should return error");
    let err_msg = res.unwrap_err();
    assert!(
        err_msg.contains("Cannot connect to OpenUnpack daemon")
            || err_msg.contains("Is daemon running"),
        "Error message should explain connection failure nicely: {err_msg}"
    );
}

#[tokio::test]
async fn status_uses_an_authenticated_queue_command_instead_of_an_unregistered_route() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut buffer = [0u8; 1024];
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
            if let Some(i) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]).to_lowercase();
        assert!(headers.starts_with("post /api/v1/commands http/1.1"));
        assert!(headers.contains("authorization: bearer synthetic-cli-token"));
        let length: usize = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        while bytes.len() < header_end + length {
            let mut buffer = [0u8; 1024];
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
        }
        let command: serde_json::Value =
            serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
        assert_eq!(command["command"], "queue_snapshot");
        let body=serde_json::json!({"requestId":"synthetic","data":{"running":false,"jobs":[]},"meta":{"apiVersion":1}}).to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket.write_all(response.as_bytes()).await.unwrap();
    });
    let conn = DaemonConnection {
        base_url: format!("http://127.0.0.1:{port}"),
        token: "synthetic-cli-token".into(),
        port,
    };
    let status = conn.get_status().await.unwrap();
    assert_eq!(status["running"], false);
    assert_eq!(status["port"], port);
    server.await.unwrap();
}
