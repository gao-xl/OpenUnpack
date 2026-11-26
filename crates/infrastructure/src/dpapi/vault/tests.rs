use super::*;
use tempfile::tempdir;

#[test]
fn csv_quote_grammar_reports_rows_without_reinterpreting_passwords() {
    for malformed in [
        "Entry,\"unclosed",
        "Entry,ab\"cd",
        "Entry,\"abc\"tail",
        "Entry,\"abc\" \n",
        "\"name\"tail,password\nEntry,valid",
    ] {
        let records = parse_import_text(malformed, "csv:name,secret", "");
        assert!(records[0].error, "malformed quoting must be reported");
        assert!(records[0].secret.is_empty());
        assert_eq!(records[0].line, 1);
    }
    // CRLF in quoted notes is content, and physical line numbers remain
    // useful after a multiline row and an invalid row.
    let records = parse_import_text(
        "name,password,notes\r\nEntry,\"  a\"\"b,🔑  \",\"first\r\nsecond\"\r\nBad,ab\"cd,ignored\r\nGood,  raw  ,tail",
        "csv",
        "",
    );
    assert_eq!(records.len(), 3);
    assert!(!records[0].error);
    assert_eq!(records[0].secret, "  a\"b,🔑  ");
    assert_eq!(records[0].notes, "first\r\nsecond");
    assert_eq!(records[0].line, 2);
    assert!(records[1].error);
    assert_eq!(records[1].line, 4);
    assert!(!records[2].error);
    assert_eq!(records[2].secret, "  raw  ");
    assert_eq!(records[2].line, 5);
    let tsv = parse_import_text("Entry\tab\"cd\tGroup\tNote", "tsv", "");
    assert!(
        !tsv[0].error,
        "CSV quoting rules must not alter TSV literals"
    );
    assert_eq!(tsv[0].secret, "ab\"cd");
}

#[cfg(windows)]
#[test]
fn malformed_csv_confirmation_preserves_existing_vault_and_preview_generation() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("csv.dat");
    let vault = Vault::open(&path).unwrap();
    let existing = vault
        .upsert(input(None, Some("synthetic-existing")))
        .unwrap();
    let memory = release_audit_state(&vault);
    let disk = fs::read(&path).unwrap();
    for malformed in ["\"unclosed", "ab\"cd", "\"abc\"tail"] {
        let csv = format!("name,password\nValid,synthetic-valid\nBad,{malformed}");
        let preview = vault.preview_import(&csv, "csv", "").unwrap();
        assert_eq!(preview.new_count, 1);
        assert_eq!(preview.error_count, 1);
        assert_eq!(preview.rows[1].line, 3);
        assert!(vault.apply_import(&preview.token, &csv, "csv", "").is_err());
        assert!(release_audit_state(&vault) == memory);
        assert!(fs::read(&path).unwrap() == disk);
    }
    drop(vault);
    let reopened = Vault::open(&path).unwrap();
    assert_eq!(reopened.views().unwrap().len(), 1);
    assert_eq!(
        reopened.reveal(&existing.id).unwrap().as_str(),
        "synthetic-existing"
    );
    let csv = "name,password,notes\nValid,\"  a\"\"b,🔑  \",\"line 1\nline 2\"";
    let preview = reopened.preview_import(csv, "csv", "").unwrap();
    assert_eq!(preview.error_count, 0);
    assert_eq!(
        reopened
            .apply_import(&preview.token, csv, "csv", "")
            .unwrap()
            .added,
        1
    );
    drop(reopened);
    let reopened = Vault::open(&path).unwrap();
    let added = reopened
        .views()
        .unwrap()
        .into_iter()
        .find(|i| i.name == "Valid")
        .unwrap();
    assert_eq!(reopened.reveal(&added.id).unwrap().as_str(), "  a\"b,🔑  ");
    assert_eq!(added.notes, "line 1\nline 2");
}

