//! Publication owns its commit gate and the verified directory across rename.
//! Callers supply the existing repository and drive cancellation/Worker requests.
use super::{
    SavedRun, aggregate_verification_scopes, conflict_resolver, direct_manifest,
    lock_private_workspace, lock_validated_publication, now, output_root, request,
    strip_empty_directories, strip_single_root_wrapper, uid, validate_manifest,
    validate_target_tree,
};
use crate::{paths, repository::Repository, route::ExtractionRoute, scanner, types::*};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Instant,
};

#[cfg(test)]
mod tests;

#[derive(Serialize, Deserialize)]
pub(super) struct Journal {
    pub(super) publication: Publication,
    pub(super) events: Vec<SuccessEvent>,
    pub(super) partial: bool,
    #[serde(default)]
    pub(super) verification_scope: String,
}

#[derive(Default)]
pub(super) struct PublicationService {
    gate: Mutex<()>,
}

/// Durable intent plus the pinned staging directory. This value is not Clone.
pub(super) struct StagedPublication {
    journal: Journal,
    output_directory: paths::DirectoryGuard,
    output: PathBuf,
    basename: String,
    work: PathBuf,
    settings: Settings,
    direct: bool,
    cleanup_allowed: bool,
    readback: bool,
    temporary_lease: Option<paths::DirectoryGuard>,
}

pub(super) struct SealedPublication {
    staged: StagedPublication,
    tree: paths::LockedTree,
    verification_millis: u64,
}

/// Keep this receipt alive until source cleanup completes: it pins the exact
/// root and ancestor identities retained by LockedTree after the rename.
pub(super) struct CommittedPublication {
    _tree: paths::LockedTree,
    cleanup_allowed: bool,
    message: String,
}

pub(super) struct RecoveryPublication {
    publication: Publication,
    journal: Journal,
    target: PathBuf,
    temporary: PathBuf,
    final_path: PathBuf,
}

