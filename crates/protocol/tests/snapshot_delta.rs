use openunpack_protocol::*;

#[test]
fn snapshot_update_wire_json_matches_contract_and_decimal_revision() {
    let delta = SnapshotDelta {
        jobs_changed: vec![],
        jobs_removed: vec!["job-removed-1".into()],
        passwords_changed: vec![],
        passwords_removed: vec![],
        settings: None,
        state: Some(SnapshotState {
            running: true,
            paused: false,
            vault_error: None,
            data_directory: "C:\\data".into(),
            engine_available: true,
            recoverable_count: 5,
            cache_used_bytes: 1048576,
        }),
    };

    let update = SnapshotUpdate {
        session_id: "test-session".into(),
        revision: 9007199254740993,
        full: None,
        delta: Some(delta),
    };

    let json = serde_json::to_string(&update).unwrap();
    assert!(json.contains(r#""revision":"9007199254740993""#));
    assert!(json.contains(r#""jobsRemoved":["job-removed-1"]"#));
    assert!(json.contains(r#""recoverableCount":5"#));

    let deserialized: SnapshotUpdate = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.revision, 9007199254740993);
    assert_eq!(deserialized.session_id, "test-session");
    assert!(deserialized.full.is_none());
    let d = deserialized.delta.unwrap();
    assert_eq!(d.jobs_removed, vec!["job-removed-1"]);
    assert_eq!(d.state.unwrap().cache_used_bytes, 1048576);
}
