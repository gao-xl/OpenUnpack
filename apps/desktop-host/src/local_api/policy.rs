//! 本机 API 客户端权限作用域、读写根目录与执行前预检策略。

use super::*;

pub(super) enum Grant {
    Read(PathBuf),
    Write(PathBuf),
    BackupRead(PathBuf),
    BackupWrite(PathBuf),
}
#[derive(Clone, Copy)]
pub(super) enum GrantKind {
    Read,
    WriteDirectory,
    CheckWrite,
    BackupRead,
    BackupWrite,
}
pub(super) struct FileRequest {
    pub(super) path: String,
    pub(super) write: bool,
    pub(super) recursive: bool,
    pub(super) kind: GrantKind,
}

// Only legacy start_jobs selects the whole current queue. Admission and retry
// authorize their own targets; preserving retries use the task's frozen options.
pub(super) enum ExecutionAccess {
    None,
    Queue,
    Selected {
        job_ids: Vec<String>,
    },
    Add {
        sources: Vec<SourceGroup>,
        options: Box<Settings>,
    },
    Retry {
        job_id: String,
        preserve_options: bool,
    },
    Recover {
        job_id: String,
        requeue: bool,
        cleanup: bool,
    },
}

pub(super) fn execution_access(command: &ApiCommand) -> ExecutionAccess {
    match command {
        ApiCommand::StartJobs => ExecutionAccess::Queue,
        ApiCommand::StartSelectedJobs { job_ids } => ExecutionAccess::Selected {
            job_ids: job_ids.clone(),
        },
        ApiCommand::AddJobs { sources, options } => ExecutionAccess::Add {
            sources: sources.clone(),
            options: Box::new(options.clone()),
        },
        ApiCommand::RetryJob {
            job_id,
            preserve_options,
            ..
        }
        | ApiCommand::RetryNode {
            job_id,
            preserve_options,
            ..
        } => ExecutionAccess::Retry {
            job_id: job_id.clone(),
            preserve_options: *preserve_options,
        },
        ApiCommand::RecoverJob { job_id, action } => ExecutionAccess::Recover {
            job_id: job_id.clone(),
            requeue: matches!(action.as_str(), "retry" | "writeback"),
            cleanup: action == "cleanup",
        },
        _ => ExecutionAccess::None,
    }
}

pub(super) fn require_execution_scopes(
    client: &ApiClient,
    options: &Settings,
    may_clean_source: bool,
) -> Result<(), ApiError> {
    if !client.scopes.contains(&Scope::FilesRead)
        || !client.scopes.contains(&Scope::FilesWrite)
        || may_clean_source
            && options.source_cleanup != "keep"
            && !client.scopes.contains(&Scope::FilesDelete)
    {
        return Err(ApiError::denied());
    }
    Ok(())
}

pub(super) fn source_execution_requests(
    source: &SourceGroup,
    options: &Settings,
) -> Result<Vec<FileRequest>, ApiError> {
    let mut requests = Vec::new();
    for path in std::iter::once(&source.path).chain(source.volumes.iter()) {
        requests.push(FileRequest {
            path: path.clone(),
            write: options.source_cleanup != "keep",
            recursive: false,
            kind: GrantKind::Read,
        });
    }
    let output = if options.output_root.trim().is_empty() {
        Path::new(&source.path)
            .parent()
            .ok_or_else(ApiError::denied)?
            .to_string_lossy()
            .into_owned()
    } else {
        options.output_root.clone()
    };
    requests.push(FileRequest {
        path: output,
        write: true,
        recursive: false,
        kind: GrantKind::WriteDirectory,
    });
    if !options.cache_root.trim().is_empty() {
        requests.push(FileRequest {
            path: options.cache_root.clone(),
            write: true,
            recursive: false,
            kind: GrantKind::WriteDirectory,
        });
    }
    Ok(requests)
}

pub(super) fn retained_job_requests(job: &JobView) -> Vec<FileRequest> {
    [&job.workspace, &job.output_path]
        .into_iter()
        .filter_map(|path| path.as_ref())
        .map(|path| FileRequest {
            path: path.clone(),
            write: true,
            recursive: false,
            kind: GrantKind::CheckWrite,
        })
        .collect()
}

