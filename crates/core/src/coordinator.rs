use crate::{
    paths, repository::Repository, route::{ArchiveProbeSummary, ExtractionRoute, RouteDecider},
    scanner, types::*, vault::Vault, worker::WorkerClient,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use tokio::sync::broadcast;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn terminal(status: &str) -> bool {
    matches!(
        status,
        "completed" | "partial" | "failed" | "cancelled" | "skipped" | "cleanupPending"
    )
}
fn recovering(status: &str) -> bool {
    matches!(status, "interrupted" | "publishPending" | "cleanupPending")
}
fn lock_error() -> String {
    "内部状态不可用；请重启并检查恢复记录。".into()
}

pub struct Coordinator {
    pub data: PathBuf,
    repository: Repository,
    vault: Result<Vault, String>,
    worker: WorkerClient,
    jobs: Mutex<Vec<JobView>>,
    settings: Mutex<Settings>,
    running: AtomicBool,
    paused: AtomicBool,
    stop_batch: AtomicBool,
    active_cancel: Mutex<Option<(String, Arc<AtomicBool>)>>,
    cancelled_jobs: Mutex<HashSet<String>>,
    partial_requests: Mutex<HashSet<String>>,
    node_retry_requests: Mutex<std::collections::HashMap<String, HashSet<String>>>,
    notification: broadcast::Sender<()>,
    mutations: Mutex<()>,
    cache_usage: AtomicU64,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    publication: Publication,
    events: Vec<SuccessEvent>,
    partial: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct NodeWork {
    id: String,
    group: SourceGroup,
    prefix: PathBuf,
    depth: u32,
    parent: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct SealedNode {
    node: NodeWork,
    directory: PathBuf,
    files: Vec<FileRecord>,
}
#[derive(Serialize, Deserialize)]
struct SavedRun {
    version: u32,
    job_id: String,
    source: SourceGroup,
    witnesses: Vec<SourceWitness>,
    options: Settings,
    input_files: Vec<CopyFile>,
    fingerprint: Option<String>,
    pending: VecDeque<NodeWork>,
    failed: Vec<(NodeWork, String)>,
    sealed: Vec<SealedNode>,
    final_files: Vec<CopyFile>,
    events: Vec<SuccessEvent>,
    expanded: u64,
    entries: u64,
    views: Vec<NodeView>,
    #[serde(default)]
    retired_files: HashSet<String>,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct SourceWitness {
    size: u64,
    modified: std::time::SystemTime,
    volume: u32,
    index: u64,
}
#[derive(Serialize, Deserialize)]
struct UnconfirmedExit {
    processes: Vec<(u32, u64)>,
}
const RUN_FILE: &str = "pipeline.json";
const UNCONFIRMED_FILE: &str = ".openunpack-exit-unconfirmed.json";

impl Coordinator {
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
            if pending.contains(&job.id) {
                job.status = "publishPending".into();
                job.message = "存在未完成的发布意图，请核对或清理临时结果。".into();
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
                    let confirmed =
                        paths::read_json::<UnconfirmedExit>(&marker).is_ok_and(|record| {
                            !record.processes.is_empty()
                                && record.processes.iter().all(|(pid, created)| {
                                    crate::worker::process_identity_exited(*pid, *created)
                                        .unwrap_or(false)
                                })
                        });
                    if confirmed {
                        fs::remove_file(marker).map_err(|_| "停止记录无法更新。")?;
                    } else {
                        job.status = "stopping".into();
                        job.message = "上次作业进程退出尚未确认，禁止访问或清理工作目录。".into();
                        repository.save_job(job)?;
                    }
                }
            }
        }
        let cache_usage = jobs
            .iter()
            .filter_map(|j| j.workspace.as_ref())
            .try_fold(0u64, |sum, w| {
                paths::tree_size(Path::new(w)).map(|b| sum.saturating_add(b))
            })?;
        let (notification, _) = broadcast::channel(32);
        let cancelled_jobs = jobs
            .iter()
            .filter(|job| job.status == "cancelled")
            .map(|job| job.id.clone())
            .collect();
        let core = Arc::new(Self {
            data: data.clone(),
            repository,
            vault: Vault::open(&data.join("passwords.vault")),
            worker: WorkerClient {
                executable: worker,
                engine,
            },
            jobs: Mutex::new(jobs),
            settings: Mutex::new(settings),
            running: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            stop_batch: AtomicBool::new(false),
            active_cancel: Mutex::new(None),
            cancelled_jobs: Mutex::new(cancelled_jobs),
            partial_requests: Mutex::new(HashSet::new()),
            node_retry_requests: Mutex::new(std::collections::HashMap::new()),
            notification,
            mutations: Mutex::new(()),
            cache_usage: AtomicU64::new(cache_usage),
        });
        core.apply_outbox();
        Ok(core)
    }
    pub fn vault(&self) -> Result<&Vault, String> {
        self.vault.as_ref().map_err(Clone::clone)
    }
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.notification.subscribe()
    }
    pub fn changed(&self) {
        let _ = self.notification.send(());
    }
    pub fn snapshot(&self) -> Result<AppSnapshot, String> {
        let jobs = self.jobs.lock().map_err(|_| lock_error())?.clone();
        let settings = self.settings.lock().map_err(|_| lock_error())?.clone();
        let (passwords, vault_error) = match self.vault() {
            Ok(v) => match v.views() {
                Ok(p) => (p, None),
                Err(e) => (vec![], Some(e)),
            },
            Err(e) => (vec![], Some(e)),
        };
        let recoverable_count = jobs.iter().filter(|j| recovering(&j.status)).count();
        let cache_used_bytes = self.cache_usage.load(Ordering::Acquire);
        Ok(AppSnapshot {
            jobs,
            passwords,
            settings,
            running: self.running.load(Ordering::Acquire),
            paused: self.paused.load(Ordering::Acquire),
            vault_error,
            data_directory: paths::path_string(&self.data),
            engine_available: self.worker.executable.is_file() && self.worker.engine.is_file(),
            recoverable_count,
            cache_used_bytes,
        })
    }
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
            };
            self.repository.save_job(&job)?;
            self.jobs
                .lock()
                .map_err(|_| lock_error())?
                .push(job.clone());
            added.push(job);
        }
        self.changed();
        Ok(added)
    }
    pub fn save_settings(&self, s: Settings) -> Result<Settings, String> {
        validate_settings(&s)?;
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        paths::atomic_json(&self.data.join("settings.json"), &s)?;
        *self.settings.lock().map_err(|_| lock_error())? = s.clone();
        self.changed();
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
            .filter(|j| terminal(&j.status) || recovering(&j.status))
            .cloned()
            .collect();
        jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        jobs.truncate(limit);
        Ok(jobs)
    }
    pub fn pause(&self) -> Result<(), String> {
        self.paused.store(true, Ordering::Release);
        self.changed();
        Ok(())
    }
    pub fn cancel(&self, id: Option<&str>) -> Result<(), String> {
        if id.is_none() {
            self.stop_batch.store(true, Ordering::Release);
            self.paused.store(false, Ordering::Release);
        }
        if let Some(id) = id {
            self.cancelled_jobs
                .lock()
                .map_err(|_| lock_error())?
                .insert(id.into());
        }
        if let Some((active, cancel)) = &*self.active_cancel.lock().map_err(|_| lock_error())? {
            if id.is_none() || id == Some(active) {
                cancel.store(true, Ordering::Release);
            }
        }
        if let Some(id) = id {
            let mut job = self.get_job(id)?;
            if job.status == "queued" {
                job.status = "cancelled".into();
                job.finished_at = Some(now());
                job.message = "尚未运行，已取消。".into();
                self.put(&job)?;
            }
        }
        self.changed();
        Ok(())
    }
    pub fn retry(&self, id: &str, partial: bool) -> Result<(), String> {
        self.enqueue_retry(id, partial, None)
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
        self.enqueue_retry(id, false, Some(node_id))
    }
    fn enqueue_retry(&self, id: &str, partial: bool, node_id: Option<&str>) -> Result<(), String> {
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        let mut job = self.get_job(id)?;
        if matches!(
            job.status.as_str(),
            "running" | "stopping" | "queued" | "completed" | "partial" | "publishPending"
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
            return Err("只有已保留成功外层和未完成内包的任务才能选择部分交付。".into());
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
        job.options = self.settings.lock().map_err(|_| lock_error())?.clone();
        job.status = "queued".into();
        job.stage = "queued".into();
        job.message = if partial {
            "本次允许保留未展开的内层包并标记部分交付。".into()
        } else {
            "使用当前设置和密码库进行新的尝试。".into()
        };
        job.finished_at = None;
        job.progress = 0.;
        self.put(&job)
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
        let job = self.get_job(id)?;
        if job.workspace.is_some() {
            return Err("请先在处理记录中清理保留的工作目录。".into());
        }
        self.repository.remove_job(id)?;
        self.jobs
            .lock()
            .map_err(|_| lock_error())?
            .retain(|j| j.id != id);
        self.changed();
        Ok(())
    }
    pub fn clear_completed(&self) -> Result<(), String> {
        let ids: Vec<_> = self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .filter(|j| {
                matches!(j.status.as_str(), "completed" | "partial" | "skipped")
                    && j.workspace.is_none()
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
        *self.jobs.lock().map_err(|_| lock_error())? = self.repository.jobs()?;
        self.changed();
        Ok(())
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
    fn get_job(&self, id: &str) -> Result<JobView, String> {
        self.jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .find(|j| j.id == id)
            .cloned()
            .ok_or("任务不存在。".into())
    }
    fn put(&self, job: &JobView) -> Result<(), String> {
        self.repository.save_job(job)?;
        let mut jobs = self.jobs.lock().map_err(|_| lock_error())?;
        if let Some(j) = jobs.iter_mut().find(|j| j.id == job.id) {
            *j = job.clone();
        }
        drop(jobs);
        self.changed();
        Ok(())
    }
    fn apply_outbox(&self) {
        if let (Ok(v), Ok(events)) = (self.vault(), self.repository.pending_success()) {
            for event in events {
                if v.record_success(&event).is_ok() {
                    let _ = self.repository.ack_success(&event.event_id);
                }
            }
        }
    }
    pub fn start(self: &Arc<Self>) -> Result<(), String> {
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
        self.paused.store(false, Ordering::Release);
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            self.changed();
            return Ok(());
        }
        self.stop_batch.store(false, Ordering::Release);
        let this = self.clone();
        tokio::spawn(async move {
            this.drive().await;
        });
        self.changed();
        Ok(())
    }
    async fn drive(self: Arc<Self>) {
        let _sleep_guard = paths::SleepBlockerGuard::acquire();
        let mut ran_any = false;
        loop {
            if self.stop_batch.load(Ordering::Acquire) {
                break;
            }
            if self.paused.load(Ordering::Acquire) {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                continue;
            }
            let mut pending: Vec<_> = match self.jobs.lock() {
                Ok(j) => j.iter().filter(|j| j.status == "queued").cloned().collect(),
                Err(_) => break,
            };
            if pending.is_empty() {
                break;
            }
            ran_any = true;
            if self.settings.lock().is_ok_and(|s| s.small_first) {
                pending.sort_by_key(|j| j.source.size);
            }
            let mut job = pending.remove(0);
            let cancel = Arc::new(AtomicBool::new(false));
            if let Ok(mut a) = self.active_cancel.lock() {
                *a = Some((job.id.clone(), cancel.clone()));
            }
            if self
                .cancelled_jobs
                .lock()
                .is_ok_and(|ids| ids.contains(&job.id))
            {
                cancel.store(true, Ordering::Release);
            }
            let partial = self
                .partial_requests
                .lock()
                .is_ok_and(|mut p| p.remove(&job.id));
            let selected = self
                .node_retry_requests
                .lock()
                .ok()
                .and_then(|mut p| p.remove(&job.id));
            job.attempts += 1;
            job.status = "running".into();
            job.stage = "checking".into();
            job.message = String::new();
            if self.put(&job).is_err() {
                break;
            }
            let result = self
                .run_job(&mut job, cancel.clone(), partial, selected.as_ref())
                .await;
            if let Err(code) = result {
                // An unresolved publication always takes precedence over a generic failure.
                let unresolved = self
                    .repository
                    .publications()
                    .map(|p| {
                        p.into_iter().any(|p| {
                            p.job_id == job.id
                                && !matches!(p.state.as_str(), "committed" | "abandoned")
                        })
                    })
                    .unwrap_or(true);
                job.status = if code == "WorkerExitUnconfirmed" {
                    "stopping"
                } else if unresolved {
                    "publishPending"
                } else if code == "Cancelled" || cancel.load(Ordering::Acquire) {
                    "cancelled"
                } else if code == "NeedsPassword" {
                    "needsPassword"
                } else if matches!(code.as_str(), "ResourceLimit" | "SharedResourceUnavailable") {
                    "waitingResource"
                } else {
                    "failed"
                }
                .into();
                job.message = message(&code);
                job.finished_at = Some(now());
                if self.put(&job).is_err() {
                    self.stop_batch.store(true, Ordering::Release);
                }
                if code == "WorkerExitUnconfirmed" || code == "StorageError" {
                    self.stop_batch.store(true, Ordering::Release);
                }
            }
            if let Ok(mut a) = self.active_cancel.lock() {
                *a = None;
            }
        }
        if let Ok(mut a) = self.active_cancel.lock() {
            *a = None;
        }
        if ran_any && self.settings.lock().is_ok_and(|s| s.completion_sound) {
            paths::play_completion_sound();
        }
        self.running.store(false, Ordering::Release);
        self.changed();
    }
    async fn checkpoint(&self, cancel: &AtomicBool) -> Result<(), String> {
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
    fn stage(&self, job: &mut JobView, stage: &str, text: &str) -> Result<(), String> {
        job.stage = stage.into();
        job.progress = 0.;
        job.message = text.into();
        self.put(job)
    }
    async fn request(
        self: &Arc<Self>,
        job: &mut JobView,
        request: WorkerRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<WorkerEvent, String> {
        let id = job.id.clone();
        let core = self.clone();
        let started = Instant::now();
        let stage = job.stage.clone();
        self.refresh_usage()?;
        let base = self.cache_usage.load(Ordering::Acquire);
        let cache_writing = matches!(stage.as_str(), "caching" | "extracting" | "nested");
        let progress = Arc::new(move |event: &WorkerEvent| {
            if cache_writing {
                core.cache_usage.store(
                    base.saturating_add(event.completed_bytes),
                    Ordering::Release,
                );
            }
            if let Ok(mut jobs) = core.jobs.lock() {
                if let Some(j) = jobs.iter_mut().find(|j| j.id == id) {
                    j.progress = if event.total_bytes > 0 {
                        (event.completed_bytes as f64 / event.total_bytes as f64).clamp(0., 1.)
                    } else {
                        0.
                    };
                }
            }
            core.changed();
        });
        let result = match self.worker.run(request, cancel, progress).await {
            Ok(result) => result,
            Err(code) => {
                if code == "WorkerExitUnconfirmed" {
                    job.status = "stopping".into();
                    job.message = message(&code);
                    if let Some(work) = &job.workspace {
                        let record = UnconfirmedExit {
                            processes: self.worker.unconfirmed_processes(),
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
        self.refresh_usage()?;
        *job.stage_times.entry(stage).or_default() += started.elapsed().as_millis() as u64;
        if result.kind == "completed" {
            Ok(result)
        } else {
            Err(if result.error_code.is_empty() {
                "IoError".into()
            } else {
                result.error_code
            })
        }
    }
    fn decide_route(&self, s: &Settings, source: &SourceGroup) -> Result<ExtractionRoute, String> {
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
            entry_count: 100,
            has_nested_archives: s.expand_nested,
            is_source_on_hdd: topology.source_is_hdd,
            is_target_on_hdd: topology.target_is_hdd,
            is_same_physical_disk: topology.is_same_physical_disk,
        };
        Ok(RouteDecider::decide(&probe, s, &target_dir, ssd_free))
    }
    fn workspace_parent(&self, _s: &Settings, _source: &SourceGroup, route: &ExtractionRoute) -> Result<PathBuf, String> {
        let parent = match route {
            ExtractionRoute::SsdCache { cache_workspace, .. } => cache_workspace.clone(),
            ExtractionRoute::Direct { target_dir, .. } => target_dir.join(".openunpack-work"),
        };
        paths::check_path(&parent)?;
        fs::create_dir_all(&parent).map_err(|_| "IoError")?;
        Ok(parent)
    }
    fn refresh_usage(&self) -> Result<(), String> {
        let work: Vec<_> = self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .iter()
            .filter_map(|j| j.workspace.clone())
            .collect();
        let bytes = work.iter().try_fold(0u64, |sum, w| {
            paths::tree_size(Path::new(w)).map(|b| sum.saturating_add(b))
        })?;
        self.cache_usage.store(bytes, Ordering::Release);
        Ok(())
    }
    fn available_budget(&self, s: &Settings, parent: &Path) -> Result<u64, String> {
        let free = paths::free_bytes(parent)
            .map_err(|_| "SharedResourceUnavailable")?
            .saturating_sub(s.min_free_bytes);
        if free == 0 {
            self.paused.store(true, Ordering::Release);
            self.stop_batch.store(true, Ordering::Release);
            return Err("SharedResourceUnavailable".into());
        }
        let quota = if s.cache_enabled {
            s.cache_quota_bytes
                .saturating_sub(paths::tree_size(parent)?)
        } else {
            u64::MAX
        };
        let budget = s.max_expanded_bytes.min(free).min(quota);
        if budget == 0 {
            Err("ResourceLimit".into())
        } else {
            Ok(budget)
        }
    }
    fn confirm_work_stopped(&self, job: &JobView) -> Result<(), String> {
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
    async fn run_job(
        self: &Arc<Self>,
        job: &mut JobView,
        cancel: Arc<AtomicBool>,
        allow_partial: bool,
        selected: Option<&HashSet<String>>,
    ) -> Result<(), String> {
        self.checkpoint(&cancel).await?;
        self.confirm_work_stopped(job)?;
        let s = job.options.clone();
        validate_settings(&s)?;
        let scan = scanner::scan(std::slice::from_ref(&job.source.path), false, &[])?;
        let source = scan
            .sources
            .into_iter()
            .find(|g| g.path.eq_ignore_ascii_case(&job.source.path))
            .ok_or("源压缩包已移动或不可读。")?;
        if source.missing_volume {
            return Err("MissingVolume".into());
        }
        let witnesses = source_witnesses(&source)?;
        let route = self.decide_route(&s, &source)?;
        let parent = self.workspace_parent(&s, &source, &route)?;
        // A run owns one immutable extraction configuration. Output/verification changes
        // can reuse sealed nodes; extraction-rule changes require a new generation.
        let mut resume = None;
        if let Some(old) = &job.workspace {
            let old_path = Path::new(old);
            paths::owned(old_path, &job.id, None)?;
            if old_path.join(RUN_FILE).is_file() {
                let saved: SavedRun = read_run(old_path)?;
                if saved.version != 1 || saved.job_id != job.id {
                    return Err("工作区清单不匹配；请显式清理后重试。".into());
                }
                if saved.source.volumes == source.volumes
                    && saved.witnesses == witnesses
                    && extraction_compatible(&saved.options, &s)
                {
                    resume = Some((old_path.to_path_buf(), saved));
                }
            }
        }
        let (work, mut state) = if let Some((work, mut saved)) = resume {
            if saved.expanded > s.max_expanded_bytes || saved.entries > s.max_entries {
                return Err("ResourceLimit".into());
            }
            validate_saved_paths(&work, &saved)?;
            self.stage(
                job,
                "checking",
                "正在复核已完成的工作区；通过后仅重试未完成内包。",
            )?;
            if !saved.input_files.is_empty() {
                let mut req = request("readback", &work.join("input"), &s);
                req.files = saved.input_files.clone();
                self.request(job, req, cancel.clone()).await?;
            }
            for sealed in &saved.sealed {
                self.checkpoint(&cancel).await?;
                let expected = retained_sealed_files(sealed, &saved.retired_files)?;
                validate_target_tree(&sealed.directory, &expected)?;
                let mut req = request("readback", &sealed.directory, &s);
                req.files = expected;
                self.request(job, req, cancel.clone()).await?;
            }
            // Do not re-run failed nodes for an explicit partial delivery. Their original
            // archive bytes remain in the parent's sealed manifest and final mapping.
            if !allow_partial {
                let mut retained = vec![];
                for (node, code) in saved.failed.drain(..) {
                    if selected.is_none_or(|ids| ids.contains(&node.id)) {
                        saved.pending.push_back(node);
                    } else {
                        retained.push((node, code));
                    }
                }
                saved.failed = retained;
            }
            job.nodes = saved.views.clone();
            self.put(job)?;
            (work, saved)
        } else {
            if allow_partial || selected.is_some() {
                return Err("没有可可信复用的已完成工作区；请先完整重试根任务。".into());
            }
            if let Some(old) = job.workspace.take() {
                paths::remove_owned(Path::new(&old), &job.id)?;
            }
            let work = parent.join(format!("{}-{}", job.id, job.attempts));
            paths::mark_owned(&work, &job.id, "")?;
            job.workspace = Some(paths::path_string(&work));
            job.nodes.clear();
            job.stage_times.clear();
            let should_cache_input = match &route {
                ExtractionRoute::SsdCache { score, reason, .. } => {
                    job.message = format!("SSD 智能加速激活 [评分: {}]: {}", score, reason);
                    s.cache_enabled && s.cache_input
                }
                ExtractionRoute::Direct { reason, .. } => {
                    job.message = format!("直通模式: {}", reason);
                    false
                }
            };
            self.put(job)?;
            let mut group = source.clone();
            let mut fingerprint = None;
            let mut input_files = vec![];
            if should_cache_input {
                self.stage(job, "caching", "正在将批准的源分卷顺序复制到缓存。")?;
                let budget = self.available_budget(&s, &parent)?;
                if source.size > budget {
                    return Err("ResourceLimit".into());
                }
                let input = work.join("input");
                fs::create_dir(&input).map_err(|_| "IoError")?;
                let files = source
                    .volumes
                    .iter()
                    .map(|p| {
                        let path = Path::new(p);
                        CopyFile {
                            source_path: p.clone(),
                            relative_path: path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into(),
                            size: fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX),
                            sha256: String::new(),
                            is_directory: false,
                        }
                    })
                    .collect();
                let mut req = request("copy", &input, &s);
                req.files = files;
                req.max_written_bytes = budget;
                let result = self.request(job, req, cancel.clone()).await?;
                // Bind ordered content, never filenames: a renamed archive/volume group
                // therefore still resolves to the same successful-password association.
                let mut hashes = vec![];
                for path in &source.volumes {
                    let name = Path::new(path)
                        .file_name()
                        .ok_or("UnsafePath")?
                        .to_string_lossy();
                    let f = result
                        .files
                        .iter()
                        .find(|f| f.relative_path.eq_ignore_ascii_case(&name))
                        .ok_or("ProtocolError")?;
                    hashes.push((f.size, f.sha256.clone()));
                    input_files.push(CopyFile {
                        source_path: paths::path_string(
                            &input.join(paths::relative(&f.relative_path)?),
                        ),
                        relative_path: f.relative_path.clone(),
                        size: f.size,
                        sha256: f.sha256.clone(),
                        is_directory: false,
                    });
                }
                fingerprint = Some(content_fingerprint(&hashes)?);
                group.path = paths::path_string(
                    &input.join(Path::new(&source.path).file_name().ok_or("UnsafePath")?),
                );
                group.volumes = source
                    .volumes
                    .iter()
                    .map(|p| {
                        paths::path_string(
                            &input.join(Path::new(p).file_name().unwrap_or_default()),
                        )
                    })
                    .collect();
                verify_witnesses(&source, &witnesses)?;
            }
            let root = NodeWork {
                id: uid(),
                group,
                prefix: PathBuf::new(),
                depth: 0,
                parent: None,
            };
            let state = SavedRun {
                version: 1,
                job_id: job.id.clone(),
                source: source.clone(),
                witnesses: witnesses.clone(),
                options: s.clone(),
                input_files,
                fingerprint,
                pending: VecDeque::from([root]),
                failed: vec![],
                sealed: vec![],
                final_files: vec![],
                events: vec![],
                expanded: 0,
                entries: 0,
                views: vec![],
                retired_files: HashSet::new(),
            };
            save_run(&work, &state)?;
            (work, state)
        };
        // Freeze the finite saved-password snapshot for this generation. Password edits
        // during decoding do not mutate its candidates or recorded revisions.
        let snapshot = unique_candidates(
            self.vault()
                .and_then(|v| {
                    v.candidates(
                        if s.password_group.is_empty() {
                            None
                        } else {
                            Some(&s.password_group)
                        },
                        state.fingerprint.as_deref(),
                    )
                })
                .unwrap_or_default(),
        );
        while let Some(node) = state.pending.front().cloned() {
            self.checkpoint(&cancel).await?;
            if allow_partial {
                state.pending.pop_front();
                state.failed.push((node.clone(), "Interrupted".into()));
                state
                    .views
                    .push(node_view(&node, "failed", "未完成的内包按部分交付保留。"));
                save_run(&work, &state)?;
                continue;
            }
            let node_index =
                if let Some(index) = state.views.iter().position(|view| view.id == node.id) {
                    index
                } else {
                    state.views.push(node_view(&node, "running", ""));
                    state.views.len() - 1
                };
            state.views[node_index].status = "running".into();
            state.views[node_index].stage = "extracting".into();
            state.views[node_index].message = String::new();
            job.nodes = state.views.clone();
            self.stage(
                job,
                if node.depth == 0 {
                    "extracting"
                } else {
                    "nested"
                },
                "正在解压；只尝试本次已保存密码快照。",
            )?;
            let node_fingerprint = if node.depth == 0 {
                state.fingerprint.clone()
            } else {
                fingerprint_for_group(&node.group, &state.final_files)?
            };
            let mut candidates = prioritize_snapshot(
                &snapshot,
                self.vault()
                    .and_then(|v| {
                        v.candidates(
                            if s.password_group.is_empty() {
                                None
                            } else {
                                Some(&s.password_group)
                            },
                            node_fingerprint.as_deref(),
                        )
                    })
                    .unwrap_or_default(),
            );
            if let Some(hint) = Path::new(&node.group.path)
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(scanner::extract_password_from_filename)
            {
                if let Some(pos) = candidates.iter().position(|c| c.secret == hint) {
                    let matched = candidates.remove(pos);
                    candidates.insert(0, matched);
                } else {
                    candidates.insert(
                        0,
                        Candidate {
                            id: "filename_hint".into(),
                            revision: 0,
                            label: "文件名提取".into(),
                            secret: hint,
                        },
                    );
                }
            }
            let mut decoded = None;
            let mut failure = "NeedsPassword".to_string();
            if node.group.missing_volume {
                failure = "MissingVolume".into();
            } else {
                let mut attempts_to_try: Vec<Option<Candidate>> = Vec::new();
                let mut skip_extraction = false;

                let mut probe_req = request("probe", &work, &s);
                probe_req.archive_path = node.group.path.clone();
                probe_req.allowed_volumes = node.group.volumes.clone();
                probe_req.candidate_passwords = candidates.iter().map(|c| c.secret.clone()).collect();

                match self.request(job, probe_req, cancel.clone()).await {
                    Ok(probe_event) => {
                        match probe_event.probe_status.as_deref() {
                            Some("NoPasswordNeeded") => {
                                attempts_to_try.push(None);
                            }
                            Some("Success") => {
                                if let Some(matched_str) = probe_event.matched_password {
                                    if let Some(c) = candidates.into_iter().find(|c| c.secret == matched_str) {
                                        attempts_to_try.push(Some(c));
                                    } else {
                                        attempts_to_try.push(Some(Candidate {
                                            id: "probe_matched".into(),
                                            revision: 0,
                                            label: "探测匹配".into(),
                                            secret: matched_str,
                                        }));
                                    }
                                } else {
                                    attempts_to_try.push(None);
                                }
                            }
                            Some("WrongPassword") => {
                                failure = "NeedsPassword".into();
                                skip_extraction = true;
                            }
                            Some("MissingVolume") => {
                                failure = "MissingVolume".into();
                                skip_extraction = true;
                            }
                            Some("Corrupt") => {
                                failure = "Corrupt".into();
                                skip_extraction = true;
                            }
                            Some("Unsupported") => {
                                failure = "Unsupported".into();
                                skip_extraction = true;
                            }
                            _ => {
                                attempts_to_try.push(None);
                                attempts_to_try.extend(candidates.into_iter().map(Some));
                            }
                        }
                    }
                    Err(code) if matches!(code.as_str(), "Cancelled" | "WorkerExitUnconfirmed") => {
                        return Err(code);
                    }
                    Err(_) => {
                        attempts_to_try.push(None);
                        attempts_to_try.extend(candidates.into_iter().map(Some));
                    }
                }

                if !skip_extraction {
                    for candidate_opt in attempts_to_try {
                        self.checkpoint(&cancel).await?;
                        let budget = self
                            .available_budget(&s, &parent)?
                            .min(s.max_expanded_bytes.saturating_sub(state.expanded));
                        if budget == 0 || state.entries >= s.max_entries {
                            return Err("ResourceLimit".into());
                        }
                        let dest = work.join(format!("node-{}-{}", node.id, uid()));
                        fs::create_dir(&dest).map_err(|_| "IoError")?;
                        let mut req = request("extract", &dest, &s);
                        req.archive_path = node.group.path.clone();
                        req.allowed_volumes = node.group.volumes.clone();
                        req.max_entries = s.max_entries - state.entries;
                        req.max_total_bytes = budget;
                        req.max_written_bytes = budget;
                        if let Some(c) = &candidate_opt {
                            req.password = Some(c.secret.clone());
                        }
                        state.views[node_index].attempts += 1;
                        job.nodes = state.views.clone();
                        save_run(&work, &state)?;
                        self.put(job)?;
                        match self.request(job, req, cancel.clone()).await {
                            Ok(event) => {
                                let mut event_success = None;
                                if event.password_used {
                                    if let Some(candidate) = &candidate_opt {
                                        state.views[node_index].password_label =
                                            Some(candidate.label.clone());
                                        if candidate.id != "filename_hint" && candidate.id != "probe_matched" {
                                            event_success = Some(SuccessEvent {
                                                event_id: uid(),
                                                password_id: candidate.id.clone(),
                                                password_revision: candidate.revision,
                                                fingerprint: node_fingerprint.clone(),
                                            });
                                        }
                                    }
                                }
                                decoded = Some((dest, event.files, event_success));
                                break;
                            }
                            Err(code)
                                if matches!(
                                    code.as_str(),
                                    "NeedsPassword" | "PasswordMismatchOrCorrupt"
                                ) =>
                            {
                                failure = code;
                                paths::remove_inside_owned(&dest, &work, &job.id)?;
                            }
                            Err(code)
                                if matches!(
                                    code.as_str(),
                                    "Corrupt" | "Unsupported" | "MissingVolume" | "UnsafePath"
                                ) =>
                            {
                                failure = code;
                                paths::remove_inside_owned(&dest, &work, &job.id)?;
                                break;
                            }
                            Err(code) => return Err(code),
                        }
                    }
                }
            }
            state.pending.pop_front();
            let Some((dest, files, event_success)) = decoded else {
                state.views[node_index].status = if matches!(
                    failure.as_str(),
                    "NeedsPassword" | "PasswordMismatchOrCorrupt"
                ) {
                    "needsPassword"
                } else {
                    "failed"
                }
                .into();
                state.views[node_index].message = message(&failure);
                state.failed.push((node.clone(), failure.clone()));
                save_run(&work, &state)?;
                job.nodes = state.views.clone();
                self.put(job)?;
                if node.depth == 0 {
                    return Err(if failure == "PasswordMismatchOrCorrupt" {
                        "NeedsPassword".into()
                    } else {
                        failure
                    });
                }
                continue;
            };
            for file in &files {
                paths::relative(&file.relative_path)?;
            }
            let bytes = files.iter().try_fold(0u64, |sum, file| {
                sum.checked_add(file.size).ok_or("ResourceLimit")
            })?;
            state.expanded = state.expanded.checked_add(bytes).ok_or("ResourceLimit")?;
            state.entries = state
                .entries
                .checked_add(files.len() as u64)
                .ok_or("ResourceLimit")?;
            if state.expanded > s.max_expanded_bytes || state.entries > s.max_entries {
                return Err("ResourceLimit".into());
            }
            if node.depth > 0 {
                let consumed: HashSet<_> = node
                    .group
                    .volumes
                    .iter()
                    .map(|p| p.to_lowercase())
                    .collect();
                state
                    .final_files
                    .retain(|f| !consumed.contains(&f.source_path.to_lowercase()));
            }
            for file in &files {
                let relative = paths::relative(&file.relative_path)?;
                state.final_files.push(CopyFile {
                    source_path: paths::path_string(&dest.join(&relative)),
                    relative_path: paths::path_string(&node.prefix.join(relative)),
                    size: file.size,
                    sha256: file.sha256.clone(),
                    is_directory: file.is_directory,
                });
            }
            if let Some(event) = event_success {
                state.events.push(event);
            }
            state.sealed.push(SealedNode {
                node: node.clone(),
                directory: dest.clone(),
                files,
            });
            state.views[node_index].status = "completed".into();
            state.views[node_index].stage = "finished".into();
            state.views[node_index].progress = 1.;
            if s.expand_nested {
                let nested = scanner::scan(&[paths::path_string(&dest)], true, &[])?;
                if node.depth + 1 < s.max_depth {
                    for child in nested.sources {
                        let archive = Path::new(&child.path);
                        let folder = archive
                            .parent()
                            .ok_or("UnsafePath")?
                            .strip_prefix(&dest)
                            .map_err(|_| "UnsafePath")?;
                        let base = node
                            .prefix
                            .join(folder)
                            .join(scanner::output_name(&child.path));
                        let reserved: Vec<_> = state
                            .pending
                            .iter()
                            .chain(state.failed.iter().map(|(node, _)| node))
                            .map(|n| n.prefix.clone())
                            .collect();
                        let prefix = unique_prefix(base, &state.final_files, &reserved);
                        state.pending.push_back(NodeWork {
                            id: uid(),
                            group: child,
                            prefix,
                            depth: node.depth + 1,
                            parent: Some(node.id.clone()),
                        });
                    }
                } else if !nested.sources.is_empty() {
                    state.views[node_index].message =
                        "已达到展开层数限制，更深内包按原文件保留。".into();
                }
            }
            // Once a child is sealed, its input archive can be reconstructed from
            // the original source and is no longer part of the final mapping.
            // Keep the immutable parent manifest and record the verified successor
            // separately before releasing these intermediate bytes.
            if node.depth > 0 {
                for volume in &node.group.volumes {
                    state.retired_files.insert(volume.to_lowercase());
                }
            }
            if node.depth == 0 && !state.input_files.is_empty() {
                state.input_files.clear();
            }
            save_run(&work, &state)?;
            if node.depth > 0 {
                for volume in &node.group.volumes {
                    let _ = paths::remove_file_inside_owned(Path::new(volume), &work, &job.id);
                }
            }
            if node.depth == 0 && work.join("input").exists() {
                let _ = paths::remove_inside_owned(&work.join("input"), &work, &job.id);
            }
            self.refresh_usage()?;
            job.nodes = state.views.clone();
            self.put(job)?;
        }
        if !state.failed.is_empty() && !allow_partial {
            return Err(
                if state.failed.iter().any(|(_, code)| {
                    matches!(code.as_str(), "NeedsPassword" | "PasswordMismatchOrCorrupt")
                }) {
                    "NeedsPassword".into()
                } else {
                    state.failed[0].1.clone()
                },
            );
        }
        let partial = !state.failed.is_empty();
        let mut final_files = state.final_files.clone();
        let events = state.events.clone();
        verify_witnesses(&source, &witnesses)?;
        self.checkpoint(&cancel).await?;
        self.stage(
            job,
            "planning",
            "正在生成最终文件映射；同名目录按策略处理。",
        )?;
        if s.unwrap_single_root {
            strip_single_root_wrapper(&mut final_files);
        }
        validate_manifest(&final_files)?;
        let output = output_root(&s, &source)?;
        paths::check_path(&output)?;
        fs::create_dir_all(&output).map_err(|_| "IoError")?;
        let basename = scanner::output_name(&source.path);
        let mut final_path = output.join(&basename);
        if final_path.exists() {
            if s.conflict_policy == "skip" {
                job.status = "skipped".into();
                job.message = "同名目标已存在，按设置跳过；原包保留。".into();
                job.finished_at = Some(now());
                paths::remove_owned(&work, &job.id)?;
                job.workspace = None;
                return self.put(job);
            }
            final_path = unique_target(&output, &basename)?;
        }
        let pub_id = uid();
        let temp = output.join(format!(".openunpack-{pub_id}.tmp"));
        let hash = paths::digest(&final_files)?;
        let mut publication = Publication {
            id: pub_id.clone(),
            job_id: job.id.clone(),
            temporary_path: paths::path_string(&temp),
            final_path: paths::path_string(&final_path),
            manifest_hash: hash.clone(),
            state: "planned".into(),
            files: final_files.clone(),
            verification: s.verification.clone(),
        };
        // Durable intent precedes temporary target creation and all writeback.
        paths::atomic_json(
            &work.join("publication.json"),
            &Journal {
                publication: publication.clone(),
                events: events.clone(),
                partial,
            },
        )?;
        self.repository.save_publication(&publication)?;
        // Ordinary extraction already lives on the destination volume. When its
        // one sealed directory is the final tree, publish it without a full second
        // copy. Nested/structure-changing mappings still use the verified copier.
        let direct = !s.cache_enabled
            && paths::same_volume(&work, &output)?
            && state.sealed.len() == 1
            && state.views.len() == 1
            && direct_manifest(&state.sealed[0], &final_files);
        if direct {
            let directory = &state.sealed[0].directory;
            paths::tag_existing(directory, &pub_id, &hash)?;
            fs::rename(directory, &temp).map_err(|_| "目标临时目录提交失败，工作区已保留。")?;
        } else {
            paths::mark_owned(&temp, &pub_id, &hash)?;
            self.stage(job, "copying", "正在顺序写回目标盘，同时对比解压流摘要。")?;
            let mut req = request("copy", &temp, &s);
            req.files = final_files.clone();
            req.max_written_bytes = s.max_expanded_bytes;
            self.request(job, req, cancel.clone()).await?;
        }
        let source_cleanup_allowed = !partial
            && s.source_cleanup != "keep"
            && !final_files.is_empty()
            && s.output_exclusions.is_empty()
            && !state.views.iter().any(|n| n.message.contains("层数限制"));
        let readback = s.verification == "strict" || source_cleanup_allowed;
        if readback {
            self.checkpoint(&cancel).await?;
            self.stage(
                job,
                "verifying",
                "正在完整回读目标临时文件，耗时取决于目标盘。",
            )?;
            let mut req = request("readback", &temp, &s);
            req.files = target_files(&final_files, &temp)?;
            self.request(job, req, cancel.clone()).await?;
        }
        publication.state = "ready".into();
        self.repository.save_publication(&publication)?;
        self.checkpoint(&cancel).await?;
        self.stage(job, "publishing", "正在提交已验证的最终目录。")?;
        paths::owned(&temp, &pub_id, Some(&hash))?;
        paths::check_path(&final_path)?;
        if final_path.exists() {
            if s.conflict_policy == "skip" {
                paths::remove_owned(&temp, &pub_id)?;
                publication.state = "abandoned".into();
                self.repository.save_publication(&publication)?;
                job.status = "skipped".into();
                job.message = "发布时发现同名目标，按策略跳过；原包保留。".into();
                job.finished_at = Some(now());
                paths::remove_owned(&work, &job.id)?;
                job.workspace = None;
                self.put(job)?;
                return self.refresh_usage();
            }
            final_path = unique_target(&output, &basename)?;
            publication.final_path = paths::path_string(&final_path);
            self.repository.save_publication(&publication)?;
            paths::atomic_json(
                &work.join("publication.json"),
                &Journal {
                    publication: publication.clone(),
                    events: events.clone(),
                    partial,
                },
            )?;
        }
        paths::publish_directory(&temp, &final_path)?;
        // Once renamed, cancellation cannot turn a delivered result into a cancelled task.
        publication.state = "committed".into();
        job.output_path = Some(paths::path_string(&final_path));
        job.status = if partial { "partial" } else { "completed" }.into();
        job.verification_scope = if readback {
            "archiveCrcAndStreamSha256+targetReadback"
        } else if direct {
            "archiveCrcAndStreamSha256"
        } else {
            "archiveCrcAndStreamSha256+sourceStreamSha256"
        }
        .into();
        job.stage = "finished".into();
        job.progress = 1.;
        job.finished_at = Some(now());
        let mut msg = if partial {
            "部分交付：未通过的内层包已保留；原始压缩包未删除。".to_string()
        } else {
            "解压与写回验证完成；原始压缩包已保留。".to_string()
        };
        self.repository
            .commit_publication(&publication, job, &events)?;

        // Safe source archive cleanup (only when 100% completed, non-empty, no exclusions, witness verified, and target tree verified)
        if source_cleanup_allowed {
            if verify_witnesses(&source, &witnesses).is_ok()
                && validate_target_tree(&final_path, &final_files).is_ok()
            {
                let mut cleanup_ok = true;
                for vol in &source.volumes {
                    let vp = Path::new(vol);
                    let res = if s.source_cleanup == "recycle" {
                        paths::move_to_recycle_bin(vp)
                    } else {
                        paths::check_path(vp).and_then(|_| {
                            fs::remove_file(vp).map_err(|_| "删除源文件失败".to_string())
                        })
                    };
                    if res.is_err() {
                        cleanup_ok = false;
                    }
                }
                if cleanup_ok {
                    msg = if s.source_cleanup == "recycle" {
                        "解压与写回验证完成；源压缩包已安全移入回收站。".to_string()
                    } else {
                        "解压与写回验证完成；源压缩包已清理。".to_string()
                    };
                } else {
                    msg = "解压与写回验证完成；部分源分卷被占用未能自动清理。".to_string();
                }
            }
        }
        job.message = msg;
        self.put(job)?;
        self.apply_outbox();
        match paths::remove_owned(&work, &job.id) {
            Ok(()) => job.workspace = None,
            Err(_) => {
                job.status = "cleanupPending".into();
                job.message = "文件已交付，缓存清理待重试。".into();
            }
        }
        self.put(job)?;
        self.refresh_usage()
    }
    pub async fn recover(self: &Arc<Self>, id: &str, action: &str) -> Result<(), String> {
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("请先暂停并结束当前任务，再执行恢复操作。".into());
        }
        let result = self.recover_inner(id, action).await;
        if let Ok(mut active) = self.active_cancel.lock() {
            *active = None;
        }
        self.running.store(false, Ordering::Release);
        self.changed();
        result
    }
    async fn recover_inner(self: &Arc<Self>, id: &str, action: &str) -> Result<(), String> {
        let mut job = self.get_job(id)?;
        self.confirm_work_stopped(&job)?;
        let publication = self
            .repository
            .publications()?
            .into_iter()
            .find(|p| p.job_id == id && !matches!(p.state.as_str(), "committed" | "abandoned"));
        if matches!(action, "retry" | "writeback") {
            if let Some(mut p) = publication {
                if Path::new(&p.final_path).exists() {
                    return Err("最终目录已存在，请先回读核对发布；不能重复写回。".into());
                }
                if paths::digest(&p.files)? != p.manifest_hash {
                    return Err("发布清单摘要不匹配，已停止。".into());
                }
                let work = PathBuf::from(
                    job.workspace
                        .as_ref()
                        .ok_or("没有已验证工作区，不能重试写回。")?,
                );
                paths::owned(&work, id, None)?;
                let saved: SavedRun = read_run(&work)?;
                validate_saved_paths(&work, &saved)?;
                verify_witnesses(&saved.source, &saved.witnesses)?;
                let temp = PathBuf::from(&p.temporary_path);
                let missing: Vec<_> = saved
                    .sealed
                    .iter()
                    .filter(|sealed| !sealed.directory.exists())
                    .collect();
                if !missing.is_empty() {
                    if missing.len() != 1
                        || saved.sealed.len() != 1
                        || !direct_manifest(missing[0], &p.files)
                    {
                        return Err("已验证文件位置不完整，请核对发布或显式清理后完整重试。".into());
                    }
                    paths::owned(&temp, &p.id, Some(&p.manifest_hash))?;
                    validate_target_tree(&temp, &p.files)?;
                    let cancel = Arc::new(AtomicBool::new(false));
                    *self.active_cancel.lock().map_err(|_| lock_error())? =
                        Some((id.into(), cancel.clone()));
                    self.stop_batch.store(false, Ordering::Release);
                    self.paused.store(false, Ordering::Release);
                    self.stage(
                        &mut job,
                        "checking",
                        "正在复核待发布目录，保留已验证结果供重试。",
                    )?;
                    let mut req = request("readback", &temp, &job.options);
                    req.files = target_files(&p.files, &temp)?;
                    self.request(&mut job, req, cancel).await?;
                    fs::remove_file(temp.join(paths::OWNER_FILE)).map_err(|_| "IoError")?;
                    paths::check_path(&missing[0].directory)?;
                    fs::rename(&temp, &missing[0].directory).map_err(|_| "IoError")?;
                } else if temp.exists() {
                    paths::owned(&temp, &p.id, Some(&p.manifest_hash))?;
                    paths::remove_owned(&temp, &p.id)?;
                }
                for sealed in &saved.sealed {
                    if sealed.directory.join(paths::OWNER_FILE).exists() {
                        paths::owned(&sealed.directory, &p.id, Some(&p.manifest_hash))?;
                        fs::remove_file(sealed.directory.join(paths::OWNER_FILE))
                            .map_err(|_| "IoError")?;
                    }
                }
                p.state = "abandoned".into();
                self.repository.save_publication(&p)?;
                job.status = "failed".into();
                job.stage = "checking".into();
                self.put(&job)?;
            }
            return self.retry(id, false);
        }
        if action == "cleanup" {
            if let Some(mut p) = publication {
                if Path::new(&p.final_path).exists() {
                    return Err("已有最终目录，必须先核对，不能清理。".into());
                }
                let temp = Path::new(&p.temporary_path);
                if temp.exists() {
                    paths::owned(temp, &p.id, Some(&p.manifest_hash))?;
                    paths::remove_owned(temp, &p.id)?;
                }
                p.state = "abandoned".into();
                self.repository.save_publication(&p)?;
            }
            if let Some(work) = &job.workspace {
                if Path::new(work).exists() {
                    paths::remove_owned(Path::new(work), id)?;
                }
            }
            job.workspace = None;
            job.status = if job.output_path.is_some() {
                if job
                    .nodes
                    .iter()
                    .any(|n| matches!(n.status.as_str(), "failed" | "needsPassword"))
                {
                    "partial"
                } else {
                    "completed"
                }
            } else {
                "cancelled"
            }
            .into();
            job.message = "本应用的临时工作目录已清理；原始文件保留。".into();
            job.finished_at = Some(now());
            self.put(&job)?;
            return self.refresh_usage();
        }
        if action != "verify" {
            return Err("恢复操作无效。".into());
        }
        let mut p = publication.ok_or("没有待核对的发布意图。")?;
        if paths::digest(&p.files)? != p.manifest_hash {
            return Err("发布清单摘要不匹配，已停止。".into());
        }
        let final_path = PathBuf::from(&p.final_path);
        let temp = PathBuf::from(&p.temporary_path);
        if final_path.exists() && temp.exists() {
            return Err("临时和最终目录同时存在，需要人工检查。".into());
        }
        let target = if final_path.exists() {
            final_path.clone()
        } else {
            temp.clone()
        };
        paths::owned(&target, &p.id, Some(&p.manifest_hash))?;
        validate_target_tree(&target, &p.files)?;
        let work = PathBuf::from(
            job.workspace
                .as_ref()
                .ok_or("缺少发布事务记录，不能猜测成功。")?,
        );
        paths::owned(&work, id, None)?;
        let journal: Journal = paths::read_json(&work.join("publication.json"))?;
        if journal.publication.id != p.id || journal.publication.manifest_hash != p.manifest_hash {
            return Err("发布事务记录不匹配。".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.active_cancel.lock().map_err(|_| lock_error())? = Some((id.into(), cancel.clone()));
        self.stop_batch.store(false, Ordering::Release);
        self.paused.store(false, Ordering::Release);
        job.status = "running".into();
        self.stage(&mut job, "verifying", "正在回读并核对可恢复的目标文件。")?;
        let mut req = request("readback", &target, &job.options);
        req.files = target_files(&p.files, &target)?;
        let outcome = self.request(&mut job, req, cancel).await;
        *self.active_cancel.lock().map_err(|_| lock_error())? = None;
        if let Err(e) = outcome {
            job.status = if e == "WorkerExitUnconfirmed" {
                "stopping"
            } else {
                "publishPending"
            }
            .into();
            job.message = message(&e);
            self.put(&job)?;
            return Err(message(&e));
        }
        if target == temp {
            paths::check_path(&final_path)?;
            paths::publish_directory(&temp, &final_path)?;
        }
        p.state = "committed".into();
        job.output_path = Some(paths::path_string(&final_path));
        job.status = if journal.partial {
            "partial"
        } else {
            "completed"
        }
        .into();
        job.verification_scope = "targetReadback".into();
        job.stage = "finished".into();
        job.progress = 1.;
        job.message = "恢复核对与交付提交完成；原包保留。".into();
        job.finished_at = Some(now());
        self.repository
            .commit_publication(&p, &job, &journal.events)?;
        self.apply_outbox();
        if paths::remove_owned(&work, id).is_ok() {
            job.workspace = None;
        } else {
            job.status = "cleanupPending".into();
        }
        self.put(&job)?;
        self.refresh_usage()
    }
}

fn request(operation: &str, dest: &Path, s: &Settings) -> WorkerRequest {
    WorkerRequest {
        protocol_version: 1,
        request_id: uid(),
        operation: operation.into(),
        archive_path: String::new(),
        destination_directory: paths::path_string(dest),
        password: None,
        allowed_volumes: vec![],
        max_entries: s.max_entries,
        max_total_bytes: s.max_expanded_bytes,
        max_written_bytes: s.max_expanded_bytes,
        min_free_bytes: s.min_free_bytes,
        files: vec![],
        verification: s.verification.clone(),
        exclusions: if operation == "extract" {
            s.output_exclusions.clone()
        } else {
            vec![]
        },
        candidate_passwords: vec![],
    }
}
fn output_root(s: &Settings, source: &SourceGroup) -> Result<PathBuf, String> {
    if s.output_root.is_empty() {
        Path::new(&source.path)
            .parent()
            .map(Path::to_path_buf)
            .ok_or("UnsafePath".into())
    } else {
        Ok(PathBuf::from(&s.output_root))
    }
}
fn read_run(work: &Path) -> Result<SavedRun, String> {
    paths::read_json_limited(&work.join(RUN_FILE), 512 * 1024 * 1024)
}
fn save_run(work: &Path, state: &SavedRun) -> Result<(), String> {
    paths::atomic_json(&work.join(RUN_FILE), state)
}
fn extraction_compatible(old: &Settings, new: &Settings) -> bool {
    old.cache_enabled == new.cache_enabled
        && old.cache_root == new.cache_root
        && old.cache_input == new.cache_input
        && old.expand_nested == new.expand_nested
        && old.max_depth == new.max_depth
        && old.output_exclusions == new.output_exclusions
}
fn node_view(node: &NodeWork, status: &str, message: &str) -> NodeView {
    NodeView {
        id: node.id.clone(),
        parent_id: node.parent.clone(),
        name: node.group.display_name.clone(),
        depth: node.depth,
        status: status.into(),
        stage: "extracting".into(),
        progress: 0.,
        attempts: 0,
        message: message.into(),
        password_label: None,
    }
}
fn content_fingerprint(hashes: &[(u64, String)]) -> Result<String, String> {
    if hashes.is_empty()
        || hashes
            .iter()
            .any(|(_, h)| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("ProtocolError".into());
    }
    paths::digest(&("OpenUnpack/content-group/v1", hashes))
}
fn fingerprint_for_group(
    group: &SourceGroup,
    files: &[CopyFile],
) -> Result<Option<String>, String> {
    let hashes: Option<Vec<_>> = group
        .volumes
        .iter()
        .map(|p| {
            files
                .iter()
                .find(|f| f.source_path.eq_ignore_ascii_case(p) && !f.is_directory)
                .map(|f| (f.size, f.sha256.clone()))
        })
        .collect();
    hashes
        .map(|hashes| content_fingerprint(&hashes))
        .transpose()
}
fn unique_candidates(candidates: Vec<Candidate>) -> Vec<Candidate> {
    use sha2::{Digest, Sha256};
    let salt = zeroize::Zeroizing::new(rand::random::<[u8; 32]>());
    let mut indexes: std::collections::HashMap<[u8; 32], Vec<usize>> =
        std::collections::HashMap::new();
    let mut result: Vec<Candidate> = vec![];
    for candidate in candidates {
        let mut hash = Sha256::new();
        hash.update(salt.as_ref());
        hash.update(candidate.secret.as_bytes());
        let key: [u8; 32] = hash.finalize().into();
        let bucket = indexes.entry(key).or_default();
        if !bucket
            .iter()
            .any(|index| result[*index].secret == candidate.secret)
        {
            bucket.push(result.len());
            result.push(candidate);
        }
    }
    result
}
fn prioritize_snapshot(snapshot: &[Candidate], priorities: Vec<Candidate>) -> Vec<Candidate> {
    let priority: std::collections::HashMap<_, _> = priorities
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.id.as_str(), index))
        .collect();
    let mut result = snapshot.to_vec();
    result.sort_by_key(|c| priority.get(c.id.as_str()).copied().unwrap_or(usize::MAX));
    result
}
fn sealed_copy_files(sealed: &SealedNode) -> Result<Vec<CopyFile>, String> {
    sealed
        .files
        .iter()
        .map(|f| {
            Ok(CopyFile {
                source_path: paths::path_string(
                    &sealed.directory.join(paths::relative(&f.relative_path)?),
                ),
                relative_path: f.relative_path.clone(),
                size: f.size,
                sha256: f.sha256.clone(),
                is_directory: f.is_directory,
            })
        })
        .collect()
}
fn retained_sealed_files(
    sealed: &SealedNode,
    retired: &HashSet<String>,
) -> Result<Vec<CopyFile>, String> {
    let original = sealed_copy_files(sealed)?;
    let mut files: Vec<_> = original
        .iter()
        .filter(|file| {
            !retired.contains(&file.source_path.to_lowercase())
                || Path::new(&file.source_path).exists()
        })
        .cloned()
        .collect();
    let mut existing: HashSet<_> = files
        .iter()
        .map(|file| file.relative_path.replace('\\', "/").to_lowercase())
        .collect();
    // Some ZIPs omit explicit parent-directory entries. Empty implicit directories
    // left after retiring an inner archive still belong to the immutable parent tree.
    for file in &original {
        for parent in paths::relative(&file.relative_path)?
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            let path = sealed.directory.join(parent);
            let relative = paths::path_string(parent);
            let key = relative.replace('\\', "/").to_lowercase();
            if path.is_dir() && existing.insert(key) {
                files.push(CopyFile {
                    source_path: paths::path_string(&path),
                    relative_path: relative,
                    size: 0,
                    sha256: String::new(),
                    is_directory: true,
                });
            }
        }
    }
    Ok(files)
}
fn direct_manifest(sealed: &SealedNode, files: &[CopyFile]) -> bool {
    sealed_copy_files(sealed)
        .ok()
        .is_some_and(|original| paths::digest(&original).ok() == paths::digest(&files).ok())
}
fn validate_saved_paths(work: &Path, state: &SavedRun) -> Result<(), String> {
    let inside = |path: &Path| -> Result<(), String> {
        paths::check_path(path)?;
        let r = path.strip_prefix(work).map_err(|_| "UnsafePath")?;
        paths::relative(&paths::path_string(r))?;
        Ok(())
    };
    for sealed in &state.sealed {
        inside(&sealed.directory)?;
        validate_manifest(&sealed_copy_files(sealed)?)?;
    }
    for file in state.final_files.iter().chain(&state.input_files) {
        inside(Path::new(&file.source_path))?;
        paths::relative(&file.relative_path)?;
    }
    validate_manifest(&state.final_files)?;
    for retired in &state.retired_files {
        if !state.sealed.iter().any(|sealed| {
            sealed.node.depth > 0
                && sealed
                    .node
                    .group
                    .volumes
                    .iter()
                    .any(|volume| volume.to_lowercase() == *retired)
        }) || state
            .final_files
            .iter()
            .any(|file| file.source_path.to_lowercase() == *retired)
        {
            return Err("工作区中间文件释放记录不匹配，已停止。".into());
        }
    }
    for node in state
        .pending
        .iter()
        .chain(state.failed.iter().map(|(node, _)| node))
    {
        if !node.prefix.as_os_str().is_empty() {
            paths::relative(&paths::path_string(&node.prefix))?;
        }
        if node.depth > 0 {
            for volume in &node.group.volumes {
                inside(Path::new(volume))?;
            }
        } else if node.group.volumes != state.source.volumes
            && node.group.volumes
                != state
                    .input_files
                    .iter()
                    .map(|f| f.source_path.clone())
                    .collect::<Vec<_>>()
        {
            return Err("UnsafePath".into());
        }
    }
    Ok(())
}
pub fn validate_settings(s: &Settings) -> Result<(), String> {
    if !matches!(s.conflict_policy.as_str(), "rename" | "skip")
        || !matches!(s.verification.as_str(), "standard" | "strict")
        || !matches!(s.theme.as_str(), "light" | "dark" | "system")
        || !matches!(s.source_cleanup.as_str(), "keep" | "recycle" | "delete")
    {
        return Err("设置选项无效。".into());
    }
    if s.max_depth == 0
        || s.max_depth > 8
        || s.max_entries == 0
        || s.max_entries > 100000
        || s.max_expanded_bytes == 0
        || s.cache_quota_bytes == 0
        || s.history_limit == 0
        || s.history_limit > 10000
    {
        return Err("资源限制超出支持范围。".into());
    }
    if s.cache_enabled && s.cache_root.trim().is_empty() {
        return Err("启用缓存时必须选择缓存目录。".into());
    }
    for p in [&s.cache_root, &s.output_root] {
        if !p.is_empty() {
            paths::check_path(Path::new(p))?;
        }
    }
    if s.input_exclusions.len() > 100
        || s.output_exclusions.len() > 100
        || s.input_exclusions
            .iter()
            .chain(&s.output_exclusions)
            .any(|p| p.len() > 256)
    {
        return Err("排除规则过多或过长。".into());
    }
    Ok(())
}
fn source_witnesses(source: &SourceGroup) -> Result<Vec<SourceWitness>, String> {
    use std::os::windows::io::AsRawHandle;
    source
        .volumes
        .iter()
        .map(|p| {
            paths::check_path(Path::new(p))?;
            let file = fs::File::open(p).map_err(|_| "IoError")?;
            let m = file.metadata().map_err(|_| "IoError")?;
            let mut info: windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION =
                unsafe { std::mem::zeroed() };
            if unsafe {
                windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(
                    file.as_raw_handle() as _,
                    &mut info,
                )
            } == 0
            {
                return Err("IoError".into());
            }
            Ok(SourceWitness {
                size: m.len(),
                modified: m.modified().map_err(|_| "IoError")?,
                volume: info.dwVolumeSerialNumber,
                index: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
            })
        })
        .collect()
}
fn verify_witnesses(source: &SourceGroup, expected: &[SourceWitness]) -> Result<(), String> {
    if source_witnesses(source)? != expected {
        Err("源压缩包在运行期间发生变化，结果尚未发布。".into())
    } else {
        Ok(())
    }
}
fn unique_prefix(base: PathBuf, files: &[CopyFile], reserved: &[PathBuf]) -> PathBuf {
    let mut result = base.clone();
    let mut n = 2;
    loop {
        let key = paths::path_string(&result)
            .replace('\\', "/")
            .to_lowercase();
        let collision = files.iter().any(|f| {
            let p = f.relative_path.replace('\\', "/").to_lowercase();
            p == key || p.starts_with(&(key.clone() + "/"))
        }) || reserved.iter().any(|r| {
            let p = paths::path_string(r).replace('\\', "/").to_lowercase();
            p == key || p.starts_with(&(key.clone() + "/")) || key.starts_with(&(p + "/"))
        });
        if !collision {
            return result;
        }
        let name = base.file_name().unwrap_or_default().to_string_lossy();
        result = base.with_file_name(format!("{name} ({n})"));
        n += 1;
    }
}
fn unique_target(parent: &Path, name: &str) -> Result<PathBuf, String> {
    for n in 2..10000 {
        let p = parent.join(format!("{name} ({n})"));
        if !p.exists() {
            return Ok(p);
        }
    }
    Err("同名目标过多，请选择另一输出目录。".into())
}

fn strip_single_root_wrapper(files: &mut Vec<CopyFile>) {
    // Collect non-empty top-level directory components
    let mut top_dirs = HashSet::new();
    let mut has_root_files = false;

    for f in files.iter() {
        let norm = f.relative_path.replace('\\', "/");
        let mut comps = norm.split('/').filter(|s| !s.is_empty());
        if let Some(first) = comps.next() {
            if comps.next().is_none() && !f.is_directory {
                // There is a file directly at the root
                has_root_files = true;
                break;
            } else {
                top_dirs.insert(first.to_string());
            }
        }
    }

    // Only unwrap if there are no root files and exactly one top-level folder
    if !has_root_files && top_dirs.len() == 1 {
        let root_name = top_dirs.into_iter().next().unwrap();
        let prefix = format!("{}/", root_name);
        let mut new_files = Vec::with_capacity(files.len());

        for f in files.iter() {
            let norm = f.relative_path.replace('\\', "/");
            if norm == root_name && f.is_directory {
                // Skip the top redundant container folder entry itself
                continue;
            }
            if let Some(stripped) = norm.strip_prefix(&prefix) {
                if !stripped.is_empty() {
                    let mut updated = f.clone();
                    updated.relative_path = stripped.replace('/', "\\");
                    new_files.push(updated);
                }
            } else {
                new_files.push(f.clone());
            }
        }
        if !new_files.is_empty() {
            *files = new_files;
        }
    }
}
fn validate_manifest(files: &[CopyFile]) -> Result<(), String> {
    let mut keys = std::collections::HashMap::with_capacity(files.len());
    for file in files {
        paths::relative(&file.relative_path)?;
        let key = file.relative_path.replace('\\', "/").to_lowercase();
        if keys.insert(key, file.is_directory).is_some() {
            return Err("内层合并出现重复路径，已停止发布。".into());
        }
        if !file.is_directory
            && (file.sha256.len() != 64 || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("解压摘要清单无效。".into());
        }
    }
    for key in keys.keys() {
        for (index, byte) in key.bytes().enumerate() {
            if byte == b'/' && keys.get(&key[..index]) == Some(&false) {
                return Err("文件与目录路径冲突，已停止发布。".into());
            }
        }
    }
    Ok(())
}
fn target_files(files: &[CopyFile], target: &Path) -> Result<Vec<CopyFile>, String> {
    files
        .iter()
        .map(|f| {
            let mut copy = f.clone();
            copy.source_path = paths::path_string(&target.join(paths::relative(&f.relative_path)?));
            Ok(copy)
        })
        .collect()
}
fn validate_target_tree(target: &Path, files: &[CopyFile]) -> Result<(), String> {
    paths::check_path(target)?;
    validate_manifest(files)?;
    let mut expected = HashSet::new();
    for file in files {
        let relative = paths::relative(&file.relative_path)?;
        let path = target.join(&relative);
        paths::check_path(&path)?;
        let meta = fs::metadata(&path).map_err(|_| "目标清单中的文件缺失，已停止恢复。")?;
        if meta.is_dir() != file.is_directory || (!file.is_directory && meta.len() != file.size) {
            return Err("目标文件类型或大小与清单不符，已停止恢复。".into());
        }
        for ancestor in relative.ancestors().filter(|p| !p.as_os_str().is_empty()) {
            expected.insert(
                paths::path_string(ancestor)
                    .replace('\\', "/")
                    .to_lowercase(),
            );
        }
    }
    for entry in walkdir::WalkDir::new(target).follow_links(false) {
        let entry = entry.map_err(|_| "IoError")?;
        paths::check_path(entry.path())?;
        let relative = entry
            .path()
            .strip_prefix(target)
            .map_err(|_| "UnsafePath")?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let key = paths::path_string(relative)
            .replace('\\', "/")
            .to_lowercase();
        if key != paths::OWNER_FILE && !expected.contains(&key) {
            return Err("目标目录出现清单之外的条目，已停止恢复。".into());
        }
    }
    Ok(())
}
fn message(code: &str) -> String {
    match code {
        "NeedsPassword" => "已保存密码未能打开压缩包，请补充密码后重试；损坏也可能导致此状态。",
        "Cancelled" => "任务已取消，作业进程已退出；原包和待检查的工作目录保留。",
        "ResourceLimit" => "磁盘空间、缓存配额或展开限制不足，请调整后重试。",
        "SharedResourceUnavailable" => {
            "工作磁盘不可用或低于保留空间，队列已暂停；请恢复磁盘后重试。"
        }
        "MissingVolume" => "压缩包缺少分卷，请补齐后重试。",
        "UnsafePath" => "压缩包含不支持的路径或链接，已停止写入。",
        "Corrupt" | "PasswordMismatchOrCorrupt" => "密码不匹配或压缩包损坏，尚未交付。",
        "WorkerExitUnconfirmed" => "无法确认作业进程退出；已停止后续任务并保留工作目录。",
        "StorageError" => "任务数据库提交失败，已停止后续操作；请检查恢复记录。",
        "ProtocolError" => "作业进程通信异常，尚未交付。",
        "IoError" => "文件读写失败；原包和工作目录已保留。",
        "Unsupported" => "此压缩格式暂不支持。",
        _ => code,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_strip_single_root_wrapper() {
        let f = |p: &str, d: bool| CopyFile {
            source_path: String::new(),
            relative_path: p.into(),
            size: 10,
            sha256: "a".repeat(64),
            is_directory: d,
        };
        // Case 1: Redundant single wrapper folder "content/..."
        let mut files1 = vec![
            f("content", true),
            f("content/a.txt", false),
            f("content/sub/b.txt", false),
        ];
        strip_single_root_wrapper(&mut files1);
        assert_eq!(files1.len(), 2);
        assert_eq!(files1[0].relative_path, "a.txt");
        assert_eq!(files1[1].relative_path, "sub\\b.txt");

        // Case 2: Root already has multiple files/folders, should NOT unwrap
        let mut files2 = vec![f("file1.txt", false), f("folder/file2.txt", false)];
        strip_single_root_wrapper(&mut files2);
        assert_eq!(files2.len(), 2);
        assert_eq!(files2[0].relative_path, "file1.txt");

        // Case 3: Multiple folders at root
        let mut files3 = vec![f("folderA/file1.txt", false), f("folderB/file2.txt", false)];
        strip_single_root_wrapper(&mut files3);
        assert_eq!(files3.len(), 2);
        assert_eq!(files3[0].relative_path, "folderA/file1.txt");
    }

    #[test]
    fn manifest_rejects_alias_conflicts() {
        let f = |p: &str, d: bool| CopyFile {
            source_path: String::new(),
            relative_path: p.into(),
            size: 0,
            sha256: if d { String::new() } else { "a".repeat(64) },
            is_directory: d,
        };
        assert!(validate_manifest(&[f("a.txt", false), f("A.txt", false)]).is_err());
        assert!(validate_manifest(&[f("a", false), f("a/b", false)]).is_err());
        assert!(validate_manifest(&[f("中文", true), f("中文/b", false)]).is_ok());
    }
    #[test]
    fn settings_reject_wrong_policy_and_empty_cache() {
        let mut s = Settings::default();
        validate_settings(&s).unwrap();
        s.cache_enabled = true;
        assert!(validate_settings(&s).is_err());
        s.cache_enabled = false;
        s.conflict_policy = "overwrite".into();
        assert!(validate_settings(&s).is_err());
    }
}
