//! Exercise the production store and shared adapters after retiring the unused v2 pipeline.
use openunpack_core::{repository::Repository, types::*};
use openunpack_infrastructure::sqlite::database_lease::DatabaseLease;
use rusqlite::{Connection, params};
use std::collections::BTreeMap;
use tempfile::tempdir;

fn frozen_job(stage: &str) -> JobView {
    JobView {
        id: format!("job-{stage}"),
        source: SourceGroup {
            path: "C:/synthetic/root.zip".into(),
            display_name: "root.zip".into(),
            volumes: vec!["C:/synthetic/root.zip".into()],
            size: 1024,
            missing_volume: false,
        },
        options: Settings {
            output_root: "C:/synthetic/frozen-output".into(),
            cache_root: "C:/synthetic/frozen-cache".into(),
            password_group: "frozen-group".into(),
            max_depth: 5,
            verification: "strict".into(),
            source_cleanup: "afterSuccess".into(),
            input_exclusions: vec!["ignored/**".into()],
            ..Settings::default()
        },
        status: "interrupted".into(),
        stage: stage.into(),
        progress: 0.4,
        message: "checkpoint".into(),
        attempts: 3,
        output_path: Some("C:/synthetic/frozen-output/result".into()),
        verification_scope: "strict".into(),
        created_at: "2026-10-20T00:00:00Z".into(),
        finished_at: None,
        workspace: Some("C:/synthetic/work/session".into()),
        nodes: vec![NodeView {
            id: "nested".into(),
            parent_id: Some("root".into()),
            name: "inner.7z".into(),
            depth: 1,
            status: "needsPassword".into(),
            stage: "nested".into(),
            progress: 0.2,
            attempts: 4,
            message: "needs password".into(),
            password_label: Some("previous label".into()),
        }],
        stage_times: BTreeMap::from([("extracting".into(), 1234), ("nested".into(), 2345)]),
        transfer_progress: None,
    }
}

#[test]
fn production_v1_reopens_wal_with_frozen_jobs_publications_and_outbox() {
    let temporary = tempdir().unwrap();
    let database = temporary.path().join("jobs.sqlite");
    let repository = Repository::open(&database).unwrap();
    // Keep a connection alive so closing the Repository is not a clean last-connection checkpoint.
    let witness = Connection::open(&database).unwrap();
    let stages = [
        "queued",
        "caching",
        "extracting",
        "nested",
        "verifying",
        "publishing",
        "cleanup",
    ];
    for stage in stages {
        repository.save_job(&frozen_job(stage)).unwrap();
    }
    let publication = Publication {
        id: "publication".into(),
        job_id: "job-publishing".into(),
        temporary_path: "C:/synthetic/staging".into(),
        final_path: "C:/synthetic/output".into(),
        manifest_hash: "synthetic-hash".into(),
        state: "committed".into(),
        files: vec![],
        verification: "strict".into(),
    };
    let event = SuccessEvent {
        event_id: "success".into(),
        password_id: "entry".into(),
        password_revision: 7,
        fingerprint: Some("synthetic-fingerprint".into()),
    };
    repository
        .commit_publication(
            &publication,
            &frozen_job("publishing"),
            std::slice::from_ref(&event),
        )
        .unwrap();
    let pending = Publication {
        id: "pending-publication".into(),
        state: "publishPending".into(),
        ..publication.clone()
    };
    repository.save_publication(&pending).unwrap();
    drop(repository);

    let reopened = Repository::open(&database).unwrap();
    let restored = reopened.jobs().unwrap();
    assert_eq!(restored.len(), stages.len());
    for stage in stages {
        let actual = restored
            .iter()
            .find(|job| job.id == format!("job-{stage}"))
            .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(frozen_job(stage)).unwrap()
        );
    }
    let publications = reopened.publications().unwrap();
    assert_eq!(publications.len(), 2);
    for expected in [publication, pending] {
        let actual = publications
            .iter()
            .find(|row| row.id == expected.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
    assert_eq!(
        serde_json::to_value(reopened.pending_success().unwrap()).unwrap(),
        serde_json::json!([event])
    );
    assert_eq!(
        witness
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        witness
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='application_storage'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn production_rejects_future_database_without_rewriting_records() {
    let temporary = tempdir().unwrap();
    let database = temporary.path().join("future.sqlite");
    let connection = Connection::open(&database).unwrap();
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA user_version=2; CREATE TABLE jobs(id TEXT PRIMARY KEY,status TEXT,json TEXT); CREATE TABLE success_outbox(event_id TEXT PRIMARY KEY,json TEXT);").unwrap();
    let foreign_json = "{\"futureFormat\":true,\"opaque\":\"keep\"}";
    connection
        .execute(
            "INSERT INTO jobs VALUES('future','running',?1)",
            [foreign_json],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO success_outbox VALUES('pending',?1)",
            [foreign_json],
        )
        .unwrap();
    assert!(
        matches!(Repository::open(&database), Err(error) if error == "UnsupportedDatabaseVersion")
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    for table in ["jobs", "success_outbox"] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT json FROM {table}"), [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            foreign_json
        );
    }
}

#[test]
fn production_rejects_unknown_job_json_without_partial_conversion() {
    let temporary = tempdir().unwrap();
    let database = temporary.path().join("jobs.sqlite");
    let repository = Repository::open(&database).unwrap();
    repository.save_job(&frozen_job("queued")).unwrap();
    let witness = Connection::open(&database).unwrap();
    let malformed = "{\"notAJobView\":true}";
    witness
        .execute(
            "INSERT INTO jobs VALUES(?1,?2,?3)",
            params!["unknown", "running", malformed],
        )
        .unwrap();
    assert!(matches!(repository.jobs(), Err(error) if error == "StorageError"));
    assert_eq!(
        witness
            .query_row("SELECT json FROM jobs WHERE id='unknown'", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
        malformed
    );
    let saved: String = witness
        .query_row("SELECT json FROM jobs WHERE id='job-queued'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&saved).unwrap(),
        serde_json::to_value(frozen_job("queued")).unwrap()
    );
}

#[test]
fn core_repository_and_infrastructure_enforce_the_same_database_lease() {
    let temporary = tempdir().unwrap();
    let database = temporary.path().join("jobs.sqlite");
    let repository = Repository::open(&database).unwrap();
    assert!(DatabaseLease::exclusive(&database).is_err());
    drop(repository);
    let exclusive = DatabaseLease::exclusive(&database).unwrap();
    assert!(Repository::open(&database).is_err());
    drop(exclusive);
    assert!(Repository::open(&database).is_ok());
}

#[test]
fn compatibility_worker_and_infrastructure_share_exit_receipt_registry() {
    let core = openunpack_core::worker::WorkerClient {
        executable: "unused.exe".into(),
        engine: "unused.dll".into(),
    };
    let infrastructure = openunpack_infrastructure::worker::WorkerClient {
        executable: "unused.exe".into(),
        engine: "unused.dll".into(),
    };
    let request = uuid::Uuid::new_v4().to_string();
    core.retain_request_receipt(&request).unwrap();
    let receipt = infrastructure
        .request_exit_receipt(&request)
        .expect("same process-global registry");
    assert!(receipt.exit_confirmed);
    assert!(receipt.processes.is_empty());
    infrastructure.forget_confirmed_request(&request);
    assert!(core.request_exit_receipt(&request).is_none());
}