#[cfg(windows)]
#[test]
fn vault_item_limit_is_shared_by_public_mutations_and_backup_reader_writer() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("legacy-count.dat");
    let mut legacy = VaultData::default();
    for index in 0..=MAX_VAULT_ITEMS {
        let id = format!("{index:036}");
        legacy.items.insert(
            id.clone(),
            VaultItem {
                id,
                name: "Synthetic".into(),
                secret: format!("synthetic-count-{index}"),
                groups: vec![],
                notes: String::new(),
                enabled: true,
                success_count: 0,
                last_success: None,
                revision: 1,
                deleted: false,
                created_at: "2000-01-01T00:00:00+00:00".into(),
                updated_at: "2000-01-01T00:00:00+00:00".into(),
            },
        );
    }
    // Construct a schema-valid legacy store directly; do not make 100001
    // encrypted commits just to establish a boundary fixture.
    let raw = Zeroizing::new(serde_json::to_vec(&legacy).unwrap());
    fs::write(&path, dpapi_encrypt(&raw).unwrap()).unwrap();
    drop(raw);
    drop(legacy);
    let vault = Vault::open(&path).unwrap();
    assert_eq!(vault.views().unwrap().len(), MAX_VAULT_ITEMS + 1);
    let destination = dir.path().join("existing.json");
    fs::write(&destination, b"retain synthetic destination").unwrap();
    assert!(
        vault
            .export_backup(&destination, "synthetic-passphrase")
            .is_err()
    );
    assert_eq!(
        fs::read(&destination).unwrap(),
        b"retain synthetic destination"
    );
    let missing = dir.path().join("missing.json");
    assert!(
        vault
            .export_backup(&missing, "synthetic-passphrase")
            .is_err()
    );
    assert!(!missing.exists());
    let original_disk = fs::read(&path).unwrap();
    let original_memory = release_audit_state(&vault);
    assert!(
        vault
            .upsert(input(None, Some("synthetic-overflow")))
            .is_err()
    );
    assert!(fs::read(&path).unwrap() == original_disk);
    assert!(release_audit_state(&vault) == original_memory);
    drop(original_disk);
    drop(original_memory);
    // An old over-limit store remains editable and can be reduced without
    // silently discarding records on open.
    let first = format!("{:036}", 0);
    vault.upsert(input(Some(first.clone()), None)).unwrap();
    vault.delete(&first).unwrap();
    assert_eq!(vault.views().unwrap().len(), MAX_VAULT_ITEMS);
    assert!(
        vault
            .upsert(input(None, Some("synthetic-overflow")))
            .is_err()
    );
    let before = fs::read(&path).unwrap();
    let preview = vault
        .preview_import("synthetic-overflow", "txt", "")
        .unwrap();
    assert_eq!(preview.error_count, 1);
    assert!(
        vault
            .apply_import(&preview.token, "synthetic-overflow", "txt", "")
            .is_err()
    );
    assert!(fs::read(&path).unwrap() == before);
    // Existing secrets can still merge at capacity.
    drop(before);
    let duplicate = vault
        .preview_import("synthetic-count-1", "txt", "")
        .unwrap();
    assert_eq!(duplicate.duplicate_count, 1);
    assert_eq!(
        vault
            .apply_import(&duplicate.token, "synthetic-count-1", "txt", "")
            .unwrap()
            .added,
        0
    );
    let small = Vault::open(&dir.path().join("small.dat")).unwrap();
    small
        .upsert(input(None, Some("synthetic-restore-overflow")))
        .unwrap();
    let small_backup = dir.path().join("small.json");
    small
        .export_backup(&small_backup, "synthetic-passphrase")
        .unwrap();
    let restore_preview = vault
        .preview_restore_backup(&small_backup, "synthetic-passphrase")
        .unwrap();
    assert_eq!(restore_preview.error_count, 1);
    let before = fs::read(&path).unwrap();
    let memory = release_audit_state(&vault);
    assert!(
        vault
            .confirm_restore_backup(
                &small_backup,
                "synthetic-passphrase",
                &restore_preview.token
            )
            .is_err()
    );
    assert!(fs::read(&path).unwrap() == before);
    assert!(release_audit_state(&vault) == memory);
    drop(before);
    drop(memory);
    // Deletion tombstones do not consume active-item capacity.
    vault.delete(&format!("{:036}", 1)).unwrap();
    vault
        .upsert(input(None, Some("synthetic-replacement")))
        .unwrap();
    vault
        .export_backup(&destination, "synthetic-passphrase")
        .unwrap();
    let (decoded, _) = Vault::decode_backup(&destination, "synthetic-passphrase").unwrap();
    assert_eq!(decoded.items.len(), MAX_VAULT_ITEMS);
    drop(decoded);
    drop(vault);
    assert_eq!(
        Vault::open(&path).unwrap().views().unwrap().len(),
        MAX_VAULT_ITEMS
    );
}

#[cfg(windows)]
#[test]
fn dpapi_envelope_at_plaintext_limit_reopens_and_oversize_mutation_is_atomic() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("size.dat");
    let vault = Vault::open(&path).unwrap();
    let mut data = VaultData::default();
    data.seen_events.insert(String::new());
    let overhead = serialized_size(&data).unwrap();
    data.seen_events.clear();
    data.seen_events
        .insert("x".repeat(MAX_BACKUP_PAYLOAD_SIZE - overhead));
    assert_eq!(serialized_size(&data).unwrap(), MAX_BACKUP_PAYLOAD_SIZE);
    vault.persist_internal(&data).unwrap();
    let file_size = fs::metadata(&path).unwrap().len() as usize;
    assert!(
        file_size > MAX_BACKUP_PAYLOAD_SIZE,
        "fixture must exercise DPAPI expansion"
    );
    assert!(file_size <= MAX_VAULT_FILE_SIZE);
    drop(data);
    drop(vault);
    let reopened = Vault::open(&path).unwrap();
    let before = Sha256::digest(fs::read(&path).unwrap());
    assert!(
        reopened
            .record_success(&SuccessEvent {
                event_id: "one-more-event".into(),
                password_id: "missing".into(),
                password_revision: 1,
                fingerprint: None,
            })
            .is_err()
    );
    assert_eq!(Sha256::digest(fs::read(&path).unwrap()), before);
    assert_eq!(reopened.data.lock().unwrap().seen_events.len(), 1);
    let mut too_large = reopened.data.lock().unwrap().clone();
    let value = too_large.seen_events.pop_first().unwrap() + "x";
    too_large.seen_events.insert(value);
    assert_eq!(
        serialized_size(&too_large).unwrap(),
        MAX_BACKUP_PAYLOAD_SIZE + 1
    );
    assert!(reopened.persist_internal(&too_large).is_err());
    assert_eq!(Sha256::digest(fs::read(&path).unwrap()), before);
}

