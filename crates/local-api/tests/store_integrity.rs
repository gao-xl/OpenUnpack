use openunpack_local_api::{ApiClientInput, ApiSettings, ApiStore, Scope};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs};

fn input(name: &str) -> ApiClientInput {
    ApiClientInput {
        name: name.into(),
        scopes: BTreeSet::from([Scope::JobsRead]),
        read_roots: vec![],
        write_roots: vec![],
        expires_at: None,
    }
}

#[test]
fn settings_default_disabled_and_zero_port_or_invalid_grants_do_not_persist() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local-api.json");
    let store = ApiStore::open(&path).unwrap();
    assert!(!store.settings().unwrap().enabled);
    assert!(
        !path.exists(),
        "opening a missing store unexpectedly changed disk"
    );
    assert!(
        store
            .save_settings(ApiSettings {
                enabled: true,
                port: 0
            })
            .is_err()
    );
    for name in ["", "   ", "synthetic\nname"] {
        assert!(store.create_client(input(name)).is_err());
    }
    let mut no_scopes = input("empty permissions");
    no_scopes.scopes.clear();
    assert!(store.create_client(no_scopes).is_err());
    let mut relative = input("relative path");
    relative.read_roots.push("relative/not-approved".into());
    assert!(store.create_client(relative).is_err());
    assert!(store.clients().unwrap().is_empty());
    assert!(!path.exists());
}

#[test]
fn tokens_are_one_time_outputs_and_only_hashes_survive_persistence_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local-api.json");
    let store = ApiStore::open(&path).unwrap();
    store
        .save_settings(ApiSettings {
            enabled: true,
            port: 47981,
        })
        .unwrap();
    let first = store
        .create_client(input("first synthetic client"))
        .unwrap();
    let second = store
        .create_client(input("second synthetic client"))
        .unwrap();
    assert_eq!(first.token.len(), 101);
    assert!(first.token.starts_with("oup_"));
    assert_ne!(first.token, second.token);
    let bytes = fs::read(&path).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(!text.contains(&first.token));
    assert!(!text.contains(&second.token));
    let parsed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["clients"].as_array().unwrap().len(), 2);
    let expected_hash = hex::encode(Sha256::digest(first.token.as_bytes()));
    assert_eq!(
        parsed["clients"][0]["tokenHash"].as_str(),
        Some(expected_hash.as_str())
    );
    let public = serde_json::to_string(&store.clients().unwrap()).unwrap();
    assert!(!public.contains("tokenHash") && !public.contains(&first.token));
    drop(store);
    let reopened = ApiStore::open(&path).unwrap();
    assert_eq!(
        reopened.authenticate(&first.token).unwrap().id,
        first.client.id
    );
    assert_eq!(
        reopened.authenticate(&second.token).unwrap().id,
        second.client.id
    );
    reopened.revoke_client(&first.client.id).unwrap();
    assert!(reopened.authenticate(&first.token).is_none());
    assert!(reopened.authenticate(&second.token).is_some());
    let after = fs::read(&path).unwrap();
    assert!(!String::from_utf8(after).unwrap().contains(&expected_hash));
    assert!(!directory.path().read_dir().unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|value| value == "tmp")
    }));
}

#[test]
fn malformed_unknown_version_and_duplicate_authorizations_preserve_original_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local-api.json");
    let store = ApiStore::open(&path).unwrap();
    store
        .save_settings(ApiSettings {
            enabled: true,
            port: 47981,
        })
        .unwrap();
    let _client = store
        .create_client(input("synthetic valid source"))
        .unwrap();
    let valid: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    drop(store);
    let mut unknown_version = valid.clone();
    unknown_version["version"] = json!(2);
    let mut zero_port = valid.clone();
    zero_port["settings"]["port"] = json!(0);
    let mut duplicate_id = valid.clone();
    duplicate_id["clients"]
        .as_array_mut()
        .unwrap()
        .push(valid["clients"][0].clone());
    let mut invalid_hash = valid.clone();
    invalid_hash["clients"][0]["tokenHash"] = json!("not-a-sha256-digest");
    let mut unknown_field = valid.clone();
    unknown_field["unexpected"] = json!(true);
    let mut cases = vec![
        b"{synthetic invalid JSON".to_vec(),
        vec![b' '; 1024 * 1024 + 1],
    ];
    for value in [
        unknown_version,
        zero_port,
        duplicate_id,
        invalid_hash,
        unknown_field,
    ] {
        cases.push(serde_json::to_vec(&value).unwrap());
    }
    for bytes in cases {
        fs::write(&path, &bytes).unwrap();
        assert!(ApiStore::open(&path).is_err());
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "invalid store was replaced or repaired implicitly"
        );
        assert_eq!(directory.path().read_dir().unwrap().count(), 1);
    }
}

