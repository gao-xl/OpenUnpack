use openunpack_protocol::desktop::{AppSnapshot, Settings};

#[test]
fn revision_wire_text_preserves_u64_and_rejects_noncanonical_input() {
    let snapshot = AppSnapshot {
        jobs: vec![],
        passwords: vec![],
        settings: Settings::default(),
        running: false,
        paused: false,
        vault_error: None,
        data_directory: String::new(),
        engine_available: false,
        recoverable_count: 0,
        cache_used_bytes: 0,
        session_id: "session".into(),
        revision: u64::MAX,
    };
    let mut value = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(value["revision"], "18446744073709551615");
    assert_eq!(
        serde_json::from_value::<AppSnapshot>(value.clone())
            .unwrap()
            .revision,
        u64::MAX
    );
    for invalid in [
        serde_json::json!(1),
        serde_json::json!("01"),
        serde_json::json!("-1"),
        serde_json::json!("18446744073709551616"),
        serde_json::json!(null),
    ] {
        value["revision"] = invalid;
        assert!(serde_json::from_value::<AppSnapshot>(value.clone()).is_err());
    }
}
