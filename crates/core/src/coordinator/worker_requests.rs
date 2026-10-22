use super::*;

impl Coordinator {
    pub(super) async fn checkpoint(&self, cancel: &AtomicBool) -> Result<(), String> {
        loop {
            if cancel.load(Ordering::Acquire) || self.stop_batch.load(Ordering::Acquire) {
                return Err("Cancelled".into());
            }
            if !self.paused.load(Ordering::Acquire) {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
    pub(super) fn stage(&self, job: &mut JobView, stage: &str, text: &str) -> Result<(), String> {
        job.stage = stage.into();
        job.progress = 0.;
        job.message = text.into();
        self.put(job)
    }
    pub(super) fn job_disk_topology(
        &self,
        source: &Path,
        target: &Path,
    ) -> crate::route::DiskTopology {
        #[cfg(test)]
        if let Ok(probe) = self.topology_probe.lock()
            && let Some(probe) = *probe
        {
            return probe(source, target);
        }
        paths::probe_disk_topology(source, target)
    }

    pub(super) async fn request(
        self: &Arc<Self>,
        job: &mut JobView,
        mut request: WorkerRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<WorkerEvent, String> {
        if !matches!(request.operation.as_str(), "copy" | "readback") || request.files.is_empty() {
            return self.request_once(job, request, cancel, 0, None).await;
        }
        request.generation = Some(job.attempts.max(1));
        let expected_count = request.files.len();
        let total = request.files.iter().try_fold(0u64, |sum, file| {
            sum.checked_add(file.size).ok_or("ResourceLimit")
        })?;
        let batches = transfer_batches(request)?;
        let mut completed = 0u64;
        let mut combined: Option<WorkerEvent> = None;
        for batch in batches {
            self.checkpoint(&cancel).await?;
            let mut event = self
                .request_once(job, batch, cancel.clone(), completed, Some(total))
                .await?;
            completed = completed
                .checked_add(event.completed_bytes)
                .ok_or("ResourceLimit")?;
            if completed > total {
                return Err("WorkerManifestCorrupted".into());
            }
            if let Some(result) = &mut combined {
                result.files.extend(std::mem::take(&mut event.files));
            } else {
                combined = Some(event);
            }
        }
        let mut result = combined.ok_or("WorkerManifestCorrupted")?;
        if completed != total || result.files.len() != expected_count {
            return Err("WorkerManifestCorrupted".into());
        }
        result.completed_bytes = completed;
        result.total_bytes = total;
        Ok(result)
    }
    pub(super) fn set_resource_wait(
        &self,
        job_id: &str,
        identity: &TransferIdentity,
        waiting: bool,
    ) {
        let Ok(_state) = self.view_state.lock() else {
            return;
        };
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(current) = jobs.iter_mut().find(|current| current.id == job_id)
            && identity.matches_job(current)
            && let Ok(transfers) = self.transfers.lock()
            && transfers
                .get(job_id)
                .is_some_and(|transfer| transfer.identity == *identity)
        {
            const WAIT_MESSAGE: &str =
                "等待可用内存，资源恢复后自动继续；可取消任务或关闭其他应用释放内存。";
            let changed = if waiting && current.message != WAIT_MESSAGE {
                current.message = WAIT_MESSAGE.into();
                true
            } else if !waiting && current.message == WAIT_MESSAGE {
                current.message.clear();
                true
            } else {
                false
            };
            if changed {
                self.changed_locked();
            }
        }
    }

    pub(super) async fn request_once(
        self: &Arc<Self>,
        job: &mut JobView,
        mut request: WorkerRequest,
        cancel: Arc<AtomicBool>,
        progress_base: u64,
        progress_total: Option<u64>,
    ) -> Result<WorkerEvent, String> {
        let id = job.id.clone();
        let generation = job.attempts.max(1);
        let concurrency = self.settings.lock().map_err(|_| lock_error())?.concurrency;
        if job.options.engine_threads == 0 {
            request.engine_threads = effective_engine_threads(0, concurrency);
        }
        request.generation = Some(generation);
        request.operation_id.get_or_insert_with(uid);
        let core = self.clone();
        let started = Instant::now();
        let stage = job.stage.clone();
        let cache_writing = matches!(stage.as_str(), "caching" | "extracting" | "nested");
        if cache_writing {
            self.refresh_usage_for(Some(&id))?;
        }
        let base = self
            .cache_usage_by_job
            .lock()
            .map_err(|_| lock_error())?
            .get(&id)
            .map_or(0, |usage| usage.bytes);
        let owner = self.begin_transfer(job, &request, started)?;
        let identity = owner.identity.clone();
        let progress = Arc::new(move |event: &WorkerEvent| {
            core.observe_transfer(
                &id,
                &identity,
                event,
                (progress_base, progress_total),
                cache_writing.then_some(base),
                Instant::now(),
            );
        });
        self.in_flight_requests
            .lock()
            .map_err(|_| lock_error())?
            .insert(job.id.clone());
        let request_id = request.request_id.clone();
        self.worker.retain_request_receipt(&request_id)?;
        if let Some(previous) = self
            .in_flight_request_ids
            .lock()
            .map_err(|_| lock_error())?
            .insert(job.id.clone(), request_id.clone())
        {
            self.worker.forget_confirmed_request(&previous);
        }
        let wait_core = self.clone();
        let wait_id = job.id.clone();
        let wait_identity = owner.identity.clone();
        let on_wait = Arc::new(move |waiting| {
            wait_core.set_resource_wait(&wait_id, &wait_identity, waiting);
        });
        let outcome = self
            .worker
            .run_with_resources(request, cancel, progress, concurrency, Some(on_wait))
            .await;
        if outcome.as_ref().err().map(String::as_str) != Some("WorkerExitUnconfirmed") {
            self.in_flight_requests
                .lock()
                .map_err(|_| lock_error())?
                .remove(&job.id);
        }
        // Clear before matching any error, post-request accounting, or another
        // await. The owner also clears if this future is dropped or unwinds.
        drop(owner);
        if cache_writing
            && outcome.is_err()
            && outcome
                .as_ref()
                .err()
                .is_none_or(|code| code != "WorkerExitUnconfirmed")
        {
            // Reconcile confirmed failures too; accounting must not mask the
            // primary extraction/control error or scan an unconfirmed writer.
            let _ = self.refresh_usage_for(Some(&job.id));
        }
        let mut result = match outcome {
            Ok(result) => result,
            Err(code) => {
                if code == "WorkerExitUnconfirmed" {
                    job.status = "stopping".into();
                    job.message = message(&code);
                    if let Some(work) = &job.workspace {
                        let record = UnconfirmedExit {
                            processes: self.job_exit_processes(&job.id),
                        };
                        let _ =
                            paths::atomic_json(&Path::new(work).join(UNCONFIRMED_FILE), &record);
                    }
                    let _ = self.put(job);
                    self.stop_batch.store(true, Ordering::Release);
                }
                return Err(code);
            }
        };
        if cache_writing {
            self.refresh_usage_for(Some(&job.id))?;
        }
        *job.stage_times.entry(stage).or_default() += started.elapsed().as_millis() as u64;
        if result.kind == "completed" {
            Ok(result)
        } else {
            Err(if result.error_code.is_empty() {
                "IoError".into()
            } else {
                std::mem::take(&mut result.error_code)
            })
        }
    }
}
