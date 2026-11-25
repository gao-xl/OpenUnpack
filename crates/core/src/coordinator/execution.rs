//! Job execution pipeline.
//!
//! `run_job` used to be a single 1033-line function: source resolution,
//! workspace resume, password probing, extraction, publication and cleanup all
//! shared one scope. Every new rule had to be threaded through that scope, and
//! no stage could be tested on its own. The pipeline is now split along its
//! natural seams:
//!
//! * [`Coordinator::resolve_source`] — validate and authorise the archive group.
//! * [`Coordinator::prepare_workspace`] — resume a saved run or create a new one.
//! * [`Coordinator::run_nodes`] — drive the derivation tree node by node.
//! * [`Coordinator::finalize_run`] — publish, verify and retire the workspace.
//!
//! The node body itself is split into candidate collection, probe, extraction,
//! external-password wait and commit. Behaviour is unchanged: every moved block
//! is the original code with its surrounding state passed explicitly.

use super::*;

/// A workspace that is ready for the node loop.
struct PreparedRun {
    source: SourceGroup,
    witnesses: Vec<SourceWitness>,
    route: ExtractionRoute,
    parent: PathBuf,
    work: PathBuf,
    state: SavedRun,
    lease: Option<paths::DirectoryGuard>,
}

/// Everything one node of the derivation tree may touch.
///
/// Grouping it keeps the helpers below at a readable arity and makes the
/// mutation points obvious: only `job`, `state` and `index` ever change.
pub(super) struct NodeRun<'a> {
    pub(super) job: &'a mut JobView,
    pub(super) state: &'a mut SavedRun,
    pub(super) node: &'a NodeWork,
    pub(super) index: usize,
    pub(super) work: &'a Path,
    pub(super) parent: &'a Path,
    pub(super) route: &'a ExtractionRoute,
    pub(super) settings: &'a Settings,
    pub(super) snapshot: &'a [Candidate],
    pub(super) cancel: Arc<AtomicBool>,
}

/// What the node loop should do with the node it just processed.
pub(super) enum NodeOutcome {
    /// The node was re-queued at the front (an external password arrived).
    Retry,
    /// The node is settled; move on to the next one.
    Advance,
}

