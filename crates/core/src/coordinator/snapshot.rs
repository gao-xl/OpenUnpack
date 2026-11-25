use super::*;

impl Coordinator {
    pub fn snapshot(&self) -> Result<AppSnapshot, String> {
        self.snapshot_projection(false)
    }
    /// UI refreshes include every live task and only the latest ended tasks.
    /// The full snapshot remains available to backend and compatibility callers.
    pub fn queue_snapshot(&self) -> Result<AppSnapshot, String> {
        self.snapshot_projection(true)
    }
    pub fn snapshot_update(&self, since: Option<u64>) -> Result<SnapshotUpdate, String> {
        // The projector takes its cache lock before this closure captures the
        // visible state. Keep this call outside every view_state critical section.
        let (session_id, revision) = self.revision_info();
        self.snapshot_projector
            .update_versioned(since, &session_id, revision, || self.queue_snapshot())
    }
    pub(super) fn snapshot_projection(&self, bounded_history: bool) -> Result<AppSnapshot, String> {
        let _state = self.view_state.lock().map_err(|_| lock_error())?;
        let stored = self.jobs.lock().map_err(|_| lock_error())?;
        let recoverable_count = stored.iter().filter(|j| recovering(&j.status)).count();
        let mut recent: Vec<_> = if bounded_history {
            stored
                .iter()
                .filter(|job| {
                    terminal(&job.status) && !recovering(&job.status) && job.workspace.is_none()
                })
                .collect()
        } else {
            Vec::new()
        };
        // Only the latest 20 ended jobs are shown; select them without sorting
        // the entire stored history on every snapshot.
        top_n_newest(&mut recent, 20);
        let recent_ids: HashSet<_> = recent.iter().map(|job| job.id.as_str()).collect();
        let mut jobs: Vec<_> = stored
            .iter()
            .filter(|job| {
                !bounded_history
                    || !terminal(&job.status)
                    || recovering(&job.status)
                    || job.workspace.is_some()
                    || recent_ids.contains(job.id.as_str())
            })
            .cloned()
            .collect();
        drop(recent);
        drop(stored);
        let transfers = self.transfers.lock().map_err(|_| lock_error())?;
        for job in &mut jobs {
            job.transfer_progress = transfers
                .get(&job.id)
                .filter(|transfer| transfer.identity.matches_job(job))
                .and_then(|transfer| transfer.view.clone());
        }
        drop(transfers);
        let settings = self.settings.lock().map_err(|_| lock_error())?.clone();
        let (passwords, vault_error) = self.snapshot_projector.cached_passwords(self.vault());
        let cache_used_bytes = self.cache_usage.load(Ordering::Acquire);
        Ok(AppSnapshot {
            jobs,
            passwords,
            settings,
            running: self.running.load(Ordering::Acquire),
            paused: self.paused.load(Ordering::Acquire),
            vault_error,
            data_directory: paths::path_string(&self.data),
            engine_available: self.engine_available(),
            recoverable_count,
            cache_used_bytes,
            session_id: self.session_id.clone(),
            revision: self.revision.load(Ordering::Acquire),
        })
    }
    pub(super) fn engine_available(&self) -> bool {
        self.snapshot_projector
            .engine_available(&self.worker.executable, &self.worker.engine)
    }
}
