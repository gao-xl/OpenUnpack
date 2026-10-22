#![cfg(windows)]

use openunpack_application::{PasswordSuccessEvent, VaultStore, VaultUseCases};
use openunpack_domain::CandidateOrigin;
use openunpack_infrastructure::DpapiVaultStore;
use openunpack_infrastructure::dpapi::Vault;
use openunpack_protocol::PasswordInput;
use std::fs::{self, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::sync::Arc;
use tempfile::tempdir;

fn password_input(id: Option<String>, secret: Option<&str>) -> PasswordInput {
    PasswordInput {
        id,
        name: "人工回归样本".into(),
        secret: secret.map(String::from),
        groups: vec!["A".into()],
        notes: "test data".into(),
        enabled: true,
    }
}

fn success(id: &str, revision: u64, event_id: &str) -> PasswordSuccessEvent {
    PasswordSuccessEvent {
        event_id: event_id.into(),
        candidate_id: id.into(),
        candidate_revision: revision,
        origin: CandidateOrigin::VaultEntry,
        fingerprint: Some("fixture-fingerprint".into()),
    }
}

#[test]
fn adapter_reads_and_updates_real_legacy_vault_without_changing_password_text() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("legacy.vault");
    let legacy = Vault::open(&path).unwrap();
    let original = legacy
        .upsert(password_input(None, Some(" leading and trailing \u{3000}")))
        .unwrap();
    let bytes_before_open = fs::read(&path).unwrap();
    drop(legacy);
    let adapter = Arc::new(DpapiVaultStore::open(&path).unwrap());
    assert_eq!(fs::read(&path).unwrap(), bytes_before_open);
    assert_eq!(adapter.list_entries().unwrap(), vec![original.clone()]);
    let use_cases = VaultUseCases::new(adapter.clone());
    assert_eq!(
        use_cases
            .import_passwords(&[
                " tail ".into(),
                "\u{3000}".into(),
                "".into(),
                "repeat".into(),
                " tail ".into()
            ])
            .unwrap(),
        3
    );
    use_cases.add_password("spaces only", "  ").unwrap();
    let secrets: Vec<_> = adapter
        .load_decrypt_candidates()
        .unwrap()
        .iter()
        .map(|c| c.secret.clone())
        .collect();
    for text in [
        " leading and trailing \u{3000}",
        " tail ",
        "\u{3000}",
        "repeat",
        "  ",
    ] {
        assert!(secrets.contains(&text.to_string()));
    }
    drop(use_cases);
    drop(adapter);
    // The legacy production implementation can still open the adapter's writes.
    let reopened = Vault::open(&path).unwrap();
    assert_eq!(
        reopened.reveal(&original.id).unwrap(),
        " leading and trailing \u{3000}"
    );
    assert_eq!(reopened.views().unwrap().len(), 5);
}

#[test]
fn success_events_preserve_revision_origin_replay_and_tombstone_semantics() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("events.vault");
    let adapter = DpapiVaultStore::open(&path).unwrap();
    let original = adapter
        .upsert_entry(password_input(None, Some("one")))
        .unwrap();
    let mut hint = success(&original.id, original.revision, "hint-event");
    hint.origin = CandidateOrigin::FileNameHint;
    adapter.record_success(&hint).unwrap();
    adapter
        .record_success(&success(&original.id, original.revision + 1, "stale-event"))
        .unwrap();
    assert_eq!(adapter.list_entries().unwrap()[0].success_count, 0);
    let delivered = success(&original.id, original.revision, "delivered-event");
    adapter.record_success(&delivered).unwrap();
    adapter.record_success(&delivered).unwrap();
    let counted = &adapter.list_entries().unwrap()[0];
    assert_eq!(counted.success_count, 1);
    assert_eq!(counted.revision, original.revision); // popularity is not a secret/metadata edit
    drop(adapter);
    let reopened = DpapiVaultStore::open(&path).unwrap();
    reopened.record_success(&delivered).unwrap();
    assert_eq!(reopened.list_entries().unwrap()[0].success_count, 1);
    let changed = reopened
        .upsert_entry(password_input(Some(original.id.clone()), Some("two")))
        .unwrap();
    assert!(changed.revision > original.revision);
    reopened
        .record_success(&success(&original.id, original.revision, "late-delivery"))
        .unwrap();
    assert_eq!(reopened.list_entries().unwrap()[0].success_count, 0);
    reopened
        .record_success(&success(&changed.id, changed.revision, "current-delivery"))
        .unwrap();
    assert_eq!(reopened.list_entries().unwrap()[0].success_count, 1);
    reopened.delete_entry(&changed.id).unwrap();
    reopened
        .record_success(&success(&changed.id, changed.revision, "after-delete"))
        .unwrap();
    assert!(reopened.list_entries().unwrap().is_empty());
    assert!(reopened.load_decrypt_candidates().unwrap().is_empty());
}

