//! Node extraction, password solving, external input wait and node failure pipeline.
use super::execution::{Decoded, NodeOutcome, NodeRun};
use super::*;

impl Coordinator {
    /// Gathers the ordered password candidates for one node, including the
    /// ephemeral and filename/companion hints that may only *reorder* them.
    pub(super) fn collect_candidates(
        self: &Arc<Self>,
        run: &mut NodeRun<'_>,
        node_fingerprint: Option<String>,
    ) -> Result<Vec<Candidate>, String> {
        let s = run.settings;
        let mut candidates = prioritize_snapshot(
            run.snapshot,
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
        if let Ok(ephem_map) = self.ephemeral_passwords.lock()
            && let Some(list) = ephem_map.get(&run.job.id)
        {
            for (idx, secret) in list.iter().enumerate() {
                if !candidates
                    .iter()
                    .any(|c| c.secret.as_str() == secret.as_str())
                {
                    candidates.insert(
                        idx,
                        Candidate {
                            id: format!("ephemeral-{}-{}", run.job.id, idx),
                            label: "即时输入密码".into(),
                            secret: secret.to_string(),
                            revision: 0,
                        },
                    );
                }
            }
        }

        let archive_p = Path::new(&run.node.group.path);
        let mut mined_hints = Vec::new();
        if let Some(file_name) = archive_p.file_name().and_then(|n| n.to_str()) {
            mined_hints.extend(scanner::extract_passwords_from_filename(file_name));
        }
        let companion_policy = self.execution_policy(&run.job.id)?;
        mined_hints.extend(scanner::sniff_accompanying_passwords_authorized(
            archive_p,
            |path| {
                companion_policy
                    .as_ref()
                    .is_none_or(|policy| policy.authorize_path(path, false).is_ok())
            },
        ));

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
            if let Some(pos) = candidates.iter().position(|c| c.secret == hint) {
                let matched = candidates.remove(pos);
                candidates.insert(0, matched);
            } else if s.password_group.is_empty() {
                candidates.insert(
                    0,
                    Candidate {
                        id: format!("mined-{}-{}", run.job.id, idx),
                        label: "提取自文件名/伴生文件".into(),
                        secret: hint,
                        revision: 0,
                    },
                );
            }
        }
        let mutations = crate::vault::derive_candidate_mutations(&candidates);
        for mut_cand in mutations {
            if !candidates.iter().any(|c| c.secret == mut_cand.secret) {
                candidates.push(mut_cand);
            }
        }
        Ok(candidates)
    }

    /// Probes the archive with the candidate batches and returns the attempts
    /// to try, the failure to report, and whether extraction must be skipped.
    pub(super) async fn probe_attempts(
        self: &Arc<Self>,
        run: &mut NodeRun<'_>,
        candidates: &[Candidate],
    ) -> Result<(Vec<Option<Candidate>>, String, bool), String> {
        let s = run.settings;
        let mut attempts_to_try: Vec<Option<Candidate>> = Vec::new();
        let mut failure = "NeedsPassword".to_string();
        let mut skip_extraction = false;

        // A request carries at most the negotiated candidate batch limit.
        // Empty vaults still probe once to distinguish ordinary archives.
        let batches: Vec<&[Candidate]> = if candidates.is_empty() {
            vec![&[]]
        } else {
            candidates.chunks(128).collect()
        };
        for (batch_index, batch) in batches.iter().enumerate() {
            let mut probe_req = request("probe", run.work, s);
            probe_req.archive_path = run.node.group.path.clone();
            probe_req.allowed_volumes = run.node.group.volumes.clone();
            probe_req.max_entries = s.max_entries;
            probe_req.max_total_bytes = s.max_expanded_bytes;
            probe_req.max_written_bytes = s.max_expanded_bytes;
            probe_req.candidates = batch
                .iter()
                .map(|candidate| CandidateItem {
                    candidate_id: candidate.id.clone(),
                    source: if candidate.id.starts_with("ephemeral-")
                        || candidate.id.starts_with("mined-")
                    {
                        "ephemeral".into()
                    } else {
                        "vault".into()
                    },
                    revision: candidate.revision,
                    label: candidate.label.clone(),
                    secret: candidate.secret.clone(),
                })
                .collect();
            match self.request(run.job, probe_req, run.cancel.clone()).await {
                Ok(probe_event) => match probe_event.probe_status.as_deref() {
                    Some("NoPasswordNeeded") => {
                        attempts_to_try.push(None);
                        break;
                    }
                    Some("Success") => {
                        let candidate = probe_event
                            .matched_candidate_id
                            .as_deref()
                            .and_then(|matched| {
                                batch.iter().find(|candidate| candidate.id == matched)
                            })
                            .ok_or("WorkerProtocolError")?;
                        attempts_to_try.push(Some(candidate.clone()));
                        break;
                    }
                    Some("WrongPassword") => {
                        if batch_index + 1 < batches.len() {
                            continue;
                        }
                        failure = "NeedsPassword".into();
                        skip_extraction = true;
                        break;
                    }
                    Some("MissingVolume") => {
                        failure = "MissingVolume".into();
                        skip_extraction = true;
                        break;
                    }
                    Some("Corrupt") => {
                        if s.salvage_corrupted {
                            attempts_to_try.push(None);
                            attempts_to_try.extend(candidates.iter().cloned().map(Some));
                        } else {
                            failure = "Corrupt".into();
                            skip_extraction = true;
                        }
                        break;
                    }
                    Some("Unsupported") => {
                        failure = "Unsupported".into();
                        skip_extraction = true;
                        break;
                    }
                    _ => {
                        if batch_index + 1 < batches.len() {
                            continue;
                        }
                        attempts_to_try.push(None);
                        attempts_to_try.extend(candidates.iter().cloned().map(Some));
                        break;
                    }
                },
                Err(code)
                    if matches!(
                        code.as_str(),
                        "Cancelled"
                            | "WorkerExitUnconfirmed"
                            | "WorkerProtocolError"
                            | "ProtocolError"
                    ) =>
                {
                    return Err(code);
                }
                Err(_) => {
                    if batch_index + 1 < batches.len() {
                        continue;
                    }
                    attempts_to_try.push(None);
                    attempts_to_try.extend(candidates.iter().cloned().map(Some));
                    break;
                }
            }
        }
        Ok((attempts_to_try, failure, skip_extraction))
    }

