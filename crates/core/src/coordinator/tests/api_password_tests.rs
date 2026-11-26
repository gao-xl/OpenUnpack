use super::*;

struct PasswordPolicy(bool);
impl JobExecutionPolicy for PasswordPolicy {
    fn authorize(&self, _: &JobView) -> Result<(), String> {
        if self.0 {
            Ok(())
        } else {
            Err("synthetic denied".into())
        }
    }
    fn authorize_path(&self, _: &Path, _: bool) -> Result<(), String> {
        Ok(())
    }
}
fn fixture() -> (tempfile::TempDir, Arc<Coordinator>, Vec<SourceGroup>) {
    let directory = tempfile::tempdir().unwrap();
    let archive = directory.path().join("sample.zip");
    fs::write(&archive, b"synthetic metadata-only fixture").unwrap();
    let core = Coordinator::open(
        directory.path().join("data"),
        directory.path().join("missing-worker.exe"),
        directory.path().join("missing-engine.dll"),
    )
    .unwrap();
    let sources = scanner::scan(&[paths::path_string(&archive)], false, &[])
        .unwrap()
        .sources;
    (directory, core, sources)
}

#[test]
fn api_admission_keeps_password_only_for_successful_new_jobs_in_memory() {
    let (_directory, core, sources) = fixture();
    assert!(
        core.add_jobs_with_password_authorized(
            sources.clone(),
            Settings::default(),
            Some(zeroize::Zeroizing::new("synthetic-denied".into())),
            Arc::new(PasswordPolicy(false)),
        )
        .is_err()
    );
    assert!(core.ephemeral_passwords.lock().unwrap().is_empty());
    assert!(core.repository.jobs().unwrap().is_empty());
    let added = core
        .add_jobs_with_password_authorized(
            sources.clone(),
            Settings::default(),
            Some(zeroize::Zeroizing::new("synthetic-one-time".into())),
            Arc::new(PasswordPolicy(true)),
        )
        .unwrap();
    assert_eq!(added.len(), 1);
    assert!(core.execution_policy(&added[0].id).unwrap().is_some());
    assert_eq!(
        core.ephemeral_passwords.lock().unwrap()[&added[0].id][0].as_str(),
        "synthetic-one-time"
    );
    assert!(
        core.add_jobs_with_password_authorized(
            sources,
            Settings::default(),
            Some(zeroize::Zeroizing::new("synthetic-duplicate".into())),
            Arc::new(PasswordPolicy(true)),
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        core.ephemeral_passwords.lock().unwrap()[&added[0].id].len(),
        1
    );
    assert!(core.vault().unwrap().views().unwrap().is_empty());
    assert!(
        !serde_json::to_string(&core.repository.jobs().unwrap())
            .unwrap()
            .contains("synthetic-one-time")
    );
}

#[test]
fn failed_job_persistence_leaves_no_temporary_password_or_execution_policy() {
    let (_directory, core, sources) = fixture();
    let connection = rusqlite::Connection::open(core.data.join("jobs.sqlite")).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_api_job BEFORE INSERT ON jobs BEGIN SELECT RAISE(ABORT, 'synthetic storage failure'); END;").unwrap();
    assert!(
        core.add_jobs_with_password_authorized(
            sources,
            Settings::default(),
            Some(zeroize::Zeroizing::new("synthetic-one-time".into())),
            Arc::new(PasswordPolicy(true)),
        )
        .is_err()
    );
    assert!(core.ephemeral_passwords.lock().unwrap().is_empty());
    assert!(core.execution_policies.lock().unwrap().is_empty());
    assert!(core.repository.jobs().unwrap().is_empty());
    assert!(core.jobs.lock().unwrap().is_empty());
}

#[test]
fn authorized_input_binds_policy_before_delivery_and_propagates_closed_waiter() {
    let (_directory, core, sources) = fixture();
    let added = core.add_jobs(sources, Settings::default()).unwrap();
    let id = &added[0].id;
    let (sender, mut receiver) = tokio::sync::oneshot::channel();
    core.pending_inputs
        .lock()
        .unwrap()
        .insert(id.clone(), sender);
    assert!(
        core.submit_input_authorized(
            id,
            "synthetic-denied".into(),
            Arc::new(PasswordPolicy(false))
        )
        .is_err()
    );
    assert!(core.pending_inputs.lock().unwrap().contains_key(id));
    assert!(core.execution_policy(id).unwrap().is_none());
    assert!(
        core.submit_input_authorized(id, "synthetic-input".into(), Arc::new(PasswordPolicy(true)))
            .unwrap()
    );
    assert!(core.execution_policy(id).unwrap().is_some());
    assert_eq!(receiver.try_recv().unwrap(), "synthetic-input");
    assert!(
        !core
            .submit_input_authorized(
                id,
                "synthetic-unused".into(),
                Arc::new(PasswordPolicy(true))
            )
            .unwrap()
    );
    let (sender, receiver) = tokio::sync::oneshot::channel();
    drop(receiver);
    core.pending_inputs
        .lock()
        .unwrap()
        .insert(id.clone(), sender);
    let previous_policy = core.execution_policy(id).unwrap().unwrap();
    assert!(
        core.submit_input_authorized(
            id,
            "synthetic-closed".into(),
            Arc::new(PasswordPolicy(true))
        )
        .is_err()
    );
    assert!(Arc::ptr_eq(
        &previous_policy,
        &core.execution_policy(id).unwrap().unwrap()
    ));
    assert_eq!(core.get_job(id).unwrap().status, "queued");
    assert!(core.vault().unwrap().views().unwrap().is_empty());
}