#[test]
fn vault_commit_failure_changes_neither_disk_nor_memory_and_event_retry_counts_once() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("locked.vault");
    let adapter = DpapiVaultStore::open(&path).unwrap();
    let entry = adapter.add_entry("fixture", "secret").unwrap();
    let before = fs::read(&path).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let event = success(&entry.id, entry.revision, "delivery-write-failure");
    assert!(adapter.add_entry("must not survive", "value").is_err());
    assert!(adapter.record_success(&event).is_err());
    assert!(adapter.import_passwords(&["not committed".into()]).is_err());
    assert_eq!(adapter.list_entries().unwrap(), vec![entry.clone()]);
    drop(lock);
    assert_eq!(fs::read(&path).unwrap(), before);
    adapter.record_success(&event).unwrap();
    adapter.record_success(&event).unwrap();
    assert_eq!(adapter.list_entries().unwrap()[0].success_count, 1);
    assert_eq!(
        DpapiVaultStore::open(&path)
            .unwrap()
            .list_entries()
            .unwrap()[0]
            .success_count,
        1
    );
}

#[test]
fn invalid_and_unknown_encrypted_vaults_are_preserved() {
    let dir = tempdir().unwrap();
    for (name, bytes) in [
        ("empty", vec![]),
        ("invalid", b"unknown payload".to_vec()),
        (
            "unknown-format",
            dpapi_encrypt(br#"{"futureSchema":999,"items":{}}"#),
        ),
        (
            "future-complete-format",
            dpapi_encrypt(br#"{"generation":0,"items":{},"seen_events":[],"fingerprint_order":{},"futureSchema":999}"#),
        ),
    ] {
        let path = dir.path().join(name);
        fs::write(&path, &bytes).unwrap();
        assert!(DpapiVaultStore::open(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

fn dpapi_encrypt(plaintext: &[u8]) -> Vec<u8> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        assert_ne!(
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output
            ),
            0
        );
        let encrypted = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as _);
        encrypted
    }
}

#[test]
fn adapter_preserves_group_import_preview_and_backup_restore_contracts() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("original.vault");
    let adapter = DpapiVaultStore::open(&path).unwrap();
    let entry = adapter
        .upsert_entry(password_input(None, Some(" fixture \u{3000}")))
        .unwrap();
    adapter
        .bulk_action(std::slice::from_ref(&entry.id), "group", &["B".into()])
        .unwrap();
    assert_eq!(adapter.groups().unwrap(), vec!["B"]);
    let text = "name\t with spaces \tC\tnote";
    let preview = adapter.preview_import(text, "tsv", "").unwrap();
    assert!(
        adapter
            .apply_import(&preview.token, "changed", "txt", "")
            .is_err()
    );
    assert_eq!(
        adapter
            .apply_import(&preview.token, text, "tsv", "")
            .unwrap()
            .added,
        1
    );
    assert!(
        adapter
            .apply_import(&preview.token, text, "tsv", "")
            .is_err()
    );
    let backup = dir.path().join("backup.json");
    adapter.export_backup(&backup, "test passphrase").unwrap();
    let restore = DpapiVaultStore::open(&dir.path().join("restored.vault")).unwrap();
    let preview = restore
        .preview_restore_backup(&backup, "test passphrase")
        .unwrap();
    assert_eq!(preview.new_count, 2);
    assert_eq!(
        restore
            .confirm_restore_backup(&backup, "test passphrase", &preview.token)
            .unwrap()
            .added,
        2
    );
    assert_eq!(restore.list_entries().unwrap().len(), 2);
    assert!(
        restore
            .load_decrypt_candidates()
            .unwrap()
            .iter()
            .any(|candidate| candidate.secret == " with spaces ")
    );
}
