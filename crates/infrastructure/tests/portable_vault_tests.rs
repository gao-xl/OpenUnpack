use openunpack_infrastructure::dpapi::{VAULT_MAGIC, Vault};
use openunpack_protocol::PasswordInput;
use std::fs;
use tempfile::tempdir;

static ENVIRONMENT: std::sync::Mutex<()> = std::sync::Mutex::new(());
struct PortableEnvironment {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
}
impl PortableEnvironment {
    fn acquire() -> Self {
        let lock = ENVIRONMENT
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous = ["OPENUNPACK_PORTABLE_VAULT", "OPENUNPACK_VAULT_KEY"]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect();
        Self {
            _lock: lock,
            previous,
        }
    }
}
impl Drop for PortableEnvironment {
    fn drop(&mut self) {
        for (name, value) in &self.previous {
            unsafe {
                if let Some(value) = value {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }
}

#[test]
fn test_portable_vault_roundtrip_with_explicit_key() {
    let _environment = PortableEnvironment::acquire();
    let dir = tempdir().unwrap();
    let vault_path = dir.path().join("portable.vlt");

    // Set custom key and portable mode
    unsafe {
        std::env::set_var("OPENUNPACK_PORTABLE_VAULT", "1");
        std::env::set_var("OPENUNPACK_VAULT_KEY", "super-secret-master-key-12345");
    }

    let vault = Vault::open(&vault_path).expect("failed to create portable vault");
    let input = PasswordInput {
        id: None,
        name: "Test Entry".into(),
        secret: Some("SecretPass123!🔑".into()),
        groups: vec!["GroupA".into()],
        notes: "Some notes".into(),
        enabled: true,
    };
    let view = vault.upsert(input).expect("failed to insert password");
    assert_eq!(view.name, "Test Entry");
    drop(vault);

    // Verify raw file on disk starts with OPENVLT1 magic
    let raw_bytes = fs::read(&vault_path).expect("failed to read vault file");
    assert!(
        raw_bytes.starts_with(VAULT_MAGIC),
        "file must start with OPENVLT1 magic"
    );

    // Reopen vault with same key
    let reopened = Vault::open(&vault_path).expect("failed to reopen portable vault");
    let views = reopened.views().expect("failed to list views");
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].name, "Test Entry");
    let secret = reopened
        .reveal(&views[0].id)
        .expect("failed to reveal secret");
    assert_eq!(secret, "SecretPass123!🔑");
    drop(reopened);

    // Test with wrong key -> must fail to decrypt
    unsafe {
        std::env::set_var("OPENUNPACK_VAULT_KEY", "wrong-key-xyz");
    }
    let fail_result = Vault::open(&vault_path);
    assert!(fail_result.is_err(), "opening with wrong key must fail");

    // Clean up env vars
    unsafe {
        std::env::remove_var("OPENUNPACK_PORTABLE_VAULT");
        std::env::remove_var("OPENUNPACK_VAULT_KEY");
    }
}

#[test]
fn test_portable_vault_autokey_generation() {
    let _environment = PortableEnvironment::acquire();
    let dir = tempdir().unwrap();
    let vault_path = dir.path().join("autokey.vlt");

    unsafe {
        std::env::set_var("OPENUNPACK_PORTABLE_VAULT", "1");
        std::env::remove_var("OPENUNPACK_VAULT_KEY");
    }

    let vault = Vault::open(&vault_path).expect("failed to open vault with auto-generated key");
    let input = PasswordInput {
        id: None,
        name: "AutoKey Entry".into(),
        secret: Some("AutoPass!@#".into()),
        groups: vec![],
        notes: "".into(),
        enabled: true,
    };
    vault.upsert(input).expect("upsert failed");
    drop(vault);

    // .vault.key file should exist alongside the vault
    let key_file = dir.path().join(".vault.key");
    assert!(
        key_file.exists(),
        "key file should have been auto-generated"
    );
    let key_bytes = fs::read(&key_file).unwrap();
    assert_eq!(key_bytes.len(), 32);

    // A key file alone must not silently re-enable portable mode: anyone who can
    // write the data directory could plant it. Reopening without the explicit
    // switch has to fail closed.
    unsafe {
        std::env::remove_var("OPENUNPACK_PORTABLE_VAULT");
        std::env::remove_var("OPENUNPACK_VAULT_KEY");
    }
    let refused = Vault::open(&vault_path)
        .err()
        .expect("reopen without opt-in must be refused");
    assert!(
        refused.contains("OPENUNPACK_PORTABLE_VAULT"),
        "error should tell the operator how to opt in, got: {refused}"
    );

    // With the explicit switch the same key file is accepted.
    unsafe {
        std::env::set_var("OPENUNPACK_PORTABLE_VAULT", "1");
    }
    let reopened = Vault::open(&vault_path).expect("reopen should succeed with explicit opt-in");
    let views = reopened.views().unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].name, "AutoKey Entry");

    unsafe {
        std::env::remove_var("OPENUNPACK_PORTABLE_VAULT");
    }
}