#[test]
fn backup_size_validation_accounts_for_local_restore_metadata() {
    let mut payload = BackupPayload {
        version: 1,
        items: (0..4040)
            .map(|index| BackupItem {
                name: "n".into(),
                secret: format!("s{index}"),
                groups: vec![],
                notes: "x".repeat(16384),
                enabled: true,
                success_count: 0,
                last_success: None,
            })
            .collect(),
    };
    assert!(serialized_size(&payload).unwrap() < MAX_BACKUP_PAYLOAD_SIZE);
    assert!(
        validate_empty_restore_size(&payload).is_err(),
        "portable payload fits, restored local representation does not"
    );
    for item in &mut payload.items {
        item.notes.truncate(16000);
    }
    assert!(validate_empty_restore_size(&payload).is_ok());
}

#[cfg(windows)]
fn release_audit_state(vault: &Vault) -> Vec<u8> {
    serde_json::to_vec(&*vault.data.lock().unwrap()).unwrap()
}

#[cfg(windows)]
fn release_audit_groups(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("分组-{index:03}")).collect()
}

#[cfg(windows)]
#[test]
fn release_audit_backup_export_rejects_overlong_passphrase_atomically() {
    let dir = tempdir().unwrap();
    let vault_path = dir.path().join("vault.dat");
    let vault = Vault::open(&vault_path).unwrap();
    vault
        .upsert(input(None, Some("  synthetic-导出口令🔑  ")))
        .unwrap();
    let memory = release_audit_state(&vault);
    let disk = fs::read(&vault_path).unwrap();
    let destination = dir.path().join("backup.json");
    let retained = b"existing synthetic destination";
    fs::write(&destination, retained).unwrap();
    for passphrase in ["x".repeat(4097), "密".repeat(1366)] {
        assert!(
            vault.export_backup(&destination, &passphrase).is_err(),
            "export must reject a passphrase that decode_backup cannot accept"
        );
        assert!(
            fs::read(&destination).unwrap() == retained,
            "rejected export must preserve an existing destination"
        );
        let missing = dir.path().join("must-not-create.json");
        assert!(vault.export_backup(&missing, &passphrase).is_err());
        assert!(!missing.exists());
        assert!(release_audit_state(&vault) == memory);
        assert!(fs::read(&vault_path).unwrap() == disk);
    }
}

#[cfg(windows)]
#[test]
fn release_audit_import_duplicate_group_union_is_atomic_at_100_plus_1() {
    let dir = tempdir().unwrap();
    let vault_path = dir.path().join("vault.dat");
    let vault = Vault::open(&vault_path).unwrap();
    vault
        .upsert(input(None, Some("synthetic-existing-unrelated")))
        .unwrap();
    let groups = release_audit_groups(100).join(",");
    // Each row is valid by itself; duplicate rows together exceed metadata limits.
    let text = format!(
        "staged\t  synthetic-new-before-failure🔑  \t合法组\t保留备注\nfirst\t  synthetic-shared🔑  \t{groups}\t原始备注\nsecond\t  synthetic-shared🔑  \t分组-100\t"
    );
    let preview = vault.preview_import(&text, "tsv", "").unwrap();
    assert_eq!(preview.error_count, 0);
    let memory = release_audit_state(&vault);
    let disk = fs::read(&vault_path).unwrap();
    assert!(
        vault
            .apply_import(&preview.token, &text, "tsv", "")
            .is_err(),
        "valid individual rows must not commit an invalid 101-group union"
    );
    assert!(
        release_audit_state(&vault) == memory,
        "failed import must preserve all in-memory state"
    );
    assert!(
        fs::read(&vault_path).unwrap() == disk,
        "failed import must preserve encrypted storage bytes"
    );
    assert_eq!(Vault::open(&vault_path).unwrap().views().unwrap().len(), 1);

    // The rejected preview still belongs to the same unchanged generation.
    assert!(
        vault
            .apply_import(&preview.token, &text, "tsv", "")
            .is_err()
    );
    assert!(release_audit_state(&vault) == memory);
}

