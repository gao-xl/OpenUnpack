//! Explicit, bounded Windows load measurements through the production Coordinator.
//! Build outside the measurement window; run this ignored test alone, without builds.
//! OPENUNPACK_LOAD_LEVELS=1,2,3,4 supports collecting the pre-change baseline.
//! See scripts/measure-load.ps1 for the supported, isolated entry point.
#![cfg(windows)]

use openunpack_core::{
    coordinator::Coordinator,
    paths::{self, DirectoryGuard},
    types::{AppSnapshot, JobView, Settings},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{Read, Write},
    mem::size_of,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        },
        Threading::{
            GetExitCodeProcess, GetProcessIoCounters, GetProcessTimes, IO_COUNTERS, OpenProcess,
            PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
        },
    },
};

const JOBS: usize = 10;
const RESERVE: u64 = 1024 * 1024 * 1024;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(100);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Only this successfully created, privately owned UUID child may be removed.
/// On panic it is retained for diagnosis; no user-selected parent is ever deleted.
struct Fixture {
    root: PathBuf,
    owner: String,
}
impl Fixture {
    fn new(parent: &Path, label: &str) -> Self {
        let parent = parent
            .canonicalize()
            .expect("fixture parent must already exist");
        paths::check_path(&parent).unwrap();
        let owner = uuid::Uuid::new_v4().to_string();
        let root = parent.join(format!("openunpack-load-{label}-{owner}"));
        assert!(!root.exists(), "fixture must be a new UUID child");
        paths::mark_owned(&root, &owner, "load-profile").unwrap();
        assert_eq!(root.parent(), Some(parent.as_path()));
        Self { root, owner }
    }

    fn cleanup(self) {
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(&self.owner),
            "refuse to remove a path outside the created fixture identity"
        );
        paths::remove_owned(&self.root, &self.owner).expect("remove only owned synthetic fixture");
    }
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn selected_worker() -> PathBuf {
    let path = std::env::var_os("OPENUNPACK_TEST_WORKER")
        .map(PathBuf::from)
        .expect("select a frozen Worker with OPENUNPACK_TEST_WORKER");
    let path = path.canonicalize().unwrap();
    assert!(path.is_file(), "selected Worker must exist");
    assert!(
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .eq_ignore_ascii_case("OpenUnpack.Worker.exe"),
        "native sampler expects the real OpenUnpack.Worker.exe child"
    );
    path
}

fn read_hash(path: &Path) -> String {
    let mut file = fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = [0; 128 * 1024];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            return hex::encode(hash.finalize());
        }
        hash.update(&buffer[..count]);
    }
}

fn ensure_space(path: &Path, needed: u64) {
    assert!(
        paths::free_bytes(path).unwrap() >= needed.saturating_add(RESERVE),
        "bounded fixture requires its estimated bytes plus at least 1 GiB reserve"
    );
}

struct Prepared {
    fixture: Fixture,
    archives: Vec<PathBuf>,
    archive_hashes: Vec<String>,
    expected: BTreeMap<String, String>,
    total_bytes: u64,
}
impl Prepared {
    fn new(parent: &Path, seven_zip: &Path, mib: u64, cpu: bool) -> Self {
        Self::with_small_files(parent, seven_zip, mib, cpu, 128)
    }

    fn with_small_files(
        parent: &Path,
        seven_zip: &Path,
        mib: u64,
        cpu: bool,
        small_files: usize,
    ) -> Self {
        assert!((16..=128).contains(&mib) && small_files <= 1024);
        let fixture = Fixture::new(parent, if cpu { "cpu" } else { "io" });
        let root = &fixture.root;
        ensure_space(
            root,
            (mib * 1024 * 1024 + small_files as u64 * 4096) * (JOBS as u64 * 2 + 2),
        );
        let payload = root.join("payload");
        fs::create_dir(&payload).unwrap();
        let mut file = fs::File::create(payload.join("large.bin")).unwrap();
        let mut state = 0x19b4_2d67_819e_31a5u64;
        let mut block = [0u8; 64 * 1024];
        for index in 0..mib * 16 {
            for value in &mut block {
                // Deterministic, generated without a large memory allocation.
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *value = (state >> 24) as u8;
            }
            if cpu && index % 2 == 0 {
                // A bounded mixed-entropy compressed case; not a zip-bomb fixture.
                for (offset, value) in block.iter_mut().enumerate() {
                    *value = (offset % 251) as u8;
                }
            }
            file.write_all(&block).unwrap();
        }
        file.sync_all().unwrap();
        drop(file);
        let mut expected = BTreeMap::new();
        expected.insert("large.bin".into(), read_hash(&payload.join("large.bin")));
        for index in 0..small_files {
            let name = format!("small-{index:03}.bin");
            let bytes = (0..4096)
                .map(|offset| ((offset + index * 17) % 251) as u8)
                .collect::<Vec<_>>();
            fs::write(payload.join(&name), &bytes).unwrap();
            expected.insert(name, hex::encode(Sha256::digest(&bytes)));
        }
        let format = if cpu { "7z" } else { "zip" };
        let first = root.join(format!("archive-00.{format}"));
        let status = Command::new(seven_zip)
            .current_dir(&payload)
            .args([
                "a",
                if cpu { "-mx=5" } else { "-mx=0" },
                "-mmt=1",
                "-y",
                "-bso0",
                "-bsp0",
                "-bse0",
            ])
            .arg(format!("-t{format}"))
            .arg(&first)
            .arg(".")
            .status()
            .expect("official fixture packer must start");
        assert!(status.success(), "official fixture archive creation failed");
        let first_hash = read_hash(&first);
        let mut archives = vec![first.clone()];
        for index in 1..JOBS {
            let archive = root.join(format!("archive-{index:02}.{format}"));
            fs::copy(&first, &archive).unwrap();
            archives.push(archive);
        }
        Self {
            fixture,
            archives,
            archive_hashes: vec![first_hash; JOBS],
            expected,
            total_bytes: mib * 1024 * 1024 + small_files as u64 * 4096,
        }
    }

    fn verify_sources(&self) {
        for (path, expected) in self.archives.iter().zip(&self.archive_hashes) {
            assert_eq!(
                &read_hash(path),
                expected,
                "source archive must remain unchanged"
            );
        }
    }
}

