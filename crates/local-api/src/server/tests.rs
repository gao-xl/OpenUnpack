//! 本机 API 服务端鉴权、请求体拒收与 Web 根目录策略测试。

use super::*;

#[cfg(test)]
pub(super) mod rejected_body_tests {
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

    pub(crate) fn fixture() -> (tempfile::TempDir, ServerState) {
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

    #[test]
    fn csp_forbids_unsafe_inline_scripts_and_blocks_outside_files() {
        let mut response = super::Response::new(axum::body::Body::empty());
        super::response_headers(&mut response, None);
        let csp = response
            .headers()
            .get(super::header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(csp.contains("script-src 'self';"));
        assert!(!csp.contains("script-src 'self' 'unsafe-inline'"));

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("web");
        let outside = dir.path().join("outside.txt");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&outside, b"secret").unwrap();
        assert!(super::is_safe_web_dir(&root).is_some());
        assert!(super::is_safe_web_file(&root, &outside).is_none());
    }
}