// A pure projection used by authorization tests. Paths from the resulting plan
// are checked against this client's roots before any NativeAccess grant occurs.
pub(super) fn execution_requests(
    client: &ApiClient,
    access: &ExecutionAccess,
    snapshot: &AppSnapshot,
) -> Result<Vec<FileRequest>, ApiError> {
    let mut requests = Vec::new();
    if matches!(access, ExecutionAccess::Queue) {
        for job in snapshot
            .jobs
            .iter()
            .filter(|job| matches!(job.status.as_str(), "queued" | "waitingResource"))
        {
            require_execution_scopes(client, &job.options, true)?;
            requests.extend(source_execution_requests(&job.source, &job.options)?);
            requests.extend(retained_job_requests(job));
        }
    }
    match access {
        ExecutionAccess::Selected { job_ids } => {
            for id in job_ids {
                let job = snapshot
                    .jobs
                    .iter()
                    .find(|job| &job.id == id)
                    .ok_or_else(ApiError::denied)?;
                require_execution_scopes(client, &job.options, true)?;
                requests.extend(source_execution_requests(&job.source, &job.options)?);
                requests.extend(retained_job_requests(job));
            }
        }
        ExecutionAccess::Add { sources, options } => {
            require_execution_scopes(client, options, true)?;
            for source in sources {
                requests.extend(source_execution_requests(source, options)?);
            }
        }
        ExecutionAccess::Retry {
            job_id,
            preserve_options,
        } => {
            let job = snapshot
                .jobs
                .iter()
                .find(|job| &job.id == job_id)
                .ok_or_else(ApiError::denied)?;
            let options = if *preserve_options {
                &job.options
            } else {
                &snapshot.settings
            };
            require_execution_scopes(client, options, true)?;
            requests.extend(source_execution_requests(&job.source, options)?);
            requests.extend(retained_job_requests(job));
        }
        ExecutionAccess::Recover {
            job_id,
            requeue,
            cleanup,
        } => {
            let job = snapshot
                .jobs
                .iter()
                .find(|job| &job.id == job_id)
                .ok_or_else(ApiError::denied)?;
            require_execution_scopes(client, &job.options, !cleanup)?;
            if !cleanup {
                requests.extend(source_execution_requests(&job.source, &job.options)?);
            }
            requests.extend(retained_job_requests(job));
            if *requeue {
                require_execution_scopes(client, &snapshot.settings, true)?;
                requests.extend(source_execution_requests(&job.source, &snapshot.settings)?);
            }
        }
        ExecutionAccess::None | ExecutionAccess::Queue => {}
    }
    Ok(requests)
}

pub(super) struct ClientJobPolicy {
    pub(super) client: ApiClient,
    pub(super) store: Arc<ApiStore>,
    pub(super) data: PathBuf,
    pub(super) cleanup_only: bool,
}
impl openunpack_core::coordinator::JobExecutionPolicy for ClientJobPolicy {
    fn authorize(&self, job: &JobView) -> Result<(), String> {
        if !self.store.active_client(&self.client.id) {
            return Err("应用授权已撤销或本机 API 已停用。".into());
        }
        require_execution_scopes(&self.client, &job.options, !self.cleanup_only)
            .map_err(|e| e.message)?;
        if self.cleanup_only && job.status == "running" {
            return Err("清理授权不能执行解压任务。".into());
        }
        if openunpack_local_api::is_trusted_all_paths() {
            return Ok(());
        }
        let data = self
            .data
            .canonicalize()
            .map_err(|_| "应用数据目录不可用。")?;
        let mut requests = if self.cleanup_only {
            Vec::new()
        } else {
            source_execution_requests(&job.source, &job.options).map_err(|e| e.message)?
        };
        requests.extend(retained_job_requests(job));
        for request in requests {
            check_client_path(
                &data,
                &self.client,
                &request.path,
                request.write,
                matches!(request.kind, GrantKind::CheckWrite),
                request.recursive,
            )
            .map_err(|e| e.message)?;
        }
        Ok(())
    }
    fn authorize_path(&self, path: &Path, write: bool) -> Result<(), String> {
        if !self.store.active_client(&self.client.id)
            || !self.client.scopes.contains(&if write {
                Scope::FilesWrite
            } else {
                Scope::FilesRead
            })
        {
            return Err("此应用没有访问该目录的权限。".into());
        }
        if openunpack_local_api::is_trusted_all_paths() {
            return Ok(());
        }
        let data = self
            .data
            .canonicalize()
            .map_err(|_| "应用数据目录不可用。")?;
        check_client_path(
            &data,
            &self.client,
            &path.to_string_lossy(),
            write,
            write,
            false,
        )
        .map(|_| ())
        .map_err(|e| e.message)
    }
}