struct NativeProcess {
    handle: HANDLE,
    pid: u32,
    created: u64,
}
impl Drop for NativeProcess {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Counters {
    cpu: u64,
    rss: u64,
    private: u64,
    read: u64,
    written: u64,
    alive: bool,
}

fn filetime(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

impl NativeProcess {
    fn open(pid: u32) -> Option<Self> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut process = Self {
            handle,
            pid,
            created: 0,
        };
        let (created, _) = process.times()?;
        process.created = created;
        Some(process)
    }

    fn times(&self) -> Option<(u64, u64)> {
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = unsafe {
            GetProcessTimes(
                self.handle,
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        };
        (ok != 0).then(|| (filetime(created), filetime(kernel) + filetime(user)))
    }

    fn counters(&self) -> Option<Counters> {
        let (created, cpu) = self.times()?;
        if created != self.created {
            return None;
        }
        let mut exit = 0;
        if unsafe { GetExitCodeProcess(self.handle, &mut exit) } == 0 {
            return None;
        }
        let mut alive = exit == 259;
        let mut io = IO_COUNTERS::default();
        if unsafe { GetProcessIoCounters(self.handle, &mut io) } == 0 {
            return None;
        }
        let mut memory = PROCESS_MEMORY_COUNTERS_EX {
            cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        if alive
            && unsafe {
                GetProcessMemoryInfo(
                    self.handle,
                    (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX)
                        .cast::<PROCESS_MEMORY_COUNTERS>(),
                    size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                )
            } == 0
        {
            // A process may exit after GetExitCodeProcess but before its memory
            // query. Preserve readable final CPU/IO counters, not a false error.
            if unsafe { GetExitCodeProcess(self.handle, &mut exit) } == 0 || exit == 259 {
                return None;
            }
            alive = false;
            memory = PROCESS_MEMORY_COUNTERS_EX::default();
        }
        Some(Counters {
            cpu,
            rss: memory.WorkingSetSize as u64,
            private: memory.PrivateUsage as u64,
            read: io.ReadTransferCount,
            written: io.WriteTransferCount,
            alive,
        })
    }
}

/// Direct-child identity excludes unrelated desktop Workers. Retained handles
/// allow final CPU and IO reads after observed children exit, avoiding PID reuse.
struct Sampler {
    host: NativeProcess,
    baseline: Counters,
    last_host: Counters,
    children: HashMap<(u32, u64), NativeProcess>,
    last: HashMap<(u32, u64), Counters>,
    errors: usize,
    missed_candidates: usize,
    peak_workers: usize,
    peak_rss: u64,
    peak_private: u64,
    peak_cpu: f64,
    previous_cpu: u64,
    previous_sample: Duration,
    rows: Vec<Value>,
    started: Instant,
}
impl Sampler {
    fn new() -> Self {
        let host = NativeProcess::open(std::process::id()).expect("open benchmark host counters");
        let baseline = host.counters().expect("read host baseline");
        Self {
            host,
            baseline,
            last_host: baseline,
            children: HashMap::new(),
            last: HashMap::new(),
            errors: 0,
            missed_candidates: 0,
            peak_workers: 0,
            peak_rss: 0,
            peak_private: 0,
            peak_cpu: 0.0,
            previous_cpu: 0,
            previous_sample: Duration::ZERO,
            rows: vec![],
            started: Instant::now(),
        }
    }

    fn discover(&mut self) {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            self.errors += 1;
            return;
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) };
        while more != 0 {
            let end = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
            if entry.th32ParentProcessID == self.host.pid
                && name.eq_ignore_ascii_case("OpenUnpack.Worker.exe")
            {
                if let Some(process) = NativeProcess::open(entry.th32ProcessID) {
                    self.children
                        .entry((process.pid, process.created))
                        .or_insert(process);
                } else {
                    // It may have exited between enumeration and OpenProcess.
                    self.missed_candidates += 1;
                }
            }
            more = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        unsafe {
            CloseHandle(snapshot);
        }
    }

    fn sample(&mut self, snapshot: &AppSnapshot, snapshot_us: u128) {
        self.discover();
        let host = self.host.counters().expect("host metrics remain readable");
        self.last_host = host;
        let mut rss = host.rss;
        let mut private = host.private;
        let mut active = Vec::new();
        for (identity, process) in &self.children {
            match process.counters() {
                Some(counters) => {
                    if counters.alive {
                        active.push(json!({"pid":identity.0,"created":identity.1.to_string()}));
                        rss += counters.rss;
                        private += counters.private;
                    }
                    self.last.insert(*identity, counters);
                }
                None => self.errors += 1,
            }
        }
        self.peak_workers = self.peak_workers.max(active.len());
        self.peak_rss = self.peak_rss.max(rss);
        self.peak_private = self.peak_private.max(private);
        let child_cpu = self.last.values().map(|counter| counter.cpu).sum::<u64>();
        let elapsed = self.started.elapsed();
        let cpu = host.cpu.saturating_sub(self.baseline.cpu) + child_cpu;
        let interval = elapsed.saturating_sub(self.previous_sample);
        let cpu_percent = (interval >= Duration::from_millis(50)).then(|| {
            cpu.saturating_sub(self.previous_cpu) as f64
                / 10_000_000.0
                / interval.as_secs_f64()
                / std::thread::available_parallelism().unwrap().get() as f64
                * 100.0
        });
        if let Some(percent) = cpu_percent {
            self.peak_cpu = self.peak_cpu.max(percent);
        }
        self.previous_cpu = cpu;
        self.previous_sample = elapsed;
        self.rows.push(json!({
            "elapsedMs":elapsed.as_secs_f64()*1000.0,
            "workers":active, "runningJobs":snapshot.jobs.iter().filter(|job|job.status=="running").count(),
            "cacheUsedBytes":snapshot.cache_used_bytes, "snapshotUs":snapshot_us,
            "hostCpu100ns":host.cpu.saturating_sub(self.baseline.cpu),
            "observedWorkersCpu100ns":child_cpu, "combinedRssBytes":rss,"combinedPrivateBytes":private,
            "sampleCpuPercentWholeMachine":cpu_percent,
            "observedWorkersReadBytes":self.last.values().map(|counter|counter.read).sum::<u64>(),
            "observedWorkersWrittenBytes":self.last.values().map(|counter|counter.written).sum::<u64>(),
            "hostReadBytes":host.read.saturating_sub(self.baseline.read),
            "hostWrittenBytes":host.written.saturating_sub(self.baseline.written),
        }));
    }

    fn result(&self, wall: Duration) -> Value {
        let host = self.last_host;
        let cpu = host.cpu.saturating_sub(self.baseline.cpu)
            + self.last.values().map(|counter| counter.cpu).sum::<u64>();
        let cores = std::thread::available_parallelism().unwrap().get() as f64;
        json!({
            "intervalMs":SAMPLE_INTERVAL.as_millis(), "logicalProcessors":cores,
            "actualObservedWorkerPeak":self.peak_workers,
            "observedWorkerIdentities":self.children.keys().map(|(pid,created)|json!({"pid":pid,"created":created.to_string()})).collect::<Vec<_>>(),
            "samplingErrors":self.errors,"exitedBeforeOpenCandidates":self.missed_candidates,
            "combinedCurrentWorkingSetPeakBytes":self.peak_rss,
            "combinedCurrentPrivateCommitPeakBytes":self.peak_private,
            "combinedObservedCpuSeconds":cpu as f64/10_000_000.0,
            "sampledPeakCpuPercentWholeMachine":self.peak_cpu,
            "averageCpuPercentWholeMachine":cpu as f64/10_000_000.0/wall.as_secs_f64()/cores*100.0,
            "processReadBytes":host.read.saturating_sub(self.baseline.read)+self.last.values().map(|counter|counter.read).sum::<u64>(),
            "processWrittenBytes":host.written.saturating_sub(self.baseline.written)+self.last.values().map(|counter|counter.written).sum::<u64>(),
            "limitations":[
                "100 ms polling can miss shorter-lived Workers and true resource peaks.",
                "CPU totals include final counters of observed Workers; undetected short-lived Workers are absent.",
                "I/O counters include file, pipe and device I/O; they are not physical disk throughput.",
                "Memory is the simultaneous sampled sum, not a sum of per-process peak counters.",
                "No system cache flush, power setting or machine-wide load isolation is performed."
            ],
        })
    }

    fn all_exited(&self) -> bool {
        self.last.values().all(|counter| !counter.alive)
    }

    fn live_identities(&mut self) -> Vec<(u32, u64)> {
        self.discover();
        self.children
            .iter()
            .filter_map(|(identity, process)| {
                process
                    .counters()
                    .filter(|counters| counters.alive)
                    .map(|_| *identity)
            })
            .collect()
    }

    fn identities_exited(&self, identities: &[(u32, u64)]) -> bool {
        identities.iter().all(|identity| {
            self.children
                .get(identity)
                .and_then(NativeProcess::counters)
                .is_some_and(|counters| !counters.alive)
        })
    }
}

struct Run {
    core: Arc<Coordinator>,
    fixture: Fixture,
    ids: Vec<String>,
    settings: Settings,
}
impl Run {
    fn new(
        prepared: &Prepared,
        worker: &Path,
        engine: &Path,
        level: u32,
        cache_parent: Option<&Path>,
    ) -> Self {
        let fixture = Fixture::new(&prepared.fixture.root, &format!("level-{level}"));
        let output = fixture.root.join("output");
        let cache = cache_parent
            .unwrap_or(&fixture.root)
            .join(format!("cache-{}", fixture.owner));
        DirectoryGuard::create(&output).unwrap();
        DirectoryGuard::create(&cache).unwrap();
        let core = Coordinator::open(
            fixture.root.join("data"),
            worker.to_path_buf(),
            engine.to_path_buf(),
        )
        .unwrap();
        let settings = Settings {
            output_root: paths::path_string(&output),
            cache_root: paths::path_string(&cache),
            concurrency: level,
            cache_enabled: cache_parent.is_some(),
            cache_policy: "always".into(),
            cache_input: true,
            min_free_bytes: RESERVE,
            max_entries: 10_000,
            max_expanded_bytes: prepared.total_bytes * 3,
            cache_quota_bytes: prepared.total_bytes * JOBS as u64 * 3,
            expand_nested: false,
            unwrap_single_root: false,
            clean_empty_dirs: false,
            source_cleanup: "keep".into(),
            verification: "strict".into(),
            ..Settings::default()
        };
        core.save_settings(settings.clone()).unwrap();
        let inputs = prepared
            .archives
            .iter()
            .map(|path| paths::path_string(path))
            .collect::<Vec<_>>();
        let scanned = core.scan(&inputs).unwrap();
        assert_eq!(
            scanned.sources.len(),
            JOBS,
            "ten independent synthetic archives"
        );
        let jobs = core.add_jobs(scanned.sources, settings.clone()).unwrap();
        assert_eq!(jobs.len(), JOBS);
        Self {
            core,
            fixture,
            ids: jobs.into_iter().map(|job| job.id).collect(),
            settings,
        }
    }

    fn cleanup(self) {
        let Self { core, fixture, .. } = self;
        drop(core);
        fixture.cleanup();
    }
}

fn verify_outputs(prepared: &Prepared, jobs: &[JobView], cancelled: Option<&str>) {
    let cancelled = cancelled
        .into_iter()
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    verify_output_set(prepared, jobs, &cancelled);
}

fn verify_output_set(prepared: &Prepared, jobs: &[JobView], cancelled: &HashSet<String>) {
    assert_eq!(jobs.len(), JOBS);
    for job in jobs {
        if cancelled.contains(&job.id) {
            assert_eq!(
                job.status, "cancelled",
                "cancelled target must not claim success"
            );
            assert!(job.output_path.is_none());
            continue;
        }
        assert_eq!(
            job.status, "completed",
            "{} {} {}",
            job.source.display_name, job.stage, job.message
        );
        let output = Path::new(
            job.output_path
                .as_ref()
                .expect("successful job publishes output"),
        );
        let mut actual = BTreeMap::new();
        for entry in walkdir::WalkDir::new(output)
            .into_iter()
            .map(Result::unwrap)
        {
            assert!(
                !entry.path().is_symlink(),
                "synthetic output may not contain links"
            );
            if entry.file_type().is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(output)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/");
                // Private ownership metadata is not archive payload.
                if relative == ".openunpack-owner.json" {
                    continue;
                }
                actual.insert(relative, read_hash(entry.path()));
            }
        }
        assert_eq!(
            actual, prepared.expected,
            "every published payload file must match"
        );
    }
    prepared.verify_sources();
}

fn snapshot(core: &Coordinator, latencies: &mut Vec<u128>) -> AppSnapshot {
    let started = Instant::now();
    let snapshot = core.snapshot().unwrap();
    latencies.push(started.elapsed().as_micros());
    snapshot
}

fn latency_summary(values: &mut [u128]) -> Value {
    values.sort_unstable();
    let at = |fraction: f64| values[((values.len() - 1) as f64 * fraction) as usize];
    json!({"samples":values.len(),"p50Us":at(0.5),"p95Us":at(0.95),"maxUs":values.last()})
}

async fn measure(prepared: &Prepared, run: Run, report: &Path, label: &str) -> Value {
    let mut sampler = Sampler::new();
    let started = Instant::now();
    let mut latencies = Vec::new();
    let mut first_running: HashMap<String, f64> = HashMap::new();
    let mut finish: HashMap<String, f64> = HashMap::new();
    let mut last_sample = Instant::now() - SAMPLE_INTERVAL;
    let mut cache_workspace_seen = false;
    let mut cache_display_peak = 0;
    run.core.start().unwrap();
    let deadline = started + Duration::from_secs(600);
    let final_snapshot = loop {
        let current = snapshot(&run.core, &mut latencies);
        cache_display_peak = cache_display_peak.max(current.cache_used_bytes);
        cache_workspace_seen |= current
            .jobs
            .iter()
            .filter_map(|job| job.workspace.as_ref())
            .any(|path| Path::new(path).starts_with(&run.settings.cache_root));
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        for job in &current.jobs {
            if job.status == "running" {
                first_running.entry(job.id.clone()).or_insert(elapsed);
            }
            if !matches!(job.status.as_str(), "queued" | "running" | "stopping") {
                finish.entry(job.id.clone()).or_insert(elapsed);
            }
        }
        if last_sample.elapsed() >= SAMPLE_INTERVAL || !current.running {
            sampler.sample(&current, *latencies.last().unwrap());
            last_sample = Instant::now();
        }
        if !current.running {
            break current;
        }
        if Instant::now() >= deadline {
            run.core.cancel(None).unwrap();
            for _ in 0..250 {
                if !run.core.snapshot().unwrap().running {
                    break;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            panic!(
                "load case timed out; fixture retained at {}",
                run.fixture.root.display()
            );
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    };
    let wall = started.elapsed();
    assert_eq!(final_snapshot.jobs.len(), run.ids.len());
    // Hash verification and report writing are outside the throughput/CPU window.
    assert!(
        sampler.all_exited(),
        "all observed Workers must be confirmed exited"
    );
    assert_eq!(
        sampler.errors, 0,
        "sampling failures must not silently produce a clean report"
    );
    verify_outputs(prepared, &final_snapshot.jobs, None);
    assert_eq!(
        final_snapshot.cache_used_bytes, 0,
        "cache display drains after completion"
    );
    assert_eq!(
        paths::tree_size(Path::new(&run.settings.cache_root)).unwrap(),
        0,
        "actual cache must drain"
    );
    let level = run.settings.concurrency;
    let result = json!({
        "case":label,"configuredConcurrency":level,"tasks":JOBS,"payloadBytesPerTask":prepared.total_bytes,
        "payloadFileCountPerTask":prepared.expected.len(),"hostDebugAssertions":cfg!(debug_assertions),
        "wallSeconds":wall.as_secs_f64(),
        "payloadMiBPerSecond":prepared.total_bytes as f64*JOBS as f64/(1024.0*1024.0)/wall.as_secs_f64(),
        "actualConcurrencyReached":sampler.peak_workers>=level as usize,
        "sampling":sampler.result(wall),"snapshotLatency":latency_summary(&mut latencies),
        "jobs":final_snapshot.jobs.iter().map(|job|json!({
            "id":job.id,"status":job.status,"firstRunningMs":first_running.get(&job.id),
            "finishedMs":finish.get(&job.id),"stageTimesMs":job.stage_times,
        })).collect::<Vec<_>>(),
        "correctness":"all files SHA-256 and exact payload list matched; every source archive unchanged",
        "cacheEnabled":run.settings.cache_enabled,
        "cacheWorkspaceObserved":cache_workspace_seen,
        "sampledCacheDisplayPeakBytes":cache_display_peak,
        "cachePeakScope":"application display only; actual cache emptiness is checked after completion",
        "settings":run.settings,
    });
    fs::write(
        report.join(format!("{label}-level-{level}-samples.json")),
        serde_json::to_vec_pretty(&sampler.rows).unwrap(),
    )
    .unwrap();
    fs::write(
        report.join(format!("{label}-level-{level}.json")),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    eprintln!("LOAD_RESULT {}", serde_json::to_string(&result).unwrap());
    assert!(
        sampler.peak_workers > 0,
        "at least one real Worker must be observed"
    );
    if label.starts_with("ssd-") {
        assert!(
            sampler.peak_workers >= level as usize,
            "requested SSD Worker overlap was not observed; do not call this a successful concurrent measurement"
        );
    }
    if run.settings.cache_enabled {
        assert!(
            cache_workspace_seen && cache_display_peak > 0,
            "SSD cache route must actually engage"
        );
    }
    run.cleanup();
    result
}

fn levels() -> Vec<u32> {
    let text = std::env::var("OPENUNPACK_LOAD_LEVELS").unwrap_or_else(|_| "1,2,3,5".into());
    let levels = text
        .split(',')
        .map(|value| value.parse::<u32>().expect("numeric concurrency"))
        .collect::<Vec<_>>();
    assert!(!levels.is_empty() && levels.iter().all(|level| (1..=5).contains(level)));
    assert!(levels.len() <= 5);
    assert_eq!(
        levels.iter().copied().collect::<HashSet<_>>().len(),
        levels.len(),
        "duplicate levels would overwrite measurements"
    );
    levels
}

fn report_directory() -> PathBuf {
    let parent = std::env::var_os("OPENUNPACK_LOAD_REPORT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join("_workspace/load-optimization-2026-10-15"));
    fs::create_dir_all(&parent).unwrap();
    let report = parent.join(format!("measurement-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&report).unwrap();
    report
}

fn fixture_parent() -> PathBuf {
    std::env::var_os("OPENUNPACK_LOAD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn fixture_mib() -> u64 {
    let mib = std::env::var("OPENUNPACK_LOAD_MIB")
        .unwrap_or_else(|_| "64".into())
        .parse::<u64>()
        .unwrap();
    assert!((16..=128).contains(&mib), "bounded 16–128 MiB/task fixture");
    mib
}

fn binary_hashes(worker: &Path, engine: &Path) -> Value {
    let mut hashes = BTreeMap::new();
    hashes.insert("OpenUnpack.Worker.exe".to_string(), read_hash(worker));
    hashes.insert("selected-7z.dll".into(), read_hash(engine));
    for name in [
        "OpenUnpack.Worker.dll",
        "OpenUnpack.Engine.dll",
        "OpenUnpack.Worker.runtimeconfig.json",
        "coreclr.dll",
    ] {
        let path = worker.parent().unwrap().join(name);
        if path.is_file() {
            hashes.insert(name.to_string(), read_hash(&path));
        }
    }
    json!(hashes)
}

fn write_matrix(
    report: &Path,
    worker: &Path,
    engine: &Path,
    prepared: &Prepared,
    results: &[Value],
) {
    fs::write(report.join("matrix.json"), serde_json::to_vec_pretty(&json!({
        "worker":worker,"payloadSha256":binary_hashes(worker,engine),
        "os":std::env::consts::OS,"hostDebugAssertions":cfg!(debug_assertions),
        "fixtureRoot":prepared.fixture.root,"results":results,
        "scope":"production Coordinator -> named pipes -> .NET Worker -> 7-Zip; no native GUI acceptance",
    })).unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit bounded SSD measurements; run alone via scripts/measure-load.ps1"]
async fn synthetic_load_matrix() {
    let workspace = workspace();
    let worker = selected_worker();
    let engine = workspace.join("tools/7zip/7z.dll");
    let seven_zip = workspace.join("tools/7zip/7z.exe");
    let parent = fixture_parent();
    let mib = fixture_mib();
    let report = report_directory();
    eprintln!("LOAD_REPORT {}", report.display());
    let prepared = Prepared::new(&parent, &seven_zip, mib, false);
    let topology = paths::probe_disk_topology(&prepared.archives[0], &prepared.fixture.root);
    assert!(
        !topology.source_is_hdd && !topology.target_is_hdd,
        "SSD matrix requires verified SSD paths"
    );
    assert!(
        topology.source_device_number.is_some(),
        "unknown topology is not an SSD benchmark"
    );
    let mut results = Vec::new();
    for level in levels() {
        let run = Run::new(&prepared, &worker, &engine, level, None);
        results.push(measure(&prepared, run, &report, "ssd-store-zip").await);
    }
    write_matrix(&report, &worker, &engine, &prepared, &results);
    prepared.fixture.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit real HDD/SSD measurements; use a new private UUID child of the selected HDD parent"]
async fn synthetic_hdd_cache_matrix() {
    let workspace = workspace();
    let worker = selected_worker();
    let engine = workspace.join("tools/7zip/7z.dll");
    let seven_zip = workspace.join("tools/7zip/7z.exe");
    let hdd_parent = std::env::var_os("OPENUNPACK_LOAD_HDD_ROOT")
        .map(PathBuf::from)
        .expect("choose an existing HDD parent explicitly");
    let report = report_directory();
    eprintln!("LOAD_REPORT {}", report.display());
    let prepared = Prepared::new(&hdd_parent, &seven_zip, fixture_mib(), false);
    let cache_fixture = Fixture::new(&fixture_parent(), "ssd-cache");
    ensure_space(&cache_fixture.root, prepared.total_bytes * JOBS as u64 * 3);
    let hdd = paths::probe_disk_topology(&prepared.archives[0], &prepared.fixture.root);
    let cross = paths::probe_disk_topology(&prepared.archives[0], &cache_fixture.root);
    assert!(
        hdd.source_is_hdd && hdd.target_is_hdd && hdd.is_same_physical_disk,
        "selected source/output must share a verified physical HDD"
    );
    assert!(
        cross.target_device_number.is_some()
            && !cross.target_is_hdd
            && !cross.is_same_physical_disk,
        "cache must be on a verified different physical SSD"
    );
    let mut results = Vec::new();
    for level in levels() {
        for enabled in [false, true] {
            let run = Run::new(
                &prepared,
                &worker,
                &engine,
                level,
                enabled.then_some(cache_fixture.root.as_path()),
            );
            let label = if enabled {
                "hdd-ssd-cache-hdd"
            } else {
                "hdd-direct"
            };
            let result = measure(&prepared, run, &report, label).await;
            assert_eq!(
                result["sampling"]["actualObservedWorkerPeak"].as_u64(),
                Some(1),
                "same-HDD work remains conservatively serialized despite its configured upper bound"
            );
            results.push(result);
        }
    }
    write_matrix(&report, &worker, &engine, &prepared, &results);
    cache_fixture.cleanup();
    prepared.fixture.cleanup();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "bounded mixed-entropy 7z CPU format measurements; fixture compression is outside timing"]
async fn synthetic_cpu_format_case() {
    let workspace = workspace();
    let worker = selected_worker();
    let engine = workspace.join("tools/7zip/7z.dll");
    let seven_zip = workspace.join("tools/7zip/7z.exe");
    let report = report_directory();
    eprintln!("LOAD_REPORT {}", report.display());
    let prepared = Prepared::new(&fixture_parent(), &seven_zip, fixture_mib(), true);
    let topo = paths::probe_disk_topology(&prepared.archives[0], &prepared.fixture.root);
    assert!(!topo.source_is_hdd && !topo.target_is_hdd && topo.source_device_number.is_some());
    let mut results = Vec::new();
    for level in levels() {
        let run = Run::new(&prepared, &worker, &engine, level, None);
        results.push(measure(&prepared, run, &report, "ssd-mixed-7z-lzma2").await);
    }
    write_matrix(&report, &worker, &engine, &prepared, &results);
    prepared.fixture.cleanup();
}

async fn observe_until(
    run: &Run,
    sampler: &mut Sampler,
    latencies: &mut Vec<u128>,
    timeout: Duration,
    label: &str,
    mut ready: impl FnMut(&AppSnapshot, &mut Sampler) -> bool,
) -> AppSnapshot {
    let deadline = Instant::now() + timeout;
    let mut last_sample = Instant::now() - SAMPLE_INTERVAL;
    loop {
        let current = snapshot(&run.core, latencies);
        if last_sample.elapsed() >= SAMPLE_INTERVAL || !current.running {
            sampler.sample(&current, *latencies.last().unwrap());
            last_sample = Instant::now();
        }
        if ready(&current, sampler) {
            return current;
        }
        if Instant::now() >= deadline {
            let _ = run.core.cancel(None);
            for _ in 0..250 {
                if !run.core.snapshot().unwrap().running {
                    break;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            panic!(
                "{label} exceeded its bound; fixture retained at {}",
                run.fixture.root.display()
            );
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

fn has_real_extract_progress(job: &JobView) -> bool {
    job.status == "running"
        && job.stage == "extracting"
        && job
            .transfer_progress
            .as_ref()
            .is_some_and(|progress| progress.completed_bytes > 0)
}

fn control_result(
    prepared: &Prepared,
    run: &Run,
    sampler: &mut Sampler,
    latencies: &mut [u128],
    final_snapshot: &AppSnapshot,
    cancelled: &HashSet<String>,
    mut result: Value,
) -> Value {
    sampler.sample(final_snapshot, *latencies.last().unwrap());
    let wall = sampler.started.elapsed();
    assert!(!final_snapshot.running && sampler.all_exited());
    assert_eq!(sampler.errors, 0, "control metrics must be readable");
    // Cancellation intentionally retains its owned partial workspace for retry.
    // Its real bytes remain accounted, including a direct-output workspace.
    let mut retained_bytes = 0u64;
    let mut retained_workspaces = Vec::new();
    for job in &final_snapshot.jobs {
        if let Some(workspace) = &job.workspace {
            assert!(
                cancelled.contains(&job.id),
                "successful work must be cleaned"
            );
            paths::owned(Path::new(workspace), &job.id, None).unwrap();
            let bytes = paths::tree_size(Path::new(workspace)).unwrap();
            retained_bytes = retained_bytes.checked_add(bytes).unwrap();
            retained_workspaces.push(json!({"job":job.id,"bytes":bytes}));
        }
    }
    assert_eq!(
        final_snapshot.cache_used_bytes, retained_bytes,
        "retained cancelled work must stay fully counted"
    );
    assert_eq!(
        paths::tree_size(Path::new(&run.settings.cache_root)).unwrap(),
        0
    );
    verify_output_set(prepared, &final_snapshot.jobs, cancelled);
    result["sampling"] = sampler.result(wall);
    result["snapshotLatency"] = latency_summary(latencies);
    result["allSourcesUnchanged"] = json!(true);
    result["payloadVerifiedForEveryCompletedJob"] = json!(true);
    result["cancelledJobCount"] = json!(cancelled.len());
    result["payloadBytesPerTask"] = json!(prepared.total_bytes);
    result["payloadFileCountPerTask"] = json!(prepared.expected.len());
    result["hostDebugAssertions"] = json!(cfg!(debug_assertions));
    result["retainedOwnedPartialWorkspaces"] = json!(retained_workspaces);
    result["retainedWorkspaceBytes"] = json!(retained_bytes);
    result
}

fn save_control(report: &Path, name: &str, sampler: &Sampler, result: &Value) {
    fs::write(
        report.join(format!("{name}.json")),
        serde_json::to_vec_pretty(result).unwrap(),
    )
    .unwrap();
    fs::write(
        report.join(format!("{name}-samples.json")),
        serde_json::to_vec_pretty(&sampler.rows).unwrap(),
    )
    .unwrap();
    eprintln!("LOAD_CONTROL {}", serde_json::to_string(result).unwrap());
}

async fn measure_pause(
    prepared: &Prepared,
    worker: &Path,
    engine: &Path,
    level: u32,
    report: &Path,
) -> Value {
    let run = Run::new(prepared, worker, engine, level, None);
    let mut sampler = Sampler::new();
    let mut latencies = Vec::new();
    run.core.start().unwrap();
    observe_until(
        &run,
        &mut sampler,
        &mut latencies,
        Duration::from_secs(60),
        "pause setup",
        |current, _| {
            current.jobs.iter().any(has_real_extract_progress)
                && current.jobs.iter().any(|job| job.status == "queued")
        },
    )
    .await;
    let command = Instant::now();
    run.core.pause().unwrap();
    let command_ms = command.elapsed().as_secs_f64() * 1000.0;
    let paused = snapshot(&run.core, &mut latencies);
    let visible_ms = command.elapsed().as_secs_f64() * 1000.0;
    assert!(paused.paused);
    let allowed = paused
        .jobs
        .iter()
        .filter(|job| job.status == "running")
        .map(|job| job.id.clone())
        .collect::<HashSet<_>>();
    let pending = paused
        .jobs
        .iter()
        .filter(|job| job.status == "queued")
        .map(|job| job.id.clone())
        .collect::<HashSet<_>>();
    assert!(!pending.is_empty(), "pause requires unclaimed work");
    let watch = Instant::now();
    observe_until(
        &run,
        &mut sampler,
        &mut latencies,
        Duration::from_secs(5),
        "pause observation",
        |current, _| {
            assert!(current.paused, "queue remains paused until explicit resume");
            for job in &current.jobs {
                if pending.contains(&job.id) {
                    assert_eq!(
                        job.status, "queued",
                        "pause returned before a later task claim"
                    );
                }
                if job.status == "running" {
                    assert!(
                        allowed.contains(&job.id),
                        "only previously claimed work may continue"
                    );
                }
            }
            watch.elapsed() >= Duration::from_secs(1)
        },
    )
    .await;
    let resume = Instant::now();
    run.core.start().unwrap();
    let resume_ms = resume.elapsed().as_secs_f64() * 1000.0;
    let final_snapshot = observe_until(
        &run,
        &mut sampler,
        &mut latencies,
        Duration::from_secs(600),
        "resumed completion",
        |current, _| !current.running,
    )
    .await;
    let result = control_result(
        prepared,
        &run,
        &mut sampler,
        &mut latencies,
        &final_snapshot,
        &HashSet::new(),
        json!({
            "control":"pause-new-tasks","configuredConcurrency":level,"commandReturnMs":command_ms,
            "pausedSnapshotVisibleMs":visible_ms,"resumeCommandReturnMs":resume_ms,
            "unclaimedTasksHeld":pending.len(),"alreadyClaimedTasksAllowedToFinish":allowed.len(),
            "observationMs":1000,"scope":"Core commands; active Workers are not suspended",
        }),
    );
    save_control(report, "pause-new-tasks", &sampler, &result);
    run.cleanup();
    result
}

async fn measure_single_cancel(
    prepared: &Prepared,
    worker: &Path,
    engine: &Path,
    report: &Path,
) -> Value {
    // With one active task and paused claiming, the sole live native Worker can
    // be attributed to this target without inventing a JobView -> PID mapping.
    let run = Run::new(prepared, worker, engine, 1, None);
    let mut sampler = Sampler::new();
    let mut latencies = Vec::new();
    run.core.start().unwrap();
    let ready = observe_until(
        &run,
        &mut sampler,
        &mut latencies,
        Duration::from_secs(60),
        "single cancel progress",
        |current, sampler| {
            current.jobs.iter().any(has_real_extract_progress)
                && sampler.live_identities().len() == 1
        },
    )
    .await;
    let target = ready
        .jobs
        .iter()
        .find(|job| has_real_extract_progress(job))
        .unwrap()
        .id
        .clone();
    run.core.pause().unwrap();
    let processes = sampler.live_identities();
    assert_eq!(
        processes.len(),
        1,
        "target must have exactly one observed live Worker before cancellation"
    );
    let command = Instant::now();
    run.core.cancel(Some(&target)).unwrap();
    let command_ms = command.elapsed().as_secs_f64() * 1000.0;
    let mut terminal_ms = None;
    let mut exit_ms = None;
    observe_until(
        &run,
        &mut sampler,
        &mut latencies,
        Duration::from_secs(5),
        "single cancel 5 second target",
        |current, sampler| {
            let elapsed = command.elapsed().as_secs_f64() * 1000.0;
            let job = current.jobs.iter().find(|job| job.id == target).unwrap();
            if job.status == "cancelled" {
                terminal_ms.get_or_insert(elapsed);
            }
            if sampler.identities_exited(&processes) {
                exit_ms.get_or_insert(elapsed);
            }
            terminal_ms.is_some() && exit_ms.is_some()
        },
    )
    .await;
    let terminal_ms = terminal_ms.unwrap();
    let exit_ms = exit_ms.unwrap();
    assert!(
        terminal_ms <= 5000.0 && exit_ms <= 5000.0,
        "5 second cancel target is a gate"
    );
    run.core.start().unwrap();
    let final_snapshot = observe_until(
        &run,
        &mut sampler,
        &mut latencies,
        Duration::from_secs(600),
        "remaining tasks after cancel",
        |current, _| !current.running,
    )
    .await;
    let result = control_result(
        prepared,
        &run,
        &mut sampler,
        &mut latencies,
        &final_snapshot,
        &HashSet::from([target.clone()]),
        json!({
            "control":"single-task-cancel-after-real-progress","configuredConcurrency":1,
            "commandReturnMs":command_ms,"terminalMs":terminal_ms,"nativeTargetWorkerExitMs":exit_ms,
            "nativeTargetWorkerIdentities":processes.iter().map(|(pid,created)|json!({"pid":pid,"created":created.to_string()})).collect::<Vec<_>>(),
            "targetJob":target,"otherTasksCompleted":JOBS-1,
            "scope":"precisely attributed target Worker; claiming was paused while stopping it",
        }),
    );
    save_control(report, "single-cancel", &sampler, &result);
    run.cleanup();
    result
}

async fn measure_batch_cancel(
    prepared: &Prepared,
    worker: &Path,
    engine: &Path,
    level: u32,
    report: &Path,
) -> Value {
    let run = Run::new(prepared, worker, engine, level, None);
    let mut sampler = Sampler::new();
    let mut latencies = Vec::new();
    run.core.start().unwrap();
    observe_until(
        &run,
        &mut sampler,
        &mut latencies,
        Duration::from_secs(60),
        "parallel cancel progress",
        |current, sampler| {
            current.jobs.iter().any(has_real_extract_progress)
                && sampler.live_identities().len() >= level as usize
        },
    )
    .await;
    run.core.pause().unwrap();
    let processes = sampler.live_identities();
    assert!(
        processes.len() >= level as usize,
        "parallel cancellation must observe real Worker overlap"
    );
    let command = Instant::now();
    run.core.cancel(None).unwrap();
    let command_ms = command.elapsed().as_secs_f64() * 1000.0;
    let mut exit_ms = None;
    let stopped = observe_until(
        &run,
        &mut sampler,
        &mut latencies,
        Duration::from_secs(5),
        "parallel cancel 5 second target",
        |current, sampler| {
            if sampler.identities_exited(&processes) {
                exit_ms.get_or_insert(command.elapsed().as_secs_f64() * 1000.0);
            }
            !current.running && exit_ms.is_some()
        },
    )
    .await;
    let terminal_ms = command.elapsed().as_secs_f64() * 1000.0;
    let exit_ms = exit_ms.unwrap();
    assert!(terminal_ms <= 5000.0 && exit_ms <= 5000.0);
    let cancelled = stopped
        .jobs
        .iter()
        .filter(|job| job.status == "cancelled")
        .map(|job| job.id.clone())
        .collect::<HashSet<_>>();
    assert!(
        !cancelled.is_empty(),
        "real progress cancellation must cancel work"
    );
    assert!(
        stopped
            .jobs
            .iter()
            .all(|job| matches!(job.status.as_str(), "queued" | "cancelled" | "completed"))
    );
    let queued = stopped
        .jobs
        .iter()
        .filter(|job| job.status == "queued")
        .count();
    run.core.start().unwrap();
    let final_snapshot = observe_until(
        &run,
        &mut sampler,
        &mut latencies,
        Duration::from_secs(600),
        "pending tasks after batch cancel",
        |current, _| !current.running,
    )
    .await;
    let result = control_result(
        prepared,
        &run,
        &mut sampler,
        &mut latencies,
        &final_snapshot,
        &cancelled,
        json!({
            "control":"parallel-batch-stop-after-real-progress","configuredConcurrency":level,
            "commandReturnMs":command_ms,"batchStoppedMs":terminal_ms,"observedWorkerSetExitMs":exit_ms,
            "nativeWorkersAtCancel":processes.iter().map(|(pid,created)|json!({"pid":pid,"created":created.to_string()})).collect::<Vec<_>>(),
            "pendingTasksResumed":queued,"scope":"all observed live Workers are stopped as a batch; no unsupported per-job PID inference",
        }),
    );
    save_control(report, "parallel-batch-cancel", &sampler, &result);
    run.cleanup();
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "real progress pause/cancel timing; explicitly run alone, no native GUI claim"]
async fn synthetic_pause_and_cancel() {
    let workspace = workspace();
    let worker = selected_worker();
    let engine = workspace.join("tools/7zip/7z.dll");
    let seven_zip = workspace.join("tools/7zip/7z.exe");
    let report = report_directory();
    eprintln!("LOAD_REPORT {}", report.display());
    // The release host can finish a small stored archive within the Worker's
    // 100 ms progress coalescing interval. Real bounded IO, including many small
    // files, provides an observable extraction interval without fake sleeps or
    // weakening the mandatory progress/PID/5-second cancellation checks.
    let prepared = Prepared::with_small_files(&fixture_parent(), &seven_zip, 128, false, 1024);
    let topo = paths::probe_disk_topology(&prepared.archives[0], &prepared.fixture.root);
    assert!(!topo.source_is_hdd && !topo.target_is_hdd && topo.source_device_number.is_some());
    let level = *levels().iter().max().unwrap();
    let results = vec![
        measure_pause(&prepared, &worker, &engine, level, &report).await,
        measure_single_cancel(&prepared, &worker, &engine, &report).await,
        measure_batch_cancel(&prepared, &worker, &engine, level, &report).await,
    ];
    write_matrix(&report, &worker, &engine, &prepared, &results);
    prepared.fixture.cleanup();
}

#[test]
#[ignore = "snapshot-only synthetic stored history; no archives or Workers are executed"]
fn synthetic_snapshot_history() {
    const SAMPLES: usize = 500;
    const WARMUP: usize = 20;
    let workspace = workspace();
    let worker = selected_worker();
    let engine = workspace.join("tools/7zip/7z.dll");
    let report = report_directory();
    eprintln!("LOAD_REPORT {}", report.display());
    let mut results = Vec::new();
    for count in [10usize, 1000] {
        let fixture = Fixture::new(&fixture_parent(), &format!("snapshot-history-{count}"));
        let data = fixture.root.join("data");
        DirectoryGuard::create(&data).unwrap();
        let repository =
            openunpack_core::repository::Repository::open(&data.join("jobs.sqlite")).unwrap();
        for index in 0..count {
            let id = format!("synthetic-history-{index:05}");
            let name = format!("synthetic-history-{index:05}.zip");
            let source = paths::path_string(&fixture.root.join(&name));
            repository
                .save_job(&JobView {
                    id: id.clone(),
                    source: openunpack_core::types::SourceGroup {
                        path: source.clone(),
                        display_name: name.clone(),
                        volumes: vec![source],
                        size: 64 * 1024 * 1024,
                        missing_volume: false,
                    },
                    options: Settings::default(),
                    status: "completed".into(),
                    stage: "completed".into(),
                    progress: 1.0,
                    message: "Synthetic stored history for snapshot measurement; never executed."
                        .into(),
                    attempts: 0,
                    output_path: None,
                    verification_scope: "syntheticSnapshotOnlyNeverExecuted".into(),
                    created_at: "2026-10-15T00:00:00Z".into(),
                    finished_at: Some("2026-10-15T00:00:00Z".into()),
                    workspace: None,
                    nodes: vec![openunpack_core::types::NodeView {
                        id: format!("{id}-node"),
                        parent_id: None,
                        name,
                        depth: 0,
                        status: "completed".into(),
                        stage: "completed".into(),
                        progress: 1.0,
                        attempts: 0,
                        message: "Synthetic metadata only.".into(),
                        password_label: None,
                    }],
                    stage_times: BTreeMap::new(),
                    transfer_progress: None,
                })
                .unwrap();
        }
        drop(repository);
        let core = Coordinator::open(data, worker.clone(), engine.clone()).unwrap();
        core.save_settings(Settings {
            history_limit: 2000,
            ..Settings::default()
        })
        .unwrap();
        for _ in 0..WARMUP {
            let view = core.snapshot().unwrap();
            assert_eq!(view.jobs.len(), count);
            std::hint::black_box(serde_json::to_vec(&view).unwrap());
            let queue = core.queue_snapshot().unwrap();
            assert_eq!(queue.jobs.len(), count.min(20));
            std::hint::black_box(serde_json::to_vec(&queue).unwrap());
        }
        let mut snapshot_times = Vec::with_capacity(SAMPLES);
        let mut serialization_times = Vec::with_capacity(SAMPLES);
        let mut combined_times = Vec::with_capacity(SAMPLES);
        let mut payload_sizes = Vec::with_capacity(SAMPLES);
        let mut queue_times = Vec::with_capacity(SAMPLES);
        let mut queue_payload_sizes = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let started = Instant::now();
            let view = core.snapshot().unwrap();
            snapshot_times.push(started.elapsed().as_micros());
            assert_eq!(view.jobs.len(), count);
            let serialized = Instant::now();
            let bytes = serde_json::to_vec(&view).unwrap();
            serialization_times.push(serialized.elapsed().as_micros());
            combined_times.push(started.elapsed().as_micros());
            payload_sizes.push(bytes.len());
            std::hint::black_box(bytes);
            let queue_started = Instant::now();
            let queue = core.queue_snapshot().unwrap();
            assert_eq!(queue.jobs.len(), count.min(20));
            let queue_bytes = serde_json::to_vec(&queue).unwrap();
            queue_times.push(queue_started.elapsed().as_micros());
            queue_payload_sizes.push(queue_bytes.len());
            std::hint::black_box(queue_bytes);
        }
        let result = json!({
            "case":"snapshot-only-synthetic-stored-history","syntheticCompletedRecords":count,
            "realExecutedTasks":0,"workerProcessesStarted":0,"warmupSnapshots":WARMUP,"measuredSnapshots":SAMPLES,
            "hostDebugAssertions":cfg!(debug_assertions),
            "snapshotCloneLatency":latency_summary(&mut snapshot_times),
            "jsonSerializationLatency":latency_summary(&mut serialization_times),
            "combinedLatency":latency_summary(&mut combined_times),
            "payloadBytesMin":payload_sizes.iter().min(),"payloadBytesMax":payload_sizes.iter().max(),
            "queueSnapshotRecords":count.min(20),
            "queueSnapshotCombinedLatency":latency_summary(&mut queue_times),
            "queueSnapshotPayloadBytesMin":queue_payload_sizes.iter().min(),
            "queueSnapshotPayloadBytesMax":queue_payload_sizes.iter().max(),
            "scope":"Legacy full snapshot versus production queue_snapshot and serde JSON over isolated synthetic stored metadata. No real extraction, frontend rendering, Tauri IPC or native GUI timing.",
        });
        let rows = json!({
            "snapshotUsSorted":snapshot_times,"serializationUsSorted":serialization_times,
            "combinedUsSorted":combined_times,"payloadBytes":payload_sizes,
            "queueCombinedUsSorted":queue_times,"queuePayloadBytes":queue_payload_sizes,
        });
        fs::write(
            report.join(format!("snapshot-history-{count}-samples.json")),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
        fs::write(
            report.join(format!("snapshot-history-{count}.json")),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        eprintln!("LOAD_SNAPSHOT {}", serde_json::to_string(&result).unwrap());
        results.push(result);
        drop(core);
        fixture.cleanup();
    }
    fs::write(
        report.join("snapshot-history.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}