#[cfg(windows)]
#[test]
fn release_audit_two_backups_group_union_rejects_100_plus_1_atomically() {
    let dir = tempdir().unwrap();
    let first = Vault::open(&dir.path().join("first.dat")).unwrap();
    let mut first_input = input(None, Some("  synthetic-备份共享🔑  "));
    first_input.groups = release_audit_groups(100);
    first.upsert(first_input).unwrap();
    let first_backup = dir.path().join("first.json");
    first
        .export_backup(&first_backup, "  synthetic-备份口令🔑  ")
        .unwrap();

    let second = Vault::open(&dir.path().join("second.dat")).unwrap();
    let mut second_input = input(None, Some("  synthetic-备份共享🔑  "));
    second_input.groups = vec!["分组-100".into()];
    second.upsert(second_input).unwrap();
    second
        .upsert(input(None, Some("synthetic-staged-backup-item")))
        .unwrap();
    let second_backup = dir.path().join("second.json");
    second
        .export_backup(&second_backup, "  synthetic-备份口令🔑  ")
        .unwrap();

    let target_path = dir.path().join("target.dat");
    let target = Vault::open(&target_path).unwrap();
    assert_eq!(
        target
            .restore_backup(&first_backup, "  synthetic-备份口令🔑  ")
            .unwrap()
            .added,
        1
    );
    let memory = release_audit_state(&target);
    let disk = fs::read(&target_path).unwrap();
    let preview = target
        .preview_restore_backup(&second_backup, "  synthetic-备份口令🔑  ")
        .unwrap();
    assert_eq!(
        (
            preview.new_count,
            preview.duplicate_count,
            preview.error_count
        ),
        (1, 1, 0)
    );
    assert!(
        target
            .confirm_restore_backup(&second_backup, "  synthetic-备份口令🔑  ", &preview.token)
            .is_err(),
        "two valid backups must not commit an invalid merged metadata union"
    );
    assert!(release_audit_state(&target) == memory);
    assert!(fs::read(&target_path).unwrap() == disk);
    let reopened = Vault::open(&target_path).unwrap();
    assert_eq!(reopened.views().unwrap()[0].groups.len(), 100);
    assert_eq!(
        reopened.candidates(None, None).unwrap()[0].secret,
        "  synthetic-备份共享🔑  "
    );
}

#[cfg(windows)]
#[test]
fn release_audit_legal_overlapping_groups_and_byte_limit_passphrase_restore() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vault.dat");
    let vault = Vault::open(&path).unwrap();
    let mut original = input(None, Some("  synthetic-合法重叠🔑  "));
    original.name = "保留名称".into();
    original.notes = "  保留备注\n第二行  ".into();
    original.groups = release_audit_groups(100);
    let stored = vault.upsert(original).unwrap();
    let text = "替换名称\t  synthetic-合法重叠🔑  \t分组-000,分组-099,分组-099\t不替换备注";
    let preview = vault.preview_import(text, "tsv", "").unwrap();
    let result = vault.apply_import(&preview.token, text, "tsv", "").unwrap();
    assert_eq!((result.added, result.duplicates), (0, 1));
    let current = vault.views().unwrap().remove(0);
    assert_eq!(current.groups, stored.groups);
    assert_eq!(current.name, stored.name);
    assert_eq!(current.notes, stored.notes);

    // Exactly 4096 UTF-8 bytes, including Unicode and untrimmed outer spaces.
    let passphrase = format!(" {}ab ", "密".repeat(1364));
    assert_eq!(passphrase.len(), 4096);
    let backup = dir.path().join("legal.json");
    vault.export_backup(&backup, &passphrase).unwrap();
    let result = vault.restore_backup(&backup, &passphrase).unwrap();
    assert_eq!((result.added, result.duplicates), (0, 1));
    assert_eq!(vault.views().unwrap()[0].groups, stored.groups);
    let restored = Vault::open(&dir.path().join("restored.dat")).unwrap();
    assert_eq!(
        restored.restore_backup(&backup, &passphrase).unwrap().added,
        1
    );
    assert_eq!(
        restored.candidates(None, None).unwrap()[0].secret,
        "  synthetic-合法重叠🔑  "
    );
    assert_eq!(restored.views().unwrap()[0].notes, stored.notes);
}

