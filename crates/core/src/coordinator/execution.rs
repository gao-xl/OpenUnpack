use super::*;

impl Coordinator {
    pub(super) async fn extraction_size_hint(
        self: &Arc<Self>,
        job: &mut JobView,
        node: &NodeWork,
        work: &Path,
        remaining_bytes: u64,
        candidate: Option<&Candidate>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Option<u64>, String> {
        let mut req = request("inspect", work, &job.options);
        req.archive_path = node.group.path.clone();
        req.allowed_volumes = node.group.volumes.clone();
        req.max_total_bytes = remaining_bytes;
        req.password = candidate.map(|candidate| candidate.secret.clone());
        match self.request(job, req, cancel).await {
            Ok(event) if event.total_bytes_known => Ok(Some(event.total_bytes.max(1))),
            Ok(_) => Ok(None),
            // Encrypted/damaged headers give no trustworthy demand. Extraction
            // still enforces the full conservative envelope, including salvage.
            Err(code) if matches!(code.as_str(), "WrongPassword" | "Corrupt") => Ok(None),
            Err(code) => Err(code),
        }
    }
    pub(super) async fn run_job(
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
        self.authorize_execution(job, &source)?;
        if source.missing_volume {
            return Err("MissingVolume".into());
        }
        let witnesses = source_witnesses(&source)?;
        let mut route = self.decide_route(&s, &source)?;
        let mut parent = self.workspace_parent(&s, &source, &route)?;
        // A run owns one immutable extraction configuration. Output/verification changes
        // can reuse sealed nodes; extraction-rule changes require a new generation.
        let mut resume = None;
        let mut workspace_lease = None;
        if let Some(old) = &job.workspace {
            let old_path = Path::new(old);
            workspace_lease = Some(lock_private_workspace(old_path)?);
            paths::owned(old_path, &job.id, None)?;
            if old_path.join(RUN_FILE).is_file() {
                let saved: SavedRun = read_run(old_path)?;
                if !matches!(saved.version, 1 | 2) || saved.job_id != job.id {
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
            // A changed policy/free-space probe can select a different new
            // workspace. Resumption must budget the retained directory itself.
            (route, parent) = resumed_storage_route(
                &saved.options,
                &saved.source,
                &work,
                output_root(&s, &source)?,
            )?;
            if saved.expanded > s.max_expanded_bytes || saved.entries > s.max_entries {
                return Err("ResourceLimit".into());
            }
            let migrated = saved.version == 1;
            if migrated {
                upgrade_legacy_run(&mut saved)?;
            }
            validate_saved_paths(&work, &saved)?;
            self.stage(
                job,
                "checking",
                "正在复核已完成的工作区；通过后仅重试未完成内包。",
            )?;
            let input = work.join("input");
            if !saved.input_files.is_empty() && (!saved.input_retired || input.exists()) {
                let expected: Vec<_> = saved
                    .input_files
                    .iter()
                    .filter(|file| !saved.input_retired || Path::new(&file.source_path).exists())
                    .cloned()
                    .collect();
                validate_target_tree(&input, &expected)?;
                let mut req = request("readback", &input, &s);
                req.files = expected;
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
            if migrated {
                // Do not commit inferred v1 retirement until every retained byte
                // has passed the same readback verification as a native v2 run.
                save_run(&work, &saved)?;
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
                return Err("无可用缓存目录，请重试整个任务。".into());
            }
            // Retiring an old generation or another job can prune this shared
            // cache parent. Pin it until the new workspace owns its own lease.
            let parent_lease = paths::DirectoryGuard::create(&parent).map_err(|_| "UnsafePath")?;
            if let Some(old) = job.workspace.clone() {
                drop(workspace_lease.take());
                // Keep the workspace association until removal actually succeeds: a
                // partial cleanup must stay retryable and keep its UI cleanup entry.
                if let Err(code) = paths::remove_owned_with_retry(Path::new(&old), &job.id).await {
                    job.message = message(&code);
                    self.put(job)?;
                    return Err(code);
                }
                job.workspace = None;
            }
            let work = parent.join(format!("{}-{}", job.id, job.attempts));
            paths::mark_owned(&work, &job.id, "")?;
            workspace_lease = Some(lock_private_workspace(&work)?);
            drop(parent_lease);
            job.workspace = Some(paths::path_string(&work));
            job.nodes.clear();
            job.stage_times.clear();
            let should_cache_input = match &route {
                ExtractionRoute::SsdCache { score, reason, .. } => {
                    job.message = format!("使用中转缓存 [评分: {}]: {}", score, reason);
                    s.cache_enabled && s.cache_input
                }
                ExtractionRoute::Direct { reason, .. } => {
                    job.message = format!("直接解压模式: {}", reason);
                    false
                }
            };
            self.put(job)?;
            let mut group = source.clone();
            let mut fingerprint = None;
            let mut input_files = vec![];
            if should_cache_input {
                self.stage(job, "caching", "正在将分卷文件复制到缓存。")?;
                let claim =
                    self.claim_budget_for_job(&job.id, &s, &route, &parent, Some(source.size))?;
                let budget = claim.budget();
                if source.size > budget {
                    return Err("ResourceLimit".into());
                }
                let input = work.join("input");
                paths::DirectoryGuard::create(&input).map_err(|_| "UnsafePath")?;
                let files = source
                    .volumes
                    .iter()
                    .map(|p| {
                        let path = Path::new(p);
                        CopyFile {
                            size_known: true,
                            source_path: p.clone(),
                            relative_path: path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into(),
                            size: fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX),
                            sha256: String::new(),
                            is_directory: false,
                            is_corrupted: false,
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
                        size_known: true,
                        source_path: paths::path_string(
                            &input.join(paths::relative(&f.relative_path)?),
                        ),
                        relative_path: f.relative_path.clone(),
                        size: f.size,
                        sha256: f.sha256.clone(),
                        is_directory: false,
                        is_corrupted: f.is_corrupted,
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
                version: 2,
                job_id: job.id.clone(),
                source: source.clone(),
                witnesses: witnesses.clone(),
                options: s.clone(),
                input_files,
                input_retired: false,
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
                    .push(node_view(&node, "failed", "内层包未完成，保留原压缩文件。"));
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
                "正在解压并尝试已保存的密码。",
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
            if let Ok(ephem_map) = self.ephemeral_passwords.lock()
                && let Some(list) = ephem_map.get(&job.id)
            {
                for (idx, secret) in list.iter().enumerate() {
                    if !candidates
                        .iter()
                        .any(|c| c.secret.as_str() == secret.as_str())
                    {
                        candidates.insert(
                            idx,
                            Candidate {
                                id: format!("ephemeral-{}-{}", job.id, idx),
                                label: "即时输入密码".into(),
                                secret: secret.to_string(),
                                revision: 0,
                            },
                        );
                    }
                }
            }
            let archive_p = Path::new(&node.group.path);
            let mut mined_hints = Vec::new();
            if let Some(file_name) = archive_p.file_name().and_then(|n| n.to_str()) {
                mined_hints.extend(scanner::extract_passwords_from_filename(file_name));
            }
            let companion_policy = self.execution_policy(&job.id)?;
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

            // Filename and companion-file hints can prioritize an already
            // authorized candidate, never introduce an unsaved/group-excluded secret.
            for hint in unique_mined.into_iter().rev() {
                if let Some(pos) = candidates.iter().position(|c| c.secret == hint) {
                    let matched = candidates.remove(pos);
                    candidates.insert(0, matched);
                }
            }
            let mutations = crate::vault::derive_candidate_mutations(&candidates);
            for mut_cand in mutations {
                if !candidates.iter().any(|c| c.secret == mut_cand.secret) {
                    candidates.push(mut_cand);
                }
            }
            let mut decoded = None;
            let mut failure = "NeedsPassword".to_string();
            if node.group.missing_volume {
                failure = "MissingVolume".into();
            } else {
                let mut attempts_to_try: Vec<Option<Candidate>> = Vec::new();
                let mut skip_extraction = false;

                // A request carries at most the negotiated candidate batch limit.
                // Empty vaults still probe once to distinguish ordinary archives.
                let batches: Vec<&[Candidate]> = if candidates.is_empty() {
                    vec![&[]]
                } else {
                    candidates.chunks(128).collect()
                };
                for (batch_index, batch) in batches.iter().enumerate() {
                    let mut probe_req = request("probe", &work, &s);
                    probe_req.archive_path = node.group.path.clone();
                    probe_req.allowed_volumes = node.group.volumes.clone();
                    probe_req.max_entries = s.max_entries;
                    probe_req.max_total_bytes = s.max_expanded_bytes;
                    probe_req.max_written_bytes = s.max_expanded_bytes;
                    probe_req.candidates = batch
                        .iter()
                        .map(|candidate| CandidateItem {
                            candidate_id: candidate.id.clone(),
                            source: if candidate.id.starts_with("ephemeral-") {
                                "ephemeral".into()
                            } else {
                                "vault".into()
                            },
                            revision: candidate.revision,
                            label: candidate.label.clone(),
                            secret: candidate.secret.clone(),
                        })
                        .collect();
                    match self.request(job, probe_req, cancel.clone()).await {
                        Ok(probe_event) => match probe_event.probe_status.as_deref() {
                            Some("NoPasswordNeeded") => {
                                attempts_to_try.push(None);
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
                            }
                            Some("WrongPassword") => {
                                if batch_index + 1 < batches.len() {
                                    continue;
                                }
                                failure = "NeedsPassword".into();
                                skip_extraction = true;
                            }
                            Some("MissingVolume") => {
                                failure = "MissingVolume".into();
                                skip_extraction = true;
                            }
                            Some("Corrupt") => {
                                if s.salvage_corrupted {
                                    attempts_to_try.push(None);
                                    attempts_to_try.extend(candidates.iter().cloned().map(Some));
                                } else {
                                    failure = "Corrupt".into();
                                    skip_extraction = true;
                                }
                            }
                            Some("Unsupported") => {
                                failure = "Unsupported".into();
                                skip_extraction = true;
                            }
                            _ => {
                                attempts_to_try.push(None);
                                attempts_to_try.extend(candidates.iter().cloned().map(Some));
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
                            attempts_to_try.push(None);
                            attempts_to_try.extend(candidates.iter().cloned().map(Some));
                        }
                    }
                    break;
                }

                if !skip_extraction {
                    for candidate_opt in attempts_to_try {
                        self.checkpoint(&cancel).await?;
                        let remaining = s.max_expanded_bytes.saturating_sub(state.expanded);
                        if remaining == 0 || state.entries >= s.max_entries {
                            return Err("ResourceLimit".into());
                        }
                        let needed = if matches!(route, ExtractionRoute::SsdCache { .. }) {
                            self.extraction_size_hint(
                                job,
                                &node,
                                &work,
                                remaining,
                                candidate_opt.as_ref(),
                                cancel.clone(),
                            )
                            .await?
                        } else {
                            None
                        };
                        let claim =
                            self.claim_budget_for_job(&job.id, &s, &route, &parent, needed)?;
                        let budget = claim
                            .budget()
                            .min(s.max_expanded_bytes.saturating_sub(state.expanded));
                        if budget == 0 || state.entries >= s.max_entries {
                            return Err("ResourceLimit".into());
                        }
                        let dest = work.join(format!("node-{}-{}", node.id, uid()));
                        paths::DirectoryGuard::create(&dest).map_err(|_| "UnsafePath")?;
                        let mut req = request("extract", &dest, &s);
                        req.archive_path = node.group.path.clone();
                        req.allowed_volumes = node.group.volumes.clone();
                        req.max_entries = s.max_entries - state.entries;
                        req.max_total_bytes = budget;
                        req.max_written_bytes = budget;
                        if let Some(c) = &candidate_opt {
                            req.password = Some(c.secret.clone());
                        }
                        let topo = self.job_disk_topology(Path::new(&node.group.path), &dest);
                        if topo.source_is_hdd || topo.target_is_hdd {
                            req.engine_threads = resolve_engine_threads_with_topology(
                                s.engine_threads,
                                s.concurrency,
                                true,
                            );
                        }
                        state.views[node_index].attempts += 1;
                        job.nodes = state.views.clone();
                        save_run(&work, &state)?;
                        self.put(job)?;
                        match self.request(job, req, cancel.clone()).await {
                            Ok(mut event) => {
                                let mut event_success = None;
                                if event.password_used
                                    && let Some(candidate) = &candidate_opt
                                {
                                    state.views[node_index].password_label =
                                        Some(candidate.label.clone());
                                    if !candidate.id.starts_with("ephemeral-") {
                                        event_success = Some(SuccessEvent {
                                            event_id: uid(),
                                            password_id: candidate.id.clone(),
                                            password_revision: candidate.revision,
                                            fingerprint: node_fingerprint.clone(),
                                        });
                                    }
                                }
                                decoded = Some((
                                    dest,
                                    std::mem::take(&mut event.files),
                                    event_success,
                                    std::mem::take(&mut event.verification_scope),
                                ));
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
            let Some((dest, files, event_success, verification_scope)) = decoded else {
                if matches!(
                    failure.as_str(),
                    "NeedsPassword" | "PasswordMismatchOrCorrupt"
                ) {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    if let Ok(mut pending) = self.pending_inputs.lock() {
                        pending.insert(job.id.clone(), tx);
                    }
                    state.views[node_index].status = "needsPassword".into();
                    state.views[node_index].message =
                        "密码库未命中，等待外部密码输入回调...".into();
                    job.nodes = state.views.clone();
                    self.stage(
                        job,
                        "needsPassword",
                        "密码库遍历完毕，等待外部客户端提供密码...",
                    )?;
                    let need_event = openunpack_protocol::NeedPasswordEvent {
                        job_id: job.id.clone(),
                        archive_path: node.group.path.clone(),
                        timeout_seconds: 60,
                        input_type: "password".into(),
                    };
                    let _ = self.need_input_broadcaster.send(need_event);

                    // A plain `timeout` kept the request parked for the whole
                    // minute even after a cancel. Poll the cancel set alongside
                    // the reply so cancelling during the wait returns at once.
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
                    let mut receiver = rx;
                    let mut external: Option<String> = None;
                    loop {
                        let cancelled = self
                            .cancelled_jobs
                            .lock()
                            .map(|set| set.contains(&job.id))
                            .unwrap_or(false);
                        if cancelled {
                            if let Ok(mut pending) = self.pending_inputs.lock() {
                                pending.remove(&job.id);
                            }
                            return Err("Cancelled".into());
                        }
                        let remaining =
                            deadline.saturating_duration_since(tokio::time::Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        match tokio::time::timeout(
                            remaining.min(Duration::from_millis(200)),
                            &mut receiver,
                        )
                        .await
                        {
                            Ok(Ok(value)) => {
                                external = Some(value);
                                break;
                            }
                            Ok(Err(_)) => break,
                            Err(_) => continue,
                        }
                    }
                    if let Some(external_pwd) = external.filter(|value| !value.trim().is_empty()) {
                        if let Ok(mut ephem_map) = self.ephemeral_passwords.lock() {
                            ephem_map
                                .entry(job.id.clone())
                                .or_default()
                                .push(zeroize::Zeroizing::new(external_pwd.clone()));
                        }
                        candidates.insert(
                            0,
                            Candidate {
                                id: format!("callback-{}-{}", job.id, uid()),
                                label: "外部输入密码".into(),
                                secret: external_pwd,
                                revision: 0,
                            },
                        );
                        state.pending.push_front(node.clone());
                        save_run(&work, &state)?;
                        continue;
                    }
                    if let Ok(mut pending) = self.pending_inputs.lock() {
                        pending.remove(&job.id);
                    }
                }
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
                state.retired_files.extend(consumed);
            }
            let fixed_relatives: Option<Vec<PathBuf>> = if s.auto_fix_encoding {
                crate::zip_encoding::probe_zip_encoding(Path::new(&node.group.path))
                    .ok()
                    .and_then(|probe| {
                        if probe.detected_encoding == "UTF-8"
                            || probe.detected_encoding == "ASCII"
                            || probe.decoded_paths.len() != files.len()
                        {
                            return None;
                        }
                        let mut resolved = Vec::with_capacity(files.len());
                        let mut names = HashSet::new();
                        for decoded in &probe.decoded_paths {
                            let rel = paths::relative(decoded).ok()?;
                            let key = paths::path_string(&rel).to_lowercase();
                            if !names.insert(key) {
                                return None;
                            }
                            resolved.push(rel);
                        }
                        Some(resolved)
                    })
            } else {
                None
            };

            for (file_idx, file) in files.iter().enumerate() {
                let orig_relative = paths::relative(&file.relative_path)?;
                let dest_relative = fixed_relatives
                    .as_ref()
                    .and_then(|fixed| fixed.get(file_idx))
                    .cloned()
                    .unwrap_or_else(|| orig_relative.clone());
                state.final_files.push(CopyFile {
                    size_known: true,
                    source_path: paths::path_string(&dest.join(&orig_relative)),
                    relative_path: paths::path_string(&node.prefix.join(dest_relative)),
                    size: file.size,
                    sha256: file.sha256.clone(),
                    is_directory: file.is_directory,
                    is_corrupted: file.is_corrupted,
                });
            }
            let node_corrupted =
                files.iter().any(|f| f.is_corrupted) || verification_scope.contains("salvaged");
            if !node_corrupted && let Some(event) = event_success {
                state.events.push(event);
            }
            state.sealed.push(SealedNode {
                node: node.clone(),
                directory: dest.clone(),
                files,
                verification_scope,
            });
            state.views[node_index].status = if node_corrupted {
                "salvaged".into()
            } else {
                "completed".into()
            };
            state.views[node_index].stage = "finished".into();
            state.views[node_index].progress = 1.;
            let mut has_child_archives = false;
            if s.expand_nested {
                let nested = scanner::scan(&[paths::path_string(&dest)], true, &[])?;
                if node.depth + 1 < s.max_depth {
                    for child in nested.sources {
                        has_child_archives = true;
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
            if node.depth == 0 && !state.input_files.is_empty() {
                state.input_retired = true;
            }
            // Persist the successful replacement and consumption intent first.
            // A crash or failed deletion then permits absence only for these
            // retired inputs; any bytes left behind must still pass readback.
            save_run(&work, &state)?;
            // Only clean up inner archive volumes if it's a leaf node without children.
            // If node spawned child archives, keep the intermediate archive intact
            // until all descendant nodes complete and the derivation tree closes.
            if node.depth > 0 && !has_child_archives {
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
        let tree_closed = is_derivation_tree_closed(&state);
        if tree_closed {
            // Bottom-up retirement: clean up inner archives in reverse depth order
            let mut sealed_nodes = state.sealed.clone();
            sealed_nodes.sort_by_key(|s| std::cmp::Reverse(s.node.depth));
            for sealed in &sealed_nodes {
                if sealed.node.depth > 0 {
                    for volume in &sealed.node.group.volumes {
                        let _ = paths::remove_file_inside_owned(Path::new(volume), &work, &job.id);
                    }
                }
            }
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
        verify_witnesses(&source, &witnesses)?;
        self.checkpoint(&cancel).await?;
        self.stage(
            job,
            "planning",
            "正在生成最终文件映射；同名目录按策略处理。",
        )?;
        let Some(staged) = self.publications.prepare(
            &self.repository,
            job,
            &state,
            &source,
            &work,
            &route,
            tree_closed,
        )?
        else {
            job.status = "skipped".into();
            if job.message.is_empty() {
                job.message = "同名目标已存在，按设置跳过；原包保留。".into();
            }
            job.finished_at = Some(now());
            drop(workspace_lease.take());
            paths::remove_owned_with_retry(&work, &job.id).await?;
            job.workspace = None;
            return self.put(job);
        };
        if let Some(req) = staged.copy_request() {
            self.stage(job, "copying", "正在顺序写回目标盘，同时对比解压流摘要。")?;
            self.request(job, req, cancel.clone()).await?;
        }
        staged.finish_copy()?;
        let readback = staged.readback();
        if readback {
            self.checkpoint(&cancel).await?;
            self.stage(
                job,
                "verifying",
                "正在完整回读目标临时文件，耗时取决于目标盘。",
            )?;
        }
        let sealed = staged.seal(&self.repository, cancel.clone()).await?;
        if readback {
            *job.stage_times.entry("verifying".into()).or_default() += sealed.verification_millis();
        }
        self.checkpoint(&cancel).await?;
        self.stage(job, "publishing", "正在提交已验证的最终目录。")?;
        let Some(committed) = self.publications.commit(&self.repository, sealed, job)? else {
            job.status = "skipped".into();
            if job.message.is_empty() {
                job.message = "发布时发现同名目标，按策略跳过；原包保留。".into();
            }
            job.finished_at = Some(now());
            drop(workspace_lease.take());
            paths::remove_owned_with_retry(&work, &job.id).await?;
            job.workspace = None;
            self.put(job)?;
            return self.refresh_usage();
        };
        let mut msg = committed.message().to_owned();

        // Safe source archive cleanup (only when 100% completed, non-empty, no exclusions, witness verified, target tree verified, and derivation tree closed)
        if committed.cleanup_allowed() {
            // Pin every volume before checking the saved witnesses. The exact
            // objects checked here are retained until their delete/recycle ends.
            let cleanup_sources =
                lock_cleanup_sources(&source, &witnesses, s.source_cleanup == "recycle");
            let mut cleanup_ok = cleanup_sources.is_ok();
            if let Ok(sources) = cleanup_sources {
                for source_file in sources {
                    let _gate = self.mutations.lock().map_err(|_| lock_error())?;
                    if cancel.load(Ordering::Acquire)
                        || self
                            .deleting_jobs
                            .lock()
                            .map_err(|_| lock_error())?
                            .contains(&job.id)
                    {
                        cleanup_ok = false;
                        break;
                    }
                    if source_file.cleanup().is_err() {
                        cleanup_ok = false;
                        break;
                    }
                }
            }
            if cleanup_ok {
                msg = if s.source_cleanup == "recycle" {
                    "解压完成并校验通过；源压缩包已移入回收站。".to_string()
                } else {
                    "解压完成并校验通过；源压缩包已删除。".to_string()
                };
            } else {
                msg = "解压完成并校验通过；源文件自动清理未完成，剩余源文件保留。".to_string();
            }
        }
        drop(committed);
        drop(workspace_lease.take());
        job.message = msg;
        self.put(job)?;
        self.apply_outbox();
        match paths::remove_owned_with_retry(&work, &job.id).await {
            Ok(()) => job.workspace = None,
            Err(reason) => {
                eprintln!("[cache_retirement_failed] {reason}");
                job.status = "cleanupPending".into();
                job.message = "文件已解压完成，缓存清理失败待重试。".into();
            }
        }
        self.put(job)?;
        self.refresh_usage()
    }
}
