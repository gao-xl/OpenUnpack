use crate::types::JobView;
use std::path::Path;

/// Triggers the configured on-complete hook script in the background with
/// task metadata passed exclusively via environment variables.
pub fn trigger_on_complete_hook(
    job: &JobView,
    hook_script: Option<&str>,
    on_failure: Option<Box<dyn FnOnce(String) + Send + 'static>>,
) {
    let Some(script) = hook_script else { return };
    let script_path = script.trim();
    if script_path.is_empty() {
        return;
    }

    let job_id = job.id.clone();
    let status = job.status.clone();
    let source_path = job.source.path.clone();
    let archive_name = job.source.display_name.clone();
    let message = job.message.clone();
    let output_path = job.output_path.clone().unwrap_or_else(|| {
        if !job.options.output_root.trim().is_empty() {
            job.options.output_root.clone()
        } else {
            Path::new(&source_path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        }
    });
    let completed_bytes = job
        .transfer_progress
        .as_ref()
        .map(|p| p.completed_bytes.to_string())
        .unwrap_or_else(|| job.source.size.to_string());
    let total_bytes = job
        .transfer_progress
        .as_ref()
        .map(|p| p.total_bytes.to_string())
        .unwrap_or_else(|| job.source.size.to_string());

    let script_cmd = script_path.to_string();
    tokio::spawn(async move {
        let script_file = std::path::Path::new(&script_cmd);
        if !script_file.is_file() {
            tracing::warn!(script = %script_cmd, "on-complete hook is not an existing file");
            return;
        }
        // Job data travels through the environment only. Passing it as argv into
        // `cmd.exe` let an archive-controlled output path inject commands at `&`.
        #[cfg(target_os = "windows")]
        let mut cmd = {
            // CREATE_NO_WINDOW keeps the console host (powershell.exe / cmd.exe)
            // from flashing a visible window when the on-complete hook runs.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            if script_cmd.to_lowercase().ends_with(".ps1") {
                let mut c = tokio::process::Command::new("powershell.exe");
                c.arg("-ExecutionPolicy")
                    .arg("Bypass")
                    .arg("-File")
                    .arg(&script_cmd)
                    .creation_flags(CREATE_NO_WINDOW);
                c
            } else if script_cmd.to_lowercase().ends_with(".bat")
                || script_cmd.to_lowercase().ends_with(".cmd")
            {
                if script_cmd.contains(['&', '|', '<', '>', '^', '%', '!', '"', '\r', '\n']) {
                    tracing::warn!(
                        script = %script_cmd,
                        "on-complete hook path contains shell metacharacters and was refused"
                    );
                    return;
                }
                let mut c = tokio::process::Command::new("cmd.exe");
                c.arg("/C")
                    .arg(&script_cmd)
                    .creation_flags(CREATE_NO_WINDOW);
                c
            } else {
                tokio::process::Command::new(&script_cmd)
            }
        };

        #[cfg(not(target_os = "windows"))]
        let mut cmd = {
            let mut c = tokio::process::Command::new("sh");
            c.arg(&script_cmd);
            c
        };

        cmd.env("OPENUNPACK_JOB_ID", &job_id)
            .env("OPENUNPACK_STATUS", &status)
            .env("OPENUNPACK_SOURCE", &source_path)
            .env("OPENUNPACK_OUTPUT", &output_path)
            .env("OPENUNPACK_ARCHIVE_NAME", &archive_name)
            .env("OPENUNPACK_MESSAGE", &message)
            .env("OPENUNPACK_COMPLETED_BYTES", &completed_bytes)
            .env("OPENUNPACK_TOTAL_BYTES", &total_bytes);

        match cmd.output().await {
            Ok(out) => {
                if !out.status.success() {
                    let detail = hook_error_detail(&out.stderr, &out.stdout);
                    tracing::warn!(
                        script = %script_cmd,
                        status = ?out.status,
                        "on-complete hook exited with a failure status"
                    );
                    if let Some(cb) = on_failure {
                        cb(detail);
                    }
                }
            }
            Err(e) => {
                tracing::error!(script = %script_cmd, error = %e, "failed to spawn on-complete hook");
                if let Some(cb) = on_failure {
                    cb(format!("无法启动完成后脚本：{e}"));
                }
            }
        }
    });
}

/// Extracts a short, UI-safe failure message from a hook process's output.
fn hook_error_detail(stderr: &[u8], stdout: &[u8]) -> String {
    let mut detail = String::from_utf8_lossy(stderr).trim().to_string();
    if detail.is_empty() {
        detail = String::from_utf8_lossy(stdout).trim().to_string();
    }
    if detail.chars().count() > 500 {
        detail = detail.chars().take(500).collect::<String>();
        detail.push('…');
    }
    if detail.is_empty() {
        detail = "脚本以非零状态退出。".into();
    }
    detail
}
