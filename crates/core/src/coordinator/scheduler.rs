use super::*;

impl Coordinator {
    pub fn start(self: &Arc<Self>) -> Result<(), String> {
        self.start_inner(None, None, false)
    }
    pub fn start_authorized(
        self: &Arc<Self>,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<(), String> {
        self.start_inner(Some(policy), None, false)
    }
    pub fn start_selected_authorized(
        self: &Arc<Self>,
        job_ids: &[String],
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<(), String> {
        self.start_inner(Some(policy), Some(job_ids), false)
    }
    /// Explicit API control only: the Host must require settings.write before
    /// choosing this entry point. Ordinary admission and retry cannot resume.
    pub fn start_selected_resuming_authorized(
        self: &Arc<Self>,
        job_ids: &[String],
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<(), String> {
        self.start_inner(Some(policy), Some(job_ids), true)
    }
    // Caller holds mutations. An API operation must never join or alter an
    // active scheduler owned by the local window, or resume its pause.
    pub(super) fn check_api_scheduler(&self) -> Result<(), String> {
        self.check_api_scheduler_allowing_pause(false)
    }
    fn check_api_scheduler_allowing_pause(&self, resume_api_pause: bool) -> Result<(), String> {
        if self.paused.load(Ordering::Acquire)
            && !(resume_api_pause && self.api_paused.load(Ordering::Acquire))
        {
            if self.api_paused.load(Ordering::Acquire) {
                return Err(
                    "API 批次已暂停，请使用具备 settings.write 权限的开始所选操作恢复。".into(),
                );
            }
            return Err("队列由本机窗口暂停，请在本机窗口恢复后重试。".into());
        }
        if self.running.load(Ordering::Acquire) {
            if self.stop_batch.load(Ordering::Acquire)
                || !self
                    .recovering_jobs
                    .lock()
                    .map_err(|_| lock_error())?
                    .is_empty()
            {
                return Err("队列正在停止或恢复，请等待当前操作完成后重试。".into());
            }
            if self
                .scheduler_scope
                .lock()
                .map_err(|_| lock_error())?
                .is_none()
            {
                return Err("本机窗口正在运行队列，请等待该批次完成后通过 API 提交任务。".into());
            }
        }
        Ok(())
    }
    pub(super) fn start_inner(
        self: &Arc<Self>,
        policy: Option<Arc<dyn JobExecutionPolicy>>,
        selected_ids: Option<&[String]>,
        resume_api_pause: bool,
    ) -> Result<(), String> {
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        if !self
            .recovering_jobs
            .lock()
            .map_err(|_| lock_error())?
            .is_empty()
        {
            return Err("正在核对恢复结果，请等待恢复操作完成后开始队列。".into());
        }
        if !self.worker.executable.is_file() || !self.worker.engine.is_file() {
            return Err("解压引擎或作业进程缺失，请运行构建准备脚本。".into());
        }
        if self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .any(|job| job.status == "stopping")
        {
            return Err("还有作业进程退出尚未确认，已停止新调度；请重启后核对恢复记录。".into());
        }
        if let Some(policy) = policy {
            let resuming = resume_api_pause
                && selected_ids.is_some()
                && self.paused.load(Ordering::Acquire)
                && self.api_paused.load(Ordering::Acquire);
            self.check_api_scheduler_allowing_pause(resuming)?;
            let active: HashSet<String> = if resuming {
                self.active_cancel
                    .lock()
                    .map_err(|_| lock_error())?
                    .keys()
                    .cloned()
                    .collect()
            } else {
                HashSet::new()
            };
            let resume_scope = if resuming {
                self.scheduler_scope
                    .lock()
                    .map_err(|_| lock_error())?
                    .clone()
            } else {
                None
            };
            let jobs = self.jobs.lock().map_err(|_| lock_error())?;
            // Clearing the global pause also wakes existing owners. Recheck
            // every active owner even when it is not a new scheduling target.
            for id in &active {
                let job = jobs
                    .iter()
                    .find(|job| &job.id == id)
                    .ok_or("正在执行的任务记录不可用。")?;
                policy.authorize(job)?;
            }
            let eligible: Vec<_> = if let Some(selected) = selected_ids {
                let unique: HashSet<_> = selected.iter().collect();
                if selected.is_empty()
                    || selected.len() > 1000
                    || unique.len() != selected.len()
                    || selected.iter().any(|id| id.trim().is_empty())
                {
                    return Err("jobIds 必须包含 1 至 1000 个不重复的非空任务 ID。".into());
                }
                selected
                    .iter()
                    .map(|id| {
                        let job = jobs
                            .iter()
                            .find(|job| &job.id == id)
                            .ok_or("选定任务不存在。")?;
                        let running_anchor = resuming
                            && job.status == "running"
                            && active.contains(id)
                            && resume_scope
                                .as_ref()
                                .is_some_and(|scope| scope.contains(id));
                        if !matches!(job.status.as_str(), "queued" | "waitingResource")
                            && !running_anchor
                        {
                            if resuming {
                                return Err(
                                    "恢复所选仅接受排队、等待资源或本 API 批次已登记的运行任务。"
                                        .into(),
                                );
                            }
                            return Err("选定任务必须全部处于排队或等待资源状态。".into());
                        }
                        Ok(job)
                    })
                    .collect::<Result<_, String>>()?
            } else {
                jobs.iter()
                    .filter(|job| matches!(job.status.as_str(), "queued" | "waitingResource"))
                    .collect()
            };
            // Authorize all selected existing tasks before changing any policy,
            // pause flag or running state. Later GUI additions are not selected.
            for job in &eligible {
                policy.authorize(job)?;
            }
            let mut ids: HashSet<_> = eligible.into_iter().map(|job| job.id.clone()).collect();
            if resuming {
                ids.extend(active);
            }
            drop(jobs);
            let mut policies = self.execution_policies.lock().map_err(|_| lock_error())?;
            for id in &ids {
                policies.insert(id.clone(), policy.clone());
            }
            self.authorization_blocked
                .lock()
                .map_err(|_| lock_error())?
                .retain(|id| !ids.contains(id));
            let mut scope = self.scheduler_scope.lock().map_err(|_| lock_error())?;
            if self.running.load(Ordering::Acquire) && !resuming {
                // An existing API run may admit additional validated task IDs.
                if let Some(scope) = scope.as_mut() {
                    scope.extend(ids);
                }
            } else {
                *scope = Some(ids);
            }
        } else {
            // A deliberate local desktop start is a new trusted operation.
            self.execution_policies
                .lock()
                .map_err(|_| lock_error())?
                .clear();
            self.authorization_blocked
                .lock()
                .map_err(|_| lock_error())?
                .clear();
            *self.scheduler_scope.lock().map_err(|_| lock_error())? = None;
        }
        let already_running = self.update_visible_state(|| {
            self.api_paused.store(false, Ordering::Release);
            self.paused.store(false, Ordering::Release);
            Ok(self
                .running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err())
        })?;
        if let Err(code) = self.requeue_resource_waiters() {
            if !already_running {
                self.running.store(false, Ordering::Release);
            }
            return Err(code);
        }
        if already_running {
            return Ok(());
        }
        self.stop_batch.store(false, Ordering::Release);
        // Admission can arrive from the folder watcher thread, which has no
        // Tokio context: tokio::spawn would panic there and leave `running`
        // latched to true, wedging every later job in the queue.
        let handle = self
            .runtime
            .clone()
            .or_else(|| tokio::runtime::Handle::try_current().ok());
        let Some(handle) = handle else {
            self.running.store(false, Ordering::Release);
            return Err("队列调度需要异步运行时，请在应用宿主内启动。".into());
        };
        let this = self.clone();
        handle.spawn(async move {
            this.drive().await;
        });
        Ok(())
    }
    pub(super) async fn drive(self: Arc<Self>) {
        let _sleep_guard = paths::SleepBlockerGuard::acquire();
        let mut batch_guard = BatchRunGuard {
            core: self.clone(),
            completed: false,
        };
        let mut join_set = tokio::task::JoinSet::new();
        let mut owners = HashMap::new();
        loop {
            let scheduling_revision = self.revision.load(Ordering::Acquire);
            if self.stop_batch.load(Ordering::Acquire) {
                while let Some(result) = join_set.join_next_with_id().await {
                    self.finish_join(result, &mut owners);
                }
                break;
            }
            let concurrency = self
                .settings
                .lock()
                .map(|s| (s.concurrency as usize).clamp(1, MAX_CONCURRENT_JOBS as usize))
                .unwrap_or(1);
            let mut scheduled_any = false;
            while !self.stop_batch.load(Ordering::Acquire)
                && !self.paused.load(Ordering::Acquire)
                && join_set.len() < concurrency
            {
                let (mut job, cancel, partial, selected) = match self.claim_next_job() {
                    Ok(Some(claim)) => claim,
                    Ok(None) => break,
                    Err(_) => {
                        self.stop_batch.store(true, Ordering::Release);
                        break;
                    }
                };
                scheduled_any = true;
                let this = self.clone();
                let job_id = job.id.clone();
                let handle = join_set.spawn(async move {
                    let mut guard = JobRunGuard {
                        core: this.clone(),
                        job_id: job.id.clone(),
                        finished_normally: false,
                    };
                    let result = this
                        .run_job(&mut job, cancel.clone(), partial, selected.as_ref())
                        .await;
                    if let Err(code) = result {
                        if code == "ApiAuthorizationDenied" {
                            // A revoked grant or a newly discovered unapproved
                            // volume leaves the task queued, without touching
                            // its source, publication or retained workspace.
                            if let Ok(mut blocked) = this.authorization_blocked.lock() {
                                blocked.insert(job.id.clone());
                            } else {
                                this.stop_batch.store(true, Ordering::Release);
                            }
                            job.status = "queued".into();
                            job.stage = "queued".into();
                            job.message =
                                "应用授权已改变；任务已保留，请重新批准或在本机窗口开始。".into();
                            job.finished_at = None;
                            if this.put(&job).is_err() {
                                this.stop_batch.store(true, Ordering::Release);
                            }
                            guard.finished_normally = true;
                            return;
                        }
                        // Check if publication is already committed or unresolved.
                        let (unresolved, committed) = this
                            .repository
                            .publications()
                            .map(|p| {
                                let mut unres = false;
                                let mut comm = false;
                                for pub_rec in p.into_iter().filter(|x| x.job_id == job.id) {
                                    if !matches!(pub_rec.state.as_str(), "committed" | "abandoned")
                                    {
                                        unres = true;
                                    }
                                    if pub_rec.state == "committed" {
                                        comm = true;
                                    }
                                }
                                (unres, comm)
                            })
                            .unwrap_or((true, false));

                        if committed {
                            // Once formally delivered (committed), post-commit storage/cleanup errors or late
                            // cancellations must never downgrade the delivered task to failed or cancelled.
                            if job.workspace.is_some() {
                                job.status = "cleanupPending".into();
                                job.message =
                                    "文件已解压完成，缓存清理或后续保存失败待重试。".into();
                            } else {
                                job.status = if job.verification_scope.contains("salvaged") {
                                    "salvaged"
                                } else if job.nodes.iter().any(|n| {
                                    matches!(n.status.as_str(), "failed" | "needsPassword")
                                }) {
                                    "partial"
                                } else {
                                    "completed"
                                }
                                .into();
                                if job.message.is_empty() {
                                    job.message = "解压完成并已交付输出目录。".into();
                                }
                            }
                            if job.finished_at.is_none() {
                                job.finished_at = Some(now());
                            }
                            if this.put(&job).is_err() {
                                this.stop_batch.store(true, Ordering::Release);
                                return;
                            }
                            if code == "WorkerExitUnconfirmed" || code == "StorageError" {
                                this.stop_batch.store(true, Ordering::Release);
                            }
                        } else {
                            // An unresolved publication always takes precedence over a generic failure.
                            job.status = if code == "WorkerExitUnconfirmed" {
                                "stopping"
                            } else if unresolved {
                                "publishPending"
                            } else if code == "Cancelled" || cancel.load(Ordering::Acquire) {
                                "cancelled"
                            } else if code == "NeedsPassword" {
                                "needsPassword"
                            } else if matches!(
                                code.as_str(),
                                "CacheQuotaContention" | "SharedResourceUnavailable"
                            ) {
                                "waitingResource"
                            } else {
                                "failed"
                            }
                            .into();
                            job.message = message(&code);
                            job.finished_at = Some(now());
                            if this.put(&job).is_err() {
                                this.stop_batch.store(true, Ordering::Release);
                                return;
                            }
                            if code == "WorkerExitUnconfirmed" || code == "StorageError" {
                                this.stop_batch.store(true, Ordering::Release);
                            }
                        }
                    }
                    guard.finished_normally = job.status != "stopping"
                        && this
                            .in_flight_requests
                            .lock()
                            .map(|requests| !requests.contains(&job.id))
                            .unwrap_or(false);
                    this.changed();
                    if terminal(&job.status) {
                        let settings_hook = this
                            .settings
                            .lock()
                            .ok()
                            .and_then(|s| s.on_complete.clone());
                        trigger_on_complete_hook(&job, settings_hook.as_deref());
                    }
                });
                owners.insert(handle.id(), job_id);
            }
            if join_set.is_empty() {
                // Only an actual owner completion wakes waiters (finish_join).
                // Hand off running under the same gate as start/claim. A start
                // that expanded scope since our last claim must be observed by
                // this driver, or must see running=false and create a new one.
                if self.finish_idle_batch(scheduling_revision, &mut batch_guard) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
            if self.paused.load(Ordering::Acquire) {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {}
                    res = join_set.join_next_with_id() => {
                        if let Some(result) = res {
                            self.finish_join(result, &mut owners);
                        }
                    }
                }
            } else if join_set.len() >= concurrency {
                if let Some(result) = join_set.join_next_with_id().await {
                    self.finish_join(result, &mut owners);
                }
            } else if !scheduled_any {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
                    res = join_set.join_next_with_id() => {
                        if let Some(result) = res {
                            self.finish_join(result, &mut owners);
                        }
                    }
                }
            }
        }
        while let Some(result) = join_set.join_next_with_id().await {
            self.finish_join(result, &mut owners);
        }
        // Completion notifications are owned by the desktop Host (per successfully
        // finished task); Core must not play a second batch-level sound, especially
        // not for batches where every task failed.
        if !batch_guard.completed {
            let _gate = self
                .mutations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _ = self.update_visible_state(|| {
                self.running.store(false, Ordering::Release);
                Ok(())
            });
            batch_guard.completed = true;
        }
    }
    pub(super) fn finish_idle_batch(
        &self,
        observed_revision: u64,
        guard: &mut BatchRunGuard,
    ) -> bool {
        let Ok(_gate) = self.mutations.lock() else {
            self.stop_batch.store(true, Ordering::Release);
            return true;
        };
        if self.clear_empty_api_pause().is_err() {
            self.stop_batch.store(true, Ordering::Release);
            return true;
        }
        if !self.stop_batch.load(Ordering::Acquire) {
            if self.revision.load(Ordering::Acquire) != observed_revision {
                return false;
            }
            let scope = self
                .scheduler_scope
                .lock()
                .ok()
                .and_then(|scope| scope.clone());
            let blocked = self.authorization_blocked.lock().ok();
            let has_queued = self.jobs.lock().is_ok_and(|jobs| {
                jobs.iter().any(|job| {
                    job.status == "queued"
                        && scope.as_ref().is_none_or(|ids| ids.contains(&job.id))
                        && blocked.as_ref().is_none_or(|ids| !ids.contains(&job.id))
                })
            });
            if self.paused.load(Ordering::Acquire) && has_queued {
                return false;
            }
        }
        if self
            .update_visible_state(|| {
                self.running.store(false, Ordering::Release);
                Ok(())
            })
            .is_err()
        {
            self.stop_batch.store(true, Ordering::Release);
            return true;
        }
        // A completed guard must never clear running after a new driver starts.
        guard.completed = true;
        true
    }
    pub(super) fn requeue_resource_waiters(&self) -> Result<bool, String> {
        let _lifecycle = self.job_lifecycle.lock().map_err(|_| lock_error())?;
        let scope = self
            .scheduler_scope
            .lock()
            .map_err(|_| lock_error())?
            .clone();
        self.update_visible_state(|| {
            let mut jobs = self.jobs.lock().map_err(|_| lock_error())?;
            let mut changed = false;
            for current in jobs.iter_mut().filter(|job| {
                job.status == "waitingResource"
                    && scope.as_ref().is_none_or(|ids| ids.contains(&job.id))
            }) {
                let mut queued = current.clone();
                queued.status = "queued".into();
                queued.stage = "queued".into();
                queued.finished_at = None;
                queued.message = "资源已释放，重新等待调度。".into();
                self.repository
                    .save_job(&queued)
                    .map_err(|_| "StorageError")?;
                *current = queued;
                changed = true;
            }
            Ok(changed)
        })
    }

    pub(super) fn finish_join(
        &self,
        result: Result<(tokio::task::Id, ()), tokio::task::JoinError>,
        owners: &mut HashMap<tokio::task::Id, String>,
    ) {
        let task_id = match &result {
            Ok((id, _)) => *id,
            Err(error) => error.id(),
        };
        let mut released_work = false;
        if let Some(job_id) = owners.remove(&task_id) {
            if result.is_err() {
                // Do not log panic payloads: their text may include archive secrets.
                self.mark_job_abnormal(&job_id);
            }
            released_work = self
                .get_job(&job_id)
                .is_ok_and(|job| job.status != "waitingResource");
        } else if result.is_err() {
            self.stop_batch.store(true, Ordering::Release);
            eprintln!("job_join_owner_missing");
        }
        // A failed claim releases no contested resource. Wait for another owner
        // to finish instead of immediately recreating the same Worker attempt.
        if released_work
            && !self.stop_batch.load(Ordering::Acquire)
            && self.requeue_resource_waiters().is_err()
        {
            self.stop_batch.store(true, Ordering::Release);
            eprintln!("resource_waiter_persist_failed");
        }
        self.changed();
    }

    pub(super) fn mark_job_abnormal(&self, job_id: &str) {
        let Ok(mut job) = self.get_job(job_id) else {
            self.stop_batch.store(true, Ordering::Release);
            return;
        };
        let unconfirmed = self.job_worker_unconfirmed(job_id);
        if !(matches!(job.status.as_str(), "running" | "stopping")
            || unconfirmed && job.status == "cleanupPending")
        {
            return;
        }
        if unconfirmed {
            self.stop_batch.store(true, Ordering::Release);
            if let Ok(active) = self.active_cancel.lock()
                && let Some(cancel) = active.get(job_id)
            {
                cancel.store(true, Ordering::Release);
            }
            job.status = "stopping".into();
            job.message =
                "任务执行异常，作业进程退出尚未确认；原包和工作目录已保留，请重启后核对恢复记录。"
                    .into();
            if let Some(work) = &job.workspace {
                let record = UnconfirmedExit {
                    processes: self.job_exit_processes(job_id),
                };
                if paths::atomic_json(&Path::new(work).join(UNCONFIRMED_FILE), &record).is_err() {
                    eprintln!("job_exit_marker_persist_failed job={job_id}");
                }
            }
        } else {
            let committed = self.repository.publications().is_ok_and(|records| {
                records
                    .iter()
                    .any(|record| record.job_id == job_id && record.state == "committed")
            });
            job.status = if committed {
                "cleanupPending"
            } else {
                "interrupted"
            }
            .into();
            job.message = if committed {
                "输出已交付，后续处理异常；原包和工作目录已保留。"
            } else {
                "任务执行出现意外异常中断；原包与工作目录已保留。"
            }
            .into();
        }
        job.finished_at = Some(now());
        if self.put(&job).is_err() {
            self.stop_batch.store(true, Ordering::Release);
            // Keep ownership on durable storage failure; still expose the failure.
            let _ = self.update_visible_state(|| {
                if let Some(current) = self
                    .jobs
                    .lock()
                    .map_err(|_| lock_error())?
                    .iter_mut()
                    .find(|current| current.id == job_id)
                {
                    current.status = "stopping".into();
                    current.message =
                        "任务异常且恢复记录保存失败，已停止调度；请保留原包和工作目录并重启核对。"
                            .into();
                }
                Ok(())
            });
            eprintln!("job_abort_persist_failed job={job_id}");
        } else if !unconfirmed && let Ok(mut active) = self.active_cancel.lock() {
            active.remove(job_id);
        }
        if !unconfirmed {
            self.clear_confirmed_job_request(job_id);
            self.release_unknown_budget_owner(job_id);
        }
        self.changed();
    }

    pub(super) fn claim_next_job(&self) -> Result<Option<JobClaim>, String> {
        // Commands and claim share this gate through persistence, publication and
        // owner registration, so a removed queue index cannot resurrect a task.
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        if self.stop_batch.load(Ordering::Acquire) || self.paused.load(Ordering::Acquire) {
            return Ok(None);
        }
        let small_first = self.settings.lock().map_err(|_| lock_error())?.small_first;
        let deleting = self.deleting_jobs.lock().map_err(|_| lock_error())?;
        let all_jobs = self.jobs.lock().map_err(|_| lock_error())?.clone();

        // 收集当前活跃 running 任务所占用的物理设备负载（机械硬盘限制单任务避免寻道，NVMe 开启并发）
        let concurrency = self.settings.lock().map_err(|_| lock_error())?.concurrency as usize;
        let mut scheduler = crate::route::MultiDeviceScheduler::new(concurrency);

        for active in all_jobs.iter().filter(|j| j.status == "running") {
            let src_path = Path::new(&active.source.path);
            let final_dst_path = if !active.options.output_root.is_empty() {
                Path::new(&active.options.output_root)
            } else {
                src_path.parent().unwrap_or(src_path)
            };
            let (cur_src, cur_dst) = match active.stage.as_str() {
                "copying" | "verifying" | "publishing" => {
                    if let Some(ref ws) = active.workspace {
                        (Path::new(ws), final_dst_path)
                    } else {
                        (src_path, final_dst_path)
                    }
                }
                _ => {
                    if let Some(ref ws) = active.workspace {
                        (src_path, Path::new(ws))
                    } else {
                        (src_path, final_dst_path)
                    }
                }
            };
            let topo = self.job_disk_topology(cur_src, cur_dst);
            let src_ep =
                crate::route::DeviceEndpoint::from_path(cur_src, topo.source_device_number);
            let dst_ep =
                crate::route::DeviceEndpoint::from_path(cur_dst, topo.target_device_number);
            let mut occupied = HashSet::from([src_ep.clone(), dst_ep.clone()]);
            scheduler.register_active_job(src_ep, topo.source_is_hdd, dst_ep, topo.target_is_hdd);

            let final_topo = self.job_disk_topology(cur_src, final_dst_path);
            if final_topo.target_is_hdd {
                let final_ep = crate::route::DeviceEndpoint::from_path(
                    final_dst_path,
                    final_topo.target_device_number,
                );
                if occupied.insert(final_ep.clone()) {
                    scheduler.register_active_job(final_ep.clone(), true, final_ep, true);
                } else {
                    scheduler.hdd_endpoints.insert(final_ep);
                }
            }
            // A newly claimed job has not chosen/materialized its workspace yet.
            // Reserve its configured cache endpoint until actual I/O paths are
            // available, so two checking jobs cannot both enter one slow cache.
            if active.workspace.is_none()
                && active.options.cache_enabled
                && !active.options.cache_root.trim().is_empty()
            {
                let cache_path = Path::new(active.options.cache_root.trim());
                let cache_topology = self.job_disk_topology(src_path, cache_path);
                let cache_ep = crate::route::DeviceEndpoint::from_path(
                    cache_path,
                    cache_topology.target_device_number,
                );
                if occupied.insert(cache_ep.clone()) {
                    scheduler.register_active_job(
                        cache_ep.clone(),
                        cache_topology.target_is_hdd,
                        cache_ep,
                        cache_topology.target_is_hdd,
                    );
                } else if cache_topology.target_is_hdd {
                    scheduler.hdd_endpoints.insert(cache_ep);
                }
            }
        }

        let scope = self
            .scheduler_scope
            .lock()
            .map_err(|_| lock_error())?
            .clone();
        let blocked = self
            .authorization_blocked
            .lock()
            .map_err(|_| lock_error())?
            .clone();
        let mut queued: Vec<_> = all_jobs
            .into_iter()
            .filter(|job| {
                job.status == "queued"
                    && !deleting.contains(&job.id)
                    && !blocked.contains(&job.id)
                    && scope.as_ref().is_none_or(|ids| ids.contains(&job.id))
            })
            .collect();
        drop(deleting);
        if small_first {
            queued.sort_by_key(|job| job.source.size);
        }

        // 多物理盘独立并发仲裁：针对机械硬盘限制单任务顺序读取避免磁头剧烈寻道，针对高速 NVMe 开启并发流水线
        let selected_idx = queued.iter().position(|job| {
            let src_path = Path::new(&job.source.path);
            let dst_path = if !job.options.output_root.is_empty() {
                Path::new(&job.options.output_root)
            } else {
                src_path.parent().unwrap_or(src_path)
            };
            let topo = self.job_disk_topology(src_path, dst_path);
            let src_ep =
                crate::route::DeviceEndpoint::from_path(src_path, topo.source_device_number);
            let dst_ep =
                crate::route::DeviceEndpoint::from_path(dst_path, topo.target_device_number);
            if !scheduler.can_schedule(&src_ep, topo.source_is_hdd, &dst_ep, topo.target_is_hdd) {
                return false;
            }
            // The cache is an additional possible I/O endpoint. Its medium can
            // be HDD or unknown despite the UI recommending an SSD location.
            if job.options.cache_enabled && !job.options.cache_root.trim().is_empty() {
                let cache_path = Path::new(job.options.cache_root.trim());
                let cache_topology = self.job_disk_topology(src_path, cache_path);
                let cache_ep = crate::route::DeviceEndpoint::from_path(
                    cache_path,
                    cache_topology.target_device_number,
                );
                return scheduler.can_schedule(
                    &cache_ep,
                    cache_topology.target_is_hdd,
                    &cache_ep,
                    cache_topology.target_is_hdd,
                );
            }
            true
        });

        let Some(job_idx) = selected_idx else {
            return Ok(None);
        };
        let mut job = queued.remove(job_idx);
        job.attempts += 1;
        job.status = "running".into();
        job.stage = "checking".into();
        job.message = String::new();
        let _lifecycle = self.job_lifecycle.lock().map_err(|_| lock_error())?;
        if self.repository.save_job(&job).is_err() {
            self.update_visible_state(|| {
                if let Some(current) = self
                    .jobs
                    .lock()
                    .map_err(|_| lock_error())?
                    .iter_mut()
                    .find(|current| current.id == job.id && current.status == "queued")
                {
                    current.message = "任务库写入失败，任务已保留在队列中，可稍后重试。".into();
                }
                Ok(())
            })?;
            return Err("StorageError".into());
        }
        let cancel = Arc::new(AtomicBool::new(
            self.cancelled_jobs
                .lock()
                .map_err(|_| lock_error())?
                .contains(&job.id),
        ));
        self.active_cancel
            .lock()
            .map_err(|_| lock_error())?
            .insert(job.id.clone(), cancel.clone());
        self.update_visible_state(|| {
            let mut jobs = self.jobs.lock().map_err(|_| lock_error())?;
            let current = jobs
                .iter_mut()
                .find(|current| current.id == job.id)
                .ok_or_else(lock_error)?;
            *current = job.clone();
            Ok(())
        })?;
        let partial = self
            .partial_requests
            .lock()
            .map_err(|_| lock_error())?
            .remove(&job.id);
        let selected = self
            .node_retry_requests
            .lock()
            .map_err(|_| lock_error())?
            .remove(&job.id);
        Ok(Some((job, cancel, partial, selected)))
    }
}

pub fn trigger_on_complete_hook(job: &JobView, hook_script: Option<&str>) {
    let Some(script) = hook_script else { return };
    let script_path = script.trim();
    if script_path.is_empty() {
        return;
    }

    let job_id = job.id.clone();
    let status = job.status.clone();
    let source_path = job.source.path.clone();
    let archive_name = job.source.display_name.clone();
    let message = job.message.clone();
    let output_path = job.output_path.clone().unwrap_or_else(|| {
        if !job.options.output_root.trim().is_empty() {
            job.options.output_root.clone()
        } else {
            Path::new(&source_path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        }
    });
    let completed_bytes = job
        .transfer_progress
        .as_ref()
        .map(|p| p.completed_bytes.to_string())
        .unwrap_or_else(|| job.source.size.to_string());
    let total_bytes = job
        .transfer_progress
        .as_ref()
        .map(|p| p.total_bytes.to_string())
        .unwrap_or_else(|| job.source.size.to_string());

    let script_cmd = script_path.to_string();
    tokio::spawn(async move {
        let script_file = std::path::Path::new(&script_cmd);
        if !script_file.is_file() {
            eprintln!("on-complete hook is not an existing file: {script_cmd}");
            return;
        }
        // Job data travels through the environment only. Passing it as argv into
        // `cmd.exe` let an archive-controlled output path inject commands at `&`.
        #[cfg(target_os = "windows")]
        let mut cmd = if script_cmd.to_lowercase().ends_with(".ps1") {
            let mut c = tokio::process::Command::new("powershell.exe");
            c.arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(&script_cmd);
            c
        } else if script_cmd.to_lowercase().ends_with(".bat")
            || script_cmd.to_lowercase().ends_with(".cmd")
        {
            if script_cmd.contains(['&', '|', '<', '>', '^', '%', '!', '"', '\r', '\n']) {
                eprintln!(
                    "on-complete hook path contains shell metacharacters and was refused: {script_cmd}"
                );
                return;
            }
            let mut c = tokio::process::Command::new("cmd.exe");
            c.arg("/C").arg(&script_cmd);
            c
        } else {
            tokio::process::Command::new(&script_cmd)
        };

        #[cfg(not(target_os = "windows"))]
        let mut cmd = {
            let mut c = tokio::process::Command::new("sh");
            c.arg(&script_cmd);
            c
        };

        cmd.env("OPENUNPACK_JOB_ID", &job_id)
            .env("OPENUNPACK_STATUS", &status)
            .env("OPENUNPACK_SOURCE", &source_path)
            .env("OPENUNPACK_OUTPUT", &output_path)
            .env("OPENUNPACK_ARCHIVE_NAME", &archive_name)
            .env("OPENUNPACK_MESSAGE", &message)
            .env("OPENUNPACK_COMPLETED_BYTES", &completed_bytes)
            .env("OPENUNPACK_TOTAL_BYTES", &total_bytes);

        match cmd.status().await {
            Ok(code) => {
                if !code.success() {
                    eprintln!("on_complete hook exited with status: {:?}", code);
                }
            }
            Err(e) => {
                eprintln!("failed to spawn on_complete hook ({}): {}", script_cmd, e);
            }
        }
    });
}