#[cfg(windows)]
#[test]
fn release_audit_legacy_overlimit_item_export_is_rejected_until_explicit_repair() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("legacy.dat");
    let initial = Vault::open(&path).unwrap();
    let mut original = input(None, Some("  synthetic-旧库保留🔑  "));
    original.groups = release_audit_groups(100);
    original.notes = "  旧版备注原文\n保留  ".into();
    let item = initial.upsert(original).unwrap();

    // Reproduce the old version's persisted result, including real DPAPI storage.
    // Opening this old file must remain supported; export must not create an
    // encrypted backup that the existing decoder cannot restore.
    let mut legacy = initial.data.lock().unwrap().clone();
    legacy
        .items
        .get_mut(&item.id)
        .unwrap()
        .groups
        .push("分组-100".into());
    initial.persist_internal(&legacy).unwrap();
    drop(initial);
    let reopened = Vault::open(&path).unwrap();
    assert_eq!(reopened.views().unwrap()[0].groups.len(), 101);
    assert_eq!(
        reopened.reveal(&item.id).unwrap().as_str(),
        "  synthetic-旧库保留🔑  "
    );
    let memory = release_audit_state(&reopened);
    let disk = fs::read(&path).unwrap();
    let backup = dir.path().join("existing-backup.json");
    let retained = b"retain existing synthetic backup destination";
    fs::write(&backup, retained).unwrap();
    let error = reopened
        .export_backup(&backup, "synthetic-legacy-backup")
        .expect_err("export must reject legacy metadata that decode_backup cannot restore");
    assert!(
        error.contains("条目") && error.contains("修正"),
        "export should explain how to repair the invalid item"
    );
    assert!(fs::read(&backup).unwrap() == retained);
    let missing = dir.path().join("must-not-create-legacy.json");
    assert!(
        reopened
            .export_backup(&missing, "synthetic-legacy-backup")
            .is_err()
    );
    assert!(!missing.exists());
    assert!(release_audit_state(&reopened) == memory);
    assert!(fs::read(&path).unwrap() == disk);

    // Repair is explicit through the supported group replacement API, without
    // silently discarding or truncating old data on open or export.
    reopened
        .bulk_action(
            std::slice::from_ref(&item.id),
            "group",
            &release_audit_groups(100),
        )
        .unwrap();
    assert_eq!(reopened.views().unwrap()[0].groups, item.groups);
    assert_eq!(reopened.views().unwrap()[0].notes, item.notes);
    assert_eq!(
        reopened.reveal(&item.id).unwrap().as_str(),
        "  synthetic-旧库保留🔑  "
    );
    reopened
        .export_backup(&backup, "synthetic-legacy-backup")
        .unwrap();
    let restored = Vault::open(&dir.path().join("legacy-restored.dat")).unwrap();
    assert_eq!(
        restored
            .restore_backup(&backup, "synthetic-legacy-backup")
            .unwrap()
            .added,
        1
    );
    assert_eq!(restored.views().unwrap()[0].groups, item.groups);
    assert_eq!(restored.views().unwrap()[0].notes, item.notes);
    assert_eq!(
        restored.candidates(None, None).unwrap()[0].secret,
        "  synthetic-旧库保留🔑  "
    );
}

#[test]
fn test_parse_tsv_and_preview_token() {
    let tsv = "label1\tsecret_123\tgrp1, grp2\tnotes here\n\tsecond_secret\t\t";
    let records = parse_import_text(tsv, "tsv", "default");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].name, "label1");
    assert_eq!(records[0].secret, "secret_123");
    assert_eq!(
        records[0].groups,
        vec!["grp1".to_string(), "grp2".to_string()]
    );
    assert_eq!(records[1].secret, "second_secret");
    assert_eq!(records[1].groups, vec!["default".to_string()]);

    let token1 = compute_preview_token(tsv, "tsv", "default", 0, &[7u8; 32]);
    let token2 = compute_preview_token(tsv, "tsv", "default", 0, &[7u8; 32]);
    let token3 = compute_preview_token(tsv, "tsv", "default", 1, &[7u8; 32]);
    assert_eq!(token1, token2);
    assert_ne!(token1, token3);
}

#[test]
fn test_backup_and_restore_crypto() {
    let payload = BackupPayload {
        version: 1,
        items: vec![BackupItem {
            name: "Test item".into(),
            secret: "super secret".into(),
            groups: vec!["g1".into()],
            notes: "note".into(),
            enabled: true,
            success_count: 5,
            last_success: None,
        }],
    };

    let raw = serde_json::to_vec(&payload).unwrap();
    let salt = [1u8; 16];
    let nonce_bytes = [2u8; 12];
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(b"password123", &salt, 100_000, &mut key);
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher.encrypt(nonce, raw.as_slice()).unwrap();

    let env = BackupEnvelope {
        version: 1,
        kdf: "PBKDF2-SHA256".to_string(),
        iterations: 100_000,
        salt: BASE64.encode(salt),
        nonce: BASE64.encode(nonce_bytes),
        ciphertext: BASE64.encode(ct),
    };

    let env_json = serde_json::to_vec(&env).unwrap();
    let parsed_env: BackupEnvelope = serde_json::from_slice(&env_json).unwrap();
    assert_eq!(parsed_env.iterations, 100_000);

    let mut key_decrypt = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(b"password123", &salt, 100_000, &mut key_decrypt);
    let cipher_dec = Aes256Gcm::new_from_slice(&key_decrypt).unwrap();
    let pt = cipher_dec
        .decrypt(
            nonce,
            BASE64.decode(&parsed_env.ciphertext).unwrap().as_slice(),
        )
        .unwrap();
    assert_eq!(pt, raw);
}

