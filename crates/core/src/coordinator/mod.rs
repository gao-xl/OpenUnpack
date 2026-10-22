//! The single production job coordinator. Submodules share this state and
//! divide responsibilities without introducing another scheduler or store.

mod api_pause;
mod cache_budget;
mod commands;
pub mod conflict_resolver;
mod execution;
mod initialization;
mod inspection;
mod integrity;
mod pipeline_helpers;
mod publication;
mod recovery;
mod resources;
mod scheduler;
pub mod selective;
mod snapshot;
mod snapshot_projector;
mod state;
mod transfers;
mod worker_requests;

use cache_budget::{BudgetClaim, BudgetError, CacheBudgetManager, RequestBudgetOwner};
pub use integrity::validate_settings;
use integrity::*;
use pipeline_helpers::*;
pub use scheduler::trigger_on_complete_hook;

#[cfg(test)]
mod tests;

use crate::{
    paths,
    repository::Repository,
    route::{ArchiveProbeSummary, ExtractionRoute, RouteDecider},
    scanner,
    transfer_progress::TransferEstimator,
    types::*,
    vault::Vault,
    worker::WorkerClient,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
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
        "completed"
            | "partial"
            | "salvaged"
            | "failed"
            | "cancelled"
            | "skipped"
            | "cleanupPending"
    )
}
fn recovering(status: &str) -> bool {
    matches!(status, "interrupted" | "publishPending" | "cleanupPending")
}
fn lock_error() -> String {
    "内部状态不可用；请重启并检查恢复记录。".into()
}

#[cfg(test)]
type TopologyProbe = fn(&Path, &Path) -> crate::route::DiskTopology;
type JobClaim = (JobView, Arc<AtomicBool>, bool, Option<HashSet<String>>);

/// An in-memory boundary for jobs admitted by an external application. The
/// trusted desktop start/retry is an explicit new authorization and clears it.
/// Implementations must not call back into Coordinator while this runs.
pub trait JobExecutionPolicy: Send + Sync {
    fn authorize(&self, job: &JobView) -> Result<(), String>;
    fn authorize_path(&self, path: &Path, write: bool) -> Result<(), String>;
}

pub struct Coordinator {
    pub data: PathBuf,
    repository: Repository,
    vault: Result<Vault, String>,
    worker: WorkerClient,
    jobs: Mutex<Vec<JobView>>,
    execution_policies: Mutex<HashMap<String, Arc<dyn JobExecutionPolicy>>>,
    authorization_blocked: Mutex<HashSet<String>>,
    scheduler_scope: Mutex<Option<HashSet<String>>>,
    settings: Mutex<Settings>,
    running: AtomicBool,
    // Captured so the folder watcher (a plain std thread) can schedule the
    // queue without calling tokio::spawn outside a runtime context.
    runtime: Option<tokio::runtime::Handle>,
    paused: AtomicBool,
    api_paused: AtomicBool,
    stop_batch: Arc<AtomicBool>,
    active_cancel: Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>,
    // Retained on unwind until the supervisor confirms child exit.
    in_flight_requests: Arc<Mutex<HashSet<String>>>,
    in_flight_request_ids: Arc<Mutex<HashMap<String, String>>>,
    publications: publication::PublicationService,
    cancelled_jobs: Mutex<HashSet<String>>,
    partial_requests: Mutex<HashSet<String>>,
    node_retry_requests: Mutex<std::collections::HashMap<String, HashSet<String>>>,
    notification: broadcast::Sender<()>,
    mutations: Mutex<()>,
    // Serialize record persistence with removal, independently of commands that call put.
    job_lifecycle: Mutex<()>,
    deleting_jobs: Mutex<HashSet<String>>,
    recovering_jobs: Mutex<HashSet<String>>,
    ephemeral_passwords: Mutex<HashMap<String, Vec<zeroize::Zeroizing<String>>>>,
    cache_usage: AtomicU64,
    // Display accounting only. Quota admission still scans the actual cache
    // root and holds independent budget reservations.
    cache_usage_by_job: Mutex<HashMap<String, CacheUsage>>,
    cache_budget: cache_budget::CacheBudgetManager,
    session_id: String,
    revision: AtomicU64,
    // Snapshot readers and visible-state writers acquire this before field locks.
    // It is separate from the command gate because commands may call `put`.
    view_state: Mutex<()>,
    // Transient observations are projected only by snapshot; never persisted.
    transfers: Mutex<HashMap<String, ActiveTransfer>>,
    snapshot_projector: snapshot_projector::SnapshotProjector,
    // Active inspect cancel token so closing or switching archive browser can terminate the worker.
    inspect_cancel: Mutex<Option<(String, Arc<AtomicBool>)>>,
    cancelled_inspections: Mutex<std::collections::VecDeque<String>>,
    preview_cancel: Mutex<Option<(String, Arc<AtomicBool>)>>,
    cancelled_previews: Mutex<std::collections::VecDeque<String>>,
    folder_watcher: Mutex<Option<crate::watcher::FolderWatcher>>,
    progress_broadcaster: broadcast::Sender<openunpack_protocol::JobProgressUpdate>,
    need_input_broadcaster: broadcast::Sender<openunpack_protocol::NeedPasswordEvent>,
    pending_inputs: Mutex<HashMap<String, tokio::sync::oneshot::Sender<String>>>,
    #[cfg(test)]
    topology_probe: Mutex<Option<TopologyProbe>>,
}

#[derive(Clone, Default)]
struct CacheUsage {
    workspace: Option<String>,
    bytes: u64,
    generation: u64,
}

fn validate_inspection_id(request_id: &str) -> Result<(), String> {
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("InvalidInspectionId".into());
    }
    Ok(())
}

