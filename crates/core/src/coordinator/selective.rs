//! Selective extraction uses the bounded Worker and verified publication guards.
use super::*;
use openunpack_protocol::{SelectiveExtractRequest, SelectiveExtractResult};

impl Coordinator {
    pub async fn extract_selective(
        &self,
        request: SelectiveExtractRequest,
    ) -> Result<SelectiveExtractResult, String> {
        self.extract_selective_inner(request, None).await
    }

    pub async fn extract_selective_authorized(
        &self,
        request: SelectiveExtractRequest,
        policy: Arc<dyn JobExecutionPolicy>,
    ) -> Result<SelectiveExtractResult, String> {
        self.extract_selective_inner(request, Some(policy)).await
    }

    async fn extract_selective_inner(
        &self,
        mut input: SelectiveExtractRequest,
        policy: Option<Arc<dyn JobExecutionPolicy>>,
    ) -> Result<SelectiveExtractResult, String> {
        let selectors = literal_selectors(&input.entry_paths)?;
        let source_path = PathBuf::from(&input.archive_path);
        paths::check_path(&source_path)?;
        if let Some(policy) = &policy {
            policy.authorize_path(&source_path, false)?;
        }
        let scan = scanner::scan(&[input.archive_path.clone()], false, &[])?;
        let source = scan
            .sources
            .into_iter()
            .next()
            .ok_or("源压缩包不存在或不可读。")?;
        if source.missing_volume {
            return Err("MissingVolume".into());
        }
        let output = input
            .destination_dir
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| source_path.parent().unwrap_or(Path::new("")).to_owned());
        paths::check_path(&output)?;
        let authorize = || -> Result<(), String> {
            if let Some(policy) = &policy {
                for volume in &source.volumes {
                    policy.authorize_path(Path::new(volume), false)?;
                }
                policy.authorize_path(&output, true)?;
            }
            Ok(())
        };
        authorize()?;
        let settings = self.settings.lock().map_err(|_| lock_error())?.clone();
        let output_guard = paths::DirectoryGuard::create(&output).map_err(|_| "UnsafePath")?;
        let witness = source_witnesses(&source)?;
        let id = uid();
        let work = output.join(format!(".openunpack-select-{id}.tmp"));
        paths::mark_owned(&work, &id, "")?;
        let lease = lock_private_workspace(&work)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let mut extraction = request("extract", &work, &settings);
        extraction.archive_path = source.path.clone();
        extraction.allowed_volumes = source.volumes.clone();
        extraction.password = input
            .password
            .take()
            .filter(|password| !password.is_empty());
        extraction.salvage_corrupted = false;
        extraction.exclusions.clear();
        extraction.inclusions = selectors.iter().map(|path| format!("./{path}")).collect();
        let event = self
            .worker
            .run_with_resources(
                extraction,
                cancel.clone(),
                Arc::new(|_: &WorkerEvent| {}),
                settings.concurrency,
                None,
            )
            .await;
        drop(lease);
        if event
            .as_ref()
            .err()
            .is_some_and(|error| error == "WorkerExitUnconfirmed")
        {
            return Err("WorkerExitUnconfirmed".into());
        }
        let mut staging = work.clone();
        let result = async {
            let mut event = event?;
            if event.kind != "completed" {
                return Err(if event.error_code.is_empty() {
                    "IoError".into()
                } else {
                    std::mem::take(&mut event.error_code)
                });
            }
            if event.verification_scope.contains("salvaged")
                || event.files.iter().any(|file| file.is_corrupted)
            {
                return Err("Corrupt".into());
            }
            let mut files = Vec::new();
            for file in std::mem::take(&mut event.files) {
                let relative = paths::relative(&file.relative_path)?;
                let normalized = paths::path_string(&relative).replace('\\', "/");
                if !selector_matches(&normalized, file.is_directory, &selectors) {
                    return Err("WorkerManifestCorrupted".into());
                }
                files.push(CopyFile {
                    source_path: paths::path_string(&work.join(relative)),
                    relative_path: file.relative_path,
                    size: file.size,
                    size_known: file.size_known,
                    sha256: file.sha256,
                    is_directory: file.is_directory,
                    is_corrupted: file.is_corrupted,
                });
            }
            if files.is_empty() {
                return Err("未找到所选归档条目。".into());
            }
            validate_manifest(&files)?;
            let verified =
                lock_validated_publication(&work, &id, "", &files, true, cancel.clone()).await?;
            drop(verified);
            if input.flatten {
                files.retain(|file| !file.is_directory);
                for file in &mut files {
                    file.relative_path = paths::relative(&file.relative_path)?
                        .file_name()
                        .ok_or("UnsafePath")?
                        .to_string_lossy()
                        .into_owned();
                }
                validate_manifest(&files)?;
                staging = output.join(format!(".openunpack-select-flat-{id}.tmp"));
                paths::mark_owned(&staging, &id, "")?;
                let lease = lock_private_workspace(&staging)?;
                let mut copy = request("copy", &staging, &settings);
                copy.files = files.clone();
                for batch in transfer_batches(copy)? {
                    authorize()?;
                    let copied = self
                        .worker
                        .run_with_resources(
                            batch,
                            cancel.clone(),
                            Arc::new(|_: &WorkerEvent| {}),
                            settings.concurrency,
                            None,
                        )
                        .await?;
                    if copied.kind != "completed" {
                        return Err(if copied.error_code.is_empty() {
                            "IoError".into()
                        } else {
                            copied.error_code.clone()
                        });
                    }
                }
                drop(lease);
            }
            authorize()?;
            verify_witnesses(&source, &witness)?;
            let mut tree =
                lock_validated_publication(&staging, &id, "", &files, true, cancel).await?;
            // Readback can be lengthy; a grant revoked during that await must
            // not authorize publication of a new final directory.
            authorize()?;
            verify_witnesses(&source, &witness)?;
            let name = format!(
                "{}-selected-{}",
                scanner::output_name(&source.path),
                &id[..8]
            );
            let target = output.join(name);
            tree.rename_no_replace(&output_guard, &target)
                .map_err(|_| "目标目录提交失败；临时结果已保留。")?;
            let total_bytes = files
                .iter()
                .filter(|file| !file.is_directory)
                .try_fold(0u64, |sum, file| sum.checked_add(file.size))
                .ok_or("ResourceLimit")?;
            let extracted_files = files
                .iter()
                .filter(|file| !file.is_directory)
                .map(|file| paths::path_string(&target.join(&file.relative_path)))
                .collect();
            drop(tree);
            remove_publication_marker(&target, &id, "")?;
            Ok(SelectiveExtractResult {
                output_directory: paths::path_string(&target),
                extracted_files,
                total_bytes,
            })
        }
        .await;
        // An unconfirmed copy preserves both workspaces until child exit can be established.
        if result
            .as_ref()
            .err()
            .is_none_or(|error| error != "WorkerExitUnconfirmed")
        {
            let _ = paths::remove_owned(&work, &id);
            if staging != work {
                let _ = paths::remove_owned(&staging, &id);
            }
        }
        result
    }
}