#[test]
#[allow(deprecated)] // verifies that the retired picker-grant path stays a no-op
fn failed_save_does_not_change_authorizations_settings_or_original_store() {
    let directory = tempfile::tempdir().unwrap();
    let live = directory.path().join("live");
    let moved = directory.path().join("temporarily-unavailable");
    fs::create_dir(&live).unwrap();
    let path = live.join("local-api.json");
    let store = ApiStore::open(&path).unwrap();
    store
        .save_settings(ApiSettings {
            enabled: true,
            port: 47981,
        })
        .unwrap();
    let client = store
        .create_client(input("retained synthetic client"))
        .unwrap();
    let before = fs::read(&path).unwrap();
    // Reversible, synthetic fault: the store's parent disappears. This forces
    // real persistence failure without admin permissions or touching user data.
    fs::rename(&live, &moved).unwrap();
    assert!(store.create_client(input("must not be installed")).is_err());
    assert!(store.revoke_client(&client.client.id).is_err());
    assert!(
        store
            .save_settings(ApiSettings {
                enabled: false,
                port: 47982
            })
            .is_err()
    );
    let grant = directory.path().to_string_lossy().into_owned();
    assert!(
        store
            .add_picker_grants(&client.client.id, &[grant], &[])
            .is_err()
    );
    assert_eq!(store.clients().unwrap().len(), 1);
    assert!(store.clients().unwrap()[0].read_roots.is_empty());
    assert!(store.authenticate(&client.token).is_some());
    assert!(store.settings().unwrap().enabled);
    assert_eq!(store.settings().unwrap().port, 47981);
    assert_eq!(fs::read(moved.join("local-api.json")).unwrap(), before);
    assert_eq!(
        moved.read_dir().unwrap().count(),
        1,
        "failed persistence leaked a temporary file"
    );
    fs::rename(&moved, &live).unwrap();
    let reopened = ApiStore::open(&path).unwrap();
    assert!(reopened.authenticate(&client.token).is_some());
    assert_eq!(reopened.clients().unwrap().len(), 1);
    store.revoke_client(&client.client.id).unwrap();
    assert!(
        ApiStore::open(&path)
            .unwrap()
            .authenticate(&client.token)
            .is_none()
    );
}

#[test]
fn client_limit_and_failed_unknown_revocation_leave_disk_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local-api.json");
    let store = ApiStore::open(&path).unwrap();
    for index in 0..32 {
        let _ = store
            .create_client(input(&format!("synthetic-{index}")))
            .unwrap();
    }
    let before = fs::read(&path).unwrap();
    assert!(store.create_client(input("over-limit")).is_err());
    assert!(
        store
            .revoke_client("00000000000000000000000000000000")
            .is_err()
    );
    assert_eq!(store.clients().unwrap().len(), 32);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(ApiStore::open(&path).unwrap().clients().unwrap().len(), 32);
}

#[test]
fn expired_tokens_are_rejected_and_activity_updates_last_used() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local-api.json");
    let store = ApiStore::open(&path).unwrap();
    store
        .save_settings(ApiSettings {
            enabled: true,
            port: 47981,
        })
        .unwrap();

    let mut expired_input = input("expired-app");
    expired_input.expires_at = Some("1000".into()); // Unix 1970
    let expired = store.create_client(expired_input).unwrap();
    assert!(store.authenticate(&expired.token).is_none());
    assert!(!store.active_client(&expired.client.id));
    assert!(store.current_client(&expired.client.id).is_none());

    let mut valid_input = input("active-app");
    valid_input.expires_at = Some("4102444800".into()); // Unix 2100
    let valid = store.create_client(valid_input).unwrap();
    let authed = store.authenticate(&valid.token);
    assert!(authed.is_some());
    assert!(authed.unwrap().last_used_at.is_some());
    assert!(store.active_client(&valid.client.id));
    assert!(store.current_client(&valid.client.id).is_some());
}

#[cfg(windows)]
#[test]
fn windows_replace_failure_preserves_old_bytes_and_cleans_the_written_temporary_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local-api.json");
    let store = ApiStore::open(&path).unwrap();
    store
        .save_settings(ApiSettings {
            enabled: true,
            port: 47981,
        })
        .unwrap();
    let retained = store.create_client(input("retained client")).unwrap();
    let before = fs::read(&path).unwrap();
    // Permit readers/writers but deny FILE_SHARE_DELETE. This lets persist write
    // and sync its new temporary file, then forces the actual replacement to fail.
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(&path)
        .unwrap();
    assert!(store.create_client(input("must not commit")).is_err());
    assert!(store.revoke_client(&retained.client.id).is_err());
    assert_eq!(store.clients().unwrap().len(), 1);
    assert!(store.authenticate(&retained.token).is_some());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(directory.path().read_dir().unwrap().count(), 1);
    drop(lock);
    let retry = store.create_client(input("successful retry")).unwrap();
    let reopened = ApiStore::open(&path).unwrap();
    assert!(reopened.authenticate(&retained.token).is_some());
    assert!(reopened.authenticate(&retry.token).is_some());
    assert_eq!(reopened.clients().unwrap().len(), 2);
}