impl PublicationService {
    // The parameters are distinct borrows of caller-owned state; grouping them
    // into a struct would only move the same list to the call site.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &self,
        repository: &Repository,
        job: &mut JobView,
        state: &SavedRun,
        source: &SourceGroup,
        work: &Path,
        route: &ExtractionRoute,
        tree_closed: bool,
    ) -> Result<Option<StagedPublication>, String> {
        let settings = &job.options;
        let partial = !state.failed.is_empty();
        let mut files = state.final_files.clone();
        if !settings.output_exclusions.is_empty() || !settings.output_inclusions.is_empty() {
            files.retain(|f| {
                !openunpack_infrastructure::engine::path_safety::matches_exclusion(
                    &f.relative_path,
                    &settings.output_exclusions,
                ) && openunpack_infrastructure::engine::path_safety::matches_inclusion(
                    &f.relative_path,
                    f.is_directory,
                    &settings.output_inclusions,
                )
            });
        }
        if settings.unwrap_single_root {
            strip_single_root_wrapper(&mut files);
        }
        if settings.clean_empty_dirs {
            strip_empty_directories(&mut files);
        }
        validate_manifest(&files)?;
        let output = output_root(settings, source)?;
        let output_directory = paths::DirectoryGuard::create(&output).map_err(|_| "UnsafePath")?;
        let basename = scanner::output_name(&source.path);
        let mtime = std::fs::metadata(&source.path)
            .and_then(|m| m.modified())
            .ok();
        let resolution = conflict_resolver::resolve_publication_target(
            &output,
            &basename,
            &settings.conflict_policy,
            &files,
            mtime,
        )?;
        let final_path = match resolution {
            conflict_resolver::ConflictResolution::DeliverTo(p) => p,
            conflict_resolver::ConflictResolution::Overwrite(p) => p,
            conflict_resolver::ConflictResolution::SkipIdentical(_) => {
                job.message = "目标目录已存在完全相同内容，自动跳过解压；原包保留。".into();
                return Ok(None);
            }
            conflict_resolver::ConflictResolution::SkipPolicy { reason, .. } => {
                job.message = format!("{reason}原包保留。");
                return Ok(None);
            }
        };
        let id = uid();
        let temporary = output.join(format!(".openunpack-{id}.tmp"));
        let hash = paths::digest(&files)?;
        let publication = Publication {
            id: id.clone(),
            job_id: job.id.clone(),
            temporary_path: paths::path_string(&temporary),
            final_path: paths::path_string(&final_path),
            manifest_hash: hash.clone(),
            state: "planned".into(),
            files,
            verification: settings.verification.clone(),
        };
        let source_scope = aggregate_verification_scopes(state.sealed.iter().map(|sealed| {
            if sealed.verification_scope.is_empty() {
                "streamSha256"
            } else {
                sealed.verification_scope.as_str()
            }
        }));
        let journal = Journal {
            publication,
            events: state.events.clone(),
            partial,
            verification_scope: source_scope,
        };
        // Both records precede temporary target creation and Worker writeback.
        paths::atomic_json(&work.join("publication.json"), &journal)?;
        repository.save_publication(&journal.publication)?;
        let direct = matches!(route, ExtractionRoute::Direct { .. })
            && paths::same_volume(work, &output)?
            && state.sealed.len() == 1
            && state.views.len() == 1
            && direct_manifest(&state.sealed[0], &journal.publication.files);
        let lease = if direct {
            let directory = &state.sealed[0].directory;
            paths::tag_existing(directory, &id, &hash)?;
            paths::publish_directory(directory, &temporary)?;
            lock_private_workspace(&temporary)?
        } else {
            paths::mark_owned(&temporary, &id, &hash)?;
            lock_private_workspace(&temporary)?
        };
        let cleanup_allowed = !partial
            && !journal.verification_scope.contains("salvaged")
            && !journal
                .publication
                .files
                .iter()
                .any(|file| file.is_corrupted)
            && !source.missing_volume
            && settings.source_cleanup != "keep"
            && !journal.publication.files.is_empty()
            && settings.output_exclusions.is_empty()
            && settings.output_inclusions.is_empty()
            && !state
                .views
                .iter()
                .any(|node| node.message.contains("层数限制"))
            && tree_closed;
        Ok(Some(StagedPublication {
            journal,
            output_directory,
            output,
            basename,
            work: work.to_owned(),
            settings: settings.clone(),
            direct,
            cleanup_allowed,
            readback: settings.verification == "strict" || cleanup_allowed,
            temporary_lease: Some(lease),
        }))
    }

    /// No await or cancellation check is allowed between rename and the durable
    /// commit. A delivered result must never become a cancelled task.
    pub(super) fn commit(
        &self,
        repository: &Repository,
        sealed: SealedPublication,
        job: &mut JobView,
    ) -> Result<Option<CommittedPublication>, String> {
        let SealedPublication {
            mut staged,
            mut tree,
            ..
        } = sealed;
        let publication = &mut staged.journal.publication;
        tree.verify_owned(&publication.id, Some(&publication.manifest_hash))?;
        let guard = self.gate.lock().map_err(|_| super::lock_error())?;
        let mut final_path = PathBuf::from(&publication.final_path);
        let mut committed_by_overwrite = false;
        if final_path.exists() {
            let mtime = std::fs::metadata(&job.source.path)
                .and_then(|m| m.modified())
                .ok();
            let resolution = conflict_resolver::resolve_publication_target(
                &staged.output,
                &staged.basename,
                &staged.settings.conflict_policy,
                &publication.files,
                mtime,
            )?;
            match resolution {
                conflict_resolver::ConflictResolution::DeliverTo(target) => {
                    final_path = target;
                    publication.final_path = paths::path_string(&final_path);
                    repository.save_publication(publication)?;
                    paths::atomic_json(&staged.work.join("publication.json"), &staged.journal)?;
                }
                conflict_resolver::ConflictResolution::Overwrite(target) => {
                    let trash_id = uid();
                    let trash_path = staged
                        .output
                        .join(format!(".openunpack-trash-{trash_id}.tmp"));
                    std::fs::rename(&target, &trash_path)
                        .map_err(|e| format!("目标目录已存在且覆盖备份失败: {e}"))?;
                    if let Err(e) = tree.rename_no_replace(&staged.output_directory, &final_path) {
                        let _ = std::fs::rename(&trash_path, &target);
                        return Err(format!("目标目录提交失败；已恢复原目录: {e}"));
                    }
                    let _ = std::fs::remove_dir_all(&trash_path);
                    committed_by_overwrite = true;
                }
                conflict_resolver::ConflictResolution::SkipIdentical(_) => {
                    drop(tree);
                    paths::remove_owned(Path::new(&publication.temporary_path), &publication.id)?;
                    publication.state = "abandoned".into();
                    repository.save_publication(publication)?;
                    job.message = "目标目录已存在完全相同内容，自动跳过解压；原包保留。".into();
                    return Ok(None);
                }
                conflict_resolver::ConflictResolution::SkipPolicy { reason, .. } => {
                    drop(tree);
                    paths::remove_owned(Path::new(&publication.temporary_path), &publication.id)?;
                    publication.state = "abandoned".into();
                    repository.save_publication(publication)?;
                    job.message = format!("{reason}原包保留。");
                    return Ok(None);
                }
            }
        }
        if !committed_by_overwrite {
            tree.rename_no_replace(&staged.output_directory, &final_path)
                .map_err(|_| "目标目录提交失败；临时结果已保留待核对。")?;
        }
        drop(guard);
        let publication = &mut staged.journal.publication;
        let has_corrupted = publication.files.iter().any(|file| file.is_corrupted)
            || staged.journal.verification_scope.contains("salvaged");
        publication.state = "committed".into();
        job.output_path = Some(paths::path_string(&final_path));
        job.status = terminal_status(has_corrupted, staged.journal.partial).into();
        job.verification_scope = if has_corrupted {
            "salvagedWithCrcWarning".into()
        } else {
            aggregate_verification_scopes([
                staged.journal.verification_scope.as_str(),
                if staged.readback {
                    "targetReadback"
                } else if !staged.direct {
                    "sourceStreamSha256"
                } else {
                    ""
                },
            ])
        };
        job.stage = "finished".into();
        job.progress = 1.;
        job.finished_at = Some(now());
        let message = if has_corrupted {
            let corrupted = publication
                .files
                .iter()
                .filter(|file| file.is_corrupted)
                .count();
            format!(
                "抢救解压完成：检测到归档结构或 CRC 异常，输出可能不完整；已抢救 {} 个文件（其中 {} 项数据损坏）；原始压缩包已保留。",
                publication.files.len(),
                corrupted
            )
        } else if staged.journal.partial {
            "部分解压完成：未解压的内层包已保留；原始压缩包未删除。".into()
        } else {
            "解压完成并校验通过；原始压缩包已保留。".into()
        };
        repository.commit_publication(publication, job, &staged.journal.events)?;
        Ok(Some(CommittedPublication {
            _tree: tree,
            cleanup_allowed: staged.cleanup_allowed,
            message,
        }))
    }

    pub(super) fn prepare_recovery(
        &self,
        publication: Publication,
        job: &JobView,
        work: &Path,
    ) -> Result<RecoveryPublication, String> {
        if paths::digest(&publication.files)? != publication.manifest_hash {
            return Err("文件清单校验不匹配，已停止。".into());
        }
        let final_path = PathBuf::from(&publication.final_path);
        let temporary = PathBuf::from(&publication.temporary_path);
        if final_path.exists() && temporary.exists() {
            return Err("临时和最终目录同时存在，需要人工检查。".into());
        }
        let target = if final_path.exists() {
            final_path.clone()
        } else {
            temporary.clone()
        };
        paths::owned(&target, &publication.id, Some(&publication.manifest_hash))?;
        validate_target_tree(&target, &publication.files)?;
        paths::owned(work, &job.id, None)?;
        let journal: Journal = paths::read_json(&work.join("publication.json"))?;
        if journal.publication.id != publication.id
            || journal.publication.manifest_hash != publication.manifest_hash
        {
            return Err("解压记录不匹配。".into());
        }
        Ok(RecoveryPublication {
            publication,
            journal,
            target,
            temporary,
            final_path,
        })
    }

    pub(super) fn commit_recovery(
        &self,
        repository: &Repository,
        recovery: RecoveryPublication,
        mut tree: paths::LockedTree,
        job: &mut JobView,
    ) -> Result<(), String> {
        let RecoveryPublication {
            mut publication,
            journal,
            target,
            temporary,
            final_path,
        } = recovery;
        if target == temporary {
            let parent = paths::DirectoryGuard::open(final_path.parent().ok_or("UnsafePath")?)
                .map_err(|_| "UnsafePath")?;
            let _guard = self.gate.lock().map_err(|_| super::lock_error())?;
            tree.rename_no_replace(&parent, &final_path)
                .map_err(|_| "目标目录提交失败；临时结果已保留待核对。")?;
        }
        drop(tree);
        let has_corrupted = publication.files.iter().any(|file| file.is_corrupted)
            || journal.verification_scope.contains("salvaged")
            || job.verification_scope.contains("salvaged");
        publication.state = "committed".into();
        job.output_path = Some(paths::path_string(&final_path));
        job.status = terminal_status(has_corrupted, journal.partial).into();
        job.verification_scope = if has_corrupted {
            "salvagedWithCrcWarning+targetReadback".into()
        } else {
            let source_scope = if !journal.verification_scope.is_empty() {
                journal.verification_scope.as_str()
            } else {
                job.verification_scope.as_str()
            };
            aggregate_verification_scopes([source_scope, "targetReadback"])
        };
        job.stage = "finished".into();
        job.progress = 1.;
        job.message = if has_corrupted {
            let corrupted = publication
                .files
                .iter()
                .filter(|file| file.is_corrupted)
                .count();
            format!(
                "文件恢复完成：检测到归档结构或 CRC 异常，输出可能不完整；已抢救 {} 个文件（其中 {} 项数据损坏）；原始压缩包已保留。",
                publication.files.len(),
                corrupted
            )
        } else if journal.partial {
            "文件恢复完成：部分文件解压完成；原始压缩包已保留。".into()
        } else {
            "文件恢复并解压完成；原始压缩包已保留。".into()
        };
        job.finished_at = Some(now());
        repository.commit_publication(&publication, job, &journal.events)
    }
}

