//! The in-place engine's coordinator path.
//!
//! This is the second production engine next to the 7-Zip worker. It is not a
//! variation of `run_job`'s staging pipeline: the whole point is that the
//! archive shrinks while it extracts, so there is no workspace to spill into
//! and no second copy to publish. What the pipeline still provides is the
//! contract around it — settings validation, authorisation, a durable job
//! record, a publication row, the success outbox and the same terminal
//! statuses.
//!
//! Invariants kept here:
//!
//! * nothing is truncated before the whole archive passed `verify_first`;
//! * a run that does not fit fails before touching the archive;
//! * a cancelled run is left resumable (HatchZip's journal and our manifest
//!   sidecar stay next to the archive);
//! * publishing records the exact entry list, never a directory walk that
//!   could claim files the person already had.

use super::*;
use openunpack_infrastructure::engine::inplace;

/// How long the person gets to type a password before the run fails.
const PASSWORD_WAIT: Duration = Duration::from_secs(60);
/// How often the blocking run's atomics are turned into visible progress.
const PROGRESS_TICK: Duration = Duration::from_millis(500);

/// RAII reservation guard protecting against concurrent multi-job disk space contention on the same volume.
struct InplaceVolumeClaim {
    volume: PathBuf,
    amount: u64,
    ledger: Arc<Mutex<HashMap<PathBuf, u64>>>,
}

impl Drop for InplaceVolumeClaim {
    fn drop(&mut self) {
        if self.amount > 0
            && let Ok(mut ledger) = self.ledger.lock()
            && let Some(existing) = ledger.get_mut(&self.volume)
        {
            *existing = existing.saturating_sub(self.amount);
            if *existing == 0 {
                ledger.remove(&self.volume);
            }
        }
    }
}

