use super::*;

impl Coordinator {
    pub(super) fn begin_transfer(
        self: &Arc<Self>,
        job: &JobView,
        request: &WorkerRequest,
        started: Instant,
    ) -> Result<TransferOwner, String> {
        let identity = TransferIdentity {
            protocol_version: request.protocol_version,
            request_id: request.request_id.clone(),
            operation_id: request.operation_id.clone().ok_or("WorkerProtocolError")?,
            generation: request.generation.ok_or("WorkerProtocolError")?,
            stage: job.stage.clone(),
            node_id: job
                .nodes
                .iter()
                .find(|node| node.status == "running")
                .map(|node| node.id.clone()),
        };
        self.update_visible_state(|| {
            if !self
                .jobs
                .lock()
                .map_err(|_| lock_error())?
                .iter()
                .any(|current| current.id == job.id)
            {
                return Err("任务已删除。".into());
            }
            self.transfers.lock().map_err(|_| lock_error())?.insert(
                job.id.clone(),
                ActiveTransfer {
                    identity: identity.clone(),
                    started,
                    estimator: TransferEstimator::default(),
                    view: None,
                    measured: matches!(request.operation.as_str(), "extract" | "copy" | "readback"),
                },
            );
            let mut usage = self.cache_usage_by_job.lock().map_err(|_| lock_error())?;
            let current = usage.entry(job.id.clone()).or_default();
            current.workspace = job.workspace.clone();
            current.generation = current.generation.wrapping_add(1);
            Ok(())
        })?;
        Ok(TransferOwner {
            core: self.clone(),
            job_id: job.id.clone(),
            identity,
        })
    }
    pub(super) fn finish_transfer(&self, job_id: &str, identity: &TransferIdentity) {
        let Ok(_state) = self.view_state.lock() else {
            return;
        };
        if let Ok(mut transfers) = self.transfers.lock()
            && transfers
                .get(job_id)
                .is_some_and(|transfer| transfer.identity == *identity)
        {
            transfers.remove(job_id);
            if let Ok(mut usage) = self.cache_usage_by_job.lock()
                && let Some(current) = usage.get_mut(job_id)
            {
                current.generation = current.generation.wrapping_add(1);
            }
            self.changed_locked();
        }
    }
    pub(super) fn observe_transfer(
        &self,
        job_id: &str,
        identity: &TransferIdentity,
        event: &WorkerEvent,
        progress: (u64, Option<u64>),
        cache_base: Option<u64>,
        observed_at: Instant,
    ) {
        let (progress_base, progress_total) = progress;
        if !identity.matches_event(event) {
            return;
        }
        let Ok(_state) = self.view_state.lock() else {
            return;
        };
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(job) = jobs.iter_mut().find(|job| job.id == job_id)
            && identity.matches_job(job)
            && let Ok(mut transfers) = self.transfers.lock()
            && let Some(transfer) = transfers.get_mut(job_id)
            && transfer.identity == *identity
        {
            if let Some(base) = cache_base {
                let Ok(mut usage) = self.cache_usage_by_job.lock() else {
                    return;
                };
                let current = usage.entry(job_id.to_string()).or_default();
                current.workspace = job.workspace.clone();
                current.bytes = base.saturating_add(event.completed_bytes);
                current.generation = current.generation.wrapping_add(1);
                self.cache_usage.store(
                    usage
                        .values()
                        .fold(0u64, |sum, entry| sum.saturating_add(entry.bytes)),
                    Ordering::Release,
                );
            }
            let total = progress_total.unwrap_or(event.total_bytes);
            let completed = progress_base.saturating_add(event.completed_bytes);
            let proportion = if total > 0 {
                (completed as f64 / total as f64).clamp(0., 1.)
            } else {
                0.
            };
            job.progress = proportion;
            if let Some(active_node) = job.nodes.iter_mut().find(|node| node.status == "running") {
                active_node.progress = proportion;
            }
            if transfer.measured {
                let estimate = transfer.estimator.observe(
                    observed_at.saturating_duration_since(transfer.started),
                    completed,
                    total,
                );
                transfer.view = Some(TransferProgress {
                    completed_bytes: completed,
                    total_bytes: total,
                    bytes_per_second: estimate.bytes_per_second,
                    remaining_seconds: estimate.remaining_seconds,
                    updated_at: now(),
                });
            }
            let progress_update = openunpack_protocol::JobProgressUpdate {
                job_id: job_id.to_string(),
                stage: job.stage.clone(),
                progress: proportion,
                completed_bytes: completed,
                total_bytes: total,
                bytes_per_second: transfer
                    .view
                    .as_ref()
                    .and_then(|v| v.bytes_per_second)
                    .unwrap_or(0.0),
                remaining_seconds: transfer.view.as_ref().and_then(|v| v.remaining_seconds),
                current_file: job
                    .nodes
                    .iter()
                    .find(|n| n.status == "running")
                    .map(|n| n.name.clone()),
            };
            let _ = self.progress_broadcaster.send(progress_update);
            self.changed_locked();
        }
    }
}
