use super::*;

impl Coordinator {
    /// Pause only an API-owned batch that this policy can control, or an idle
    /// queue. The Host separately requires settings.write for this operation.
    pub fn pause_authorized(&self, policy: Arc<dyn JobExecutionPolicy>) -> Result<(), String> {
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        if self.paused.load(Ordering::Acquire) && !self.api_paused.load(Ordering::Acquire) {
            return Err("队列由本机窗口或资源保护暂停，不能通过 API 接管。".into());
        }
        if !self
            .recovering_jobs
            .lock()
            .map_err(|_| lock_error())?
            .is_empty()
            || self.running.load(Ordering::Acquire) && self.stop_batch.load(Ordering::Acquire)
        {
            return Err("队列正在停止或恢复，请等待当前操作完成。".into());
        }
        let scope = self
            .scheduler_scope
            .lock()
            .map_err(|_| lock_error())?
            .clone();
        let mut controlled: HashSet<_> = self
            .active_cancel
            .lock()
            .map_err(|_| lock_error())?
            .keys()
            .cloned()
            .collect();
        let active = controlled.clone();
        if scope.is_none() && (self.running.load(Ordering::Acquire) || !controlled.is_empty()) {
            return Err("本机窗口正在运行队列，API 不能暂停该批次。".into());
        }
        if let Some(scope) = &scope {
            controlled.extend(scope.iter().cloned());
        }
        let jobs = self.jobs.lock().map_err(|_| lock_error())?.clone();
        if !jobs.iter().any(|job| {
            (matches!(job.status.as_str(), "queued" | "waitingResource")
                || job.status == "running" && active.contains(&job.id))
                && scope.as_ref().is_none_or(|ids| ids.contains(&job.id))
        }) {
            return Err("当前没有可暂停的任务。".into());
        }
        for job in jobs.iter().filter(|job| controlled.contains(&job.id)) {
            policy.authorize(job)?;
        }
        self.update_visible_state(|| {
            self.api_paused.store(true, Ordering::Release);
            self.paused.store(true, Ordering::Release);
            Ok(())
        })
    }

    // Caller holds mutations. Clear only a fully idle API pause. Active owners
    // remain paused until an explicitly authorized running-anchor resume.
    pub(super) fn clear_empty_api_pause(&self) -> Result<(), String> {
        if !self.api_paused.load(Ordering::Acquire) {
            return Ok(());
        }
        // An ordinary cancellation/deletion must not wake a different active
        // owner. A privileged selected resume can use its running ID instead.
        if !self
            .active_cancel
            .lock()
            .map_err(|_| lock_error())?
            .is_empty()
        {
            return Ok(());
        }
        let scope = self
            .scheduler_scope
            .lock()
            .map_err(|_| lock_error())?
            .clone();
        if self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .any(|job| {
                matches!(job.status.as_str(), "queued" | "waitingResource")
                    && scope.as_ref().is_none_or(|ids| ids.contains(&job.id))
            })
        {
            return Ok(());
        }
        self.update_visible_state(|| {
            self.api_paused.store(false, Ordering::Release);
            self.paused.store(false, Ordering::Release);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