/// RAII cancellation guard ensuring in-place blocking task is signaled on early drop/error.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl Coordinator {
    #[doc(hidden)]
    pub fn inplace_volume_reservations(&self) -> Arc<Mutex<HashMap<PathBuf, u64>>> {
        self.inplace_reservations.clone()
    }

    pub(super) async fn run_inplace_job(
        self: &Arc<Self>,
        job: &mut JobView,
        source: &SourceGroup,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let s = job.options.clone();
        let archive = PathBuf::from(&source.path);
        let output = inplace_output(&s, &archive)?;
        if let Some(policy) = self.execution_policy(&job.id)? {
            policy.authorize_path(&archive, true)?;
            for volume in &source.volumes {
                policy.authorize_path(Path::new(volume), true)?;
            }
            policy.authorize_path(&output, true)?;
        }

        // 1. 同卷强制校验 (Same-Volume Precondition)
        if !paths::same_volume(&archive, &output).unwrap_or(false) {
            job.status = "failed".into();
            job.stage = "finished".into();
            job.message = "输出目录与压缩包不在同一磁盘：原地解压不会释放空间，反而会破坏源包。请把输出目录改到源包所在磁盘，或改用标准解压。（UnsafePath）".into();
            job.finished_at = Some(now());
            return self.put(job);
        }

        let volume_root = paths::volume_root(&output).unwrap_or_else(|_| output.clone());
        let active_reserved = self
            .inplace_reservations
            .lock()
            .map(|m| m.get(&volume_root).copied().unwrap_or(0))
            .unwrap_or(0);

        job.transfer_progress = None;
        let mut request = inplace::Request {
            archive: archive.clone(),
            output: output.clone(),
            passwords: self.inplace_passwords(job),
            keep_free: s.min_free_bytes.saturating_add(active_reserved),
            replace: s.conflict_policy == "overwrite",
            verify_first: true,
            max_entries: s.max_entries,
            max_expanded_bytes: s.max_expanded_bytes,
        };

        self.stage(
            job,
            "checking",
            "正在预演原地解压所需空间与输出冲突；不会改动压缩包。",
        )?;
        let preflight = loop {
            let attempt = request.clone();
            match blocking(move || inplace::preflight(&attempt)).await {
                Ok(plan) => break plan,
                Err(error) if error.code == "NeedsPassword" => {
                    let Some(password) = self.ask_inplace_password(job, &cancel).await? else {
                        return Err("NeedsPassword".into());
                    };
                    self.absorb_password(&mut request, job, password);
                    self.stage(job, "checking", "已收到密码，正在重新预演原地解压。")?;
                }
                Err(error) => {
                    job.message = error.detail.clone();
                    return Err(error.code);
                }
            }
        };

        if preflight.state == inplace::PreflightState::Orphan {
            let archive_path = archive.clone();
            blocking(move || inplace::finish_orphaned(&archive_path))
                .await
                .map_err(|e| e.code)?;
            return self
                .finish_inplace(job, &archive, &preflight.output, true)
                .await;
        }

        if let Some(reason) = preflight.blocker(s.max_entries, s.max_expanded_bytes) {
            let code = if preflight.refusal.is_some() || preflight.has_links {
                "Unsupported"
            } else if !preflight.reclaims {
                "UnsafePath"
            } else if preflight.entries > s.max_entries || preflight.bytes > s.max_expanded_bytes {
                "ResourceLimit"
            } else {
                "NoSpace"
            };
            job.status = "failed".into();
            job.stage = "finished".into();
            job.message = format!("{reason}（{code}）");
            job.finished_at = Some(now());
            return self.put(job);
        }

        if !preflight.existing.is_empty() {
            let first = preflight
                .existing
                .first()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            match s.conflict_policy.as_str() {
                "overwrite" => request.replace = true,
                "skip" => {
                    job.status = "skipped".into();
                    job.stage = "finished".into();
                    job.message = format!(
                        "输出目录中已存在 {} 等同名文件，按设置跳过；压缩包已保留。",
                        first
                    );
                    job.finished_at = Some(now());
                    return self.put(job);
                }
                _ => {
                    job.status = "failed".into();
                    job.stage = "finished".into();
                    job.message = format!(
                        "输出目录中已存在 {} 等同名文件；原地解压无法重命名，请选择覆盖或更换输出目录。压缩包已保留。",
                        first
                    );
                    job.finished_at = Some(now());
                    return self.put(job);
                }
            }
        }

        // 2 & 3. 独立物理磁盘可用空间原子化校验与同卷并发预留登记 (Atomic Volume Headroom & Reservation)
        let _volume_claim = {
            let mut ledger = self.inplace_reservations.lock().map_err(|_| lock_error())?;
            let current_reserved = ledger.get(&volume_root).copied().unwrap_or(0);
            let raw_free =
                paths::free_bytes(&output).map_err(|_| "无法读取剩余磁盘空间。".to_string())?;
            let effective_free = raw_free.saturating_sub(current_reserved);
            let headroom = effective_free.saturating_sub(s.min_free_bytes);
            if headroom < preflight.needed {
                job.status = "failed".into();
                job.stage = "finished".into();
                job.message = format!(
                    "磁盘空间不足：原地解压最多需要 {}，当前可用扣除保留空间后仅剩 {}，还差 {}。（NoSpace）",
                    inplace::human_bytes(preflight.needed),
                    inplace::human_bytes(headroom),
                    inplace::human_bytes(preflight.needed.saturating_sub(headroom))
                );
                job.finished_at = Some(now());
                self.put(job)?;
                return Ok(());
            }

            ledger.insert(
                volume_root.clone(),
                current_reserved.saturating_add(preflight.needed),
            );
            InplaceVolumeClaim {
                volume: volume_root.clone(),
                amount: preflight.needed,
                ledger: self.inplace_reservations.clone(),
            }
        };

        self.stage(
            job,
            "verifying",
            "正在通读压缩包并核对每个条目的校验和；通过后才会释放空间。耗时取决于磁盘速度。",
        )?;

        loop {
            let progress = Arc::new(inplace::Progress::default());
            let stop = Arc::new(AtomicBool::new(false));
            let _stop_guard = StopOnDrop(stop.clone());
            let mut handle = tokio::task::spawn_blocking({
                let request = request.clone();
                let progress = progress.clone();
                let stop = stop.clone();
                move || inplace::execute(&request, &progress, &stop)
            });
            let mut ticker = tokio::time::interval(PROGRESS_TICK);
            let result = loop {
                tokio::select! {
                    joined = &mut handle => break joined.map_err(|_| "InplaceFailed".to_string())?,
                    _ = ticker.tick() => {
                        if cancel.load(Ordering::Acquire)
                            || self.paused.load(Ordering::Acquire)
                            || self.stop_batch.load(Ordering::Acquire)
                        {
                            stop.store(true, Ordering::SeqCst);
                        }
                        if let Err(err) = self.inplace_progress(job, &progress) {
                            stop.store(true, Ordering::SeqCst);
                            let _ = handle.await;
                            return Err(err);
                        }
                    }
                }
            };

            match result {
                Ok(inplace::Executed::Finished { .. }) | Ok(inplace::Executed::AlreadyFinished) => {
                    return self.finish_inplace(job, &archive, &output, false).await;
                }
                Ok(inplace::Executed::Paused) => {
                    job.status = if cancel.load(Ordering::Acquire) {
                        "cancelled"
                    } else {
                        "interrupted"
                    }
                    .into();
                    job.stage = "finished".into();
                    job.message = format!(
                        "原地解压已暂停：压缩包已释放 {}，剩余 {}。重新开始该任务将从中断处继续，请勿删除压缩包旁的 .hatchzip 文件。",
                        inplace::human_bytes(progress.committed()),
                        inplace::human_bytes(progress.total().saturating_sub(progress.done()))
                    );
                    job.finished_at = Some(now());
                    return self.put(job);
                }
                Err(error) if error.code == "NeedsPassword" => {
                    // Ask once per attempt, exactly like the worker path; a
                    // supplied password joins the next attempt's candidates.
                    let Some(password) = self.ask_inplace_password(job, &cancel).await? else {
                        job.status = "failed".into();
                        job.stage = "finished".into();
                        job.message =
                            "密码库未命中，且未在时限内收到密码；压缩包尚未改动。补充密码库后可重试。"
                                .into();
                        job.finished_at = Some(now());
                        return self.put(job);
                    };
                    self.absorb_password(&mut request, job, password);
                    self.stage(job, "verifying", "已收到密码，正在重新校验压缩包。")?;
                }
                Err(error) => {
                    job.message = error.detail.clone();
                    return Err(error.code);
                }
            }
        }
    }

    fn absorb_password(&self, request: &mut inplace::Request, job: &JobView, password: String) {
        request.passwords = self.inplace_passwords(job);
        if !request.passwords.iter().any(|p| p == &password) {
            request.passwords.insert(0, password);
        }
    }

    fn inplace_passwords(&self, job: &JobView) -> Vec<String> {
        let group = job.options.password_group.trim();
        let mut raw_candidates = Vec::new();
        if let Ok(vault) = self.vault()
            && let Ok(candidates) = vault.candidates((!group.is_empty()).then_some(group), None)
        {
            raw_candidates = candidates;
        }

        let archive_p = Path::new(&job.source.path);
        let mut mined_hints = Vec::new();
        if let Some(file_name) = archive_p.file_name().and_then(|n| n.to_str()) {
            mined_hints.extend(scanner::extract_passwords_from_filename(file_name));
        }
        if let Ok(companion_policy) = self.execution_policy(&job.id) {
            mined_hints.extend(scanner::sniff_accompanying_passwords_authorized(
                archive_p,
                |path| {
                    companion_policy
                        .as_ref()
                        .is_none_or(|policy| policy.authorize_path(path, false).is_ok())
                },
            ));
        }
        let mut seen_mined = std::collections::HashSet::new();
        let mut unique_mined = Vec::new();
        for hint in mined_hints {
            if seen_mined.insert(hint.clone()) {
                unique_mined.push(hint);
            }
        }

        // Filename and companion-file hints are prioritized as dynamic candidates
        // (id = "mined-..."), tried automatically with top priority when no restricted
        // password group is configured. When a group is specified, hints may only
        // reorder pre-authorized group candidates.
        for (idx, hint) in unique_mined.into_iter().enumerate().rev() {
            if let Some(pos) = raw_candidates.iter().position(|c| c.secret == hint) {
                let matched = raw_candidates.remove(pos);
                raw_candidates.insert(0, matched);
            } else if job.options.password_group.is_empty() {
                raw_candidates.insert(
                    0,
                    Candidate {
                        id: format!("mined-{}-{}", job.id, idx),
                        label: "提取自文件名/伴生文件".into(),
                        secret: hint,
                        revision: 0,
                    },
                );
            }
        }

        let mutations = crate::vault::derive_candidate_mutations(&raw_candidates);
        for mut_cand in mutations {
            if !raw_candidates.iter().any(|c| c.secret == mut_cand.secret) {
                raw_candidates.push(mut_cand);
            }
        }

        let mut passwords: Vec<String> = Vec::new();
        if let Ok(map) = self.ephemeral_passwords.lock()
            && let Some(items) = map.get(&job.id)
        {
            for secret in items {
                if !passwords.iter().any(|p| p == secret.as_str()) {
                    passwords.push(secret.to_string());
                }
            }
        }

        for candidate in raw_candidates {
            if !passwords.iter().any(|p| p == &candidate.secret) {
                passwords.push(candidate.secret.clone());
            }
        }

        passwords
    }

    async fn ask_inplace_password(
        self: &Arc<Self>,
        job: &mut JobView,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Option<String>, String> {
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        if let Ok(mut pending) = self.pending_inputs.lock() {
            pending.insert(job.id.clone(), tx);
        }
        let _ = self
            .need_input_broadcaster
            .send(openunpack_protocol::NeedPasswordEvent {
                job_id: job.id.clone(),
                archive_path: job.source.path.clone(),
                timeout_seconds: PASSWORD_WAIT.as_secs() as u32,
                input_type: "password".into(),
            });
        self.stage(
            job,
            "needsPassword",
            "压缩包已加密，密码库未命中；等待手动输入密码…",
        )?;
        let deadline = tokio::time::Instant::now() + PASSWORD_WAIT;
        let mut password = None;
        loop {
            if cancel.load(Ordering::Acquire) {
                if let Ok(mut pending) = self.pending_inputs.lock() {
                    pending.remove(&job.id);
                }
                return Ok(None);
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining.min(Duration::from_millis(200)), &mut rx).await {
                Ok(Ok(value)) => {
                    password = Some(value);
                    break;
                }
                Ok(Err(_)) => break,
                Err(_) => continue,
            }
        }
        if password
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            job.status = "running".into();
            job.message = "已收到密码，正在继续。".into();
            self.put(job)?;
            return Ok(password);
        }
        if let Ok(mut pending) = self.pending_inputs.lock() {
            pending.remove(&job.id);
        }
        Ok(None)
    }

    fn inplace_progress(
        &self,
        job: &mut JobView,
        progress: &Arc<inplace::Progress>,
    ) -> Result<(), String> {
        let phase = progress.phase();
        let stage = match phase {
            inplace::Phase::Verifying => "verifying",
            inplace::Phase::Extracting => "extracting",
            inplace::Phase::Idle => "checking",
        };
        let fraction = progress.fraction();
        let overall = match phase {
            inplace::Phase::Verifying => fraction * 0.2,
            inplace::Phase::Extracting => 0.2 + fraction * 0.8,
            inplace::Phase::Idle => 0.,
        };
        if job.stage != stage {
            job.stage = stage.into();
        }
        job.progress = overall.clamp(0., 1.);
        let message = match phase {
            inplace::Phase::Verifying => format!(
                "正在校验压缩包：{} / {}（此时压缩包仍未改动）",
                inplace::human_bytes(progress.done()),
                inplace::human_bytes(progress.total())
            ),
            inplace::Phase::Extracting => format!(
                "正在原地解压：{} / {}，已释放压缩包空间 {}",
                inplace::human_bytes(progress.done()),
                inplace::human_bytes(progress.total()),
                inplace::human_bytes(progress.committed())
            ),
            inplace::Phase::Idle => job.message.clone(),
        };
        job.message = message;
        self.put(job)
    }

    async fn finish_inplace(
        self: &Arc<Self>,
        job: &mut JobView,
        archive: &Path,
        output: &Path,
        recovered: bool,
    ) -> Result<(), String> {
        self.stage(
            job,
            "publishing",
            "正在为解压结果生成校验清单并提交发布记录。",
        )?;
        let files = {
            let archive = archive.to_path_buf();
            let output = output.to_path_buf();
            blocking(move || inplace::manifest(&archive, &output))
                .await
                .map_err(|e| e.code)?
        };
        let manifest_hash = paths::digest(&files)?;
        let publication = Publication {
            id: uid(),
            job_id: job.id.clone(),
            temporary_path: paths::path_string(output),
            final_path: paths::path_string(output),
            manifest_hash,
            state: "committed".into(),
            files,
            verification: "inPlaceVerified".into(),
        };
        job.output_path = Some(paths::path_string(output));
        job.status = "completed".into();
        job.verification_scope = "inPlaceVerified".into();
        job.stage = "finished".into();
        job.progress = 1.;
        job.finished_at = Some(now());
        job.message = if recovered {
            "原地解压已完成：上次运行在记录发布前结束，已核对输出并补登记。".into()
        } else {
            "原地解压完成：压缩包空间已随解压逐批释放，输出文件已按校验清单核对；原始压缩包不再保留。"
                .into()
        };
        self.repository.commit_publication(&publication, job, &[])?;
        self.put(job)?;
        if let Err(e) = inplace::forget_state(archive) {
            tracing::error!(
                archive = %archive.display(),
                reason = %e,
                "inplace_forget_state_failed"
            );
        }
        inplace::cleanup_archive(archive);
        self.apply_outbox();
        let _ = self.refresh_usage();
        Ok(())
    }
}

/// Where an in-place run puts its files: the archive's own folder unless an
/// output root is configured, in which case the archive's name is kept.
fn inplace_output(s: &Settings, archive: &Path) -> Result<PathBuf, String> {
    let default = inplace::default_output(archive).map_err(|error| error.code)?;
    let Some(name) = default.file_name() else {
        return Err("UnsafePath".into());
    };
    if s.output_root.is_empty() {
        return Ok(default);
    }
    let root = PathBuf::from(&s.output_root);
    paths::check_path(&root)?;
    let output = root.join(name);
    if output == archive {
        return Err("UnsafePath".into());
    }
    Ok(output)
}

async fn blocking<T, F>(work: F) -> Result<T, inplace::InplaceError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, inplace::InplaceError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| inplace::InplaceError::new("InplaceFailed", e.to_string()))?
}
