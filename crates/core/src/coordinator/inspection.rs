use super::*;

impl Coordinator {
    pub async fn preview_output_rules(
        &self,
        paths: &[String],
        patterns: &[String],
    ) -> Result<Vec<bool>, String> {
        self.preview_output_rules_with_id(paths, patterns, uid())
            .await
    }

    pub async fn preview_output_rules_with_id(
        &self,
        paths: &[String],
        patterns: &[String],
        request_id: String,
    ) -> Result<Vec<bool>, String> {
        let cancel = self.begin_preview(&request_id)?;
        let _registration = PreviewRegistration {
            coordinator: self,
            request_id,
            cancel: cancel.clone(),
        };
        let concurrency = self.settings.lock().map_err(|_| lock_error())?.concurrency;
        self.worker
            .preview_output_rules_with_resources(paths, patterns, cancel, concurrency, None)
            .await
    }

    pub(super) fn begin_preview(&self, request_id: &str) -> Result<Arc<AtomicBool>, String> {
        validate_inspection_id(request_id).map_err(|_| "InvalidPreviewId")?;
        let mut active = self.preview_cancel.lock().map_err(|_| lock_error())?;
        let cancelled = self.cancelled_previews.lock().map_err(|_| lock_error())?;
        if cancelled.iter().any(|id| id == request_id) {
            return Err("Cancelled".into());
        }
        if let Some((_, previous)) = active.take() {
            previous.store(true, Ordering::Release);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some((request_id.to_string(), cancel.clone()));
        Ok(cancel)
    }

    pub fn cancel_preview_output_rules_request(&self, request_id: &str) -> Result<(), String> {
        validate_inspection_id(request_id).map_err(|_| "InvalidPreviewId")?;
        let mut active = self.preview_cancel.lock().map_err(|_| lock_error())?;
        let mut cancelled = self.cancelled_previews.lock().map_err(|_| lock_error())?;
        if !cancelled.iter().any(|id| id == request_id) {
            if cancelled.len() >= 128 {
                cancelled.pop_front();
            }
            cancelled.push_back(request_id.to_string());
        }
        if active.as_ref().is_some_and(|(id, _)| id == request_id)
            && let Some((_, cancel)) = active.take()
        {
            cancel.store(true, Ordering::Release);
        }
        Ok(())
    }

    pub fn cancel_inspect_archive(&self) -> Result<(), String> {
        let mut active = self.inspect_cancel.lock().map_err(|_| lock_error())?;
        if let Some((_, cancel)) = active.take() {
            cancel.store(true, Ordering::Release);
        }
        Ok(())
    }

    pub fn cancel_inspect_archive_request(&self, request_id: &str) -> Result<(), String> {
        validate_inspection_id(request_id)?;
        let mut active = self.inspect_cancel.lock().map_err(|_| lock_error())?;
        let mut cancelled = self
            .cancelled_inspections
            .lock()
            .map_err(|_| lock_error())?;
        // Remember early cancellations: command dispatch may deliver close before load.
        if !cancelled.iter().any(|id| id == request_id) {
            if cancelled.len() >= 128 {
                cancelled.pop_front();
            }
            cancelled.push_back(request_id.to_string());
        }
        if active.as_ref().is_some_and(|(id, _)| id == request_id)
            && let Some((_, cancel)) = active.take()
        {
            cancel.store(true, Ordering::Release);
        }
        Ok(())
    }

    pub(super) fn begin_inspection(&self, request_id: &str) -> Result<Arc<AtomicBool>, String> {
        validate_inspection_id(request_id)?;
        let mut active = self.inspect_cancel.lock().map_err(|_| lock_error())?;
        let cancelled = self
            .cancelled_inspections
            .lock()
            .map_err(|_| lock_error())?;
        if cancelled.iter().any(|id| id == request_id) {
            return Err("Cancelled".into());
        }
        if let Some((_, old_cancel)) = active.take() {
            old_cancel.store(true, Ordering::Release);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some((request_id.to_string(), cancel.clone()));
        Ok(cancel)
    }

    pub async fn inspect_archive(
        &self,
        path: &str,
        password: Option<String>,
    ) -> Result<ArchiveInspectionView, String> {
        self.inspect_archive_with_id(path, password, uid()).await
    }

    pub async fn inspect_archive_with_id(
        &self,
        path: &str,
        password: Option<String>,
        request_id: String,
    ) -> Result<ArchiveInspectionView, String> {
        self.inspect_archive_with_policy(path, password, request_id, None)
            .await
    }
    pub async fn inspect_archive_authorized(
        &self,
        path: &str,
        password: Option<String>,
        request_id: String,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<ArchiveInspectionView, String> {
        self.inspect_archive_with_policy(path, password, request_id, Some(policy))
            .await
    }
    pub(super) async fn inspect_archive_with_policy(
        &self,
        path: &str,
        password: Option<String>,
        request_id: String,
        policy: Option<Arc<dyn JobExecutionPolicy>>,
    ) -> Result<ArchiveInspectionView, String> {
        let cancel = self.begin_inspection(&request_id)?;
        let _registration = InspectionRegistration {
            coordinator: self,
            request_id: request_id.clone(),
            cancel: cancel.clone(),
        };
        if !self.worker.executable.is_file() || !self.worker.engine.is_file() {
            return Err("WorkerUnavailable".into());
        }
        let archive_p = Path::new(path);
        paths::check_path(archive_p)?;
        if !archive_p.is_file() {
            return Err("源压缩包不存在或不可读。".into());
        }

        let allowed_volumes = match scanner::scan(&[path.to_string()], false, &[]) {
            Ok(scan) => {
                if let Some(group) = scan.sources.into_iter().find(|g| {
                    g.path.eq_ignore_ascii_case(path)
                        || g.volumes.iter().any(|v| v.eq_ignore_ascii_case(path))
                }) {
                    if group.missing_volume {
                        return Err("MissingVolume".into());
                    }
                    group.volumes
                } else {
                    vec![path.to_string()]
                }
            }
            Err(_) => vec![path.to_string()],
        };
        if let Some(policy) = policy {
            for volume in &allowed_volumes {
                policy.authorize_path(Path::new(volume), false)?;
            }
        }

        let (
            max_entries,
            max_expanded_bytes,
            min_free_bytes,
            verification,
            engine_threads,
            worker_priority,
            salvage_corrupted,
        ) = {
            let s = self.settings.lock().map_err(|_| lock_error())?;
            (
                s.max_entries,
                s.max_expanded_bytes,
                s.min_free_bytes,
                s.verification.clone(),
                effective_engine_threads(s.engine_threads, s.concurrency),
                s.worker_priority.clone(),
                s.salvage_corrupted,
            )
        };

        // Metadata responses can exceed the named-pipe frame budget. Give this
        // request an exclusive scratch directory for the existing hashed spill.
        // A failed/abandoned Worker run preserves it; never race an unconfirmed
        // process by deleting its files while it might still be writing.
        let inspect_id = uid();
        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }
        let inspect_root = self.data.join(format!("inspect-{inspect_id}"));
        paths::mark_owned(&inspect_root, &inspect_id, "")?;
        let req = WorkerRequest {
            protocol_version: 2,
            request_id: uid(),
            operation: "inspect".into(),
            operation_id: Some(uid()),
            generation: Some(1),
            archive_path: path.to_string(),
            destination_directory: paths::path_string(&inspect_root),
            password: password.filter(|p| !p.is_empty()),
            allowed_volumes,
            max_entries,
            max_total_bytes: max_expanded_bytes,
            max_written_bytes: max_expanded_bytes,
            min_free_bytes,
            files: vec![],
            verification,
            exclusions: vec![],
            inclusions: vec![],
            candidate_passwords: vec![],
            candidates: vec![],
            engine_threads,
            worker_priority,
            salvage_corrupted,
        };

        let progress = Arc::new(|_: &WorkerEvent| {});
        let concurrency = self.settings.lock().map_err(|_| lock_error())?.concurrency;
        let run_res = self
            .worker
            .run_with_resources(req, cancel.clone(), progress, concurrency, None)
            .await;

        {
            if let Ok(mut active) = self.inspect_cancel.lock()
                && active
                    .as_ref()
                    .is_some_and(|(id, current)| id == &request_id && Arc::ptr_eq(current, &cancel))
            {
                active.take();
            }
        }
        // WorkerClient confirms process exit and consumes/validates the spill
        // before returning. Only this request's owned directory is removed if process exit is confirmed.
        if !matches!(
            run_res.as_ref().err().map(String::as_str),
            Some("WorkerExitUnconfirmed")
        ) {
            let _ = paths::remove_owned(&inspect_root, &inspect_id);
        }

        if cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }

        let mut event = run_res?;

        if event.kind != "completed" {
            let code = if event.error_code.is_empty() {
                "IoError"
            } else {
                event.error_code.as_str()
            };
            return Err(code.to_string());
        }

        let zip_probe = crate::zip_encoding::probe_zip_encoding(archive_p).ok();
        let detected_encoding = zip_probe
            .as_ref()
            .map(|p| p.detected_encoding.clone())
            .unwrap_or_default();

        let entries: Vec<ArchiveEntryView> = std::mem::take(&mut event.files)
            .into_iter()
            .enumerate()
            .map(|(idx, f)| {
                // The worker decodes entry names without the archive's legacy
                // code page, so a GBK/Big5/Shift-JIS archive arrives as CP437
                // mojibake. Prefer the re-decoded path for display and keep the
                // worker's raw name as the alternate form.
                let decoded_path = zip_probe
                    .as_ref()
                    .and_then(|p| p.decoded_paths.get(idx))
                    .map(|s| s.trim_end_matches(&['/', '\\'][..]).to_string());
                let path = match decoded_path {
                    Some(value) if !value.is_empty() => value,
                    _ => f.relative_path.clone(),
                };
                ArchiveEntryView {
                    path,
                    size: f.size,
                    size_known: f.size_known,
                    is_directory: f.is_directory,
                    is_encrypted: f.is_encrypted,
                    raw_path: f.relative_path,
                }
            })
            .collect();

        let total_entries = entries.len();
        let total_size = entries
            .iter()
            .filter(|e| !e.is_directory)
            .try_fold(0u64, |total, entry| total.checked_add(entry.size))
            .ok_or("WorkerManifestLimit")?;
        let encrypted_entries = entries.iter().filter(|e| e.is_encrypted).count();

        Ok(ArchiveInspectionView {
            archive_path: path.to_string(),
            total_entries,
            total_size,
            total_size_known: event.total_bytes_known,
            encrypted_entries,
            entries,
            detected_encoding,
        })
    }
}
