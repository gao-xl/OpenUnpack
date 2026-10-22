use super::*;

impl Coordinator {
    pub(super) fn get_job(&self, id: &str) -> Result<JobView, String> {
        self.jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .find(|j| j.id == id)
            .cloned()
            .ok_or("任务不存在。".into())
    }
    pub(super) fn put(&self, job: &JobView) -> Result<(), String> {
        let _lifecycle = self.job_lifecycle.lock().map_err(|_| lock_error())?;
        if !self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .any(|current| current.id == job.id)
        {
            return Err("任务已删除。".into());
        }
        let mut durable_job = job.clone();
        durable_job.transfer_progress = None;
        self.repository.save_job(&durable_job)?;
        self.update_visible_state(|| {
            let mut jobs = self.jobs.lock().map_err(|_| lock_error())?;
            if let Some(j) = jobs.iter_mut().find(|j| j.id == job.id) {
                *j = durable_job;
            }
            let mut transfers = self.transfers.lock().map_err(|_| lock_error())?;
            if transfers
                .get(&job.id)
                .is_some_and(|transfer| !transfer.identity.matches_job(job))
            {
                transfers.remove(&job.id);
            }
            let mut usage = self.cache_usage_by_job.lock().map_err(|_| lock_error())?;
            let current = usage.entry(job.id.clone()).or_default();
            current.generation = current.generation.wrapping_add(1);
            if current.workspace != job.workspace {
                current.workspace = job.workspace.clone();
                current.bytes = 0;
                self.cache_usage.store(
                    usage
                        .values()
                        .fold(0u64, |sum, entry| sum.saturating_add(entry.bytes)),
                    Ordering::Release,
                );
            }
            Ok(())
        })
    }
    pub(super) fn apply_outbox(&self) {
        if let (Ok(v), Ok(events)) = (self.vault(), self.repository.pending_success()) {
            for event in events {
                if self
                    .update_visible_state(|| v.record_success(&event))
                    .is_ok()
                {
                    let _ = self.repository.ack_success(&event.event_id);
                }
            }
        }
    }
}