struct InspectionRegistration<'a> {
    coordinator: &'a Coordinator,
    request_id: String,
    cancel: Arc<AtomicBool>,
}
struct PreviewRegistration<'a> {
    coordinator: &'a Coordinator,
    request_id: String,
    cancel: Arc<AtomicBool>,
}
impl Drop for PreviewRegistration<'_> {
    fn drop(&mut self) {
        let mut active = self
            .coordinator
            .preview_cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active
            .as_ref()
            .is_some_and(|(id, cancel)| id == &self.request_id && Arc::ptr_eq(cancel, &self.cancel))
        {
            active.take();
        }
    }
}
impl Drop for InspectionRegistration<'_> {
    fn drop(&mut self) {
        let mut active = self
            .coordinator
            .inspect_cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active
            .as_ref()
            .is_some_and(|(id, cancel)| id == &self.request_id && Arc::ptr_eq(cancel, &self.cancel))
        {
            active.take();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TransferIdentity {
    protocol_version: u32,
    request_id: String,
    operation_id: String,
    generation: u64,
    stage: String,
    node_id: Option<String>,
}
impl TransferIdentity {
    fn matches_job(&self, job: &JobView) -> bool {
        job.attempts.max(1) == self.generation
            && job.stage == self.stage
            && matches!(job.status.as_str(), "running" | "publishPending")
            && job
                .nodes
                .iter()
                .find(|node| node.status == "running")
                .map(|node| &node.id)
                == self.node_id.as_ref()
    }
    fn matches_event(&self, event: &WorkerEvent) -> bool {
        event.kind == "progress"
            && event.protocol_version == self.protocol_version
            && event.request_id == self.request_id
            && event.operation_id.as_deref() == Some(self.operation_id.as_str())
            && event.generation == Some(self.generation)
    }
}
struct ActiveTransfer {
    identity: TransferIdentity,
    started: Instant,
    estimator: TransferEstimator,
    view: Option<TransferProgress>,
    measured: bool,
}
struct TransferOwner {
    core: Arc<Coordinator>,
    job_id: String,
    identity: TransferIdentity,
}
struct DeleteIntent {
    core: Arc<Coordinator>,
    job_id: String,
}
impl Drop for DeleteIntent {
    fn drop(&mut self) {
        if let Ok(mut deleting) = self.core.deleting_jobs.lock() {
            deleting.remove(&self.job_id);
        }
        self.core.changed();
    }
}
struct RecoveryOwner {
    core: Arc<Coordinator>,
    job_id: String,
    completed: bool,
}
impl Drop for RecoveryOwner {
    fn drop(&mut self) {
        if !self.completed && !self.core.recovery_exit_confirmed(&self.job_id) {
            // Dropping an awaiting caller only requests process termination; it
            // cannot prove exit. Retain ownership until restart/recovery review.
            self.core.stop_batch.store(true, Ordering::Release);
            if let Ok(mut job) = self.core.get_job(&self.job_id) {
                job.status = "stopping".into();
                job.message = "恢复操作已中断，进程退出尚未确认；请重启应用后核对恢复记录。".into();
                if let Some(work) = &job.workspace {
                    let record = UnconfirmedExit {
                        processes: self.core.job_exit_processes(&self.job_id),
                    };
                    if !record.processes.is_empty()
                        && paths::atomic_json(&Path::new(work).join(UNCONFIRMED_FILE), &record)
                            .is_err()
                    {
                        eprintln!("recovery_exit_marker_persist_failed job={}", self.job_id);
                    }
                }
                let _ = self.core.put(&job);
            }
            return;
        }
        if !self.completed {
            self.core.mark_job_abnormal(&self.job_id);
            self.core.clear_confirmed_job_request(&self.job_id);
            if let Ok(mut active) = self.core.active_cancel.lock() {
                active.remove(&self.job_id);
            }
            self.core.release_unknown_budget_owner(&self.job_id);
            let _ = self.core.update_visible_state(|| {
                self.core.running.store(false, Ordering::Release);
                Ok(())
            });
            self.core.clear_confirmed_job_request(&self.job_id);
        }
        if let Ok(mut recovering) = self.core.recovering_jobs.lock() {
            recovering.remove(&self.job_id);
        }
        self.core.clear_confirmed_job_request(&self.job_id);
    }
}
impl Drop for TransferOwner {
    fn drop(&mut self) {
        self.core.finish_transfer(&self.job_id, &self.identity);
    }
}
struct JobRunGuard {
    core: Arc<Coordinator>,
    job_id: String,
    finished_normally: bool,
}
impl Drop for JobRunGuard {
    fn drop(&mut self) {
        if !self.finished_normally {
            self.core.mark_job_abnormal(&self.job_id);
        }
        if !self.core.job_worker_unconfirmed(&self.job_id) {
            if let Ok(mut active) = self.core.active_cancel.lock() {
                active.remove(&self.job_id);
            }
            self.core.release_unknown_budget_owner(&self.job_id);
        }
    }
}
struct BatchRunGuard {
    core: Arc<Coordinator>,
    completed: bool,
}
impl Drop for BatchRunGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.core.stop_batch.store(true, Ordering::Release);
            let owners = self
                .core
                .active_cancel
                .lock()
                .map(|active| active.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            for job_id in owners {
                self.core.mark_job_abnormal(&job_id);
            }
        }
        if !self.completed {
            self.core.running.store(false, Ordering::Release);
            self.core.changed();
        }
    }
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
    #[serde(default)]
    verification_scope: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct SavedRun {
    version: u32,
    job_id: String,
    source: SourceGroup,
    witnesses: Vec<SourceWitness>,
    options: Settings,
    input_files: Vec<CopyFile>,
    #[serde(default)]
    input_retired: bool,
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
