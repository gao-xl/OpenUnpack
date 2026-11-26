//! 解压后系统动作与 Windows 资源管理器右键菜单集成命令。

use super::*;

#[tauri::command]
pub(crate) async fn execute_post_action(
    window: Window,
    state: State<'_, AppState>,
    action: String,
    target_path: Option<String>,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let trimmed = action.trim();
    match trimmed {
        "openOutput" => {
            if let Some(path) = target_path
                && !path.trim().is_empty()
            {
                open_path(window, state, path).await?;
            }
            Ok(())
        }
        "sleep" | "shutdown" => {
            // 安全前置守卫：校验 Coordinator 状态与配置匹配
            let snapshot = state
                .coordinator
                .snapshot()
                .map_err(|e| format!("无法获取运行状态快照: {e}"))?;

            if snapshot.running {
                return Err("解压队列仍在运行中，已拦截关机/休眠操作。".into());
            }

            let has_active_jobs = snapshot.jobs.iter().any(|j| {
                !matches!(
                    j.status.as_str(),
                    "completed" | "salvaged" | "partial" | "failed" | "cancelled" | "skipped"
                )
            });
            if has_active_jobs {
                return Err("队列中仍有未完成的任务，已拦截关机/休眠操作。".into());
            }

            if snapshot.settings.post_extraction_action != trimmed {
                return Err(format!(
                    "当前设置的后置操作为 '{}'，拒绝执行未经配置的 '{}' 操作。",
                    snapshot.settings.post_extraction_action, trimmed
                ));
            }

            if trimmed == "sleep" {
                spawn_blocking(|| -> Result<(), String> {
                    let system_root = std::env::var_os("SystemRoot")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
                    let rundll = system_root.join(r"System32\rundll32.exe");
                    let _ = Command::new(rundll)
                        .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                        .status();
                    Ok(())
                })
                .await
                .map_err(|e| format!("执行休眠失败: {e}"))?
            } else {
                spawn_blocking(|| -> Result<(), String> {
                    let system_root = std::env::var_os("SystemRoot")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
                    let shutdown = system_root.join(r"System32\shutdown.exe");
                    let _ = Command::new(shutdown).args(["/s", "/t", "0"]).status();
                    Ok(())
                })
                .await
                .map_err(|e| format!("执行关机失败: {e}"))?
            }
        }
        "none" => Ok(()),
        _ => Err(format!("未知的操作类型: {}", action)),
    }
}

pub(super) fn reg_command(reg_exe: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(reg_exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[tauri::command]
pub(crate) async fn get_shell_context_menu_status(window: Window) -> Result<bool, String> {
    verify_frontend_window(&window)?;
    spawn_blocking(|| {
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let reg = system_root.join(r"System32\reg.exe");
        let output = reg_command(&reg)
            .args(["query", r"HKCU\Software\Classes\*\shell\OpenUnpack"])
            .output()
            .map_err(|e| format!("查询注册表失败: {e}"))?;
        Ok(output.status.success())
    })
    .await
    .map_err(|e| format!("后台操作失败: {e}"))?
}

#[tauri::command]
pub(crate) async fn set_shell_context_menu_status(
    window: Window,
    enabled: bool,
) -> Result<bool, String> {
    verify_frontend_window(&window)?;
    spawn_blocking(move || {
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let reg = system_root.join(r"System32\reg.exe");
        let targets = [
            r"HKCU\Software\Classes\*\shell\OpenUnpack",
            r"HKCU\Software\Classes\Directory\shell\OpenUnpack",
        ];
        if enabled {
            let current_exe =
                std::env::current_exe().map_err(|e| format!("无法定位当前程序路径: {e}"))?;
            let exe_str = current_exe.to_string_lossy();
            let icon_val = format!("\"{}\"", exe_str);
            let cmd_val = format!("\"{}\" \"%1\"", exe_str);

            for key in targets {
                let cmd_key = format!(r"{key}\command");
                let status = reg_command(&reg)
                    .args(["add", key, "/ve", "/d", "使用 OpenUnpack 解压", "/f"])
                    .status()
                    .map_err(|e| format!("写入注册表主项失败: {e}"))?;
                if !status.success() {
                    return Err("注册表添加主项失败".into());
                }

                let _ = reg_command(&reg)
                    .args(["add", key, "/v", "Icon", "/d", &icon_val, "/f"])
                    .status();

                let status_cmd = reg_command(&reg)
                    .args(["add", &cmd_key, "/ve", "/d", &cmd_val, "/f"])
                    .status()
                    .map_err(|e| format!("写入注册表命令项失败: {e}"))?;
                if !status_cmd.success() {
                    return Err("注册表添加命令项失败".into());
                }
            }
            Ok(true)
        } else {
            for key in targets {
                let _ = reg_command(&reg).args(["delete", key, "/f"]).status();
            }
            Ok(false)
        }
    })
    .await
    .map_err(|e| format!("后台操作失败: {e}"))?
}