#[cfg(windows)]
#[test]
fn test_windows_vault_lifecycle() {
    let dir = tempdir().unwrap();
    let vault_file = dir.path().join("vault.dat");

    let vault = Vault::open(&vault_file).unwrap();
    let item = vault
        .upsert(PasswordInput {
            id: None,
            name: "Alpha".to_string(),
            secret: Some("MySecretUnicode🔑".to_string()),
            groups: vec!["Work".to_string()],
            notes: "None".to_string(),
            enabled: true,
        })
        .unwrap();

    assert_eq!(vault.views().unwrap().len(), 1);
    assert_eq!(
        vault.reveal(&item.id).unwrap().as_str(),
        "MySecretUnicode🔑"
    );

    let candidates = vault.candidates(Some("Work"), None).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].secret, "MySecretUnicode🔑");

    vault.delete(&item.id).unwrap();
    assert_eq!(vault.views().unwrap().len(), 0);
    assert!(vault.reveal(&item.id).is_err());
}

#[cfg(windows)]
fn input(id: Option<String>, secret: Option<&str>) -> PasswordInput {
    PasswordInput {
        id,
        name: "Synthetic entry".into(),
        secret: secret.map(str::to_string),
        groups: vec!["A".into()],
        notes: String::new(),
        enabled: true,
    }
}
#[cfg(windows)]
#[test]
fn corrupt_storage_and_failed_commit_never_replace_good_memory() {
    let dir = tempdir().unwrap();
    let empty = dir.path().join("empty.dat");
    fs::write(&empty, []).unwrap();
    assert!(Vault::open(&empty).is_err());
    assert_eq!(fs::read(&empty).unwrap().len(), 0);
    let path = dir.path().join("vault.dat");
    let vault = Vault::open(&path).unwrap();
    vault
        .upsert(input(None, Some("synthetic-original")))
        .unwrap();
    let good = fs::read(&path).unwrap();
    let retained = dir.path().join("retained.dat");
    fs::rename(&path, &retained).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(vault.upsert(input(None, Some("synthetic-failed"))).is_err());
    assert_eq!(vault.views().unwrap().len(), 1);
    assert_eq!(fs::read(&retained).unwrap(), good);
    let corrupt = dir.path().join("corrupt.dat");
    fs::write(&corrupt, b"invalid synthetic blob").unwrap();
    assert!(Vault::open(&corrupt).is_err());
    assert_eq!(fs::read(&corrupt).unwrap(), b"invalid synthetic blob");
}
#[cfg(windows)]
#[test]
fn success_events_obey_revision_dedupe_and_tombstones() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vault.dat");
    let vault = Vault::open(&path).unwrap();
    let item = vault.upsert(input(None, Some("first-secret"))).unwrap();
    let renamed = vault.upsert(input(Some(item.id.clone()), None)).unwrap();
    assert_eq!(renamed.revision, 1);
    let changed = vault
        .upsert(input(Some(item.id.clone()), Some("second-secret")))
        .unwrap();
    assert_eq!(changed.revision, 2);
    let mut ev = SuccessEvent {
        event_id: "old-event".into(),
        password_id: item.id.clone(),
        password_revision: 1,
        fingerprint: Some("old-file".into()),
    };
    vault.record_success(&ev).unwrap();
    assert_eq!(vault.views().unwrap()[0].success_count, 0);
    ev.event_id = "new-event".into();
    ev.password_revision = 2;
    ev.fingerprint = None;
    vault.record_success(&ev).unwrap();
    vault.record_success(&ev).unwrap();
    assert_eq!(vault.views().unwrap()[0].success_count, 1);
    vault.delete(&item.id).unwrap();
    ev.event_id = "after-delete".into();
    vault.record_success(&ev).unwrap();
    assert!(
        vault
            .upsert(input(Some(item.id.clone()), Some("resurrection")))
            .is_err()
    );
    drop(vault);
    assert!(Vault::open(&path).unwrap().views().unwrap().is_empty());
}
#[cfg(windows)]
#[test]
fn import_tokens_are_bound_and_unicode_whitespace_is_preserved() {
    let dir = tempdir().unwrap();
    let vault = Vault::open(&dir.path().join("vault.dat")).unwrap();
    let text = "  🔑秘密  \nrepeat\nrepeat";
    let preview = vault.preview_import(text, "txt", "A").unwrap();
    assert_eq!((preview.new_count, preview.duplicate_count), (2, 1));
    let view_json = serde_json::to_string(&preview).unwrap();
    assert!(!view_json.contains("秘密") && !view_json.contains("repeat"));
    assert!(
        vault
            .apply_import(&preview.token, "changed", "txt", "A")
            .is_err()
    );
    let result = vault
        .apply_import(&preview.token, text, "txt", "A")
        .unwrap();
    assert_eq!((result.added, result.duplicates), (2, 1));
    assert!(
        vault
            .apply_import(&preview.token, text, "txt", "A")
            .is_err()
    );
    assert!(
        vault
            .candidates(None, None)
            .unwrap()
            .iter()
            .any(|c| c.secret == "  🔑秘密  ")
    );
    let again = vault.preview_import("repeat", "txt", "B").unwrap();
    vault
        .apply_import(&again.token, "repeat", "txt", "B")
        .unwrap();
    let candidate = vault.candidates(Some("B"), None).unwrap().remove(0);
    assert_eq!(candidate.secret, "repeat");
    assert_eq!(
        vault
            .views()
            .unwrap()
            .into_iter()
            .find(|v| v.id == candidate.id)
            .unwrap()
            .groups,
        ["A", "B"]
    );
}
#[cfg(windows)]
#[test]
fn concurrent_writers_do_not_lose_commits() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vault.dat");
    let vault = std::sync::Arc::new(Vault::open(&path).unwrap());
    let threads: Vec<_> = (0..12)
        .map(|index| {
            let vault = vault.clone();
            std::thread::spawn(move || {
                let item = vault
                    .upsert(input(None, Some(&format!("synthetic-{index}"))))
                    .unwrap();
                let ev = SuccessEvent {
                    event_id: format!("event-{index}"),
                    password_id: item.id,
                    password_revision: item.revision,
                    fingerprint: None,
                };
                vault.record_success(&ev).unwrap();
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(vault.views().unwrap().len(), 12);
    assert!(vault.views().unwrap().iter().all(|v| v.success_count == 1));
    drop(vault);
    assert_eq!(Vault::open(&path).unwrap().views().unwrap().len(), 12);
}
#[cfg(windows)]
#[test]
fn editing_a_password_removes_its_old_fingerprint_priority() {
    let dir = tempdir().unwrap();
    let vault = Vault::open(&dir.path().join("vault.dat")).unwrap();
    let item = vault.upsert(input(None, Some("original-secret"))).unwrap();
    vault
        .record_success(&SuccessEvent {
            event_id: "original-success".into(),
            password_id: item.id.clone(),
            password_revision: 1,
            fingerprint: Some("old-archive".into()),
        })
        .unwrap();
    let mut competitor = input(None, Some("other-secret"));
    competitor.name = "A competitor".into();
    let competitor = vault.upsert(competitor).unwrap();
    assert_eq!(
        vault.candidates(None, Some("old-archive")).unwrap()[0].id,
        item.id
    );
    let changed = vault
        .upsert(input(Some(item.id), Some("changed-secret")))
        .unwrap();
    assert_eq!(changed.success_count, 0);
    assert_eq!(changed.revision, 2);
    assert_eq!(
        vault.candidates(None, Some("old-archive")).unwrap()[0].id,
        competitor.id
    );
}
#[cfg(windows)]
#[test]
fn portable_backup_wrong_password_and_hostile_kdf_leave_vault_unchanged() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vault.dat");
    let backup_path = dir.path().join("backup.json");
    let vault = Vault::open(&path).unwrap();
    vault
        .upsert(input(None, Some("synthetic-backup-secret")))
        .unwrap();
    let before = fs::read(&path).unwrap();
    assert!(vault.export_backup(&path, "backup phrase").is_err());
    vault.export_backup(&backup_path, "backup phrase").unwrap();
    assert!(vault.restore_backup(&backup_path, "wrong phrase").is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    let restored = Vault::open(&dir.path().join("restored.dat")).unwrap();
    assert_eq!(
        restored
            .restore_backup(&backup_path, "backup phrase")
            .unwrap()
            .added,
        1
    );
    assert_eq!(
        restored.candidates(None, None).unwrap()[0].secret,
        "synthetic-backup-secret"
    );
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&backup_path).unwrap()).unwrap();
    envelope["iterations"] = serde_json::json!(u32::MAX);
    fs::write(&backup_path, serde_json::to_vec(&envelope).unwrap()).unwrap();
    assert!(vault.restore_backup(&backup_path, "backup phrase").is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[cfg(windows)]
#[test]
fn csv_import_preserves_password_whitespace_through_preview_and_encrypted_save() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("csv-spaces.dat");
    let vault = Vault::open(&path).unwrap();
    let csv = "名称,密码,分组\r\nQuoted,\"  合法 密码  \", Group \r\nPlain,  raw secret  ,Group\r\nSpaces,\"   \",Group\r\nQuotes,\" a\"\"b \",Group";
    let preview = vault.preview_import(csv, "csv", "").unwrap();
    assert_eq!(preview.new_count, 4);
    assert_eq!(preview.error_count, 0);
    let result = vault.apply_import(&preview.token, csv, "csv", "").unwrap();
    assert_eq!(result.added, 4);
    drop(vault);
    let vault = Vault::open(&path).unwrap();
    let candidates = vault.candidates(None, None).unwrap();
    for expected in ["  合法 密码  ", "  raw secret  ", "   ", " a\"b "] {
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.secret == expected)
        );
    }
    assert!(
        vault
            .views()
            .unwrap()
            .iter()
            .all(|entry| entry.groups == ["Group"])
    );
    let custom = "Title,\"  mapped password  \",Group";
    let preview = vault
        .preview_import(custom, "csv:name,secret,group", "")
        .unwrap();
    vault
        .apply_import(&preview.token, custom, "csv:name,secret,group", "")
        .unwrap();
    assert!(
        vault
            .candidates(None, None)
            .unwrap()
            .iter()
            .any(|c| c.secret == "  mapped password  ")
    );
}

