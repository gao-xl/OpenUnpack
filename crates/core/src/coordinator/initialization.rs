use super::*;

impl Coordinator {
    pub(super) fn execution_policy(
        &self,
        id: &str,
    ) -> Result<Option<Arc<dyn JobExecutionPolicy>>, String> {
        Ok(self
            .execution_policies
            .lock()
            .map_err(|_| lock_error())?
            .get(id)
            .cloned())
    }
    pub(super) fn authorize_execution(
        &self,
        job: &JobView,
        source: &SourceGroup,
    ) -> Result<(), String> {
        if let Some(policy) = self.execution_policy(&job.id)? {
            let mut actual = job.clone();
            actual.source = source.clone();
            policy
                .authorize(&actual)
                .map_err(|_| "ApiAuthorizationDenied".to_string())?;
        }
        Ok(())
    }
    pub fn open(data: PathBuf, worker: PathBuf, engine: PathBuf) -> Result<Arc<Self>, String> {
        paths::check_path(&data)?;
        fs::create_dir_all(&data).map_err(|_| "应用数据目录无法创建。")?;
        let repository = Repository::open(&data.join("jobs.sqlite"))?;
        let mut jobs = repository.jobs()?;
        let settings = if data.join("settings.json").exists() {
            paths::read_json(&data.join("settings.json"))?
        } else {
            Settings::default()
        };
        validate_settings(&settings)?;
        let pending: HashSet<_> = repository
            .publications()?
            .into_iter()
            .filter(|p| !matches!(p.state.as_str(), "committed" | "abandoned"))
            .map(|p| p.job_id)
            .collect();
        for job in &mut jobs {
            // Legacy/imported JSON may contain an old observation. It has no
            // meaning in a fresh process, even for non-running history rows.
            job.transfer_progress = None;
            if pending.contains(&job.id) {
                job.status = "publishPending".into();
                job.message = "上次解压未完成，请检查或清理临时文件。".into();
                repository.save_job(job)?;
            } else if matches!(job.status.as_str(), "running" | "stopping") {
                job.status = "interrupted".into();
                job.message = "上次运行中断；原包和工作目录已保留。".into();
                repository.save_job(job)?;
            }
            if let Some(work) = &job.workspace {
                let marker = Path::new(work).join(UNCONFIRMED_FILE);
                if marker.exists() {
                    paths::owned(Path::new(work), &job.id, None)?;
                    let directory = paths::DirectoryGuard::open(Path::new(work))
                        .map_err(|_| "停止记录目录无法核对。")?;
                    let marker_handle = directory.lock_entry(&marker, true).ok();
                    let confirmed = marker_handle.as_ref().is_some_and(|entry| {
                        entry
                            .metadata()
                            .is_ok_and(|metadata| metadata.len() <= 16 * 1024 * 1024)
                            && serde_json::from_reader::<_, UnconfirmedExit>(
                                std::io::BufReader::new(entry.file()),
                            )
                            .is_ok_and(|record| {
                                !record.processes.is_empty()
                                    && record.processes.iter().all(|(pid, created)| {
                                        crate::worker::process_identity_exited(*pid, *created)
                                            .unwrap_or(false)
                                    })
                            })
                    });
                    if confirmed {
                        marker_handle
                            .ok_or("停止记录无法核对。")?
                            .delete()
                            .map_err(|_| "停止记录无法更新。")?;
                    } else {
                        job.status = "stopping".into();
                        job.message = "上次解压进程尚未确认退出，请稍后再操作工作目录。".into();
                        repository.save_job(job)?;
                    }
                }
            }
        }
        let mut cache_usage_by_job = HashMap::new();
        for job in &jobs {
            if let Some(workspace) = &job.workspace {
                cache_usage_by_job.insert(
                    job.id.clone(),
                    CacheUsage {
                        workspace: Some(workspace.clone()),
                        bytes: paths::tree_size(Path::new(workspace))?,
                        generation: 0,
                    },
                );
            }
        }
        let cache_usage = cache_usage_by_job
            .values()
            .fold(0u64, |sum, usage| sum.saturating_add(usage.bytes));
        let (notification, _) = broadcast::channel(32);
        let (progress_broadcaster, _) = broadcast::channel(128);
        let (need_input_broadcaster, _) = broadcast::channel(32);
        let cancelled_jobs = jobs
            .iter()
            .filter(|job| job.status == "cancelled")
            .map(|job| job.id.clone())
            .collect();
        let worker = WorkerClient {
            executable: worker,
            engine,
        };
        let stop_batch = Arc::new(AtomicBool::new(false));
        let in_flight_requests = Arc::new(Mutex::new(HashSet::new()));
        let in_flight_request_ids = Arc::new(Mutex::new(HashMap::new()));
        let cache_budget = CacheBudgetManager::new(Arc::new(RequestBudgetOwner::new(
            in_flight_requests.clone(),
            in_flight_request_ids.clone(),
            worker.clone(),
            stop_batch.clone(),
        )));
        let core = Arc::new(Self {
            data: data.clone(),
            repository,
            vault: Vault::open(&data.join("passwords.vault")),
            worker,
            jobs: Mutex::new(jobs),
            execution_policies: Mutex::new(HashMap::new()),
            authorization_blocked: Mutex::new(HashSet::new()),
            scheduler_scope: Mutex::new(None),
            settings: Mutex::new(settings),
            running: AtomicBool::new(false),
            runtime: tokio::runtime::Handle::try_current().ok(),
            paused: AtomicBool::new(false),
            api_paused: AtomicBool::new(false),
            stop_batch,
            active_cancel: Mutex::new(std::collections::HashMap::new()),
            in_flight_requests,
            in_flight_request_ids,
            publications: publication::PublicationService::default(),
            cancelled_jobs: Mutex::new(cancelled_jobs),
            partial_requests: Mutex::new(HashSet::new()),
            node_retry_requests: Mutex::new(std::collections::HashMap::new()),
            notification,
            mutations: Mutex::new(()),
            job_lifecycle: Mutex::new(()),
            deleting_jobs: Mutex::new(HashSet::new()),
            recovering_jobs: Mutex::new(HashSet::new()),
            ephemeral_passwords: Mutex::new(HashMap::new()),
            cache_usage: AtomicU64::new(cache_usage),
            cache_usage_by_job: Mutex::new(cache_usage_by_job),
            cache_budget,
            session_id: uuid::Uuid::new_v4().to_string(),
            revision: AtomicU64::new(1),
            view_state: Mutex::new(()),
            transfers: Mutex::new(HashMap::new()),
            snapshot_projector: snapshot_projector::SnapshotProjector::default(),
            inspect_cancel: Mutex::new(None),
            cancelled_inspections: Mutex::new(std::collections::VecDeque::new()),
            preview_cancel: Mutex::new(None),
            cancelled_previews: Mutex::new(std::collections::VecDeque::new()),
            folder_watcher: Mutex::new(None),
            progress_broadcaster,
            need_input_broadcaster,
            pending_inputs: Mutex::new(HashMap::new()),
            #[cfg(test)]
            topology_probe: Mutex::new(None),
        });
        core.apply_outbox();
        core.init_folder_watcher();
        Ok(core)
    }
    pub fn revision_info(&self) -> (String, u64) {
        (
            self.session_id.clone(),
            self.revision.load(Ordering::Acquire),
        )
    }
    pub fn newly_finished_jobs(&self, known: &mut HashSet<String>) -> bool {
        let Ok(jobs) = self.jobs.lock() else {
            return false;
        };
        let mut beep = false;
        for j in jobs.iter() {
            if matches!(
                j.status.as_str(),
                "completed" | "partial" | "salvaged" | "cleanupPending"
            ) && known.insert(j.id.clone())
            {
                beep = true;
            }
        }
        beep
    }
    pub fn completion_sound(&self) -> bool {
        self.settings
            .lock()
            .map(|s| s.completion_sound)
            .unwrap_or(false)
    }
    pub fn vault(&self) -> Result<&Vault, String> {
        self.vault.as_ref().map_err(Clone::clone)
    }
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.notification.subscribe()
    }
    pub fn subscribe_progress(
        &self,
    ) -> broadcast::Receiver<openunpack_protocol::JobProgressUpdate> {
        self.progress_broadcaster.subscribe()
    }
    pub fn subscribe_need_input(
        &self,
    ) -> broadcast::Receiver<openunpack_protocol::NeedPasswordEvent> {
        self.need_input_broadcaster.subscribe()
    }
    pub fn submit_input(&self, job_id: &str, input: String) -> Result<bool, String> {
        let mut pending = self.pending_inputs.lock().map_err(|_| lock_error())?;
        if let Some(sender) = pending.remove(job_id) {
            let _ = sender.send(input);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn changed(&self) {
        if let Ok(_state) = self.view_state.lock() {
            self.changed_locked();
        }
    }
    pub(super) fn changed_locked(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        let _ = self.notification.send(());
    }
    /// Host-side vault mutations must use this gate so their data and revision
    /// become visible together. The closure must not call other gated methods.
    pub fn update_visible_state<T>(
        &self,
        update: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _state = self.view_state.lock().map_err(|_| lock_error())?;
        let result = update();
        self.changed_locked();
        result
    }
}
