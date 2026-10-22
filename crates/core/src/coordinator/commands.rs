use super::*;

impl Coordinator {
    pub fn scan(&self, inputs: &[String]) -> Result<ScanResult, String> {
        self.scan_with_cancel(inputs, &AtomicBool::new(false))
    }
    pub fn scan_with_cancel(
        &self,
        inputs: &[String],
        cancel: &AtomicBool,
    ) -> Result<ScanResult, String> {
        if inputs.is_empty() || inputs.len() > 10000 {
            return Err("一次最多添加一万个路径。".into());
        }
        for p in inputs {
            paths::check_path(Path::new(p))?;
        }
        let s = self.settings.lock().map_err(|_| lock_error())?.clone();
        scanner::scan_with_cancel(inputs, s.recursive_scan, &s.input_exclusions, cancel)
    }
    pub fn add_jobs(
        &self,
        sources: Vec<SourceGroup>,
        options: Settings,
    ) -> Result<Vec<JobView>, String> {
        self.add_jobs_inner(sources, options, None, None)
    }
    pub fn add_jobs_authorized(
        &self,
        sources: Vec<SourceGroup>,
        options: Settings,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<Vec<JobView>, String> {
        self.add_jobs_inner(sources, options, Some(policy), None)
    }
    /// Admit API jobs and their one-time password under the same mutation gate.
    /// The password only enters the existing in-memory candidate map after the
    /// corresponding job has been persisted; it never becomes a vault entry.
    pub fn add_jobs_with_password_authorized(
        &self,
        sources: Vec<SourceGroup>,
        options: Settings,
        password: Option<zeroize::Zeroizing<String>>,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<Vec<JobView>, String> {
        let password = password.filter(|secret| !secret.is_empty());
        self.add_jobs_inner(sources, options, Some(policy), password)
    }
    /// Validate the current frozen job and bind its ongoing execution before
    /// waking an input waiter. A closed waiter is an error, never a retry hint.
    pub fn submit_input_authorized(
        &self,
        job_id: &str,
        input: String,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<bool, String> {
        let input = zeroize::Zeroizing::new(input);
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        let job = self.get_job(job_id)?;
        policy.authorize(&job)?;
        self.confirm_work_stopped(&job)?;
        let mut pending = self.pending_inputs.lock().map_err(|_| lock_error())?;
        if !pending.contains_key(job_id) {
            return Ok(false);
        }
        let mut policies = self.execution_policies.lock().map_err(|_| lock_error())?;
        let previous = policies.insert(job_id.to_owned(), policy);
        let sender = pending.remove(job_id).ok_or_else(lock_error)?;
        if let Err(undelivered) = sender.send(input.to_string()) {
            let _undelivered = zeroize::Zeroizing::new(undelivered);
            if let Some(previous) = previous {
                policies.insert(job_id.to_owned(), previous);
            } else {
                policies.remove(job_id);
            }
            return Err("任务的输入等待已经结束，请刷新任务状态。".into());
        }
        Ok(true)
    }
    pub(super) fn add_jobs_inner(
        &self,
        sources: Vec<SourceGroup>,
        options: Settings,
        policy: Option<Arc<dyn JobExecutionPolicy>>,
        password: Option<zeroize::Zeroizing<String>>,
    ) -> Result<Vec<JobView>, String> {
        validate_settings(&options)?;
        if sources.is_empty() || sources.len() > 10000 {
            return Err("任务数量无效。".into());
        }
        // Never trust volumes or sizes supplied over IPC: rescan the selected archive starts.
        let inputs: Vec<_> = sources.into_iter().map(|s| s.path).collect();
        for p in &inputs {
            paths::check_path(Path::new(p))?;
        }
        let scan = scanner::scan(&inputs, false, &options.input_exclusions)?;
        if scan.sources.is_empty() {
            return Err("没有可处理的压缩包。".into());
        }
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        if policy.is_some() {
            self.check_api_scheduler()?;
        }
        let existing = self.jobs.lock().map_err(|_| lock_error())?.clone();
        let mut added = vec![];
        for source in scan.sources {
            if existing.iter().chain(added.iter()).any(|j: &JobView| {
                j.source.path.eq_ignore_ascii_case(&source.path) && !terminal(&j.status)
            }) {
                continue;
            }
            let job = JobView {
                id: uid(),
                source,
                options: options.clone(),
                status: "queued".into(),
                stage: "queued".into(),
                progress: 0.,
                message: String::new(),
                attempts: 0,
                output_path: None,
                verification_scope: String::new(),
                created_at: now(),
                finished_at: None,
                workspace: None,
                nodes: vec![],
                stage_times: BTreeMap::new(),
                transfer_progress: None,
            };
            added.push(job);
        }
        // Reject authorization for the entire new selection before admitting
        // any member. This does not make later database I/O transactional.
        if let Some(policy) = &policy {
            for job in &added {
                policy.authorize(job)?;
            }
        }
        let mut ephemeral = password
            .as_ref()
            .map(|_| self.ephemeral_passwords.lock().map_err(|_| lock_error()))
            .transpose()?;
        for job in &added {
            // The mutation gate excludes claim until both policy and record
            // have been installed. Failed persistence cannot admit this job.
            let mut policies = self.execution_policies.lock().map_err(|_| lock_error())?;
            self.repository.save_job(job)?;
            if let (Some(ephemeral), Some(password)) = (ephemeral.as_mut(), password.as_ref()) {
                ephemeral.insert(job.id.clone(), vec![password.clone()]);
            }
            if let Some(policy) = &policy {
                policies.insert(job.id.clone(), policy.clone());
            }
            drop(policies);
            self.update_visible_state(|| {
                self.jobs
                    .lock()
                    .map_err(|_| lock_error())?
                    .push(job.clone());
                Ok(())
            })?;
        }
        Ok(added)
    }
    pub fn init_folder_watcher(self: &Arc<Self>) {
        let watched = {
            let Ok(s) = self.settings.lock() else {
                return;
            };
            s.watched_folders.clone()
        };
        let folders: Vec<PathBuf> = watched
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect();
        let watcher_res = crate::watcher::FolderWatcher::start_with_coordinator(
            Arc::downgrade(self),
            folders,
            crate::watcher::WatcherConfig::default(),
        );
        if let Ok(w) = watcher_res
            && let Ok(mut lock) = self.folder_watcher.lock()
        {
            *lock = Some(w);
        }
    }
    pub fn enqueue_discovered_files(
        self: &Arc<Self>,
        paths: &[PathBuf],
    ) -> Result<Vec<JobView>, String> {
        if paths.is_empty() {
            return Ok(vec![]);
        }
        let s = self.settings.lock().map_err(|_| lock_error())?.clone();
        let inputs: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        for p in &inputs {
            paths::check_path(Path::new(p))?;
        }
        let scan = scanner::scan(&inputs, false, &s.input_exclusions)?;
        if scan.sources.is_empty() {
            return Ok(vec![]);
        }
        let complete_sources: Vec<_> = scan
            .sources
            .into_iter()
            .filter(|src| !src.missing_volume)
            .collect();
        if complete_sources.is_empty() {
            return Ok(vec![]);
        }
        let added = self.add_jobs(complete_sources, s.clone())?;
        if !added.is_empty() && s.auto_start_on_add && !self.running.load(Ordering::Acquire) {
            let _ = self.start();
        }
        Ok(added)
    }
    pub fn save_settings(&self, s: Settings) -> Result<Settings, String> {
        validate_settings(&s)?;
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        paths::atomic_json(&self.data.join("settings.json"), &s)?;
        self.update_visible_state(|| {
            *self.settings.lock().map_err(|_| lock_error())? = s.clone();
            Ok(())
        })?;
        let target_folders: Vec<PathBuf> = s
            .watched_folders
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect();
        if let Ok(lock) = self.folder_watcher.lock()
            && let Some(watcher) = lock.as_ref()
        {
            let _ = watcher.sync_folders(target_folders);
        }
        Ok(s)
    }
    pub fn history(&self) -> Result<Vec<JobView>, String> {
        let limit = self
            .settings
            .lock()
            .map_err(|_| lock_error())?
            .history_limit;
        let mut jobs: Vec<_> = self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .filter(|j| {
                terminal(&j.status)
                    || recovering(&j.status)
                    || matches!(
                        j.status.as_str(),
                        "needsPassword" | "waitingResource" | "stopping"
                    )
            })
            .cloned()
            .collect();
        jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        jobs.truncate(limit);
        Ok(jobs)
    }
    pub fn job_details(&self, job_id: &str) -> Result<Option<JobView>, String> {
        let _state = self.view_state.lock().map_err(|_| lock_error())?;
        let mut job = self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .find(|job| job.id == job_id)
            .cloned();
        if let Some(job) = job.as_mut() {
            job.transfer_progress = self
                .transfers
                .lock()
                .map_err(|_| lock_error())?
                .get(&job.id)
                .filter(|transfer| transfer.identity.matches_job(job))
                .and_then(|transfer| transfer.view.clone());
        }
        Ok(job)
    }
    pub fn history_page(&self, query: HistoryPageQuery) -> Result<JobHistoryPage, String> {
        if query.limit == 0
            || query.limit > 100
            || query.search.len() > 4096
            || query.status.len() > 64
        {
            return Err("历史记录查询条件无效。".into());
        }
        let _state = self.view_state.lock().map_err(|_| lock_error())?;
        let history_limit = self
            .settings
            .lock()
            .map_err(|_| lock_error())?
            .history_limit;
        let stored = self.jobs.lock().map_err(|_| lock_error())?;
        // This check covers hidden pages and records beyond the display limit.
        // The mutation still performs its own authoritative lifecycle checks.
        let clear_allowed = !self.running.load(Ordering::Acquire)
            && !stored.iter().any(|job| {
                job.status == "publishPending"
                    || job.status == "stopping"
                    || job.workspace.is_some()
            });
        let mut history: Vec<_> = stored
            .iter()
            .filter(|job| {
                terminal(&job.status)
                    || recovering(&job.status)
                    || matches!(
                        job.status.as_str(),
                        "needsPassword" | "waitingResource" | "stopping"
                    )
            })
            .collect();
        history.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        history.truncate(history_limit);
        let total = history.len();
        let completed = history
            .iter()
            .filter(|job| job.status == "completed")
            .count();
        let failed = history.iter().filter(|job| job.status == "failed").count();
        let recoverable = history
            .iter()
            .filter(|job| {
                matches!(
                    job.status.as_str(),
                    "needsPassword" | "failed" | "cancelled" | "interrupted" | "publishPending"
                )
            })
            .count();
        let search = query.search.trim().to_lowercase();
        history.retain(|job| {
            (query.status == "all" || job.status == query.status)
                && (search.is_empty()
                    || job.source.display_name.to_lowercase().contains(&search)
                    || job.source.path.to_lowercase().contains(&search))
        });
        let filtered_total = history.len();
        let last_offset = filtered_total.saturating_sub(1) / query.limit * query.limit;
        let offset = query.offset.min(last_offset);
        let items = history
            .iter()
            .skip(offset)
            .take(query.limit)
            .map(|job| JobSummary {
                id: job.id.clone(),
                source: job.source.clone(),
                status: job.status.clone(),
                stage: job.stage.clone(),
                progress: job.progress,
                message: job.message.clone(),
                attempts: job.attempts,
                output_path: job.output_path.clone(),
                verification_scope: job.verification_scope.clone(),
                created_at: job.created_at.clone(),
                finished_at: job.finished_at.clone(),
                workspace: job.workspace.clone(),
            })
            .collect();
        Ok(JobHistoryPage {
            items,
            offset,
            limit: query.limit,
            filtered_total,
            total,
            completed,
            failed,
            recoverable,
            clear_allowed,
            session_id: self.session_id.clone(),
            revision: self.revision.load(Ordering::Acquire),
        })
    }
    pub fn pause(&self) -> Result<(), String> {
        // A claim keeps this gate through persistence and owner registration.
        // Once pause returns, every earlier claim is fully visible and no later
        // claim can start until the queue is explicitly resumed.
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        self.update_visible_state(|| {
            self.api_paused.store(false, Ordering::Release);
            self.paused.store(true, Ordering::Release);
            Ok(())
        })
    }
    pub fn cancel(&self, id: Option<&str>) -> Result<(), String> {
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        if id.is_none() {
            self.stop_batch.store(true, Ordering::Release);
            self.update_visible_state(|| {
                self.api_paused.store(false, Ordering::Release);
                self.paused.store(false, Ordering::Release);
                Ok(())
            })?;
        }
        if let Some(id) = id {
            self.cancelled_jobs
                .lock()
                .map_err(|_| lock_error())?
                .insert(id.into());
        }
        {
            let active = self.active_cancel.lock().map_err(|_| lock_error())?;
            if let Some(id) = id {
                if let Some(cancel) = active.get(id) {
                    cancel.store(true, Ordering::Release);
                }
            } else {
                for cancel in active.values() {
                    cancel.store(true, Ordering::Release);
                }
            }
        }
        if let Some(id) = id {
            let mut job = self.get_job(id)?;
            if matches!(job.status.as_str(), "queued" | "waitingResource") {
                job.status = "cancelled".into();
                job.stage = "cancelled".into();
                job.finished_at = Some(now());
                job.message = "尚未运行，已取消。".into();
                self.put(&job)?;
            }
        }
        self.clear_empty_api_pause()?;
        self.changed();
        Ok(())
    }
    pub fn retry(&self, id: &str, partial: bool) -> Result<(), String> {
        self.enqueue_retry(id, partial, None, None, None, false)
    }
    pub fn retry_with_password(
        &self,
        id: &str,
        partial: bool,
        password: Option<String>,
    ) -> Result<(), String> {
        self.enqueue_retry(id, partial, None, password, None, false)
    }
    pub fn retry_authorized(
        &self,
        id: &str,
        partial: bool,
        password: Option<String>,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<(), String> {
        self.enqueue_retry(id, partial, None, password, Some(policy), false)
    }
    pub fn retry_preserving_options_authorized(
        &self,
        id: &str,
        partial: bool,
        password: Option<String>,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<(), String> {
        self.enqueue_retry(id, partial, None, password, Some(policy), true)
    }
    pub fn retry_node(&self, id: &str, node_id: &str) -> Result<(), String> {
        let job = self.get_job(id)?;
        if !job.nodes.iter().any(|n| {
            n.id == node_id
                && n.depth > 0
                && matches!(n.status.as_str(), "failed" | "needsPassword")
        }) {
            return Err("只能重试未完成的内包节点。".into());
        }
        self.enqueue_retry(id, false, Some(node_id), None, None, false)
    }
    pub fn retry_node_authorized(
        &self,
        id: &str,
        node_id: &str,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<(), String> {
        let job = self.get_job(id)?;
        if !job.nodes.iter().any(|n| {
            n.id == node_id
                && n.depth > 0
                && matches!(n.status.as_str(), "failed" | "needsPassword")
        }) {
            return Err("只能重试未完成的内包节点。".into());
        }
        self.enqueue_retry(id, false, Some(node_id), None, Some(policy), false)
    }
    pub fn retry_node_preserving_options_authorized(
        &self,
        id: &str,
        node_id: &str,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<(), String> {
        let job = self.get_job(id)?;
        if !job.nodes.iter().any(|node| {
            node.id == node_id
                && node.depth > 0
                && matches!(node.status.as_str(), "failed" | "needsPassword")
        }) {
            return Err("只能重试未完成的内包节点。".into());
        }
        self.enqueue_retry(id, false, Some(node_id), None, Some(policy), true)
    }
    pub(super) fn enqueue_retry(
        &self,
        id: &str,
        partial: bool,
        node_id: Option<&str>,
        password: Option<String>,
        policy: Option<Arc<dyn JobExecutionPolicy>>,
        preserve_options: bool,
    ) -> Result<(), String> {
        let password = password.map(zeroize::Zeroizing::new);
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        if self
            .deleting_jobs
            .lock()
            .map_err(|_| lock_error())?
            .contains(id)
        {
            return Err("任务正在删除，请等待删除完成。".into());
        }
        let mut job = self.get_job(id)?;
        if matches!(
            job.status.as_str(),
            "running"
                | "stopping"
                | "queued"
                | "completed"
                | "partial"
                | "salvaged"
                | "publishPending"
        ) {
            return Err("当前状态不能重试，请先处理发布恢复记录。".into());
        }
        self.confirm_work_stopped(&job)?;
        if partial
            && !job
                .nodes
                .iter()
                .any(|n| n.depth > 0 && matches!(n.status.as_str(), "failed" | "needsPassword"))
        {
            return Err("只有包含未完成内层包的任务才能保留部分解压结果。".into());
        }
        if !preserve_options {
            job.options = self.settings.lock().map_err(|_| lock_error())?.clone();
        }
        if let Some(policy) = &policy {
            // Recovery already owns running while it requeues its own target.
            if !self
                .recovering_jobs
                .lock()
                .map_err(|_| lock_error())?
                .contains(id)
            {
                self.check_api_scheduler()?;
            }
            policy.authorize(&job)?;
        }
        if let Some(pwd) = password.filter(|pwd| !pwd.is_empty()) {
            let mut ephem = self.ephemeral_passwords.lock().map_err(|_| lock_error())?;
            let entry = ephem.entry(id.into()).or_default();
            if !entry.iter().any(|secret| secret.as_str() == pwd.as_str()) {
                entry.insert(0, pwd);
            }
        }
        if partial {
            self.partial_requests
                .lock()
                .map_err(|_| lock_error())?
                .insert(id.into());
        }
        self.cancelled_jobs
            .lock()
            .map_err(|_| lock_error())?
            .remove(id);
        if let Some(node_id) = node_id {
            self.node_retry_requests
                .lock()
                .map_err(|_| lock_error())?
                .insert(id.into(), HashSet::from([node_id.into()]));
        } else {
            self.node_retry_requests
                .lock()
                .map_err(|_| lock_error())?
                .remove(id);
        }
        let mut policies = self.execution_policies.lock().map_err(|_| lock_error())?;
        if let Some(policy) = policy {
            policies.insert(id.into(), policy);
        } else {
            policies.remove(id);
        }
        self.authorization_blocked
            .lock()
            .map_err(|_| lock_error())?
            .remove(id);
        drop(policies);
        job.status = "queued".into();
        job.stage = "queued".into();
        job.message = if partial {
            "保留未解压的内层包，仅输出已完成内容。".into()
        } else if preserve_options {
            "保留原任务设置，使用当前密码库进行新的尝试。".into()
        } else {
            "使用当前设置和密码库进行新的尝试。".into()
        };
        job.finished_at = None;
        job.progress = 0.;
        self.put(&job)
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        if self
            .deleting_jobs
            .lock()
            .map_err(|_| lock_error())?
            .contains(id)
            || self
                .recovering_jobs
                .lock()
                .map_err(|_| lock_error())?
                .contains(id)
            || self
                .active_cancel
                .lock()
                .map_err(|_| lock_error())?
                .contains_key(id)
        {
            return Err("任务尚未停止或正在恢复，请稍后删除。".into());
        }
        let job = self.get_job(id)?;
        if job.workspace.is_some() {
            return Err("请先在处理记录中清理保留的工作目录。".into());
        }
        self.remove_record(id)
    }
    pub(super) fn remove_record(&self, id: &str) -> Result<(), String> {
        let _lifecycle = self.job_lifecycle.lock().map_err(|_| lock_error())?;
        if let Ok(mut ephem) = self.ephemeral_passwords.lock() {
            ephem.remove(id);
        }
        self.repository.remove_job(id)?;
        self.execution_policies
            .lock()
            .map_err(|_| lock_error())?
            .remove(id);
        self.authorization_blocked
            .lock()
            .map_err(|_| lock_error())?
            .remove(id);
        if let Ok(mut cancelled) = self.cancelled_jobs.lock() {
            cancelled.remove(id);
        }
        self.update_visible_state(|| {
            self.jobs
                .lock()
                .map_err(|_| lock_error())?
                .retain(|j| j.id != id);
            self.transfers.lock().map_err(|_| lock_error())?.remove(id);
            Ok(())
        })?;
        self.clear_empty_api_pause()
    }
    /// Cancel a task, wait for its complete operation owner, then remove only its
    /// owned temporary workspace and record. No synchronous gate crosses await.
    pub async fn delete_task(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let core = self.clone();
        let job_id = id.to_string();
        let intent = tokio::task::spawn_blocking(move || core.begin_delete(&job_id))
            .await
            .map_err(|_| lock_error())??;
        let Some(intent) = intent else {
            return Ok(());
        };
        let mut changes = self.subscribe();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let core = self.clone();
            let job_id = id.to_string();
            let operation_intent = intent.clone();
            let done = tokio::task::spawn_blocking(move || {
                let _intent = operation_intent;
                core.finish_delete(&job_id)
            })
            .await
            .map_err(|_| lock_error())??;
            if done {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(
                    "任务已请求停止，但退出尚未确认；删除未完成，任务和恢复记录已保留。".into(),
                );
            }
            tokio::select! {
                _ = changes.recv() => {},
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {},
            }
        }
    }
    pub(super) fn begin_delete(
        self: &Arc<Self>,
        id: &str,
    ) -> Result<Option<Arc<DeleteIntent>>, String> {
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        let mut job = match self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .find(|job| job.id == id)
            .cloned()
        {
            Some(job) => job,
            None => return Ok(None),
        };
        if self
            .recovering_jobs
            .lock()
            .map_err(|_| lock_error())?
            .contains(id)
        {
            return Err("任务正在核对恢复结果，请等待恢复操作完成后删除。".into());
        }
        if !self
            .deleting_jobs
            .lock()
            .map_err(|_| lock_error())?
            .insert(id.into())
        {
            return Err("任务正在删除，请等待删除完成。".into());
        }
        // The blocking operation creates its own guard before any write. Its
        // lifetime also covers dropped callers and detached cleanup operations.
        let intent = Arc::new(DeleteIntent {
            core: self.clone(),
            job_id: id.into(),
        });
        {
            self.cancelled_jobs
                .lock()
                .map_err(|_| lock_error())?
                .insert(id.into());
            if let Some(cancel) = self.active_cancel.lock().map_err(|_| lock_error())?.get(id) {
                cancel.store(true, Ordering::Release);
            }
            if job.status == "queued" {
                job.status = "cancelled".into();
                job.finished_at = Some(now());
                job.message = "尚未运行，正在删除任务。".into();
                self.put(&job)?;
            }
        }
        Ok(Some(intent))
    }
    pub(super) fn finish_delete(&self, id: &str) -> Result<bool, String> {
        let mut job = {
            let _gate = self.mutations.lock().map_err(|_| lock_error())?;
            if self
                .active_cancel
                .lock()
                .map_err(|_| lock_error())?
                .contains_key(id)
            {
                return Ok(false);
            }
            let job = match self
                .jobs
                .lock()
                .map_err(|_| lock_error())?
                .iter()
                .find(|job| job.id == id)
                .cloned()
            {
                Some(job) => job,
                None => return Ok(true),
            };
            self.confirm_work_stopped(&job)?;
            if matches!(job.status.as_str(), "running" | "publishPending")
                || self.repository.publications()?.iter().any(|publication| {
                    publication.job_id == id
                        && !matches!(publication.state.as_str(), "committed" | "abandoned")
                })
            {
                return Err("任务停止或发布结果尚未确认，请先核对恢复记录；任务未删除。".into());
            }
            job
        };
        // The delete intent prevents claiming, retry and recovery of this task.
        // Potentially slow filesystem cleanup holds no command or snapshot lock.
        if let Some(workspace) = &job.workspace {
            let workspace = Path::new(workspace);
            paths::check_path(workspace)?;
            if workspace.exists() {
                paths::remove_owned(workspace, id)?;
            }
            job.workspace = None;
            if job.status == "cleanupPending" {
                job.status = if job.output_path.is_none() {
                    "cancelled"
                } else if job.verification_scope.contains("salvaged") {
                    "salvaged"
                } else if job
                    .nodes
                    .iter()
                    .any(|node| matches!(node.status.as_str(), "failed" | "needsPassword"))
                {
                    "partial"
                } else {
                    "completed"
                }
                .into();
            }
            job.message = "临时工作目录已清理；正在删除任务记录。".into();
            // Persist cleanup before removing the record. Failed writes retain the
            // prior durable record so restart/retry can recover the task.
            self.put(&job)?;
        }
        {
            let _gate = self.mutations.lock().map_err(|_| lock_error())?;
            self.remove_record(id)?;
            self.partial_requests
                .lock()
                .map_err(|_| lock_error())?
                .remove(id);
            self.node_retry_requests
                .lock()
                .map_err(|_| lock_error())?
                .remove(id);
        }
        let _ = self.refresh_usage();
        Ok(true)
    }
    pub fn clear_completed(&self) -> Result<(), String> {
        let ids: Vec<_> = self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .filter(|j| {
                matches!(
                    j.status.as_str(),
                    "completed" | "partial" | "salvaged" | "skipped"
                ) && j.workspace.is_none()
            })
            .map(|j| j.id.clone())
            .collect();
        for id in ids {
            self.remove(&id)?;
        }
        Ok(())
    }
    pub fn clear_history(&self) -> Result<(), String> {
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        let _lifecycle = self.job_lifecycle.lock().map_err(|_| lock_error())?;
        if !self
            .deleting_jobs
            .lock()
            .map_err(|_| lock_error())?
            .is_empty()
            || !self
                .recovering_jobs
                .lock()
                .map_err(|_| lock_error())?
                .is_empty()
            || !self
                .active_cancel
                .lock()
                .map_err(|_| lock_error())?
                .is_empty()
        {
            return Err("还有任务正在处理，请稍后清空记录。".into());
        }
        if self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .any(|j| terminal(&j.status) && j.workspace.is_some())
        {
            return Err("请先清理历史任务保留的工作目录。".into());
        }
        self.repository.clear_history()?;
        let jobs = self.repository.jobs()?;
        if let Ok(mut cancelled) = self.cancelled_jobs.lock() {
            let active_ids: HashSet<&str> = jobs.iter().map(|j| j.id.as_str()).collect();
            cancelled.retain(|id| active_ids.contains(id.as_str()));
        }
        self.update_visible_state(|| {
            *self.jobs.lock().map_err(|_| lock_error())? = jobs;
            Ok(())
        })
    }
    pub fn authorize_open(&self, path: &Path) -> Result<(), String> {
        paths::check_path(path)?;
        let allowed = self.jobs.lock().map_err(|_| lock_error())?.iter().any(|j| {
            Path::new(&j.source.path) == path
                || Path::new(&j.source.path).parent() == Some(path)
                || j.output_path
                    .as_deref()
                    .is_some_and(|p| Path::new(p) == path)
                || j.workspace.as_deref().is_some_and(|p| Path::new(p) == path)
        });
        if allowed || path == self.data {
            Ok(())
        } else {
            Err("此路径不属于已登记任务。".into())
        }
    }
}

#[cfg(test)]
mod api_password_tests {
    use super::*;

    struct PasswordPolicy(bool);
    impl JobExecutionPolicy for PasswordPolicy {
        fn authorize(&self, _: &JobView) -> Result<(), String> {
            if self.0 {
                Ok(())
            } else {
                Err("synthetic denied".into())
            }
        }
        fn authorize_path(&self, _: &Path, _: bool) -> Result<(), String> {
            Ok(())
        }
    }
    fn fixture() -> (tempfile::TempDir, Arc<Coordinator>, Vec<SourceGroup>) {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("sample.zip");
        fs::write(&archive, b"synthetic metadata-only fixture").unwrap();
        let core = Coordinator::open(
            directory.path().join("data"),
            directory.path().join("missing-worker.exe"),
            directory.path().join("missing-engine.dll"),
        )
        .unwrap();
        let sources = scanner::scan(&[paths::path_string(&archive)], false, &[])
            .unwrap()
            .sources;
        (directory, core, sources)
    }

    #[test]
    fn api_admission_keeps_password_only_for_successful_new_jobs_in_memory() {
        let (_directory, core, sources) = fixture();
        assert!(
            core.add_jobs_with_password_authorized(
                sources.clone(),
                Settings::default(),
                Some(zeroize::Zeroizing::new("synthetic-denied".into())),
                Arc::new(PasswordPolicy(false)),
            )
            .is_err()
        );
        assert!(core.ephemeral_passwords.lock().unwrap().is_empty());
        assert!(core.repository.jobs().unwrap().is_empty());
        let added = core
            .add_jobs_with_password_authorized(
                sources.clone(),
                Settings::default(),
                Some(zeroize::Zeroizing::new("synthetic-one-time".into())),
                Arc::new(PasswordPolicy(true)),
            )
            .unwrap();
        assert_eq!(added.len(), 1);
        assert!(core.execution_policy(&added[0].id).unwrap().is_some());
        assert_eq!(
            core.ephemeral_passwords.lock().unwrap()[&added[0].id][0].as_str(),
            "synthetic-one-time"
        );
        assert!(
            core.add_jobs_with_password_authorized(
                sources,
                Settings::default(),
                Some(zeroize::Zeroizing::new("synthetic-duplicate".into())),
                Arc::new(PasswordPolicy(true)),
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            core.ephemeral_passwords.lock().unwrap()[&added[0].id].len(),
            1
        );
        assert!(core.vault().unwrap().views().unwrap().is_empty());
        assert!(
            !serde_json::to_string(&core.repository.jobs().unwrap())
                .unwrap()
                .contains("synthetic-one-time")
        );
    }

    #[test]
    fn failed_job_persistence_leaves_no_temporary_password_or_execution_policy() {
        let (_directory, core, sources) = fixture();
        let connection = rusqlite::Connection::open(core.data.join("jobs.sqlite")).unwrap();
        connection.execute_batch("CREATE TRIGGER reject_api_job BEFORE INSERT ON jobs BEGIN SELECT RAISE(ABORT, 'synthetic storage failure'); END;").unwrap();
        assert!(
            core.add_jobs_with_password_authorized(
                sources,
                Settings::default(),
                Some(zeroize::Zeroizing::new("synthetic-one-time".into())),
                Arc::new(PasswordPolicy(true)),
            )
            .is_err()
        );
        assert!(core.ephemeral_passwords.lock().unwrap().is_empty());
        assert!(core.execution_policies.lock().unwrap().is_empty());
        assert!(core.repository.jobs().unwrap().is_empty());
        assert!(core.jobs.lock().unwrap().is_empty());
    }

    #[test]
    fn authorized_input_binds_policy_before_delivery_and_propagates_closed_waiter() {
        let (_directory, core, sources) = fixture();
        let added = core.add_jobs(sources, Settings::default()).unwrap();
        let id = &added[0].id;
        let (sender, mut receiver) = tokio::sync::oneshot::channel();
        core.pending_inputs
            .lock()
            .unwrap()
            .insert(id.clone(), sender);
        assert!(
            core.submit_input_authorized(
                id,
                "synthetic-denied".into(),
                Arc::new(PasswordPolicy(false))
            )
            .is_err()
        );
        assert!(core.pending_inputs.lock().unwrap().contains_key(id));
        assert!(core.execution_policy(id).unwrap().is_none());
        assert!(
            core.submit_input_authorized(
                id,
                "synthetic-input".into(),
                Arc::new(PasswordPolicy(true))
            )
            .unwrap()
        );
        assert!(core.execution_policy(id).unwrap().is_some());
        assert_eq!(receiver.try_recv().unwrap(), "synthetic-input");
        assert!(
            !core
                .submit_input_authorized(
                    id,
                    "synthetic-unused".into(),
                    Arc::new(PasswordPolicy(true))
                )
                .unwrap()
        );
        let (sender, receiver) = tokio::sync::oneshot::channel();
        drop(receiver);
        core.pending_inputs
            .lock()
            .unwrap()
            .insert(id.clone(), sender);
        let previous_policy = core.execution_policy(id).unwrap().unwrap();
        assert!(
            core.submit_input_authorized(
                id,
                "synthetic-closed".into(),
                Arc::new(PasswordPolicy(true))
            )
            .is_err()
        );
        assert!(Arc::ptr_eq(
            &previous_policy,
            &core.execution_policy(id).unwrap().unwrap()
        ));
        assert_eq!(core.get_job(id).unwrap().status, "queued");
        assert!(core.vault().unwrap().views().unwrap().is_empty());
    }
}
