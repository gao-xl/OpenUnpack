//! Native filesystem/SQLite boundary tests; no Worker or GUI is simulated here.
use super::super::{NodeWork, SavedRun, SealedNode, now};
use super::{PublicationService, SealedPublication};
use crate::{paths, repository::Repository, route::ExtractionRoute, types::*};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

struct Fixture {
    directory: tempfile::TempDir,
    repository: Repository,
    job: JobView,
    run: SavedRun,
    work: PathBuf,
    service: PublicationService,
}

impl Fixture {
    fn new() -> Self {
        use sha2::{Digest, Sha256};
        let directory = tempfile::tempdir().unwrap();
        let work = directory.path().join("work");
        let source = directory.path().join("archive.zip");
        fs::write(&source, b"source preserved").unwrap();
        paths::mark_owned(&work, "publication-job", "").unwrap();
        let sealed = work.join("sealed");
        fs::create_dir(&sealed).unwrap();
        fs::write(sealed.join("result.txt"), b"verified result").unwrap();
        let hash = hex::encode(Sha256::digest(b"verified result"));
        let file = FileRecord {
            relative_path: "result.txt".into(),
            size: 15,
            sha256: hash.clone(),
            ..FileRecord::default()
        };
        let source = SourceGroup {
            path: paths::path_string(&source),
            display_name: "archive.zip".into(),
            volumes: vec![paths::path_string(&source)],
            size: 16,
            missing_volume: false,
        };
        let settings = Settings {
            output_root: paths::path_string(&directory.path().join("output")),
            verification: "strict".into(),
            source_cleanup: "keep".into(),
            unwrap_single_root: false,
            clean_empty_dirs: false,
            ..Settings::default()
        };
        let job = JobView {
            id: "publication-job".into(),
            source: source.clone(),
            options: settings.clone(),
            status: "running".into(),
            stage: "planning".into(),
            progress: 0.,
            attempts: 1,
            message: String::new(),
            output_path: None,
            verification_scope: String::new(),
            created_at: now(),
            finished_at: None,
            workspace: Some(paths::path_string(&work)),
            nodes: vec![NodeView {
                id: "root".into(),
                parent_id: None,
                name: "root".into(),
                depth: 0,
                status: "completed".into(),
                stage: "finished".into(),
                progress: 1.,
                attempts: 1,
                message: String::new(),
                password_label: None,
            }],
            stage_times: BTreeMap::new(),
            transfer_progress: None,
        };
        let run = SavedRun {
            version: 2,
            job_id: job.id.clone(),
            source: source.clone(),
            witnesses: vec![],
            options: settings,
            input_files: vec![],
            input_retired: true,
            fingerprint: None,
            pending: VecDeque::new(),
            failed: vec![],
            sealed: vec![SealedNode {
                node: NodeWork {
                    id: "root".into(),
                    group: source,
                    prefix: PathBuf::new(),
                    depth: 0,
                    parent: None,
                },
                directory: sealed.clone(),
                files: vec![file],
                verification_scope: "backendCrc+streamSha256".into(),
            }],
            final_files: vec![CopyFile {
                source_path: paths::path_string(&sealed.join("result.txt")),
                relative_path: "result.txt".into(),
                size: 15,
                sha256: hash,
                ..CopyFile::default()
            }],
            events: vec![SuccessEvent {
                event_id: "publication-event".into(),
                password_id: "password".into(),
                password_revision: 1,
                fingerprint: None,
            }],
            expanded: 15,
            entries: 1,
            views: job.nodes.clone(),
            retired_files: HashSet::new(),
        };
        let repository = Repository::open(&directory.path().join("jobs.sqlite")).unwrap();
        repository.save_job(&job).unwrap();
        Self {
            directory,
            repository,
            job,
            run,
            work,
            service: PublicationService::default(),
        }
    }

