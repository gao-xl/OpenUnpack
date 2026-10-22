//! Explicit Windows acceptance: real HTTP -> DesktopBackend -> Coordinator -> Worker -> 7-Zip.
//! Synthetic fixtures only. Opt in with OPENUNPACK_TEST_WORKER and --ignored.
use super::*;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

async fn command(port: u16, token: &str, value: Value) -> Value {
    command_status(port, token, value, 200).await
}

async fn command_status(port: u16, token: &str, value: Value, status: u16) -> Value {
    let token = token.to_owned();
    tokio::task::spawn_blocking(move || {
        let body = serde_json::to_vec(&value).unwrap();
        let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        socket.set_write_timeout(Some(Duration::from_secs(10))).unwrap();
        write!(socket, "POST /api/v1/commands HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
        socket.write_all(&body).unwrap();
        let mut response = Vec::new();
        socket.take(2 * 1024 * 1024).read_to_end(&mut response).unwrap();
        let boundary = response.windows(4).position(|bytes| bytes == b"\r\n\r\n").unwrap();
        let header = std::str::from_utf8(&response[..boundary]).unwrap();
        let envelope: Value = serde_json::from_slice(&response[boundary + 4..]).unwrap();
        assert!(header.starts_with(&format!("HTTP/1.1 {status} ")), "unexpected status: {header}; API error: {}", envelope.get("error").unwrap_or(&Value::Null));
        envelope.get(if status == 200 { "data" } else { "error" }).expect("response envelope").clone()
    }).await.unwrap()
}

async fn wait_job(core: &Coordinator, id: &str, input: bool) -> JobView {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let job = core.job_details(id).unwrap().unwrap();
        if input && job.stage == "needsPassword" && job.status == "running" {
            return job;
        }
        if !input && job.status == "completed" {
            return job;
        }
        assert!(
            !matches!(
                job.status.as_str(),
                "failed" | "cancelled" | "interrupted" | "needsPassword" | "stopping"
            ),
            "job failed: {} / {} / {}",
            job.status,
            job.stage,
            job.message
        );
        assert!(
            Instant::now() < deadline,
            "job timeout: {} / {} / {}",
            job.status,
            job.stage,
            job.message
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an explicitly built Windows Worker and official 7-Zip payload"]
async fn native_http_extraction_keeps_passwords_temporary_and_gui_jobs_queued() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let worker = PathBuf::from(
        std::env::var_os("OPENUNPACK_TEST_WORKER").expect("set OPENUNPACK_TEST_WORKER"),
    );
    let engine = PathBuf::from(
        std::env::var_os("OPENUNPACK_TEST_ENGINE").expect("set OPENUNPACK_TEST_ENGINE"),
    );
    let seven_zip = workspace.join("tools/7zip/7z.exe");
    assert!(worker.is_file() && engine.is_file() && seven_zip.is_file());
    let directory = tempfile::tempdir().unwrap();
    let private = directory.path().join("private");
    openunpack_core::paths::DirectoryGuard::open(directory.path())
        .unwrap()
        .create_private_child(&private)
        .unwrap();
    let data = private.join("data");
    let approved = private.join("approved");
    let outside = private.join("outside");
    for directory in [&data, &approved, &outside] {
        fs::create_dir(directory).unwrap();
    }
    let core = Coordinator::open(data.clone(), worker, engine).unwrap();
    core.save_settings(Settings {
        output_root: approved.to_string_lossy().into_owned(),
        cache_root: approved.to_string_lossy().into_owned(),
        min_free_bytes: 0,
        clean_empty_dirs: false,
        source_cleanup: "keep".into(),
        ..Settings::default()
    })
    .unwrap();
    let sentinel = outside.join("unrelated-gui.zip");
    fs::write(&sentinel, b"must remain queued").unwrap();
    let gui = core
        .add_jobs(
            openunpack_core::scanner::scan(&[sentinel.to_string_lossy().into_owned()], false, &[])
                .unwrap()
                .sources,
            core.snapshot().unwrap().settings,
        )
        .unwrap();
    let store = Arc::new(ApiStore::open(&data.join("api.json")).unwrap());
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    store
        .save_settings(ApiSettings {
            enabled: true,
            port,
        })
        .unwrap();
    let created = store
        .create_client(ApiClientInput {
            name: "native API acceptance fixture".into(),
            scopes: [
                Scope::JobsRead,
                Scope::JobsWrite,
                Scope::FilesRead,
                Scope::FilesWrite,
            ]
            .into_iter()
            .collect(),
            read_roots: vec![approved.to_string_lossy().into_owned()],
            write_roots: vec![approved.to_string_lossy().into_owned()],
            expires_at: None,
        })
        .unwrap();
    let state = Arc::new(AppState {
        vault_use_cases: Arc::new(openunpack_application::VaultUseCases::new(Arc::new(
            CoordinatorVaultStore(core.clone()),
        ))),
        sys_env: Arc::new(openunpack_infrastructure::WindowsSystemEnvironment::new()),
        coordinator: core.clone(),
        access: Arc::new(Mutex::new(NativeAccess::default())),
        _lock_file: File::create(data.join("fixture.lock")).unwrap(),
        scans: Mutex::new(HashMap::new()),
        clipboard_sequence: Mutex::new(None),
        exiting: AtomicBool::new(false),
        minimize_to_tray: AtomicBool::new(false),
    });
    let backend = Arc::new(DesktopBackend {
        app: None,
        standalone_state: Some(state.clone()),
        store: store.clone(),
    });
    let server = LocalApiServer::start(store.clone(), backend).await.unwrap();
    let token = created.token;
    for mode in ["plain", "provided", "callback"] {
        let source = approved.join(format!("source-{mode}"));
        fs::create_dir(&source).unwrap();
        let filename = format!("payload-{mode}.txt");
        let expected = format!("OpenUnpack native API fixture: {mode}");
        fs::write(source.join(&filename), expected.as_bytes()).unwrap();
        let archive = approved.join(format!("archive-{mode}.zip"));
        let mut pack = std::process::Command::new(&seven_zip);
        pack.current_dir(&source)
            .args(["a", "-tzip", "-mx=0", "-y", "-bso0", "-bsp0", "-bse0"]);
        if mode != "plain" {
            pack.args(["-pSyntheticNativeApiPassword!", "-mem=AES256"]);
        }
        let packed = pack.arg(&archive).arg(&filename).output().unwrap();
        assert!(
            packed.status.success(),
            "7-Zip fixture creation failed: {}",
            String::from_utf8_lossy(&packed.stderr)
        );
        let added = command(port, &token, serde_json::json!({ "command": "add_archive", "args": { "path": archive, "password": if mode == "provided" { Some("SyntheticNativeApiPassword!") } else { None }, "deleteSource": false, "autoStart": true } })).await;
        let id = added[0]["id"].as_str().unwrap();
        if mode == "callback" {
            wait_job(&core, id, true).await;
            command(port, &token, serde_json::json!({ "command": "submit_input", "args": { "jobId": id, "inputType": "password", "value": "SyntheticNativeApiPassword!" } })).await;
        }
        let completed = wait_job(&core, id, false).await;
        assert_eq!(
            fs::read(Path::new(completed.output_path.as_ref().unwrap()).join(&filename)).unwrap(),
            expected.as_bytes()
        );
        assert!(
            archive.is_file(),
            "deleteSource=false preserves the original"
        );
        assert!(
            core.vault().unwrap().views().unwrap().is_empty(),
            "implicit password persistence"
        );
        let sentinel = core.job_details(&gui[0].id).unwrap().unwrap();
        assert_eq!(sentinel.status, "queued");
        assert_eq!(sentinel.attempts, 0);
        while core.snapshot().unwrap().running {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    // The public selective command uses the real Worker without starting queued GUI jobs.
    let selected_archive = approved.join("archive-provided.zip");
    let selected_source = fs::read(&selected_archive).unwrap();
    let password_needed = command_status(
        port,
        &token,
        serde_json::json!({"command":"extract_selective", "args":{"request":{
            "archivePath": selected_archive, "entryPaths":["payload-provided.txt"],
            "destinationDir": approved, "flatten":false
        }}}),
        400,
    )
    .await;
    let password_message = password_needed["message"].as_str().unwrap();
    assert!(
        password_message.contains("NeedsPassword")
            || password_message.contains("PasswordMismatchOrCorrupt"),
        "password UI must receive a recognized, non-secret error code"
    );
    let selected = command(
        port,
        &token,
        serde_json::json!({"command":"extract_selective", "args":{"request":{
            "archivePath": selected_archive, "entryPaths":["payload-provided.txt"],
            "destinationDir": approved, "password":"SyntheticNativeApiPassword!", "flatten":false
        }}}),
    )
    .await;
    let selected_output = PathBuf::from(selected["outputDirectory"].as_str().unwrap());
    assert_eq!(
        fs::read(selected_output.join("payload-provided.txt")).unwrap(),
        b"OpenUnpack native API fixture: provided"
    );
    assert_eq!(selected["extractedFiles"].as_array().unwrap().len(), 1);
    assert_eq!(fs::read(&selected_archive).unwrap(), selected_source);
    let denied = command_status(
        port,
        &token,
        serde_json::json!({"command":"extract_selective", "args":{"request":{
            "archivePath": selected_archive, "entryPaths":["payload-provided.txt"],
            "destinationDir": outside, "password":"SyntheticNativeApiPassword!", "flatten":false
        }}}),
        403,
    )
    .await;
    assert_eq!(denied["code"], "permission_denied");
    let snapshot = command(
        port,
        &token,
        serde_json::json!({ "command": "queue_snapshot" }),
    )
    .await;
    if let Some(evidence) = std::env::var_os("OPENUNPACK_TEST_SNAPSHOT") {
        fs::write(evidence, serde_json::to_vec_pretty(&snapshot).unwrap()).unwrap();
    }
    serde_json::from_value::<AppSnapshot>(snapshot.clone()).unwrap();
    assert_eq!(snapshot["passwords"], serde_json::json!([]));
    assert!(
        snapshot
            .to_string()
            .find(&outside.to_string_lossy().replace('\\', "\\\\"))
            .is_none()
    );
    if let Some(connection_file) = std::env::var_os("OPENUNPACK_TEST_BROWSER_CONNECTION") {
        // Only opt-in acceptance tooling receives this synthetic fixture key.
        fs::write(
            connection_file,
            serde_json::to_vec(
                &serde_json::json!({ "url": format!("http://127.0.0.1:{port}"), "token": token }),
            )
            .unwrap(),
        )
        .unwrap();
        tokio::time::sleep(Duration::from_secs(120)).await;
    }
    {
        let access = state.access.lock().unwrap();
        assert!(access.readable_entries.is_empty() && access.writable_directories.is_empty());
    }
    server.shutdown().await;
}