impl StagedPublication {
    pub(super) fn copy_request(&self) -> Option<WorkerRequest> {
        if self.direct {
            return None;
        }
        let mut req = request(
            "copy",
            Path::new(&self.journal.publication.temporary_path),
            &self.settings,
        );
        req.files = self.journal.publication.files.clone();
        req.max_written_bytes = self.settings.max_expanded_bytes;
        Some(req)
    }

    pub(super) fn deduplicate(
        &self,
    ) -> Result<openunpack_infrastructure::dedup::DedupReport, String> {
        let temp_dir = Path::new(&self.journal.publication.temporary_path);
        openunpack_infrastructure::dedup::deduplicate_extracted_files(
            temp_dir,
            &self.journal.publication.files,
        )
        .map_err(|e| format!("DeduplicationError: {e}"))
    }

    pub(super) fn finish_copy(&self) -> Result<(), String> {
        if self.settings.clean_empty_dirs {
            paths::prune_empty_directories(Path::new(&self.journal.publication.temporary_path))?;
        }
        if self.settings.deduplicate_hardlinks {
            let _ = self.deduplicate();
        }
        Ok(())
    }

    pub(super) fn readback(&self) -> bool {
        self.readback
    }

    pub(super) async fn seal(
        mut self,
        repository: &Repository,
        cancel: Arc<AtomicBool>,
    ) -> Result<SealedPublication, String> {
        drop(self.temporary_lease.take());
        let publication = &mut self.journal.publication;
        let verification_started = Instant::now();
        let tree = lock_validated_publication(
            Path::new(&publication.temporary_path),
            &publication.id,
            &publication.manifest_hash,
            &publication.files,
            self.readback,
            cancel,
        )
        .await?;
        let verification_millis = verification_started.elapsed().as_millis() as u64;
        publication.state = "ready".into();
        repository.save_publication(publication)?;
        Ok(SealedPublication {
            staged: self,
            tree,
            verification_millis,
        })
    }
}

impl SealedPublication {
    pub(super) fn verification_millis(&self) -> u64 {
        self.verification_millis
    }
}

impl CommittedPublication {
    pub(super) fn cleanup_allowed(&self) -> bool {
        self.cleanup_allowed
    }
    pub(super) fn message(&self) -> &str {
        &self.message
    }
}

impl RecoveryPublication {
    pub(super) async fn seal(&self, cancel: Arc<AtomicBool>) -> Result<paths::LockedTree, String> {
        let p = &self.publication;
        lock_validated_publication(
            &self.target,
            &p.id,
            &p.manifest_hash,
            &p.files,
            true,
            cancel,
        )
        .await
    }
}

fn terminal_status(corrupted: bool, partial: bool) -> &'static str {
    if corrupted {
        "salvaged"
    } else if partial {
        "partial"
    } else {
        "completed"
    }
}
