use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};

use crate::types::{JobView, Publication, SuccessEvent};

#[path = "database_lease.rs"]
pub mod database_lease;
use database_lease::DatabaseLease;

fn storage_error(context: &'static str, err: impl std::fmt::Display) -> String {
    tracing::warn!(context, error = %err, "repository storage error");
    "StorageError".to_string()
}

pub struct Repository {
    conn: Mutex<Connection>,
    _storage_lease: DatabaseLease,
}

impl Repository {
    pub fn open(path: &Path) -> Result<Self, String> {
        let storage_lease = DatabaseLease::shared(path)?;
        let path = storage_lease.database_path();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| storage_error("sqlite", e))?;
        }

        let mut conn = Connection::open(path).map_err(|e| storage_error("sqlite", e))?;

        conn.busy_timeout(Duration::from_secs(2))
            .map_err(|e| storage_error("sqlite", e))?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| storage_error("sqlite", e))?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(|e| storage_error("sqlite", e))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| storage_error("sqlite", e))?;

        let user_version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|e| storage_error("sqlite", e))?;

        if user_version > 2 {
            return Err("UnsupportedDatabaseVersion".to_string());
        }

        if user_version == 0 {
            let tx = conn.transaction().map_err(|e| storage_error("sqlite", e))?;

            tx.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS jobs (
                    id TEXT PRIMARY KEY,
                    status TEXT NOT NULL,
                    json TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS publications (
                    id TEXT PRIMARY KEY,
                    job_id TEXT NOT NULL,
                    state TEXT NOT NULL,
                    json TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS success_outbox (
                    event_id TEXT PRIMARY KEY,
                    json TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_publications_job_state
                    ON publications(job_id, state);
                "#,
            )
            .map_err(|e| storage_error("sqlite", e))?;

            tx.pragma_update(None, "user_version", 2)
                .map_err(|e| storage_error("sqlite", e))?;

            tx.commit().map_err(|e| storage_error("sqlite", e))?;
        } else if user_version == 1 {
            // Migration v1 -> v2: index publications by (job_id, state) so that
            // job-scoped delete/recover/cleanup queries no longer scan the whole
            // table as the history grows.
            let tx = conn.transaction().map_err(|e| storage_error("sqlite", e))?;
            tx.execute_batch(
                r#"
                CREATE INDEX IF NOT EXISTS idx_publications_job_state
                    ON publications(job_id, state);
                "#,
            )
            .map_err(|e| storage_error("sqlite", e))?;
            tx.pragma_update(None, "user_version", 2)
                .map_err(|e| storage_error("sqlite", e))?;
            tx.commit().map_err(|e| storage_error("sqlite", e))?;
        }

        Ok(Self {
            conn: Mutex::new(conn),
            _storage_lease: storage_lease,
        })
    }

    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.conn
            .lock()
            .map_err(|_| "database lock poisoned".to_string())
    }

    pub fn save_job(&self, job: &JobView) -> Result<(), String> {
        let json = serde_json::to_string(job).map_err(|e| storage_error("sqlite", e))?;
        let conn = self.lock_conn()?;
        conn.execute(
            r#"
            INSERT INTO jobs (id, status, json)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                json = excluded.json
            "#,
            params![job.id, job.status, json],
        )
        .map_err(|e| storage_error("sqlite", e))?;
        Ok(())
    }

    pub fn jobs(&self) -> Result<Vec<JobView>, String> {
        let conn = self.lock_conn()?;
        let mut stmt = conn
            .prepare("SELECT json FROM jobs ORDER BY id ASC")
            .map_err(|e| storage_error("sqlite", e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| storage_error("sqlite", e))?;

        let mut out = Vec::new();
        for r in rows {
            let json_str = r.map_err(|e| storage_error("sqlite", e))?;
            let item: JobView =
                serde_json::from_str(&json_str).map_err(|e| storage_error("sqlite", e))?;
            out.push(item);
        }
        Ok(out)
    }

    pub fn remove_job(&self, id: &str) -> Result<(), String> {
        let mut conn = self.lock_conn()?;
        let tx = conn.transaction().map_err(|e| storage_error("sqlite", e))?;

        let status_opt: Option<String> = tx
            .query_row("SELECT status FROM jobs WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| storage_error("sqlite", e))?;

        let Some(status) = status_opt else {
            return Ok(());
        };

        if status == "running" || status == "stopping" {
            return Err("ActiveJob".to_string());
        }

        let has_unresolved: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM publications WHERE job_id = ?1 AND state NOT IN ('committed', 'abandoned'))",
                params![id],
                |r| r.get(0),
            )
            .map_err(|e| storage_error("sqlite", e))?;

        if has_unresolved {
            return Err("UnresolvedPublication".to_string());
        }

        tx.execute("DELETE FROM publications WHERE job_id = ?1", params![id])
            .map_err(|e| storage_error("sqlite", e))?;
        tx.execute("DELETE FROM jobs WHERE id = ?1", params![id])
            .map_err(|e| storage_error("sqlite", e))?;

        tx.commit().map_err(|e| storage_error("sqlite", e))?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<(), String> {
        let mut conn = self.lock_conn()?;
        let tx = conn.transaction().map_err(|e| storage_error("sqlite", e))?;

        tx.execute(
            r#"
            DELETE FROM publications
            WHERE job_id IN (
                SELECT id FROM jobs
                WHERE status IN ('completed', 'failed', 'cancelled', 'skipped', 'partial', 'cleanupPending')
                  AND NOT EXISTS (
                      SELECT 1 FROM publications p2
                      WHERE p2.job_id = jobs.id AND p2.state NOT IN ('committed', 'abandoned')
                  )
            )
            "#,
            [],
        )
        .map_err(|e| storage_error("sqlite", e))?;

        tx.execute(
            r#"
            DELETE FROM jobs
            WHERE status IN ('completed', 'failed', 'cancelled', 'skipped', 'partial', 'cleanupPending')
              AND NOT EXISTS (
                  SELECT 1 FROM publications
                  WHERE publications.job_id = jobs.id AND publications.state NOT IN ('committed', 'abandoned')
              )
            "#,
            [],
        )
        .map_err(|e| storage_error("sqlite", e))?;

        tx.commit().map_err(|e| storage_error("sqlite", e))?;
        Ok(())
    }

    pub fn save_publication(&self, publication: &Publication) -> Result<(), String> {
        let json = serde_json::to_string(publication).map_err(|e| storage_error("sqlite", e))?;
        let conn = self.lock_conn()?;
        conn.execute(
            r#"
            INSERT INTO publications (id, job_id, state, json)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(id) DO UPDATE SET
                job_id = excluded.job_id,
                state = excluded.state,
                json = excluded.json
            "#,
            params![publication.id, publication.job_id, publication.state, json],
        )
        .map_err(|e| storage_error("sqlite", e))?;
        Ok(())
    }

    pub fn publications(&self) -> Result<Vec<Publication>, String> {
        let conn = self.lock_conn()?;
        let mut stmt = conn
            .prepare("SELECT json FROM publications ORDER BY id ASC")
            .map_err(|e| storage_error("sqlite", e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| storage_error("sqlite", e))?;

        let mut out = Vec::new();
        for r in rows {
            let json_str = r.map_err(|e| storage_error("sqlite", e))?;
            let item: Publication =
                serde_json::from_str(&json_str).map_err(|e| storage_error("sqlite", e))?;
            out.push(item);
        }
        Ok(out)
    }

    pub fn commit_publication(
        &self,
        publication: &Publication,
        job: &JobView,
        events: &[SuccessEvent],
    ) -> Result<(), String> {
        if publication.job_id != job.id || publication.state != "committed" {
            return Err("publication.job_id does not match job.id".to_string());
        }

        let pub_json =
            serde_json::to_string(publication).map_err(|e| storage_error("sqlite", e))?;
        let job_json = serde_json::to_string(job).map_err(|e| storage_error("sqlite", e))?;

        let mut event_jsons = Vec::with_capacity(events.len());
        for ev in events {
            let ej = serde_json::to_string(ev).map_err(|e| storage_error("sqlite", e))?;
            event_jsons.push((&ev.event_id, ej));
        }

        let mut conn = self.lock_conn()?;
        let tx = conn.transaction().map_err(|e| storage_error("sqlite", e))?;

        tx.execute(
            r#"
            INSERT INTO publications (id, job_id, state, json)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(id) DO UPDATE SET
                job_id = excluded.job_id,
                state = excluded.state,
                json = excluded.json
            "#,
            params![
                publication.id,
                publication.job_id,
                publication.state,
                pub_json
            ],
        )
        .map_err(|e| storage_error("sqlite", e))?;

        tx.execute(
            r#"
            INSERT INTO jobs (id, status, json)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                json = excluded.json
            "#,
            params![job.id, job.status, job_json],
        )
        .map_err(|e| storage_error("sqlite", e))?;

        for (event_id, ej) in event_jsons {
            tx.execute(
                r#"
                INSERT OR IGNORE INTO success_outbox (event_id, json)
                VALUES (?1, ?2)
                "#,
                params![event_id, ej],
            )
            .map_err(|e| storage_error("sqlite", e))?;
        }

        tx.commit().map_err(|e| storage_error("sqlite", e))?;
        Ok(())
    }

    pub fn pending_success(&self) -> Result<Vec<SuccessEvent>, String> {
        let conn = self.lock_conn()?;
        let mut stmt = conn
            .prepare("SELECT json FROM success_outbox ORDER BY event_id ASC")
            .map_err(|e| storage_error("sqlite", e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| storage_error("sqlite", e))?;

        let mut out = Vec::new();
        for r in rows {
            let json_str = r.map_err(|e| storage_error("sqlite", e))?;
            let item: SuccessEvent =
                serde_json::from_str(&json_str).map_err(|e| storage_error("sqlite", e))?;
            out.push(item);
        }
        Ok(out)
    }

    pub fn ack_success(&self, event_id: &str) -> Result<(), String> {
        let conn = self.lock_conn()?;
        conn.execute(
            "DELETE FROM success_outbox WHERE event_id = ?1",
            params![event_id],
        )
        .map_err(|e| storage_error("sqlite", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Settings, SourceGroup};
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn job(id: &str, status: &str) -> JobView {
        JobView {
            id: id.into(),
            source: SourceGroup {
                path: "synthetic.zip".into(),
                display_name: "synthetic.zip".into(),
                volumes: vec![],
                size: 1,
                missing_volume: false,
            },
            options: Settings::default(),
            status: status.into(),
            stage: "test".into(),
            progress: 0.0,
            message: String::new(),
            attempts: 0,
            output_path: None,
            verification_scope: String::new(),
            created_at: "2026-10-08T00:00:00Z".into(),
            finished_at: None,
            workspace: None,
            nodes: vec![],
            stage_times: BTreeMap::new(),
            transfer_progress: None,
        }
    }
    fn publication(id: &str, job_id: &str, state: &str) -> Publication {
        Publication {
            id: id.into(),
            job_id: job_id.into(),
            temporary_path: "synthetic-tmp".into(),
            final_path: "synthetic-final".into(),
            manifest_hash: String::new(),
            state: state.into(),
            files: vec![],
            verification: "standard".into(),
        }
    }
    fn event() -> SuccessEvent {
        SuccessEvent {
            event_id: "event-1".into(),
            password_id: "entry-1".into(),
            password_revision: 1,
            fingerprint: None,
        }
    }
    #[test]
    fn transaction_rolls_back_all_rows_and_outbox_is_idempotent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("jobs.sqlite");
        let repo = Repository::open(&path).unwrap();
        repo.save_job(&job("job-1", "queued")).unwrap();
        repo.conn.lock().unwrap().execute_batch("CREATE TRIGGER injected_error BEFORE INSERT ON success_outbox BEGIN SELECT RAISE(ABORT, 'synthetic fault'); END;").unwrap();
        let published = publication("pub-1", "job-1", "committed");
        assert!(
            repo.commit_publication(&published, &job("job-1", "completed"), &[event()])
                .is_err()
        );
        assert!(repo.publications().unwrap().is_empty());
        assert!(repo.pending_success().unwrap().is_empty());
        assert_eq!(repo.jobs().unwrap()[0].status, "queued");
        repo.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER injected_error;")
            .unwrap();
        repo.commit_publication(&published, &job("job-1", "completed"), &[event(), event()])
            .unwrap();
        repo.commit_publication(&published, &job("job-1", "completed"), &[event()])
            .unwrap();
        assert_eq!(repo.pending_success().unwrap().len(), 1);
        drop(repo);
        let reopened = Repository::open(&path).unwrap();
        assert_eq!(reopened.jobs().unwrap()[0].status, "completed");
        assert_eq!(reopened.publications().unwrap()[0].state, "committed");
        assert_eq!(reopened.pending_success().unwrap().len(), 1);
        reopened.ack_success("event-1").unwrap();
        reopened.ack_success("event-1").unwrap();
        assert!(reopened.pending_success().unwrap().is_empty());
    }
    #[test]
    fn history_and_remove_preserve_uncommitted_and_active_jobs() {
        let dir = tempdir().unwrap();
        let repo = Repository::open(&dir.path().join("jobs.sqlite")).unwrap();
        for (id, status) in [
            ("done", "completed"),
            ("pending", "failed"),
            ("active", "running"),
            ("recovery", "interrupted"),
        ] {
            repo.save_job(&job(id, status)).unwrap();
        }
        repo.save_publication(&publication("done-pub", "done", "committed"))
            .unwrap();
        repo.save_publication(&publication("pending-pub", "pending", "publishPending"))
            .unwrap();
        assert!(repo.remove_job("pending").is_err());
        assert!(repo.remove_job("active").is_err());
        repo.clear_history().unwrap();
        let ids: Vec<_> = repo.jobs().unwrap().into_iter().map(|j| j.id).collect();
        assert_eq!(ids, ["active", "pending", "recovery"]);
        assert_eq!(repo.publications().unwrap().len(), 1);
        repo.remove_job("missing").unwrap();
    }
    #[test]
    fn busy_lock_has_a_finite_wait_and_can_recover() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("jobs.sqlite");
        let repo = Repository::open(&path).unwrap();
        let blocker = Connection::open(&path).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();
        let start = std::time::Instant::now();
        assert_eq!(
            repo.save_job(&job("blocked", "queued")).unwrap_err(),
            "StorageError"
        );
        assert!(start.elapsed() < Duration::from_secs(6));
        blocker.execute_batch("ROLLBACK;").unwrap();
        repo.save_job(&job("recovered", "queued")).unwrap();
        assert_eq!(repo.jobs().unwrap().len(), 1);
    }

    fn legacy_schema_without_index(conn: &Connection) {
        conn.execute_batch(
            r#"
            CREATE TABLE jobs (
                id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                json TEXT NOT NULL
            );
            CREATE TABLE publications (
                id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                state TEXT NOT NULL,
                json TEXT NOT NULL
            );
            CREATE TABLE success_outbox (
                event_id TEXT PRIMARY KEY,
                json TEXT NOT NULL
            );
            "#,
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
    }

    fn index_present(conn: &Connection) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_publications_job_state'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap()
    }

    #[test]
    fn v1_database_migrates_to_v2_and_indexes_publications() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("jobs.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            legacy_schema_without_index(&conn);
        }
        let repo = Repository::open(&path).unwrap();
        let conn = repo.conn.lock().unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        assert!(index_present(&conn));
    }

    #[test]
    fn v1_migration_rolls_back_without_bumping_version_on_failure() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("jobs.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            // Corrupted v1: version is 1 but the publications table is missing,
            // so index creation must fail and leave the version untouched.
            conn.execute_batch(
                "CREATE TABLE jobs (id TEXT PRIMARY KEY, status TEXT NOT NULL, json TEXT NOT NULL);",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        assert!(Repository::open(&path).is_err());
        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
        assert!(!index_present(&conn));
    }

    #[test]
    fn fresh_database_lands_directly_on_v2_with_index() {
        let dir = tempdir().unwrap();
        let repo = Repository::open(&dir.path().join("jobs.sqlite")).unwrap();
        let conn = repo.conn.lock().unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        assert!(index_present(&conn));
    }
}
