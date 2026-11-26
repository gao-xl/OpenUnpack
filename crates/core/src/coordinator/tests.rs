//! 协调器核心单元测试入口，按子域拆分为独立测试文件。
use super::*;

mod api_password_tests;
mod api_policy_tests;
mod resource_waiter_tests;
mod route_load_tests;
mod snapshot_deletion_tests;
mod transfer_cache_tests;

fn telemetry_job(id: &str) -> JobView {
    JobView {
        id: id.into(),
        source: SourceGroup {
            path: "synthetic.zip".into(),
            display_name: "synthetic.zip".into(),
            volumes: vec![],
            size: 1000,
            missing_volume: false,
        },
        options: Settings::default(),
        status: "running".into(),
        stage: "extracting".into(),
        progress: 0.,
        message: String::new(),
        attempts: 2,
        output_path: None,
        verification_scope: String::new(),
        created_at: now(),
        finished_at: None,
        workspace: None,
        stage_times: BTreeMap::new(),
        transfer_progress: None,
        nodes: vec![NodeView {
            id: "root-node".into(),
            parent_id: None,
            name: "root".into(),
            depth: 0,
            status: "running".into(),
            stage: "extracting".into(),
            progress: 0.,
            attempts: 2,
            message: String::new(),
            password_label: None,
        }],
    }
}
fn telemetry_core() -> (tempfile::TempDir, Arc<Coordinator>, JobView) {
    let directory = tempfile::tempdir().unwrap();
    let core = Coordinator::open(
        directory.path().join("data"),
        directory.path().join("missing-worker.exe"),
        directory.path().join("missing-7z.dll"),
    )
    .unwrap();
    let job = telemetry_job("job-one");
    core.update_visible_state(|| {
        core.jobs.lock().unwrap().push(job.clone());
        Ok(())
    })
    .unwrap();
    core.put(&job).unwrap();
    (directory, core, job)
}
fn telemetry_request(job: &JobView) -> WorkerRequest {
    let mut req = request("extract", Path::new("synthetic-output"), &job.options);
    req.generation = Some(job.attempts.max(1));
    req
}
fn telemetry_event(identity: &TransferIdentity, completed: u64) -> WorkerEvent {
    let mut event = WorkerEvent::default();
    event.protocol_version = identity.protocol_version;
    event.request_id = identity.request_id.clone();
    event.operation_id = Some(identity.operation_id.clone());
    event.generation = Some(identity.generation);
    event.kind = "progress".into();
    event.completed_bytes = completed;
    event.total_bytes = 3000;
    event
}

fn deletion_core(status: &str) -> (tempfile::TempDir, Arc<Coordinator>, JobView) {
    let (directory, core, mut job) = telemetry_core();
    let source = directory.path().join("original.zip");
    fs::write(&source, b"synthetic original archive").unwrap();
    job.source.path = paths::path_string(&source);
    job.source.volumes = vec![job.source.path.clone()];
    job.status = status.into();
    core.put(&job).unwrap();
    (directory, core, job)
}
