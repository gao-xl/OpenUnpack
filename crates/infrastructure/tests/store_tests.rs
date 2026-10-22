use openunpack_application::{PasswordSuccessEvent, VaultStore};
use openunpack_domain::CandidateOrigin;
use openunpack_infrastructure::DpapiVaultStore;
use tempfile::tempdir;

#[test]
fn test_dpapi_vault_store_lifecycle() {
    let dir = tempdir().unwrap();
    let vault_path = dir.path().join("vault.json");

    let store = DpapiVaultStore::open(&vault_path).expect("open vault");

    let view = store.add_entry("MyPass", "Secret123!").expect("add pass");
    assert_eq!(view.name, "MyPass");

    let list = store.list_entries().expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, view.id);

    let revealed = store.reveal_entry(&view.id).expect("reveal");
    assert_eq!(revealed, "Secret123!");

    let candidates = store.load_decrypt_candidates().expect("load candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].secret, "Secret123!");

    store
        .record_success(&PasswordSuccessEvent {
            event_id: "test-delivered-1".into(),
            candidate_id: view.id.clone(),
            candidate_revision: view.revision,
            origin: CandidateOrigin::VaultEntry,
            fingerprint: None,
        })
        .expect("record success");
    let candidates_after = store.load_decrypt_candidates().expect("load candidates");
    assert_eq!(candidates_after[0].success_count, 1);

    store.delete_entry(&view.id).expect("delete");
    let list_after = store.list_entries().expect("list after delete");
    assert_eq!(list_after.len(), 0);
}