/// A successful extraction of one node, before it is committed to the run.
pub(super) struct Decoded {
    pub(super) dest: PathBuf,
    pub(super) files: Vec<FileRecord>,
    pub(super) success: Option<SuccessEvent>,
    pub(super) verification_scope: String,
}

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
            Err(code)
                if matches!(
                    code.as_str(),
                    "WrongPassword"
                        | "Corrupt"
                        | "NeedsPassword"
                        | "PasswordMismatchOrCorrupt"
                        | "InspectFailed"
                ) =>
            {
                Ok(None)
            }
            Err(code) => Err(code),
        }
    }

    /// Runs one job from source resolution to workspace retirement.
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

        let (source, is_inplace) = self.resolve_source(job, &s).await?;
        if is_inplace {
            return self.run_inplace_job(job, &source, cancel).await;
        }
        let witnesses = source_witnesses(&source)?;
        let mut prepared = self
            .prepare_workspace(
                job,
                &s,
                &source,
                &witnesses,
                cancel.clone(),
                allow_partial,
                selected,
            )
            .await?;

        self.run_nodes(
            job,
            &s,
            &prepared.route,
            &prepared.parent,
            &prepared.work,
            &mut prepared.state,
            cancel.clone(),
            allow_partial,
        )
        .await?;

        self.finalize_run(
            job,
            &s,
            &prepared.source,
            &prepared.witnesses,
            &prepared.route,
            &prepared.work,
            &mut prepared.state,
            prepared.lease,
            cancel,
            allow_partial,
        )
        .await
    }

    /// Validates the source group and reports whether this is an in-place run.
    async fn resolve_source(
        self: &Arc<Self>,
        job: &mut JobView,
        s: &Settings,
    ) -> Result<(SourceGroup, bool), String> {
        let archive_path = Path::new(&job.source.path);
        let is_inplace = s.in_place();

        // 统一源信息与授权解析：若原地解压已有活动会话，则直接接续；否则进行完整包扫描与完整性检查
        let source = if is_inplace
            && openunpack_infrastructure::engine::inplace::has_session(archive_path)
        {
            let mut source = job.source.clone();
            source.missing_volume = false;
            self.authorize_execution(job, &source)?;
            source
        } else {
            let path = job.source.path.clone();
            let scan = tokio::task::spawn_blocking(move || {
                scanner::scan(std::slice::from_ref(&path), false, &[])
            })
            .await
            .map_err(|e| e.to_string())??;
            let source = scan
                .sources
                .into_iter()
                .find(|g| g.path.eq_ignore_ascii_case(&job.source.path))
                .ok_or("源压缩包已移动或不可读。")?;
            self.authorize_execution(job, &source)?;
            if source.missing_volume {
                return Err("MissingVolume".into());
            }
            source
        };
        Ok((source, is_inplace))
    }

    /// Reuses a saved run when it is byte-identical, otherwise creates a
    /// fresh workspace (copying volumes to the cache when routing asks for it).
    #[allow(clippy::too_many_arguments)]
    async fn prepare_workspace(
        self: &Arc<Self>,
        job: &mut JobView,
        s: &Settings,
        source: &SourceGroup,
        witnesses: &[SourceWitness],
        cancel: Arc<AtomicBool>,
        allow_partial: bool,
        selected: Option<&HashSet<String>>,
    ) -> Result<PreparedRun, String> {
        let source = source.clone();
        let witnesses = witnesses.to_vec();
        let mut route = self.decide_route(s, &source)?;
        let mut parent = self.workspace_parent(s, &source, &route)?;
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
                    && extraction_compatible(&saved.options, s)
                {
                    resume = Some((old_path.to_path_buf(), saved));
                }
            }
        }

        let (work, state) = if let Some((work, mut saved)) = resume {
            // A changed policy/free-space probe can select a different new
            // workspace. Resumption must budget the retained directory itself.
            (route, parent) = resumed_storage_route(
                &saved.options,
                &saved.source,
                &work,
                output_root(s, &source)?,
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
                let mut req = request("readback", &input, s);
                req.files = expected;
                self.request(job, req, cancel.clone()).await?;
            }
            for sealed in &saved.sealed {
                self.checkpoint(&cancel).await?;
                let expected = retained_sealed_files(sealed, &saved.retired_files)?;
                validate_target_tree(&sealed.directory, &expected)?;
                let mut req = request("readback", &sealed.directory, s);
                req.files = expected;
                self.request(job, req, cancel.clone()).await?;
            }
            if migrated {
                // Do not commit inferred v1 retirement until every retained byte
                // has passed the same readback verification as a native v2 run.
                save_run_async(&work, &saved).await?;
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
            self.create_fresh_workspace(
                job,
                s,
                &source,
                &witnesses,
                &route,
                &parent,
                &mut workspace_lease,
                cancel.clone(),
                allow_partial,
                selected,
            )
            .await?
        };

        Ok(PreparedRun {
            source,
            witnesses,
            route,
            parent,
            work,
            state,
            lease: workspace_lease,
        })
    }

    /// Builds a brand new workspace generation: retires the previous one,
    /// optionally stages the volumes in the SSD cache, and persists the plan.
    #[allow(clippy::too_many_arguments)]
    async fn create_fresh_workspace(
        self: &Arc<Self>,
        job: &mut JobView,
        s: &Settings,
        source: &SourceGroup,
        witnesses: &[SourceWitness],
        route: &ExtractionRoute,
        parent: &Path,
        workspace_lease: &mut Option<paths::DirectoryGuard>,
        cancel: Arc<AtomicBool>,
        allow_partial: bool,
        selected: Option<&HashSet<String>>,
    ) -> Result<(PathBuf, SavedRun), String> {
        if allow_partial || selected.is_some() {
            return Err("无可用缓存目录，请重试整个任务。".into());
        }
        // Retiring an old generation or another job can prune this shared
        // cache parent. Pin it until the new workspace owns its own lease.
        let parent_lease = paths::DirectoryGuard::create(parent).map_err(|_| "UnsafePath")?;
        if let Some(old) = job.workspace.clone() {
            drop(workspace_lease.take());
            // Keep the workspace association until removal actually succeeds: a
            // partial cleanup must stay retryable and keep its UI cleanup entry.
            if let Err(code) = remove_owned_retrying_async(Path::new(&old), &job.id).await {
                job.message = message(&code);
                self.put(job)?;
                return Err(code);
            }
            job.workspace = None;
        }
        let work = parent.join(format!("{}-{}", job.id, job.attempts));
        paths::mark_owned(&work, &job.id, "")?;
        *workspace_lease = Some(lock_private_workspace(&work)?);
        drop(parent_lease);
        job.workspace = Some(paths::path_string(&work));
        job.nodes.clear();
        job.stage_times.clear();
        let should_cache_input = match route {
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

        let (group, fingerprint, input_files) = if should_cache_input {
            self.cache_input(job, s, route, parent, &work, source, witnesses, cancel)
                .await?
        } else {
            (source.clone(), None, vec![])
        };

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
            witnesses: witnesses.to_vec(),
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
        save_run_async(&work, &state).await?;
        Ok((work, state))
    }

    /// Copies the archive volumes into the workspace cache and fingerprints the
    /// ordered content (never the filenames).
    #[allow(clippy::too_many_arguments)]
    async fn cache_input(
        self: &Arc<Self>,
        job: &mut JobView,
        s: &Settings,
        route: &ExtractionRoute,
        parent: &Path,
        work: &Path,
        source: &SourceGroup,
        witnesses: &[SourceWitness],
        cancel: Arc<AtomicBool>,
    ) -> Result<(SourceGroup, Option<String>, Vec<CopyFile>), String> {
        let mut group = source.clone();
        let mut input_files = vec![];
        self.stage(job, "caching", "正在将分卷文件复制到缓存。")?;
        let claim = self.claim_budget_for_job(&job.id, s, route, parent, Some(source.size))?;
        let budget = claim.budget();
        if source.size > budget {
            return Err("ResourceLimit".into());
        }
        let input = work.join("input");
        paths::DirectoryGuard::create(&input).map_err(|_| "UnsafePath")?;
        let volumes_clone = source.volumes.clone();
        let files = tokio::task::spawn_blocking(move || -> Result<Vec<CopyFile>, String> {
            let mut list = Vec::with_capacity(volumes_clone.len());
            for p in &volumes_clone {
                let path = Path::new(p);
                // A missing/unreadable source must fail the job explicitly; a
                // sentinel size here would overflow the summed budget below.
                let size = fs::metadata(path)
                    .map(|m| m.len())
                    .map_err(|e| format!("IoError: {e}"))?;
                list.push(CopyFile {
                    size_known: true,
                    source_path: p.clone(),
                    relative_path: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    size,
                    sha256: String::new(),
                    is_directory: false,
                    is_corrupted: false,
                });
            }
            Ok(list)
        })
        .await
        .map_err(|e| e.to_string())??;
        let mut req = request("copy", &input, s);
        req.files = files;
        req.max_written_bytes = budget;
        let result = self.request(job, req, cancel).await?;
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
                source_path: paths::path_string(&input.join(paths::relative(&f.relative_path)?)),
                relative_path: f.relative_path.clone(),
                size: f.size,
                sha256: f.sha256.clone(),
                is_directory: false,
                is_corrupted: f.is_corrupted,
            });
        }
        let fingerprint = Some(content_fingerprint(&hashes)?);
        group.path = paths::path_string(
            &input.join(Path::new(&source.path).file_name().ok_or("UnsafePath")?),
        );
        group.volumes = source
            .volumes
            .iter()
            .map(|p| paths::path_string(&input.join(Path::new(p).file_name().unwrap_or_default())))
            .collect();
        verify_witnesses(source, witnesses)?;
        Ok((group, fingerprint, input_files))
    }

    /// Processes every pending node until the derivation tree is drained.
    #[allow(clippy::too_many_arguments)]
    async fn run_nodes(
        self: &Arc<Self>,
        job: &mut JobView,
        s: &Settings,
        route: &ExtractionRoute,
        parent: &Path,
        work: &Path,
        state: &mut SavedRun,
        cancel: Arc<AtomicBool>,
        allow_partial: bool,
    ) -> Result<(), String> {
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
                save_run_async(work, state).await?;
                continue;
            }
            let index = match state.views.iter().position(|view| view.id == node.id) {
                Some(index) => index,
                None => {
                    state.views.push(node_view(&node, "running", ""));
                    state.views.len() - 1
                }
            };
            let mut run = NodeRun {
                job: &mut *job,
                state: &mut *state,
                node: &node,
                index,
                work,
                parent,
                route,
                settings: s,
                snapshot: &snapshot,
                cancel: cancel.clone(),
            };
            match self.process_node(&mut run).await? {
                NodeOutcome::Retry => continue,
                NodeOutcome::Advance => {}
            }
        }
        Ok(())
    }

    /// Extracts one node and commits its result to the run.
    async fn process_node(self: &Arc<Self>, run: &mut NodeRun<'_>) -> Result<NodeOutcome, String> {
        run.state.views[run.index].status = "running".into();
        run.state.views[run.index].stage = "extracting".into();
        run.state.views[run.index].message = String::new();
        run.job.nodes = run.state.views.clone();
        self.stage(
            run.job,
            if run.node.depth == 0 {
                "extracting"
            } else {
                "nested"
            },
            "正在解压并尝试已保存的密码。",
        )?;

        let node_fingerprint = if run.node.depth == 0 {
            run.state.fingerprint.clone()
        } else {
            fingerprint_for_group(&run.node.group, &run.state.final_files)?
        };
        let candidates = self.collect_candidates(run, node_fingerprint.clone())?;
        let mut decoded = None;
        let failure;

        if run.node.group.missing_volume {
            failure = "MissingVolume".to_string();
        } else {
            let (attempts, probed_failure, skip_extraction) =
                self.probe_attempts(run, &candidates).await?;
            if skip_extraction {
                failure = probed_failure;
            } else {
                let (extracted, code) = self.try_extract(run, attempts, node_fingerprint).await?;
                decoded = extracted;
                failure = code;
            }
        }

        run.state.pending.pop_front();
        let Some(decoded) = decoded else {
            return self.settle_failed_node(run, failure).await;
        };
        self.commit_decoded_node(run, decoded).await?;
        Ok(NodeOutcome::Advance)
    }

    /// Folds a successful extraction into the run: budgets, final mapping,
    /// sealed manifest, nested discovery, and workspace housekeeping.
    async fn commit_decoded_node(
        self: &Arc<Self>,
        run: &mut NodeRun<'_>,
        decoded: Decoded,
    ) -> Result<(), String> {
        let s = run.settings;
        let node = run.node;
        let index = run.index;
        let work = run.work;
        let job_id = run.job.id.clone();
        let Decoded {
            dest,
            files,
            success: event_success,
            verification_scope,
        } = decoded;

        for file in &files {
            paths::relative(&file.relative_path)?;
        }
        let bytes = files.iter().try_fold(0u64, |sum, file| {
            sum.checked_add(file.size).ok_or("ResourceLimit")
        })?;
        run.state.expanded = run
            .state
            .expanded
            .checked_add(bytes)
            .ok_or("ResourceLimit")?;
        run.state.entries = run
            .state
            .entries
            .checked_add(files.len() as u64)
            .ok_or("ResourceLimit")?;
        if run.state.expanded > s.max_expanded_bytes || run.state.entries > s.max_entries {
            return Err("ResourceLimit".into());
        }
        if node.depth > 0 {
            let consumed: HashSet<_> = node
                .group
                .volumes
                .iter()
                .map(|p| p.to_lowercase())
                .collect();
            run.state
                .final_files
                .retain(|f| !consumed.contains(&f.source_path.to_lowercase()));
            run.state.retired_files.extend(consumed);
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
            run.state.final_files.push(CopyFile {
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
            run.state.events.push(event);
        }
        run.state.sealed.push(SealedNode {
            node: node.clone(),
            directory: dest.clone(),
            files,
            verification_scope,
        });
        run.state.views[index].status = if node_corrupted {
            "salvaged".into()
        } else {
            "completed".into()
        };
        run.state.views[index].stage = "finished".into();
        run.state.views[index].progress = 1.;
        let mut has_child_archives = false;
        if s.expand_nested {
            let dest_str = paths::path_string(&dest);
            let nested = tokio::task::spawn_blocking(move || scanner::scan(&[dest_str], true, &[]))
                .await
                .map_err(|e| e.to_string())??;
            let total_nodes =
                run.state.sealed.len() + run.state.pending.len() + run.state.failed.len();
            if total_nodes >= MAX_DERIVATION_NODES {
                if !nested.sources.is_empty() {
                    run.state.views[index].message =
                        "已达到最大派生节点数量限制，更深内包按原文件保留。".into();
                }
            } else if node.depth + 1 < s.max_depth {
                for child in nested.sources {
                    if run.state.sealed.len() + run.state.pending.len() + run.state.failed.len()
                        >= MAX_DERIVATION_NODES
                    {
                        run.state.views[index].message =
                            "已达到最大派生节点数量限制，部分内包按原文件保留。".into();
                        break;
                    }
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
                    let reserved: Vec<_> = run
                        .state
                        .pending
                        .iter()
                        .chain(run.state.failed.iter().map(|(node, _)| node))
                        .map(|n| n.prefix.clone())
                        .collect();
                    let prefix = unique_prefix(base, &run.state.final_files, &reserved);
                    run.state.pending.push_back(NodeWork {
                        id: uid(),
                        group: child,
                        prefix,
                        depth: node.depth + 1,
                        parent: Some(node.id.clone()),
                    });
                }
            } else if !nested.sources.is_empty() {
                run.state.views[index].message =
                    "已达到展开层数限制，更深内包按原文件保留。".into();
            }
        }
        if node.depth == 0 && !run.state.input_files.is_empty() {
            run.state.input_retired = true;
        }
        // Persist the successful replacement and consumption intent first.
        // A crash or failed deletion then permits absence only for these
        // retired inputs; any bytes left behind must still pass readback.
        save_run_async(work, run.state).await?;
        // Only clean up inner archive volumes if it's a leaf node without children.
        // If node spawned child archives, keep the intermediate archive intact
        // until all descendant nodes complete and the derivation tree closes.
        if node.depth > 0 && !has_child_archives {
            for volume in &node.group.volumes {
                let _ = paths::remove_file_inside_owned(Path::new(volume), work, &job_id);
            }
        }
        if node.depth == 0 && work.join("input").exists() {
            let _ = paths::remove_inside_owned(&work.join("input"), work, &job_id);
        }
        self.refresh_usage()?;
        run.job.nodes = run.state.views.clone();
        self.put(run.job)?;
        Ok(())
    }
}
