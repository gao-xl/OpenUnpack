use openunpack_protocol::{
    desktop::*, error::WireErrorCode, worker::*,
};

#[test]
fn test_desktop_settings_serialization_camel_case() {
    let settings = Settings::default();
    let json = serde_json::to_string(&settings).expect("serialize settings");
    assert!(json.contains("\"outputRoot\":"));
    assert!(json.contains("\"conflictPolicy\":\"rename\""));
    assert!(json.contains("\"cacheEnabled\":false"));
    assert!(json.contains("\"unwrapSingleRoot\":false"));
    assert!(json.contains("\"minimizeToTray\":false"));

    let deserialized: Settings = serde_json::from_str(&json).expect("deserialize settings");
    assert_eq!(settings, deserialized);
}

#[test]
fn test_desktop_job_view_roundtrip() {
    let job_json = r#"{
        "id": "job-101",
        "source": {
            "path": "C:\\test\\archive.zip",
            "displayName": "archive.zip",
            "volumes": ["C:\\test\\archive.zip"],
            "size": 1024,
            "missingVolume": false
        },
        "options": {},
        "status": "queued",
        "stage": "queued",
        "progress": 0.0,
        "message": "ready",
        "attempts": 0,
        "outputPath": null,
        "verificationScope": "standard",
        "createdAt": "2026-10-10T12:00:00Z",
        "finishedAt": null,
        "workspace": null,
        "nodes": [
            {
                "id": "node-1",
                "parentId": null,
                "name": "root",
                "depth": 0,
                "status": "queued",
                "stage": "queued",
                "progress": 0.0,
                "attempts": 0,
                "message": "",
                "passwordLabel": null
            }
        ],
        "stageTimes": {}
    }"#;

    let view: JobView = serde_json::from_str(job_json).expect("deserialize job view");
    assert_eq!(view.id, "job-101");
    assert_eq!(view.source.display_name, "archive.zip");
    assert_eq!(view.nodes.len(), 1);
    assert_eq!(view.nodes[0].name, "root");

    let serialized = serde_json::to_string(&view).expect("serialize job view");
    assert!(serialized.contains("\"id\":\"job-101\""));
    assert!(serialized.contains("\"displayName\":\"archive.zip\""));
}

#[test]
fn test_worker_request_v1_v2_compat() {
    let req = WorkerRequest {
        protocol_version: WORKER_PROTOCOL_VERSION_V1,
        request_id: "req-1".into(),
        operation: "extract".into(),
        operation_id: Some("op-1".into()),
        generation: Some(1),
        archive_path: "C:\\a.7z".into(),
        destination_directory: "C:\\out".into(),
        password: None,
        allowed_volumes: vec![],
        max_entries: 1000,
        max_total_bytes: 1024 * 1024,
        max_written_bytes: 1024 * 1024,
        min_free_bytes: 1024,
        files: vec![],
        verification: "standard".into(),
        exclusions: vec![],
        candidate_passwords: vec!["pw1".into()],
        candidates: vec![CandidateItem {
            candidate_id: "cand-1".into(),
            source: "vault".into(),
            revision: 1,
            label: "test".into(),
            secret: "pw1".into(),
        }],
    };

    let json = serde_json::to_string(&req).expect("serialize worker request");
    assert!(json.contains("\"protocolVersion\":1"));
    assert!(json.contains("\"candidatePasswords\":[\"pw1\"]"));
    assert!(json.contains("\"candidates\":["));

    let deserialized: WorkerRequest = serde_json::from_str(&json).expect("deserialize worker request");
    assert_eq!(req, deserialized);
}

#[test]
fn test_worker_event_error_code_mapping() {
    let event = WorkerEvent {
        protocol_version: WORKER_PROTOCOL_VERSION_V1,
        request_id: "req-1".into(),
        kind: "completed".into(),
        operation_id: Some("op-1".into()),
        generation: Some(1),
        completed_bytes: 100,
        total_bytes: 100,
        message: "done".into(),
        error_code: WireErrorCode::PasswordRequired.as_str().to_string(),
        files: vec![],
        manifest_path: None,
        manifest_id: None,
        manifest_sha256: None,
        manifest_byte_length: None,
        password_used: true,
        matched_password: None,
        matched_candidate_id: Some("cand-1".into()),
        probe_status: None,
        fingerprint: None,
        verification_scope: "standard".into(),
    };

    let json = serde_json::to_string(&event).expect("serialize worker event");
    assert!(json.contains("\"errorCode\":\"PASSWORD_REQUIRED\""));
    assert!(json.contains("\"matchedCandidateId\":\"cand-1\""));

    let err_code = WireErrorCode::from_raw(&event.error_code);
    assert_eq!(err_code, WireErrorCode::PasswordRequired);
}
