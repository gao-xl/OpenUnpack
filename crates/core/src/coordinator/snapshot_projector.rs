use openunpack_protocol::{
    AppSnapshot, JobView, PasswordView, SnapshotDelta, SnapshotState, SnapshotUpdate,
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Owns the transient state needed to publish desktop snapshot updates.
/// The authoritative jobs, settings and revision remain in the visible-state store.
#[derive(Default)]
pub(super) struct SnapshotProjector {
    delta_cache: Mutex<Option<AppSnapshot>>,
    engine_probe: Mutex<Option<(Instant, bool)>>,
}

impl SnapshotProjector {
    /// Capture and publish are serialized together. Capturing before taking this
    /// lock would let an older, slower reader overwrite a newer delta baseline.
    ///
    /// The caller may acquire its visible-state lock inside `capture`; it must
    /// never call this method while already holding that lock. The closure is
    /// called synchronously and is never retained by the projector.
    pub(super) fn update(
        &self,
        since: Option<u64>,
        capture: impl FnOnce() -> Result<AppSnapshot, String>,
    ) -> Result<SnapshotUpdate, String> {
        let mut cache = self
            .delta_cache
            .lock()
            .map_err(|_| "内部状态不可用；请重启并检查恢复记录。".to_string())?;
        let current = capture()?;
        let delta = cache
            .as_ref()
            .filter(|cached| {
                since == Some(cached.revision) && cached.session_id == current.session_id
            })
            .map(|cached| compute_snapshot_delta(cached, &current));
        let update = SnapshotUpdate {
            session_id: current.session_id.clone(),
            revision: current.revision,
            full: delta.is_none().then(|| current.clone()),
            delta,
        };
        *cache = Some(current);
        Ok(update)
    }

    /// The installed worker/engine paths are fixed for this projector's lifetime.
    /// A poisoned probe lock falls back to direct stats so availability is never
    /// hidden by an unrelated polling failure.
    pub(super) fn engine_available(&self, executable: &Path, engine: &Path) -> bool {
        const PROBE_TTL: Duration = Duration::from_secs(2);
        let probe_files = || executable.is_file() && engine.is_file();
        let Ok(mut probe) = self.engine_probe.lock() else {
            return probe_files();
        };
        if let Some((probed_at, available)) = *probe
            && probed_at.elapsed() < PROBE_TTL
        {
            return available;
        }
        let available = probe_files();
        *probe = Some((Instant::now(), available));
        available
    }
}

fn compute_snapshot_delta(base: &AppSnapshot, current: &AppSnapshot) -> SnapshotDelta {
    let base_jobs: HashMap<&str, &JobView> = base.jobs.iter().map(|j| (j.id.as_str(), j)).collect();
    let current_job_ids: HashSet<&str> = current.jobs.iter().map(|j| j.id.as_str()).collect();

    let mut jobs_changed = Vec::new();
    for job in &current.jobs {
        match base_jobs.get(job.id.as_str()) {
            Some(&base_job) if base_job == job => {}
            _ => jobs_changed.push(job.clone()),
        }
    }
    let jobs_removed = base
        .jobs
        .iter()
        .filter(|j| !current_job_ids.contains(j.id.as_str()))
        .map(|j| j.id.clone())
        .collect();

    let base_passwords: HashMap<&str, &PasswordView> =
        base.passwords.iter().map(|p| (p.id.as_str(), p)).collect();
    let current_password_ids: HashSet<&str> =
        current.passwords.iter().map(|p| p.id.as_str()).collect();

    let mut passwords_changed = Vec::new();
    for pwd in &current.passwords {
        match base_passwords.get(pwd.id.as_str()) {
            Some(&base_pwd) if base_pwd == pwd => {}
            _ => passwords_changed.push(pwd.clone()),
        }
    }
    let passwords_removed = base
        .passwords
        .iter()
        .filter(|p| !current_password_ids.contains(p.id.as_str()))
        .map(|p| p.id.clone())
        .collect();

    let settings = (current.settings != base.settings).then(|| current.settings.clone());
    let base_state = snapshot_state(base);
    let current_state = snapshot_state(current);
    let state = (current_state != base_state).then_some(current_state);

    SnapshotDelta {
        jobs_changed,
        jobs_removed,
        passwords_changed,
        passwords_removed,
        settings,
        state,
    }
}

fn snapshot_state(snapshot: &AppSnapshot) -> SnapshotState {
    SnapshotState {
        running: snapshot.running,
        paused: snapshot.paused,
        vault_error: snapshot.vault_error.clone(),
        data_directory: snapshot.data_directory.clone(),
        engine_available: snapshot.engine_available,
        recoverable_count: snapshot.recoverable_count,
        cache_used_bytes: snapshot.cache_used_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::SnapshotProjector;
    use openunpack_protocol::{AppSnapshot, JobView, PasswordView, Settings, SourceGroup};
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex, mpsc},
        thread,
        time::Duration,
    };

    fn snapshot(revision: u64) -> AppSnapshot {
        AppSnapshot {
            jobs: vec![],
            passwords: vec![],
            settings: Settings::default(),
            running: false,
            paused: false,
            vault_error: None,
            data_directory: "data".into(),
            engine_available: true,
            recoverable_count: 0,
            cache_used_bytes: 0,
            session_id: "session-one".into(),
            revision,
        }
    }

    fn job(id: &str) -> JobView {
        JobView {
            id: id.into(),
            source: SourceGroup {
                path: format!("{id}.zip"),
                display_name: format!("{id}.zip"),
                volumes: vec![format!("{id}.zip")],
                size: 1,
                missing_volume: false,
            },
            options: Settings::default(),
            status: "queued".into(),
            stage: "queued".into(),
            progress: 0.,
            message: String::new(),
            attempts: 0,
            output_path: None,
            verification_scope: String::new(),
            created_at: "2026-10-20T00:00:00Z".into(),
            finished_at: None,
            workspace: None,
            nodes: vec![],
            stage_times: BTreeMap::new(),
            transfer_progress: None,
        }
    }

    fn password(id: &str) -> PasswordView {
        PasswordView {
            id: id.into(),
            name: id.into(),
            groups: vec![],
            notes: String::new(),
            enabled: true,
            success_count: 0,
            last_success: None,
            revision: 1,
        }
    }

    #[test]
    fn update_round_trip_preserves_additions_changes_removals_and_state() {
        let projector = SnapshotProjector::default();
        let mut base = snapshot(1);
        base.jobs = vec![job("unchanged"), job("changed"), job("removed")];
        base.passwords = vec![password("changed"), password("removed")];
        let first = projector.update(None, || Ok(base.clone())).unwrap();
        assert_eq!(first.full, Some(base.clone()));
        assert!(first.delta.is_none());

        let mut current = snapshot(2);
        let mut changed = job("changed");
        changed.progress = 0.75;
        let mut changed_password = password("changed");
        changed_password.success_count = 1;
        current.jobs = vec![job("unchanged"), changed, job("added")];
        current.passwords = vec![changed_password, password("added")];
        current.settings.completion_sound = true;
        current.running = true;
        current.paused = true;
        current.vault_error = Some("vault unavailable".into());
        current.data_directory = "new-data".into();
        current.engine_available = false;
        current.recoverable_count = 2;
        current.cache_used_bytes = 4096;
        let update = projector.update(Some(1), || Ok(current.clone())).unwrap();
        assert!(update.full.is_none());
        let delta = update.delta.unwrap();
        assert_eq!(delta.jobs_removed, ["removed"]);
        assert_eq!(delta.passwords_removed, ["removed"]);
        assert_eq!(delta.jobs_changed.len(), 2);
        assert_eq!(delta.passwords_changed.len(), 2);
        assert_eq!(
            delta.apply_to(&base, update.session_id, update.revision),
            current
        );
    }

    #[test]
    fn changed_session_or_unknown_revision_requires_full_snapshot() {
        let projector = SnapshotProjector::default();
        projector.update(None, || Ok(snapshot(4))).unwrap();
        let mut restarted = snapshot(4);
        restarted.session_id = "session-two".into();
        let update = projector.update(Some(4), || Ok(restarted.clone())).unwrap();
        assert_eq!(update.full, Some(restarted.clone()));
        assert!(update.delta.is_none());
        let stale = projector.update(Some(3), || Ok(restarted.clone())).unwrap();
        assert_eq!(stale.full, Some(restarted));
        assert!(stale.delta.is_none());
    }

    #[test]
    fn failed_capture_retains_the_last_delivered_baseline() {
        let projector = SnapshotProjector::default();
        projector.update(None, || Ok(snapshot(1))).unwrap();
        assert_eq!(
            projector.update(Some(1), || Err("capture failed".into())),
            Err("capture failed".into())
        );
        let update = projector.update(Some(1), || Ok(snapshot(2))).unwrap();
        assert!(update.full.is_none());
        assert!(update.delta.is_some());
    }

    #[test]
    fn concurrent_pollers_cannot_capture_and_publish_an_older_baseline_last() {
        let projector = Arc::new(SnapshotProjector::default());
        let visible = Arc::new(Mutex::new(snapshot(2)));
        projector.update(None, || Ok(snapshot(1))).unwrap();
        let (sampled_tx, sampled_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        // Hold the first poll after it sampled revision 2. A second poll now
        // races with a newer visible revision, reproducing the old late-write
        // window without relying on the relative speed of snapshot generation.
        let first = {
            let projector = Arc::clone(&projector);
            let visible = Arc::clone(&visible);
            thread::spawn(move || {
                projector.update(Some(1), || {
                    let captured = visible.lock().unwrap().clone();
                    sampled_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(captured)
                })
            })
        };
        sampled_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        *visible.lock().unwrap() = snapshot(3);
        let (started_tx, started_rx) = mpsc::channel();
        let (second_sampled_tx, second_sampled_rx) = mpsc::channel();
        let second = {
            let projector = Arc::clone(&projector);
            let visible = Arc::clone(&visible);
            thread::spawn(move || {
                started_tx.send(()).unwrap();
                projector.update(Some(2), || {
                    second_sampled_tx.send(()).unwrap();
                    Ok(visible.lock().unwrap().clone())
                })
            })
        };
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let sampled_before_first_published = second_sampled_rx
            .recv_timeout(Duration::from_millis(100))
            .is_ok();
        release_tx.send(()).unwrap();
        let first_update = first.join().unwrap().unwrap();
        let second_update = second.join().unwrap().unwrap();
        assert!(!sampled_before_first_published);
        assert_eq!(first_update.revision, 2);
        assert_eq!(second_update.revision, 3);
        let delta = second_update.delta.unwrap();
        assert_eq!(
            delta.apply_to(&snapshot(2), second_update.session_id, 3),
            snapshot(3)
        );

        // The next client can still continue from the newest response. With
        // capture outside the cache lock, the slow first poll could evict it.
        let next = projector.update(Some(3), || Ok(snapshot(3))).unwrap();
        assert!(next.full.is_none());
        assert!(next.delta.is_some());
    }
}