    async fn seal(&mut self) -> SealedPublication {
        let source = self.job.source.clone();
        let target_dir = PathBuf::from(&self.job.options.output_root);
        let staged = self
            .service
            .prepare(
                &self.repository,
                &mut self.job,
                &self.run,
                &source,
                &self.work,
                &ExtractionRoute::Direct {
                    target_dir,
                    reason: "fixture".into(),
                },
                true,
            )
            .unwrap()
            .unwrap();
        assert!(staged.copy_request().is_none());
        staged.finish_copy().unwrap();
        staged
            .seal(&self.repository, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn commit_pins_verified_target_and_commits_job_publication_outbox_together() {
    let mut fixture = Fixture::new();
    let sealed = fixture.seal().await;
    let committed = fixture
        .service
        .commit(&fixture.repository, sealed, &mut fixture.job)
        .unwrap()
        .unwrap();
    let target = PathBuf::from(fixture.job.output_path.as_ref().unwrap()).join("result.txt");
    assert_eq!(fs::read(&target).unwrap(), b"verified result");
    let output = target.parent().unwrap();
    let replacement = output.with_extension("replacement");
    assert!(
        fs::rename(output, &replacement).is_err(),
        "published root identity stays pinned through source cleanup"
    );
    assert_eq!(fixture.repository.jobs().unwrap()[0].status, "completed");
    assert_eq!(
        fixture.repository.publications().unwrap()[0].state,
        "committed"
    );
    assert_eq!(fixture.repository.pending_success().unwrap().len(), 1);
    assert!(!committed.cleanup_allowed());
    drop(committed);
    fs::rename(output, replacement).unwrap();
    assert_eq!(
        fs::read(&fixture.job.source.path).unwrap(),
        b"source preserved"
    );
}

#[tokio::test]
async fn database_failure_after_rename_preserves_intent_for_verified_recovery() {
    let mut fixture = Fixture::new();
    let database = fixture.directory.path().join("jobs.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_completion BEFORE INSERT ON jobs WHEN NEW.status = 'completed' BEGIN SELECT RAISE(ABORT, 'injected completion failure'); END;").unwrap();
    let sealed = fixture.seal().await;
    assert!(
        fixture
            .service
            .commit(&fixture.repository, sealed, &mut fixture.job)
            .is_err()
    );
    let publication = fixture.repository.publications().unwrap().remove(0);
    assert!(PathBuf::from(&publication.final_path).is_dir());
    assert!(!PathBuf::from(&publication.temporary_path).exists());
    assert_eq!(publication.state, "ready");
    assert_eq!(fixture.repository.jobs().unwrap()[0].status, "running");
    assert!(fixture.repository.pending_success().unwrap().is_empty());
    assert_eq!(
        fs::read(&fixture.job.source.path).unwrap(),
        b"source preserved"
    );
    connection
        .execute_batch("DROP TRIGGER fail_completion;")
        .unwrap();
    let mut recovered_job = fixture.repository.jobs().unwrap().remove(0);
    let recovered_service = PublicationService::default();
    let recovery = recovered_service
        .prepare_recovery(publication, &recovered_job, &fixture.work)
        .unwrap();
    let tree = recovery
        .seal(Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    recovered_service
        .commit_recovery(&fixture.repository, recovery, tree, &mut recovered_job)
        .unwrap();
    assert_eq!(recovered_job.status, "completed");
    assert_eq!(fixture.repository.pending_success().unwrap().len(), 1);
    assert_eq!(
        fixture.repository.publications().unwrap()[0].state,
        "committed"
    );
}

#[tokio::test]
async fn late_skip_preserves_concurrent_output_and_abandons_only_owned_staging() {
    let mut fixture = Fixture::new();
    fixture.job.options.conflict_policy = "skip".into();
    let sealed = fixture.seal().await;
    let publication = fixture.repository.publications().unwrap().remove(0);
    fs::create_dir(&publication.final_path).unwrap();
    let sentinel = PathBuf::from(&publication.final_path).join("existing.txt");
    fs::write(&sentinel, b"keep existing").unwrap();
    assert!(
        fixture
            .service
            .commit(&fixture.repository, sealed, &mut fixture.job)
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read(sentinel).unwrap(), b"keep existing");
    assert!(!PathBuf::from(publication.temporary_path).exists());
    assert_eq!(
        fixture.repository.publications().unwrap()[0].state,
        "abandoned"
    );
    assert!(fixture.repository.pending_success().unwrap().is_empty());
}

#[tokio::test]
async fn cancellation_after_sealing_cannot_reclassify_an_atomic_commit() {
    let mut fixture = Fixture::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let route = ExtractionRoute::Direct {
        target_dir: PathBuf::from(&fixture.job.options.output_root),
        reason: "fixture".into(),
    };
    let source = fixture.job.source.clone();
    let staged = fixture
        .service
        .prepare(
            &fixture.repository,
            &mut fixture.job,
            &fixture.run,
            &source,
            &fixture.work,
            &route,
            true,
        )
        .unwrap()
        .unwrap();
    staged.finish_copy().unwrap();
    let sealed = staged
        .seal(&fixture.repository, cancel.clone())
        .await
        .unwrap();
    cancel.store(true, Ordering::Release);
    let _committed = fixture
        .service
        .commit(&fixture.repository, sealed, &mut fixture.job)
        .unwrap()
        .unwrap();
    assert_eq!(fixture.job.status, "completed");
}