    /// Tries each candidate in turn until one extraction succeeds.
    pub(super) async fn try_extract(
        self: &Arc<Self>,
        run: &mut NodeRun<'_>,
        attempts: Vec<Option<Candidate>>,
        node_fingerprint: Option<String>,
    ) -> Result<(Option<Decoded>, String), String> {
        let s = run.settings;
        let mut failure = "NeedsPassword".to_string();
        for candidate_opt in attempts {
            self.checkpoint(&run.cancel).await?;
            let remaining = s.max_expanded_bytes.saturating_sub(run.state.expanded);
            if remaining == 0 || run.state.entries >= s.max_entries {
                return Err("ResourceLimit".into());
            }
            let needed = if matches!(run.route, ExtractionRoute::SsdCache { .. }) {
                self.extraction_size_hint(
                    run.job,
                    run.node,
                    run.work,
                    remaining,
                    candidate_opt.as_ref(),
                    run.cancel.clone(),
                )
                .await?
            } else {
                None
            };
            let claim = self.claim_budget_for_job(&run.job.id, s, run.route, run.parent, needed)?;
            let budget = claim
                .budget()
                .min(s.max_expanded_bytes.saturating_sub(run.state.expanded));
            if budget == 0 || run.state.entries >= s.max_entries {
                return Err("ResourceLimit".into());
            }
            let dest = run.work.join(format!("node-{}-{}", run.node.id, uid()));
            paths::DirectoryGuard::create(&dest).map_err(|_| "UnsafePath")?;
            let mut req = request("extract", &dest, s);
            req.archive_path = run.node.group.path.clone();
            req.allowed_volumes = run.node.group.volumes.clone();
            req.max_entries = s.max_entries - run.state.entries;
            req.max_total_bytes = budget;
            req.max_written_bytes = budget;
            if let Some(c) = &candidate_opt {
                req.password = Some(c.secret.clone());
            }
            let topo = self.job_disk_topology(Path::new(&run.node.group.path), &dest);
            if topo.source_is_hdd || topo.target_is_hdd {
                req.engine_threads =
                    resolve_engine_threads_with_topology(s.engine_threads, s.concurrency, true);
            }
            run.state.views[run.index].attempts += 1;
            run.job.nodes = run.state.views.clone();
            save_run_async(run.work, run.state).await?;
            self.put(run.job)?;
            match self.request(run.job, req, run.cancel.clone()).await {
                Ok(mut event) => {
                    let mut event_success = None;
                    if event.password_used
                        && let Some(candidate) = &candidate_opt
                    {
                        run.state.views[run.index].password_label = Some(candidate.label.clone());
                        if !candidate.id.starts_with("ephemeral-")
                            && !candidate.id.starts_with("mined-")
                        {
                            event_success = Some(SuccessEvent {
                                event_id: uid(),
                                password_id: candidate.id.clone(),
                                password_revision: candidate.revision,
                                fingerprint: node_fingerprint.clone(),
                            });
                        }
                    }
                    return Ok((
                        Some(Decoded {
                            dest,
                            files: std::mem::take(&mut event.files),
                            success: event_success,
                            verification_scope: std::mem::take(&mut event.verification_scope),
                        }),
                        failure,
                    ));
                }
                Err(code)
                    if matches!(code.as_str(), "NeedsPassword" | "PasswordMismatchOrCorrupt") =>
                {
                    failure = code;
                    paths::remove_inside_owned(&dest, run.work, &run.job.id)?;
                }
                Err(code)
                    if matches!(
                        code.as_str(),
                        "Corrupt"
                            | "CorruptArchive"
                            | "ExtractionFailed"
                            | "Unsupported"
                            | "MissingVolume"
                            | "UnsafePath"
                    ) =>
                {
                    failure = if matches!(code.as_str(), "CorruptArchive" | "ExtractionFailed") {
                        "Corrupt".into()
                    } else {
                        code
                    };
                    paths::remove_inside_owned(&dest, run.work, &run.job.id)?;
                    break;
                }
                Err(code) => return Err(code),
            }
        }
        Ok((None, failure))
    }