fn literal_selectors(paths_input: &[String]) -> Result<Vec<String>, String> {
    if paths_input.is_empty() || paths_input.len() > 10_000 {
        return Err("所选归档条目数量无效。".into());
    }
    paths_input
        .iter()
        .map(|path| {
            if path.len() > 510 {
                return Err("所选归档条目路径过长。".into());
            }
            paths::relative(path.trim_end_matches(['/', '\\']))
                .map(|path| paths::path_string(&path).replace('\\', "/"))
        })
        .collect()
}

fn selector_matches(path: &str, directory: bool, selectors: &[String]) -> bool {
    selectors.iter().any(|selected| {
        path == selected.as_str()
            || path.starts_with(&(selected.clone() + "/"))
            || (directory && selected.starts_with(&(path.to_owned() + "/")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectors_are_literal_root_relative_and_reject_unsafe_paths() {
        let selectors = literal_selectors(&["root.txt".into(), "folder/child.txt".into()]).unwrap();
        assert!(selector_matches("root.txt", false, &selectors));
        assert!(!selector_matches("nested/root.txt", false, &selectors));
        assert!(selector_matches("folder", true, &selectors));
        for invalid in ["../outside.txt", "C:/outside", "a:*", ""] {
            assert!(literal_selectors(&[invalid.into()]).is_err());
        }
    }
}
