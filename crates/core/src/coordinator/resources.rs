use super::*;

impl Coordinator {
    pub(super) fn decide_route(
        &self,
        s: &Settings,
        source: &SourceGroup,
    ) -> Result<ExtractionRoute, String> {
        let target_dir = output_root(s, source)?;
        let ssd_free = if s.cache_enabled && !s.cache_root.trim().is_empty() {
            paths::free_bytes(Path::new(&s.cache_root)).unwrap_or(0)
        } else {
            0
        };
        let topology = paths::probe_disk_topology(Path::new(&source.path), &target_dir);
        let probe = ArchiveProbeSummary {
            total_size_bytes: source.size,
            estimated_extracted_bytes: source.size.saturating_mul(2),
            // No header inspection has run yet. Neither archive size nor the
            // user's nesting preference establishes its actual contents.
            entry_count: 0,
            has_nested_archives: false,
            is_source_on_hdd: topology.source_is_hdd,
            is_target_on_hdd: topology.target_is_hdd,
            is_same_physical_disk: topology.is_same_physical_disk,
        };
        Ok(RouteDecider::decide(&probe, s, &target_dir, ssd_free))
    }
    pub(super) fn workspace_parent(
        &self,
        _s: &Settings,
        _source: &SourceGroup,
        route: &ExtractionRoute,
    ) -> Result<PathBuf, String> {
        let parent = match route {
            ExtractionRoute::SsdCache {
                cache_workspace, ..
            } => cache_workspace.clone(),
            ExtractionRoute::Direct { target_dir, .. } => target_dir.join(".openunpack-work"),
        };
        paths::DirectoryGuard::create(&parent).map_err(|_| "UnsafePath")?;
        Ok(parent)
    }
    pub(super) fn refresh_usage(&self) -> Result<(), String> {
        self.refresh_usage_for(None)
    }
    pub(super) fn refresh_usage_for(&self, job_id: Option<&str>) -> Result<(), String> {
        self.reconcile_usage_with(job_id, paths::tree_size)
    }
    pub(super) fn reconcile_usage_with(
        &self,
        job_id: Option<&str>,
        mut measure: impl FnMut(&Path) -> Result<u64, String>,
    ) -> Result<(), String> {
        let work: HashMap<_, _> = self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .filter_map(|job| {
                job.workspace
                    .clone()
                    .map(|workspace| (job.id.clone(), workspace))
            })
            .collect();
        let mut busy: HashSet<_> = self
            .transfers
            .lock()
            .map_err(|_| lock_error())?
            .keys()
            .cloned()
            .collect();
        busy.extend(
            self.in_flight_requests
                .lock()
                .map_err(|_| lock_error())?
                .iter()
                .cloned(),
        );
        let generations: HashMap<_, _> = self
            .cache_usage_by_job
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .map(|(id, usage)| (id.clone(), usage.generation))
            .collect();
        let mut measured = Vec::new();
        for (id, workspace) in &work {
            if job_id.is_some_and(|selected| id != selected) || busy.contains(id) {
                continue;
            }
            measured.push((
                id.clone(),
                workspace.clone(),
                measure(Path::new(workspace)),
                generations.get(id).copied().unwrap_or(0),
            ));
        }
        self.update_visible_state(|| {
            let jobs = self.jobs.lock().map_err(|_| lock_error())?;
            let workspaces: HashMap<_, _> = jobs
                .iter()
                .filter_map(|job| {
                    job.workspace
                        .as_ref()
                        .map(|workspace| (job.id.as_str(), workspace))
                })
                .collect();
            let transfers = self.transfers.lock().map_err(|_| lock_error())?;
            let in_flight = self.in_flight_requests.lock().map_err(|_| lock_error())?;
            let mut usage = self.cache_usage_by_job.lock().map_err(|_| lock_error())?;
            usage.retain(|id, entry| {
                workspaces.get(id.as_str()).copied() == entry.workspace.as_ref()
                    && entry.workspace.is_some()
            });
            for (id, workspace, bytes, generation) in measured {
                if transfers.contains_key(&id)
                    || in_flight.contains(&id)
                    || workspaces.get(id.as_str()).copied() != Some(&workspace)
                    || usage
                        .get(&id)
                        .is_some_and(|entry| entry.generation != generation)
                {
                    continue;
                }
                // A writer can start after the idle snapshot and remove a file
                // while it is being scanned. Discard obsolete errors with the
                // obsolete measurement; only a still-current idle scan can fail.
                let bytes = match bytes {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        // Retention/removal and earlier valid scans may already
                        // have updated this ledger. Keep its public aggregate
                        // consistent even when this still-current scan fails.
                        self.cache_usage.store(
                            usage
                                .values()
                                .fold(0u64, |sum, entry| sum.saturating_add(entry.bytes)),
                            Ordering::Release,
                        );
                        return Err(error);
                    }
                };
                usage.insert(
                    id,
                    CacheUsage {
                        workspace: Some(workspace),
                        bytes,
                        generation: generation.wrapping_add(1),
                    },
                );
            }
            self.cache_usage.store(
                usage
                    .values()
                    .fold(0u64, |sum, entry| sum.saturating_add(entry.bytes)),
                Ordering::Release,
            );
            Ok(())
        })
    }
    /// Scheduler reactions remain here; the manager owns quota and reservations.
    #[cfg(test)]
    pub(super) fn claim_budget(
        &self,
        s: &Settings,
        route: &ExtractionRoute,
        parent: &Path,
        estimated_needed: Option<u64>,
    ) -> Result<BudgetClaim, String> {
        self.cache_budget
            .claim(s, route, parent, estimated_needed)
            .map_err(|error| self.handle_budget_error(error))
    }
    pub(super) fn claim_budget_for_job(
        self: &Arc<Self>,
        job_id: &str,
        settings: &Settings,
        route: &ExtractionRoute,
        parent: &Path,
        needed: Option<u64>,
    ) -> Result<BudgetClaim, String> {
        // A completed extraction phase releases its reservation before the
        // owning task finishes writeback and removes its actual cache files.
        // Snapshot live peers before reading usage so that a peer completing
        // during the check cannot turn a temporary wait into a permanent error.
        let live_cache_peer = if matches!(route, ExtractionRoute::SsdCache { .. }) {
            self.has_live_cache_peer(job_id, settings)?
        } else {
            false
        };
        self.cache_budget
            .claim_for_job(settings, route, parent, needed, job_id, live_cache_peer)
            .map_err(|error| self.handle_budget_error(error))
    }

    fn handle_budget_error(&self, error: BudgetError) -> String {
        if error.pause_batch {
            if let Err(error) = self.update_visible_state(|| {
                self.api_paused.store(false, Ordering::Release);
                self.paused.store(true, Ordering::Release);
                Ok(())
            }) {
                return error;
            }
            self.stop_batch.store(true, Ordering::Release);
        }
        error.code
    }

    pub(super) fn has_live_cache_peer(
        &self,
        job_id: &str,
        settings: &Settings,
    ) -> Result<bool, String> {
        let root = fs::canonicalize(settings.cache_root.trim())
            .map_err(|_| "SharedResourceUnavailable")?;
        let active: HashSet<String> = self
            .active_cancel
            .lock()
            .map_err(|_| lock_error())?
            .keys()
            .filter(|id| id.as_str() != job_id)
            .cloned()
            .collect();
        let workspaces: Vec<String> = self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .filter(|job| active.contains(&job.id) && job.options.cache_enabled)
            .filter_map(|job| job.workspace.clone())
            .collect();
        // Filesystem checks run outside runtime mutexes. A workspace that has
        // already disappeared has already released its materialized capacity.
        Ok(workspaces
            .iter()
            .any(|workspace| fs::canonicalize(workspace).is_ok_and(|path| path.starts_with(&root))))
    }

    pub(super) fn release_unknown_budget_owner(&self, job_id: &str) {
        self.cache_budget.release_unknown_owner(job_id);
    }
    pub(super) fn job_worker_unconfirmed(&self, job_id: &str) -> bool {
        if !self
            .in_flight_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(job_id)
        {
            return false;
        }
        let ids = self
            .in_flight_request_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ids.get(job_id)
            .and_then(|id| self.worker.request_exit_receipt(id))
            .is_none_or(|receipt| !receipt.exit_confirmed)
    }
    pub(super) fn job_exit_processes(&self, job_id: &str) -> Vec<(u32, u64)> {
        self.in_flight_request_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(job_id)
            .and_then(|id| self.worker.request_exit_receipt(id))
            .map(|receipt| receipt.processes)
            .unwrap_or_default()
    }
    pub(super) fn recovery_exit_confirmed(&self, job_id: &str) -> bool {
        self.in_flight_request_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(job_id)
            .and_then(|id| self.worker.request_exit_receipt(id))
            .is_some_and(|receipt| receipt.exit_confirmed)
    }
    pub(super) fn clear_confirmed_job_request(&self, job_id: &str) {
        self.in_flight_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(job_id);
        if let Some(id) = self
            .in_flight_request_ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(job_id)
        {
            self.worker.forget_confirmed_request(&id);
        }
    }
    pub(super) fn confirm_work_stopped(&self, job: &JobView) -> Result<(), String> {
        if job.status == "stopping"
            || job
                .workspace
                .as_ref()
                .is_some_and(|w| Path::new(w).join(UNCONFIRMED_FILE).exists())
        {
            return Err(
                "作业进程退出尚未确认，禁止重试或清理；请重启应用后按恢复记录处理。".into(),
            );
        }
        Ok(())
    }
}