    /// Records a node that produced no output: asks the client for a password
    /// when one is missing, and otherwise persists the failed node.
    pub(super) async fn settle_failed_node(
        self: &Arc<Self>,
        run: &mut NodeRun<'_>,
        failure: String,
    ) -> Result<NodeOutcome, String> {
        if matches!(
            failure.as_str(),
            "NeedsPassword" | "PasswordMismatchOrCorrupt"
        ) {
            let (tx, rx) = tokio::sync::oneshot::channel();
            if let Ok(mut pending) = self.pending_inputs.lock() {
                pending.insert(run.job.id.clone(), tx);
            }
            run.state.views[run.index].status = "needsPassword".into();
            run.state.views[run.index].message = "密码库未命中，等待外部密码输入回调...".into();
            run.job.nodes = run.state.views.clone();
            self.stage(
                run.job,
                "needsPassword",
                "密码库遍历完毕，等待外部客户端提供密码...",
            )?;
            let need_event = openunpack_protocol::NeedPasswordEvent {
                job_id: run.job.id.clone(),
                archive_path: run.node.group.path.clone(),
                timeout_seconds: 60,
                input_type: "password".into(),
            };
            let _ = self.need_input_broadcaster.send(need_event);

            let external = self.await_external_password(run, rx).await?;
            if let Some(external_pwd) = external {
                if external_pwd != "\0__OPENUNPACK_VAULT_RETRY__"
                    && let Ok(mut ephem_map) = self.ephemeral_passwords.lock()
                {
                    let entry = ephem_map.entry(run.job.id.clone()).or_default();
                    if !entry.iter().any(|s| s.as_str() == external_pwd.as_str()) {
                        entry.insert(0, zeroize::Zeroizing::new(external_pwd));
                    }
                }
                run.state.pending.push_front(run.node.clone());
                save_run_async(run.work, run.state).await?;
                return Ok(NodeOutcome::Retry);
            }
            if let Ok(mut pending) = self.pending_inputs.lock() {
                pending.remove(&run.job.id);
            }
        }
        run.state.views[run.index].status = if matches!(
            failure.as_str(),
            "NeedsPassword" | "PasswordMismatchOrCorrupt"
        ) {
            "needsPassword"
        } else {
            "failed"
        }
        .into();
        run.state.views[run.index].message = message(&failure);
        run.state.failed.push((run.node.clone(), failure.clone()));
        save_run_async(run.work, run.state).await?;
        run.job.nodes = run.state.views.clone();
        self.put(run.job)?;
        if run.node.depth == 0 {
            return Err(if failure == "PasswordMismatchOrCorrupt" {
                "NeedsPassword".into()
            } else {
                failure
            });
        }
        Ok(NodeOutcome::Advance)
    }

    /// Waits for an externally supplied password while staying cancellable.
    ///
    /// A plain `timeout` kept the request parked for the whole minute even
    /// after a cancel. Polling the cancel set alongside the reply makes
    /// cancelling during the wait return at once.
    async fn await_external_password(
        self: &Arc<Self>,
        run: &mut NodeRun<'_>,
        mut receiver: tokio::sync::oneshot::Receiver<String>,
    ) -> Result<Option<String>, String> {
        let has_external_client = self.execution_policy(&run.job.id).ok().flatten().is_some()
            || self.need_input_broadcaster.receiver_count() > 0;
        let timeout_secs = std::env::var("OPENUNPACK_NEED_PASSWORD_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| {
                if cfg!(test)
                    || std::env::var_os("CARGO_MANIFEST_DIR").is_some()
                    || !has_external_client
                {
                    1
                } else {
                    60
                }
            });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
        loop {
            let cancelled = self.stop_batch.load(Ordering::Acquire)
                || run.cancel.load(Ordering::Acquire)
                || self
                    .cancelled_jobs
                    .lock()
                    .map(|set| set.contains(&run.job.id))
                    .unwrap_or(false);
            if cancelled {
                if let Ok(mut pending) = self.pending_inputs.lock() {
                    pending.remove(&run.job.id);
                }
                return Err("Cancelled".into());
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            match tokio::time::timeout(remaining.min(Duration::from_millis(200)), &mut receiver)
                .await
            {
                Ok(Ok(value)) => {
                    if value == "\0__OPENUNPACK_SKIP__" {
                        return Ok(None);
                    }
                    return Ok(Some(value).filter(|value: &String| !value.trim().is_empty()));
                }
                Ok(Err(_)) => return Ok(None),
                Err(_) => continue,
            }
        }
    }
}
