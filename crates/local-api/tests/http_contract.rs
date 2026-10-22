mod support;

use openunpack_local_api::{ApiCommand, ApiSettings, Scope};
use openunpack_protocol::{Settings, StateInvalidated};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    net::{Ipv4Addr, TcpListener, TcpStream},
};
use support::{EventStream, Harness, event_json, exchange};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listener_is_loopback_and_shutdown_releases_the_exact_port() {
    let api = Harness::start().await;
    let port = api.port;
    let health = api.request("GET", "/api/v1/health", &[], b"").await;
    assert_eq!(health.status, 200);
    assert_eq!(health.json()["data"]["service"], "OpenUnpack");
    assert_eq!(health.headers["x-frame-options"], "DENY");
    // Windows may permit overlapping wildcard/loopback binds, so assert the
    // address captured from the real listener rather than a bind-conflict oracle.
    assert_eq!(api.local_addr().ip(), Ipv4Addr::LOCALHOST);
    assert_eq!(api.local_addr().port(), port);
    api.stop().await;
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
    let reused =
        TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("shutdown retained listener");
    assert_eq!(reused.local_addr().unwrap().ip(), Ipv4Addr::LOCALHOST);
}

#[tokio::test]
async fn selective_extraction_requires_both_file_scopes_and_valid_relative_entries() {
    let api = Harness::start().await;
    let source = std::env::current_dir().unwrap().join("synthetic.zip");
    let request = json!({"command":"extract_selective", "args":{"request":{
        "archivePath":source,"entryPaths":["folder/file.txt"],"flatten":false
    }}});
    let reader = api.client(&[Scope::FilesRead]);
    api.command(&reader.token, request.clone())
        .await
        .error(403, "permission_denied");
    assert_eq!(api.backend.count(), 0);
    let writer = api.client(&[Scope::FilesRead, Scope::FilesWrite]);
    let valid = api.command(&writer.token, request.clone()).await;
    assert_eq!(valid.status, 200);
    assert_eq!(api.backend.count(), 1);
    for path in [
        "../escape.txt",
        "/absolute",
        "file.txt:stream",
        "a/../b",
        "",
    ] {
        let mut bad = request.clone();
        bad["args"]["request"]["entryPaths"] = json!([path]);
        let invalid = api.command(&writer.token, bad).await;
        assert_eq!(invalid.status, 400);
    }
    assert_eq!(api.backend.count(), 1);
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_wrong_revoked_and_disabled_tokens_never_reach_the_backend() {
    let api = Harness::start().await;
    let client = api.client(&[Scope::JobsRead]);
    let missing = api.request("GET", "/api/v1/capabilities", &[], b"").await;
    missing.error(401, "unauthorized");
    assert_eq!(missing.headers["www-authenticate"], "Bearer");
    let mut wrong = client.token.clone();
    let final_char = wrong.pop().unwrap();
    wrong.push(if final_char == 'a' { 'b' } else { 'a' });
    for value in [
        format!("Bearer {wrong}"),
        client.token.clone(),
        "Basic synthetic".into(),
    ] {
        api.request(
            "GET",
            "/api/v1/capabilities",
            &[("Authorization", &value)],
            b"",
        )
        .await
        .error(401, "unauthorized");
    }
    let valid_header = format!("Bearer {}", client.token);
    let wrong_header = format!("Bearer {wrong}");
    for headers in [
        vec![
            ("Authorization", valid_header.as_str()),
            ("Authorization", valid_header.as_str()),
        ],
        vec![
            ("Authorization", valid_header.as_str()),
            ("Authorization", wrong_header.as_str()),
        ],
        vec![
            ("Authorization", wrong_header.as_str()),
            ("Authorization", valid_header.as_str()),
        ],
    ] {
        api.request("GET", "/api/v1/capabilities", &headers, b"")
            .await
            .error(401, "unauthorized");
    }
    let authorized = api
        .command(&client.token, json!({"command":"queue_snapshot"}))
        .await;
    assert_eq!(authorized.status, 200);
    assert_eq!(api.backend.count(), 1);
    api.store.revoke_client(&client.client.id).unwrap();
    api.command(&client.token, json!({"command":"queue_snapshot"}))
        .await
        .error(401, "unauthorized");
    let replacement = api.client(&[Scope::JobsRead]);
    api.store
        .save_settings(ApiSettings {
            enabled: false,
            port: api.port,
        })
        .unwrap();
    api.command(&replacement.token, json!({"command":"queue_snapshot"}))
        .await
        .error(401, "unauthorized");
    assert_eq!(api.backend.count(), 1);
    // The public liveness endpoint contains no user state even when disabled.
    assert_eq!(
        api.request("GET", "/api/v1/health", &[], b"").await.status,
        200
    );
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_origin_and_query_credentials_are_rejected_before_dispatch() {
    let api = Harness::start().await;
    let client = api.client(&[Scope::JobsRead]);
    let auth = format!("Bearer {}", client.token);
    for host in [
        "evil.example:47980".to_owned(),
        format!("127.0.0.1.evil.example:{}", api.port),
        "127.0.0.1".into(),
    ] {
        api.request(
            "GET",
            "/api/v1/capabilities",
            &[("Host", &host), ("Authorization", &auth)],
            b"",
        )
        .await
        .error(403, "permission_denied");
    }
    for origin in [
        "null".to_owned(),
        "https://evil.example".into(),
        format!("https://127.0.0.1:{}", api.port),
    ] {
        api.request(
            "GET",
            "/api/v1/capabilities",
            &[("Origin", &origin), ("Authorization", &auth)],
            b"",
        )
        .await
        .error(403, "permission_denied");
    }
    let same = format!("http://127.0.0.1:{}", api.port);
    assert_eq!(
        api.request(
            "GET",
            "/api/v1/capabilities",
            &[("Origin", &same), ("Authorization", &auth)],
            b""
        )
        .await
        .status,
        200
    );
    for origin in [
        format!("http://localhost:{}", api.port),
        "http://tauri.localhost".to_string(),
        "tauri://localhost".to_string(),
    ] {
        let response = api
            .request(
                "GET",
                "/api/v1/capabilities",
                &[("Origin", &origin), ("Authorization", &auth)],
                b"",
            )
            .await;
        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("access-control-allow-origin"),
            Some(&origin)
        );
    }
    for path in [
        format!("/api/v1/capabilities?token={}", client.token),
        "/api/v1/capabilities?unrelated=1".into(),
    ] {
        let response = api
            .request("GET", &path, &[("Authorization", &auth)], b"")
            .await;
        response.error(400, "invalid_request");
        assert!(!response.text().contains(&client.token));
    }
    let missing_host = exchange(
        api.port,
        b"GET /api/v1/health HTTP/1.0\r\nConnection: close\r\n\r\n".to_vec(),
    )
    .await;
    assert!(matches!(missing_host.status, 400 | 403));
    assert_eq!(api.backend.count(), 0);
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_and_named_commands_agree_and_reject_unknown_or_extra_arguments() {
    let api = Harness::start().await;
    let client = api.client(&[Scope::JobsRead]);
    let typed = api
        .command(
            &client.token,
            json!({"command":"job_details","args":{"jobId":"synthetic-job"}}),
        )
        .await;
    let named = api
        .request(
            "POST",
            "/api/v1/commands/job_details",
            &[("Authorization", &format!("Bearer {}", client.token))],
            br#"{"jobId":"synthetic-job"}"#,
        )
        .await;
    assert_eq!(typed.status, 200);
    assert_eq!(named.status, 200);
    assert_eq!(typed.json()["data"], named.json()["data"]);
    assert_ne!(typed.json()["requestId"], named.json()["requestId"]);
    for body in [
        json!({"command":"api_create_client","args":{}}),
        json!({"command":"job_details","args":{}}),
        json!({"command":"job_details","args":{"jobId":"x","unexpected":true}}),
        json!({"command":"queue_snapshot","unexpected":true}),
    ] {
        api.command(&client.token, body)
            .await
            .error(400, "invalid_request");
    }
    api.request(
        "POST",
        "/api/v1/commands/not_a_command",
        &[("Authorization", &format!("Bearer {}", client.token))],
        b"{}",
    )
    .await
    .error(400, "invalid_request");
    assert_eq!(api.backend.count(), 2);
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scope_checks_and_source_cleanup_require_explicit_file_delete_permission() {
    let api = Harness::start().await;
    let reader = api.client(&[Scope::JobsRead]);
    for body in [
        json!({"command":"snapshot"}),
        json!({"command":"read_settings"}),
        json!({"command":"pause_jobs"}),
        json!({"command":"reveal_password","args":{"id":"synthetic-id"}}),
    ] {
        api.command(&reader.token, body)
            .await
            .error(403, "permission_denied");
    }
    assert_eq!(api.backend.count(), 0);
    let setter = api.client(&[Scope::SettingsWrite]);
    let settings = Settings::default();
    assert_eq!(
        api.command(
            &setter.token,
            json!({"command":"save_settings","args":{"settings":settings}})
        )
        .await
        .status,
        200
    );
    let deleting = Settings {
        source_cleanup: "delete".into(),
        ..Settings::default()
    };
    api.command(
        &setter.token,
        json!({"command":"save_settings","args":{"settings":deleting}}),
    )
    .await
    .error(403, "permission_denied");
    let writer = api.client(&[Scope::JobsWrite, Scope::FilesRead, Scope::FilesWrite]);
    api.command(
        &writer.token,
        json!({"command":"add_jobs","args":{"sources":[],"options":deleting}}),
    )
    .await
    .error(403, "permission_denied");
    let toolbox = json!({"command":"execute_toolbox_operation","args":{"request":{"options":{"kind":"flattenDirectory","sourceDirectory":api.directory.path().to_string_lossy(),"targetDirectory":null,"recursive":false,"removeEmptyFolders":true,"conflictStrategy":"rename","renameRule":null},"approvedPaths":[],"token":null}}});
    api.command(&writer.token, toolbox.clone())
        .await
        .error(403, "permission_denied");
    assert_eq!(api.backend.count(), 1);
    let approved = api.client(&[
        Scope::SettingsWrite,
        Scope::JobsWrite,
        Scope::FilesRead,
        Scope::FilesWrite,
        Scope::FilesDelete,
    ]);
    assert_eq!(
        api.command(
            &approved.token,
            json!({"command":"save_settings","args":{"settings":deleting}})
        )
        .await
        .status,
        200
    );
    assert_eq!(
        api.command(
            &approved.token,
            json!({"command":"add_jobs","args":{"sources":[],"options":deleting}})
        )
        .await
        .status,
        200
    );
    assert_eq!(api.command(&approved.token, toolbox).await.status, 200);
    assert_eq!(api.backend.count(), 4);
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_and_oversized_json_never_echo_synthetic_request_content() {
    let api = Harness::start().await;
    let client = api.client(&[Scope::JobsRead]);
    let authorization = format!("Bearer {}", client.token);
    let marker = "SYNTHETIC_BODY_MUST_NOT_APPEAR_IN_ERRORS";
    for (content_type, body) in [
        ("application/json", format!("{{\"{marker}\":")),
        ("text/plain", marker.into()),
        ("application/json", format!("{{\"command\":\"{marker}\"}}")),
    ] {
        let response = api
            .request(
                "POST",
                "/api/v1/commands",
                &[
                    ("Authorization", &authorization),
                    ("Content-Type", content_type),
                ],
                body.as_bytes(),
            )
            .await;
        response.error(400, "invalid_request");
        assert!(!response.text().contains(marker));
        assert!(!response.text().contains(&client.token));
    }
    let mut oversized = vec![b' '; 20 * 1024 * 1024 + 1];
    oversized[..marker.len()].copy_from_slice(marker.as_bytes());
    let response = api
        .request(
            "POST",
            "/api/v1/commands",
            &[("Authorization", &authorization)],
            &oversized,
        )
        .await;
    response.error(413, "request_too_large");
    assert!(!response.text().contains(marker));
    assert!(!response.text().contains(&client.token));
    assert_eq!(api.backend.count(), 0);
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_start_requires_jobs_write_and_valid_unique_ids_on_both_routes() {
    let api = Harness::start().await;
    let reader = api.client(&[Scope::JobsRead]);
    let writer = api.client(&[Scope::JobsWrite]);
    api.command(
        &reader.token,
        json!({"command":"start_selected_jobs","args":{"jobIds":["job-one"]}}),
    )
    .await
    .error(403, "permission_denied");
    for ids in [
        json!([]),
        json!([""]),
        json!([" "]),
        json!(["one", "one"]),
        json!(
            (0..1001)
                .map(|index| format!("job-{index}"))
                .collect::<Vec<_>>()
        ),
    ] {
        api.command(
            &writer.token,
            json!({"command":"start_selected_jobs","args":{"jobIds":ids}}),
        )
        .await
        .error(400, "invalid_request");
    }
    assert_eq!(api.backend.count(), 0);
    assert_eq!(
        api.command(
            &writer.token,
            json!({"command":"start_selected_jobs","args":{"jobIds":["job-one","job-two"]}})
        )
        .await
        .status,
        200
    );
    let auth = format!("Bearer {}", writer.token);
    assert_eq!(
        api.request(
            "POST",
            "/api/v1/commands/start_selected_jobs",
            &[("Authorization", &auth)],
            br#"{"jobIds":["job-three"]}"#
        )
        .await
        .status,
        200
    );
    api.request(
        "POST",
        "/api/v1/commands/start_selected_jobs",
        &[("Authorization", &auth)],
        br#"{"job_ids":["job-three"]}"#,
    )
    .await
    .error(400, "invalid_request");
    assert_eq!(api.backend.count(), 2);
    api.stop().await;
}

#[test]
fn retry_preserve_options_is_opt_in_and_uses_the_camel_case_contract() {
    let legacy = serde_json::from_value::<ApiCommand>(json!({
        "command":"retry_job", "args":{"jobId":"one"}
    }))
    .unwrap_or_else(|_| panic!("legacy retry request must remain valid"));
    assert!(matches!(
        legacy,
        ApiCommand::RetryJob {
            partial: false,
            preserve_options: false,
            password: None,
            ..
        }
    ));
    let preserving = serde_json::from_value::<ApiCommand>(json!({
        "command":"retry_job", "args":{"jobId":"one","preserveOptions":true,"password":"synthetic"}
    }))
    .unwrap_or_else(|_| panic!("preserving retry request must be valid"));
    assert!(matches!(
        preserving,
        ApiCommand::RetryJob {
            preserve_options: true,
            ..
        }
    ));
    assert!(
        serde_json::from_value::<ApiCommand>(json!({
            "command":"retry_job", "args":{"jobId":"one","preserve_options":true}
        }))
        .is_err()
    );
    let legacy_node = serde_json::from_value::<ApiCommand>(json!({
        "command":"retry_node", "args":{"jobId":"one", "nodeId":"inner"}
    }))
    .unwrap_or_else(|_| panic!("legacy node retry must remain valid"));
    assert!(matches!(
        legacy_node,
        ApiCommand::RetryNode {
            preserve_options: false,
            ..
        }
    ));
    let preserving_node = serde_json::from_value::<ApiCommand>(json!({
        "command":"retry_node", "args":{"jobId":"one", "nodeId":"inner", "preserveOptions":true}
    }))
    .unwrap_or_else(|_| panic!("preserving node retry must be valid"));
    assert!(matches!(
        preserving_node,
        ApiCommand::RetryNode {
            preserve_options: true,
            ..
        }
    ));
}

fn attributes(html: &str, name: &str) -> Vec<String> {
    let mut values = Vec::new();
    for quote in ['\"', '\''] {
        let pattern = format!("{name}={quote}");
        for tail in html.split(&pattern).skip(1) {
            if let Some(value) = tail.split(quote).next() {
                values.push(value.to_owned());
            }
        }
    }
    values
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn openapi_and_swagger_are_served_with_local_assets_and_no_key_admin_route() {
    let api = Harness::start().await;
    let spec = api.request("GET", "/openapi.json", &[], b"").await;
    assert_eq!(spec.status, 200);
    let spec = spec.json();
    assert!(spec["openapi"].as_str().unwrap().starts_with('3'));
    assert_eq!(
        spec["components"]["securitySchemes"]["bearerAuth"]["scheme"],
        "bearer"
    );
    assert!(spec["paths"]["/api/v1/commands"]["post"]["requestBody"].is_object());
    assert!(spec["paths"]["/api/v1/events"]["get"]["security"].is_array());
    assert!(
        !spec["paths"]
            .as_object()
            .unwrap()
            .keys()
            .any(|path| path.contains("api_create_client") || path.contains("revoke_client"))
    );
    let docs = api.request("GET", "/docs/", &[], b"").await;
    assert_eq!(docs.status, 200);
    let html = docs.text();
    let assets: Vec<_> = attributes(&html, "src")
        .into_iter()
        .chain(attributes(&html, "href"))
        .filter(|asset| !asset.starts_with("data:") && asset != "#")
        .collect();
    assert!(
        assets.iter().any(|asset| asset.ends_with(".js")),
        "Swagger JavaScript asset missing"
    );
    assert!(
        assets.iter().any(|asset| asset.ends_with(".css")),
        "Swagger stylesheet missing"
    );
    for asset in assets {
        assert!(
            !asset.contains("://") && !asset.starts_with("//"),
            "Swagger references an external resource"
        );
        let target = if asset.starts_with('/') {
            asset
        } else {
            format!("/docs/{}", asset.trim_start_matches("./"))
        };
        let response = api.request("GET", &target, &[], b"").await;
        assert_eq!(
            response.status, 200,
            "local Swagger asset unavailable: {target}"
        );
        assert!(!response.body.is_empty());
    }
    assert_eq!(api.backend.count(), 0);
    api.stop().await;
}

fn resolve_schema<'a>(mut schema: &'a Value, spec: &'a Value) -> &'a Value {
    for _ in 0..8 {
        match schema.get("$ref").and_then(Value::as_str) {
            Some(reference) => {
                assert!(reference.starts_with("#/components/schemas/"));
                schema = spec
                    .pointer(&reference[1..])
                    .expect("unresolved local OpenAPI schema");
            }
            None => return schema,
        }
    }
    panic!("OpenAPI schema reference cycle");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_catalog_and_openapi_have_all_46_unique_matching_command_names() {
    let api = Harness::start().await;
    let client = api.client(&[Scope::JobsRead]);
    let response = api
        .request(
            "GET",
            "/api/v1/capabilities",
            &[("Authorization", &format!("Bearer {}", client.token))],
            b"",
        )
        .await;
    assert_eq!(response.status, 200);
    let response = response.json();
    let catalog = response["data"]["commands"]
        .as_array()
        .expect("command catalog missing");
    let catalog_names: BTreeSet<_> = catalog
        .iter()
        .map(|name| name.as_str().expect("non-string catalog entry"))
        .collect();
    let implemented: BTreeSet<_> = ApiCommand::NAMES.iter().copied().collect();
    assert_eq!(ApiCommand::NAMES.len(), 46);
    assert_eq!(
        implemented.len(),
        46,
        "duplicate command in implementation catalog"
    );
    assert_eq!(catalog.len(), 46);
    assert_eq!(
        catalog_names.len(),
        catalog.len(),
        "duplicate advertised command"
    );
    assert_eq!(catalog_names, implemented);
    let response = api.request("GET", "/openapi.json", &[], b"").await;
    assert_eq!(response.status, 200);
    let spec = response.json();
    let variants = spec["components"]["schemas"]["ApiCommand"]["oneOf"]
        .as_array()
        .expect("typed command oneOf missing");
    assert_eq!(variants.len(), 46);
    let names: BTreeSet<_> = variants
        .iter()
        .map(|variant| {
            let variant = resolve_schema(variant, &spec);
            let command = resolve_schema(&variant["properties"]["command"], &spec);
            if let Some(value) = command.get("const").and_then(Value::as_str) {
                return value;
            }
            let names = command["enum"]
                .as_array()
                .expect("command discriminator missing");
            assert_eq!(names.len(), 1, "command variant must have one name");
            names[0].as_str().expect("non-string command discriminator")
        })
        .collect();
    assert_eq!(
        names.len(),
        variants.len(),
        "duplicate OpenAPI command variant"
    );
    assert_eq!(names, implemented);
    assert_eq!(api.backend.count(), 0);
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sse_contains_only_revision_invalidations_and_closes_after_revocation() {
    let api = Harness::start().await;
    let denied = api.client(&[Scope::SettingsRead]);
    api.request(
        "GET",
        "/api/v1/events",
        &[("Authorization", &format!("Bearer {}", denied.token))],
        b"",
    )
    .await
    .error(403, "permission_denied");
    let client = api.client(&[Scope::JobsRead]);
    let stream = EventStream::open(api.port, client.token.clone()).await;
    let (stream, ready) = stream.read_frame().await;
    assert_eq!(
        event_json(&ready.unwrap(), "ready"),
        json!({"apiVersion":1,"resync":true})
    );
    api.backend
        .invalidations
        .send(StateInvalidated {
            session_id: "synthetic-session".into(),
            revision: "18446744073709551616".into(),
        })
        .unwrap();
    let (stream, changed) = stream.read_frame().await;
    let value = event_json(&changed.unwrap(), "state-changed");
    assert_eq!(
        value,
        json!({"sessionId":"synthetic-session","revision":"18446744073709551616"})
    );
    let keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(api.backend.count(), 0);
    api.store.revoke_client(&client.client.id).unwrap();
    let (stream, closed) = stream.read_frame().await;
    assert!(
        closed.is_none(),
        "revoked application retained an event stream"
    );
    drop(stream);
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabling_api_closes_existing_event_stream_without_waiting_for_an_update() {
    let api = Harness::start().await;
    let client = api.client(&[Scope::JobsRead]);
    let stream = EventStream::open(api.port, client.token).await;
    let (stream, ready) = stream.read_frame().await;
    assert!(ready.is_some());
    api.store
        .save_settings(ApiSettings {
            enabled: false,
            port: api.port,
        })
        .unwrap();
    let (stream, closed) = stream.read_frame().await;
    assert!(closed.is_none());
    drop(stream);
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn response_envelope_includes_production_metadata_and_duration() {
    let api = Harness::start().await;
    let client = api.client(&[Scope::JobsRead]);
    let response = api
        .command(&client.token, json!({"command":"queue_snapshot"}))
        .await;
    assert_eq!(response.status, 200);
    let json = response.json();
    assert!(json["requestId"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(json["data"].is_object());
    let meta = &json["meta"];
    assert_eq!(meta["apiVersion"], 1);
    assert!(meta["timestamp"].as_i64().is_some_and(|ts| ts > 0));
    assert!(meta["durationMs"].as_f64().is_some_and(|d| d >= 0.0));
    api.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn validation_errors_conform_to_rfc7807_problem_details_with_field_errors() {
    let api = Harness::start().await;
    let client = api.client(&[Scope::JobsRead]);
    // 发送带有非法字符的 jobId
    let response = api
        .command(
            &client.token,
            json!({"command":"job_details","args":{"jobId":"bad/id/with/slashes"}}),
        )
        .await;
    assert_eq!(response.status, 400);
    let json = response.json();
    assert_eq!(json["error"]["code"], "invalid_request");
    assert_eq!(
        json["error"]["type"],
        "https://openunpack.local/errors/validation-failed"
    );
    assert_eq!(json["error"]["title"], "Validation Failed");
    let invalid_fields = json["error"]["invalidFields"]
        .as_array()
        .expect("field errors list missing");
    assert!(!invalid_fields.is_empty());
    assert_eq!(invalid_fields[0]["field"], "jobId");
    assert!(invalid_fields[0]["reason"].as_str().is_some());
    api.stop().await;
}
