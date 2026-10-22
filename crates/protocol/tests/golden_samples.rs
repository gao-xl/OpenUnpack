use openunpack_protocol::{desktop::*, error::WireErrorCode, worker::*};

#[test]
fn test_desktop_settings_serialization_camel_case() {
    let settings = Settings::default();
    let json = serde_json::to_string(&settings).expect("serialize settings");
    assert!(json.contains("\"outputRoot\":"));
    assert!(json.contains("\"conflictPolicy\":\"rename\""));
    assert!(json.contains("\"cacheEnabled\":false"));
    assert!(json.contains("\"cachePolicy\":\"auto\""));
    assert!(json.contains("\"unwrapSingleRoot\":false"));
    assert!(json.contains("\"minimizeToTray\":false"));
    assert!(json.contains("\"concurrency\":1"));

    let deserialized: Settings = serde_json::from_str(&json).expect("deserialize settings");
    assert_eq!(settings, deserialized);

    // Verify backward compatibility when concurrency field is missing
    let json_without_concurrency = r#"{
        "outputRoot": "",
        "conflictPolicy": "rename",
        "passwordGroup": "",
        "cacheEnabled": false,
        "cacheInput": true,
        "cacheRoot": "",
        "cacheQuotaBytes": 21474836480,
        "minFreeBytes": 1073741824,
        "expandNested": true,
        "maxDepth": 2,
        "verification": "standard",
        "maxEntries": 100000,
        "maxExpandedBytes": 107374182400,
        "recursiveScan": true,
        "inputExclusions": [],
        "outputExclusions": [],
        "smallFirst": false,
        "theme": "system",
        "historyLimit": 500,
        "alwaysOnTop": false,
        "completionSound": false,
        "minimizeToTray": false,
        "unwrapSingleRoot": false,
        "sourceCleanup": "keep"
    }"#;
    let old_compat: Settings =
        serde_json::from_str(json_without_concurrency).expect("deserialize old settings");
    assert_eq!(old_compat.concurrency, 1);
    assert_eq!(old_compat.cache_policy, "auto");
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
    assert!(
        view.transfer_progress.is_none(),
        "old task JSON has no runtime observation"
    );

    let serialized = serde_json::to_string(&view).expect("serialize job view");
    assert!(serialized.contains("\"id\":\"job-101\""));
    assert!(serialized.contains("\"displayName\":\"archive.zip\""));
    assert!(serialized.contains("\"transferProgress\":null"));
}

#[test]
fn test_transfer_progress_roundtrip_has_camel_case_nullable_estimates() {
    let progress = TransferProgress {
        completed_bytes: 1024,
        total_bytes: 2048,
        bytes_per_second: Some(512.0),
        remaining_seconds: Some(2.0),
        updated_at: "2026-10-11T00:00:00Z".into(),
    };
    let json = serde_json::to_value(&progress).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "completedBytes": 1024, "totalBytes": 2048,
            "bytesPerSecond": 512.0, "remainingSeconds": 2.0,
            "updatedAt": "2026-10-11T00:00:00Z"
        })
    );
    assert_eq!(
        serde_json::from_value::<TransferProgress>(json).unwrap(),
        progress
    );
    let warming = TransferProgress {
        bytes_per_second: None,
        remaining_seconds: None,
        ..progress
    };
    let warming_json = serde_json::to_value(warming).unwrap();
    assert!(warming_json.get("bytesPerSecond").unwrap().is_null());
    assert!(warming_json.get("remainingSeconds").unwrap().is_null());
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
        inclusions: vec![],
        candidate_passwords: vec!["pw1".into()],
        candidates: vec![CandidateItem {
            candidate_id: "cand-1".into(),
            source: "vault".into(),
            revision: 1,
            label: "test".into(),
            secret: "pw1".into(),
        }],
        engine_threads: 0,
        worker_priority: "belowNormal".into(),
        salvage_corrupted: false,
    };

    let json = serde_json::to_string(&req).expect("serialize worker request");
    assert!(json.contains("\"protocolVersion\":1"));
    assert!(json.contains("\"candidatePasswords\":[\"pw1\"]"));
    assert!(json.contains("\"candidates\":["));

    let deserialized: WorkerRequest =
        serde_json::from_str(&json).expect("deserialize worker request");
    assert_eq!(req, deserialized);
}

#[test]
fn test_worker_event_error_code_mapping() {
    let event = WorkerEvent {
        protocol_version: WORKER_PROTOCOL_VERSION_V1,
        request_id: "req-1".into(),
        kind: "completed".into(),
        hello: None,
        operation_id: Some("op-1".into()),
        generation: Some(1),
        completed_bytes: 100,
        total_bytes: 100,
        total_bytes_known: true,
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

#[test]
fn test_archive_inspection_and_file_record_encryption_roundtrip() {
    let record_json = r#"{"relativePath":"folder/secret.txt","size":512,"sha256":""}"#;
    let record: FileRecord = serde_json::from_str(record_json)
        .expect("deserialize file record with default isEncrypted");
    assert!(!record.is_directory);
    assert!(!record.is_encrypted);

    let enc_record = FileRecord {
        relative_path: "folder/secret.txt".into(),
        size: 512,
        size_known: true,
        sha256: String::new(),
        is_directory: false,
        is_encrypted: true,
        is_corrupted: false,
    };
    let enc_json = serde_json::to_string(&enc_record).expect("serialize encrypted file record");
    assert!(enc_json.contains("\"isEncrypted\":true"));
    let enc_roundtrip: FileRecord =
        serde_json::from_str(&enc_json).expect("roundtrip encrypted file record");
    assert_eq!(enc_record, enc_roundtrip);

    let view = ArchiveInspectionView {
        archive_path: "C:\\test\\sample.7z".into(),
        total_entries: 2,
        total_size: 512,
        total_size_known: true,
        encrypted_entries: 1,
        entries: vec![
            ArchiveEntryView {
                path: "folder".into(),
                size: 0,
                size_known: true,
                is_directory: true,
                is_encrypted: false,
                raw_path: String::new(),
            },
            ArchiveEntryView {
                path: "folder/secret.txt".into(),
                size: 512,
                size_known: true,
                is_directory: false,
                is_encrypted: true,
                raw_path: String::new(),
            },
        ],
        detected_encoding: String::new(),
    };
    let view_json = serde_json::to_value(&view).expect("serialize ArchiveInspectionView");
    assert_eq!(
        view_json,
        serde_json::json!({
            "archivePath": "C:\\test\\sample.7z",
            "totalEntries": 2,
            "totalSize": 512,
            "totalSizeKnown": true,
            "encryptedEntries": 1,
            "entries": [
                {
                    "path": "folder",
                    "size": 0,
                    "sizeKnown": true,
                    "isDirectory": true,
                    "isEncrypted": false,
                    "rawPath": ""
                },
                {
                    "path": "folder/secret.txt",
                    "size": 512,
                    "sizeKnown": true,
                    "isDirectory": false,
                    "isEncrypted": true,
                    "rawPath": ""
                }
            ],
            "detectedEncoding": ""
        })
    );
    let view_roundtrip: ArchiveInspectionView =
        serde_json::from_value(view_json).expect("deserialize ArchiveInspectionView");
    assert_eq!(view, view_roundtrip);
}
