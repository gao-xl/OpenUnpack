//! Post-extraction reconciliation, publication, cleanup and retirement pipeline.
use super::*;

/// 完成任务发布与清理收尾时所需的参数。
pub(super) struct FinalizeRunArgs<'a> {
    pub(super) settings: &'a Settings,
    pub(super) source: &'a SourceGroup,
    pub(super) witnesses: &'a [SourceWitness],
    pub(super) route: &'a ExtractionRoute,
    pub(super) work: &'a Path,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) allow_partial: bool,
}

impl Coordinator {
    /// Publishes the verified output, optionally retires the source archive,
    /// and releases the workspace.
    pub(super) async fn finalize_run(
        self: &Arc<Self>,
        job: &mut JobView,
        state: &mut SavedRun,
        mut workspace_lease: Option<paths::DirectoryGuard>,
        args: FinalizeRunArgs<'_>,
    ) -> Result<(), String> {
        let FinalizeRunArgs {
            settings: s,
            source,
            witnesses,
            route,
            work,
            cancel,
            allow_partial,
        } = args;
        let tree_closed = is_derivation_tree_closed(state);
        if tree_closed {
            // Bottom-up retirement: clean up inner archives in reverse depth order
            let mut sealed_nodes = state.sealed.clone();
            sealed_nodes.sort_by_key(|s| std::cmp::Reverse(s.node.depth));
            for sealed in &sealed_nodes {
                if sealed.node.depth > 0 {
                    for volume in &sealed.node.group.volumes {
                        let _ = paths::remove_file_inside_owned(Path::new(volume), work, &job.id);
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
        verify_witnesses(source, witnesses)?;
        self.checkpoint(&cancel).await?;
        self.stage(
            job,
            "planning",
            "正在生成最终文件映射；同名目录按策略处理。",
        )?;
        let Some(staged) = self.publications.prepare(
            &self.repository,
            job,
            publication::PreparePublicationArgs {
                state,
                source,
                work,
                route,
                tree_closed,
            },
        )?
        else {
            job.status = "skipped".into();
            if job.message.is_empty() {
                job.message = "同名目标已存在，按设置跳过；原包保留。".into();
            }
            job.finished_at = Some(now());
            drop(workspace_lease.take());
            remove_owned_retrying_async(work, &job.id).await?;
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
            remove_owned_retrying_async(work, &job.id).await?;
            job.workspace = None;
            self.put(job)?;
            return self.refresh_usage();
        };
        let mut msg = committed.message().to_owned();

        // Safe source archive cleanup (only when 100% completed, non-empty, no exclusions, witness verified, target tree verified, and derivation tree closed)
        if committed.cleanup_allowed() {
            // Pin every volume before checking the saved witnesses. The exact
            // objects checked here are retained until their delete/recycle ends.
            let is_recycle = s.source_cleanup == "recycle";
            let source_cloned = source.clone();
            let witnesses_cloned = witnesses.to_vec();
            let cleanup_sources = tokio::task::spawn_blocking(move || {
                lock_cleanup_sources(&source_cloned, &witnesses_cloned, is_recycle)
            })
            .await
            .map_err(|e| e.to_string())?;

            let mut cleanup_ok = cleanup_sources.is_ok();
            if let Ok(sources) = cleanup_sources {
                for source_file in sources {
                    let interrupted = {
                        let _gate = self.mutations.lock().map_err(|_| lock_error())?;
                        cancel.load(Ordering::Acquire)
                            || self
                                .deleting_jobs
                                .lock()
                                .map_err(|_| lock_error())?
                                .contains(&job.id)
                    };
                    if interrupted {
                        cleanup_ok = false;
                        break;
                    }
                    // Offload slow shell recycle / disk unlinking to dedicated blocking thread pool
                    let clean_res = tokio::task::spawn_blocking(move || source_file.cleanup())
                        .await
                        .map_err(|e| e.to_string())?;
                    if clean_res.is_err() {
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
        let mut retire_result = remove_owned_retrying_async(work, &job.id).await;
        if retire_result.is_err() {
            // 指数退避重试以容忍 Windows Defender 实时扫描或后台句柄释放延迟
            for delay in [150, 400, 1000] {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                retire_result = remove_owned_retrying_async(work, &job.id).await;
                if retire_result.is_ok() {
                    break;
                }
            }
        }

        match retire_result {
            Ok(()) => job.workspace = None,
            Err(reason) => {
                // This is the only cleanup signal that survives into the CI log,
                // so the concrete reason must never be collapsed again.
                tracing::error!(
                    job = %job.id,
                    workspace = %work.display(),
                    reason = %reason,
                    "workspace_retire_failed"
                );
                job.status = "cleanupPending".into();
                // Keep the delivery summary before the marker so an automatic
                // settle can restore the exact completed message later.
                job.message = with_cleanup_pending_note(&job.message, &reason);
            }
        }
        self.put(job)?;
        self.refresh_usage()
    }
}