pub(super) fn check_client_path(
    data: &Path,
    client: &ApiClient,
    raw: &str,
    write: bool,
    save: bool,
    recursive: bool,
) -> Result<PathBuf, ApiError> {
    if raw.is_empty() || !Path::new(raw).is_absolute() {
        return Err(ApiError::denied());
    }
    let drive_colon = match Path::new(raw).components().next() {
        Some(std::path::Component::Prefix(prefix)) => match prefix.kind() {
            std::path::Prefix::Disk(_) => Some(1),
            std::path::Prefix::VerbatimDisk(_) => Some(5),
            _ => None,
        },
        _ => None,
    };
    let has_illegal_colon = raw
        .char_indices()
        .any(|(i, c)| c == ':' && Some(i) != drive_colon);
    if has_illegal_colon {
        return Err(ApiError::denied());
    }
    let target = if save {
        canonicalize_for_save(raw)
    } else {
        canonicalize_existing(raw)
    }
    .map_err(|_| ApiError::denied())?;
    if openunpack_local_api::is_trusted_all_paths() {
        return Ok(target);
    }
    if path_starts_with_case_insensitive(&target, data)
        || (recursive || write) && path_starts_with_case_insensitive(data, &target)
    {
        return Err(ApiError::denied());
    }
    let allowed = if write {
        client.write_roots.iter().collect::<Vec<_>>()
    } else {
        client
            .read_roots
            .iter()
            .chain(&client.write_roots)
            .collect::<Vec<_>>()
    };
    if !allowed.into_iter().any(|root| {
        canonicalize_existing(root)
            .or_else(|_| canonicalize_for_save(root))
            .is_ok_and(|root| {
                path_eq_case_insensitive(&target, &root)
                    || root.is_dir() && path_starts_with_case_insensitive(&target, &root)
            })
    }) {
        return Err(ApiError::denied());
    }
    Ok(target)
}
pub(super) fn file_requests(command: &ApiCommand) -> Result<Vec<FileRequest>, ApiError> {
    let mut requests = Vec::new();
    let mut add = |path: &str, kind, write, recursive| {
        requests.push(FileRequest {
            path: path.into(),
            kind,
            write,
            recursive,
        })
    };
    match command {
        ApiCommand::AddJobs { options, .. } | ApiCommand::SaveSettings { settings: options } => {
            for path in [&options.output_root, &options.cache_root] {
                if !path.trim().is_empty() {
                    add(path, GrantKind::WriteDirectory, true, false);
                }
            }
        }
        _ => {}
    }
    match command {
        ApiCommand::ScanSources { paths, .. } => {
            if paths.len() > 1000 {
                return Err(ApiError::new(
                    400,
                    "invalid_request",
                    "一次最多提交 1000 个输入位置。",
                ));
            }
            for path in paths {
                add(path, GrantKind::Read, false, true);
            }
        }
        ApiCommand::AddJobs { sources, options } => {
            if sources.len() > 1000 {
                return Err(ApiError::new(
                    400,
                    "invalid_request",
                    "一次最多添加 1000 个任务。",
                ));
            }
            for source in sources {
                for path in std::iter::once(&source.path).chain(source.volumes.iter()) {
                    add(
                        path,
                        GrantKind::Read,
                        options.source_cleanup != "keep",
                        false,
                    );
                }
                if options.output_root.trim().is_empty() {
                    let parent = Path::new(&source.path)
                        .parent()
                        .ok_or_else(ApiError::denied)?
                        .to_string_lossy();
                    add(&parent, GrantKind::WriteDirectory, true, false);
                }
            }
        }
        ApiCommand::InspectArchive { path, .. }
        | ApiCommand::ReadImportFile { path, .. }
        | ApiCommand::OpenPath { path } => add(path, GrantKind::Read, false, false),
        ApiCommand::ExportBackup { path, .. } => add(path, GrantKind::BackupWrite, true, false),
        ApiCommand::PreviewRestoreBackup { path, .. }
        | ApiCommand::ConfirmRestoreBackup { path, .. } => {
            add(path, GrantKind::BackupRead, false, false)
        }
        ApiCommand::PreviewToolboxOperation { options }
        | ApiCommand::ExecuteToolboxOperation {
            request: ToolboxExecuteRequest { options, .. },
        } => {
            add(
                &options.source_directory,
                GrantKind::Read,
                false,
                options.recursive,
            );
            add(
                &options.source_directory,
                GrantKind::WriteDirectory,
                true,
                options.recursive,
            );
            if let Some(target) = &options.target_directory
                && !target.trim().is_empty()
            {
                add(target, GrantKind::WriteDirectory, true, false);
            }
        }
        // The convenience endpoint first resolves its effective settings and
        // actual volumes, then validates without widening NativeAccess.
        ApiCommand::AddArchive { .. } => {}
        ApiCommand::SubmitInput { .. } => {}
        _ => {}
    }
    if let ApiCommand::ExecuteToolboxOperation { request } = command
        && let Some(paths) = &request.approved_paths
    {
        for path in paths {
            add(path, GrantKind::Read, true, false);
        }
    }
    Ok(requests)
}
pub(super) fn preflight(
    coord_data: &Path,
    access: &Mutex<NativeAccess>,
    coordinator: &Coordinator,
    client: &ApiClient,
    requests: Vec<FileRequest>,
    execution: ExecutionAccess,
) -> Result<(), ApiError> {
    let grants = preflight_grants(coord_data, coordinator, client, requests, execution)?;
    let mut access_guard = access.lock().map_err(|_| ApiError::internal())?;
    for grant in grants {
        match grant {
            Grant::Read(path) => {
                let dir = path.is_dir();
                access_guard.grant_read(path, dir);
            }
            Grant::Write(path) => access_guard.grant_write_dir(path),
            Grant::BackupRead(path) => access_guard.grant_backup_read(path),
            Grant::BackupWrite(path) => access_guard.grant_backup_write(path),
        }
    }
    Ok(())
}
pub(super) fn preflight_grants(
    coord_data: &Path,
    coordinator: &Coordinator,
    client: &ApiClient,
    mut requests: Vec<FileRequest>,
    execution: ExecutionAccess,
) -> Result<Vec<Grant>, ApiError> {
    let data = coord_data
        .canonicalize()
        .map_err(|_| ApiError::internal())?;
    let mut grants = Vec::new();
    let check = |raw: &str, write: bool, save: bool, recursive: bool| {
        check_client_path(&data, client, raw, write, save, recursive)
    };
    if !matches!(execution, ExecutionAccess::None) {
        let snapshot = coordinator.snapshot().map_err(|_| ApiError::internal())?;
        requests.extend(execution_requests(client, &execution, &snapshot)?);
    }
    // Core independently discovers real sibling volumes. Authorize its complete
    // current volume set too, rather than trusting a caller's supplied list.
    for request in &requests {
        check(
            &request.path,
            request.write,
            matches!(request.kind, GrantKind::BackupWrite | GrantKind::CheckWrite),
            request.recursive,
        )?;
    }
    let sources: Vec<_> = requests
        .iter()
        .filter(|request| {
            !matches!(execution, ExecutionAccess::None)
                && matches!(request.kind, GrantKind::Read)
                && !request.recursive
        })
        .collect();
    for source in sources {
        let scan = openunpack_core::scanner::scan(std::slice::from_ref(&source.path), false, &[])
            .map_err(|_| ApiError::denied())?;
        for group in scan.sources {
            for path in std::iter::once(&group.path).chain(group.volumes.iter()) {
                check(path, source.write, false, false)?;
            }
        }
    }
    for request in requests {
        let target = check(
            &request.path,
            request.write,
            matches!(request.kind, GrantKind::BackupWrite | GrantKind::CheckWrite),
            request.recursive,
        )?;
        grants.push(match request.kind {
            GrantKind::Read => Grant::Read(target),
            GrantKind::WriteDirectory => {
                if target.is_file() {
                    return Err(ApiError::denied());
                }
                Grant::Write(target)
            }
            GrantKind::BackupRead => Grant::BackupRead(target),
            GrantKind::BackupWrite => Grant::BackupWrite(target),
            GrantKind::CheckWrite => continue,
        });
    }
    Ok(grants)
}