#[cfg(windows)]
#[test]
fn csv_import_supports_auto_headers_quoted_fields_and_custom_column_mapping() {
    let dir = tempdir().unwrap();
    let vault = Vault::open(&dir.path().join("vault.dat")).unwrap();

    // 1. 带表头与双引号逗号字段的 CSV
    let csv_with_header = "名称,密码,分组,备注\n\"Archive, A\",\"pass,123\",\"Game;Anime\",\"Note \"\"quoted\"\"\"\nSiteB,sec456,,Simple";
    let preview = vault
        .preview_import(csv_with_header, "csv", "DefaultGrp")
        .unwrap();
    assert_eq!(preview.new_count, 2);
    assert_eq!(preview.error_count, 0);
    let res = vault
        .apply_import(&preview.token, csv_with_header, "csv", "DefaultGrp")
        .unwrap();
    assert_eq!(res.added, 2);

    let cands = vault.candidates(None, None).unwrap();
    assert!(
        cands
            .iter()
            .any(|c| c.secret == "pass,123" && c.label == "Archive, A")
    );
    assert!(
        cands
            .iter()
            .any(|c| c.secret == "sec456" && c.label == "SiteB")
    );

    // 2. 自定义列映射 (csv:secret=0,name=2,group=1)
    let custom_csv = "custom_sec_1,VIP,ItemOne\ncustom_sec_2,VIP,ItemTwo";
    let preview2 = vault
        .preview_import(custom_csv, "csv:secret=0,name=2,group=1", "")
        .unwrap();
    assert_eq!(preview2.new_count, 2);
    vault
        .apply_import(
            &preview2.token,
            custom_csv,
            "csv:secret=0,name=2,group=1",
            "",
        )
        .unwrap();
    let vip_cands = vault.candidates(Some("VIP"), None).unwrap();
    assert_eq!(vip_cands.len(), 2);
    assert!(
        vip_cands
            .iter()
            .any(|c| c.secret == "custom_sec_1" && c.label == "ItemOne")
    );
}

