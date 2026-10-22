use super::*;

impl Coordinator {
    pub async fn recover(self: &Arc<Self>, id: &str, action: &str) -> Result<(), String> {
        self.recover_with_policy(id, action, None).await
    }
    pub async fn recover_authorized(
        self: &Arc<Self>,
        id: &str,
        action: &str,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<(), String> {
        self.recover_with_policy(id, action, Some(policy)).await
    }
    pub(super) async fn recover_with_policy(
        self: &Arc<Self>,
        id: &str,
        action: &str,
        policy: Option<Arc<dyn JobExecutionPolicy>>,
    ) -> Result<(), String> {
        {
            let _gate = self.mutations.lock().map_err(|_| lock_error())?;
            let job = self.get_job(id)?;
            if let Some(policy) = &policy {
                policy.authorize(&job)?;
                if matches!(action, "retry" | "writeback") {
                    let mut retry = job.clone();
                    retry.options = self.settings.lock().map_err(|_| lock_error())?.clone();
                    policy.authorize(&retry)?;
                }
            }
            if self
                .deleting_jobs
                .lock()
                .map_err(|_| lock_error())?
                .contains(id)
            {
                return Err("任务正在删除，请等待删除完成。".into());
            }
            if self
                .recovering_jobs
                .lock()
                .map_err(|_| lock_error())?
                .contains(id)
            {
                return Err("任务正在核对恢复结果，请等待恢复操作完成。".into());
            }
            self.confirm_work_stopped(&job)?;
            let mut policies = self.execution_policies.lock().map_err(|_| lock_error())?;
            self.update_visible_state(|| {
                self.running
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .map(|_| ())
                    .map_err(|_| "请先暂停并结束当前任务，再执行恢复操作。".into())
            })?;
            if let Some(policy) = &policy {
                policies.insert(id.into(), policy.clone());
            } else {
                policies.remove(id);
            }
            drop(policies);
            self.recovering_jobs
                .lock()
                .map_err(|_| lock_error())?
                .insert(id.into());
        }
        let mut owner = RecoveryOwner {
            core: self.clone(),
            job_id: id.into(),
            completed: false,
        };
        let recovery_id = uid();
        self.worker.retain_request_receipt(&recovery_id)?;
        self.in_flight_request_ids
            .lock()
            .map_err(|_| lock_error())?
            .insert(id.to_string(), recovery_id);
        let result = self.recover_inner(id, action).await;
        let confirmed = self
            .in_flight_requests
            .lock()
            .map(|requests| !requests.contains(id))
            .unwrap_or(false)
            && self.get_job(id).is_ok_and(|job| job.status != "stopping");
        if confirmed && let Ok(mut active) = self.active_cancel.lock() {
            active.remove(id);
        }
        self.update_visible_state(|| {
            self.running.store(false, Ordering::Release);
            Ok(())
        })?;
        owner.completed = confirmed;
        result
    }
    pub(super) async fn recover_inner(
        self: &Arc<Self>,
        id: &str,
        action: &str,
    ) -> Result<(), String> {
        let mut job = self.get_job(id)?;
        self.confirm_work_stopped(&job)?;
        let publication = self
            .repository
            .publications()?
            .into_iter()
            .find(|p| p.job_id == id && !matches!(p.state.as_str(), "committed" | "abandoned"));
        if let Some(policy) = self.execution_policy(id)? {
            policy.authorize(&job)?;
            if let Some(publication) = &publication {
                for path in [&publication.final_path, &publication.temporary_path] {
                    policy.authorize_path(Path::new(path), true)?;
                }
            }
        }
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
                    self.active_cancel
                        .lock()
                        .map_err(|_| lock_error())?
                        .insert(id.into(), cancel.clone());
                    self.stop_batch.store(false, Ordering::Release);
                    self.update_visible_state(|| {
                        self.api_paused.store(false, Ordering::Release);
                        self.paused.store(false, Ordering::Release);
                        Ok(())
                    })?;
                    self.stage(
                        &mut job,
                        "checking",
                        "正在复核待发布目录，保留已验证结果供重试。",
                    )?;
                    let mut tree = lock_validated_publication(
                        &temp,
                        &p.id,
                        &p.manifest_hash,
                        &p.files,
                        true,
                        cancel,
                    )
                    .await?;
                    let parent = paths::DirectoryGuard::open(
                        missing[0].directory.parent().ok_or("UnsafePath")?,
                    )
                    .map_err(|_| "UnsafePath")?;
                    tree.rename_no_replace(&parent, &missing[0].directory)
                        .map_err(|_| "IoError")?;
                    drop(tree);
                    remove_publication_marker(&missing[0].directory, &p.id, &p.manifest_hash)?;
                } else if temp.exists() {
                    paths::owned(&temp, &p.id, Some(&p.manifest_hash))?;
                    paths::remove_owned(&temp, &p.id)?;
                }
                for sealed in &saved.sealed {
                    if sealed.directory.join(paths::OWNER_FILE).exists() {
                        remove_publication_marker(&sealed.directory, &p.id, &p.manifest_hash)?;
                    }
                }
                p.state = "abandoned".into();
                self.repository.save_publication(&p)?;
                job.status = "failed".into();
                job.stage = "checking".into();
                self.put(&job)?;
            }
            return self.enqueue_retry(id, false, None, None, self.execution_policy(id)?, false);
        }
        if action == "cleanup" {
            let has_corrupted = publication
                .as_ref()
                .map(|p| p.files.iter().any(|f| f.is_corrupted))
                .unwrap_or(false)
                || job.verification_scope.contains("salvaged")
                || job.nodes.iter().any(|node| node.status == "salvaged")
                || job.status == "salvaged";

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
            if let Some(work) = &job.workspace
                && Path::new(work).exists()
            {
                paths::remove_owned(Path::new(work), id)?;
            }
            job.workspace = None;
            job.status = if job.output_path.is_some() {
                if has_corrupted {
                    "salvaged"
                } else if job
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
            job.message = if job.status == "salvaged" {
                "本应用的临时工作目录已清理；已保留抢救解压输出（存在 CRC 损坏警告）及原始文件。"
                    .into()
            } else {
                "本应用的临时工作目录已清理；原始文件保留。".into()
            };
            job.finished_at = Some(now());
            self.put(&job)?;
            return self.refresh_usage();
        }
        if action != "verify" {
            return Err("恢复操作无效。".into());
        }
        let p = publication.ok_or("未找到待恢复的解压任务。")?;
        let work = PathBuf::from(
            job.workspace
                .as_ref()
                .ok_or("缺少解压核验记录，无法确认完成。")?,
        );
        let recovery = self.publications.prepare_recovery(p, &job, &work)?;
        let cancel = Arc::new(AtomicBool::new(false));
        self.active_cancel
            .lock()
            .map_err(|_| lock_error())?
            .insert(id.into(), cancel.clone());
        self.stop_batch.store(false, Ordering::Release);
        self.update_visible_state(|| {
            self.api_paused.store(false, Ordering::Release);
            self.paused.store(false, Ordering::Release);
            Ok(())
        })?;
        job.status = "running".into();
        self.stage(&mut job, "verifying", "正在校验可恢复的目标文件。")?;
        let outcome = recovery.seal(cancel).await;
        if self
            .in_flight_requests
            .lock()
            .map(|requests| !requests.contains(id))
            .unwrap_or(false)
            && let Ok(mut active) = self.active_cancel.lock()
        {
            active.remove(id);
        }
        let tree = match outcome {
            Ok(tree) => tree,
            Err(e) => {
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
        };
        self.publications
            .commit_recovery(&self.repository, recovery, tree, &mut job)?;
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