#[cfg(windows)]
#[test]
fn candidate_mutations_derive_bounded_variants_and_attribute_success() {
    let dir = tempdir().unwrap();
    let vault = Vault::open(&dir.path().join("vault.dat")).unwrap();
    let item = vault.upsert(input(None, Some("[pwd:My_Secret]"))).unwrap();

    let base = vault.candidates(None, None).unwrap();
    let mutations = derive_candidate_mutations(&base);
    assert!(!mutations.is_empty());
    assert!(mutations.len() <= 3);
    assert!(mutations.iter().any(|m| m.secret == "My_Secret"));

    // 变体命中应归因回原始条目
    let variant_id = &mutations[0].id;
    vault
        .record_success(&SuccessEvent {
            event_id: "mut-hit-1".into(),
            password_id: variant_id.clone(),
            password_revision: item.revision,
            fingerprint: Some("fp-1".into()),
        })
        .unwrap();
    assert_eq!(vault.views().unwrap()[0].success_count, 1);
}

#[test]
fn test_derive_candidate_mutations_pure_rules() {
    let base = vec![
        Candidate {
            id: "cand-1".into(),
            revision: 1,
            label: "Domain".into(),
            secret: "example.com".into(),
        },
        Candidate {
            id: "cand-2".into(),
            revision: 1,
            label: "Spaced".into(),
            secret: "123 456".into(),
        },
        Candidate {
            id: "cand-3".into(),
            revision: 1,
            label: "Tagged".into(),
            secret: "【解压密码: secret_code!】".into(),
        },
    ];
    let mutations = derive_candidate_mutations(&base);
    assert!(!mutations.is_empty());

    let secrets: Vec<&str> = mutations.iter().map(|m| m.secret.as_str()).collect();
    // Domain should derive www. variant
    assert!(secrets.contains(&"www.example.com"));
    // Spaced should derive space-stripped variant
    assert!(secrets.contains(&"123456"));
    // Tagged should derive stripped variant
    assert!(secrets.contains(&"secret_code"));
}
