//! 系统托盘菜单生命周期与多语言原生菜单文本同步。

use super::*;

pub(super) struct NativeUi {
    pub(super) language: Mutex<&'static str>,
    pub(super) show: tauri::menu::MenuItem<tauri::Wry>,
    pub(super) exit: tauri::menu::MenuItem<tauri::Wry>,
}

pub(super) fn native_text(language: &str) -> [&'static str; 6] {
    match language {
        "en-US" => [
            "Open OpenUnpack",
            "Exit",
            "Exit OpenUnpack",
            "Extraction tasks are running. Choose Yes to cancel tasks and exit, or No to keep running.",
            "Exit timed out",
            "The extraction process has not fully stopped. Temporary directories have been preserved. The window will remain open; check the task status.",
        ],
        "ja-JP" => [
            "OpenUnpack を開く",
            "終了",
            "OpenUnpack を終了",
            "解凍タスクが実行中です。「はい」でタスクをキャンセルして終了し、「いいえ」で実行を続けます。",
            "終了待機がタイムアウトしました",
            "バックグラウンドの解凍プロセスがまだ終了していません。一時フォルダーは保持されています。ウィンドウは開いたままです。タスクの状態を確認してください。",
        ],
        _ => [
            "打开 OpenUnpack",
            "退出",
            "退出 OpenUnpack",
            "当前有解压任务正在进行。选择“是”取消任务并退出；选择“否”继续运行。",
            "退出等待超时",
            "后台解压进程尚未完全退出。已保留临时目录，窗口继续打开，请检查任务状态。",
        ],
    }
}

#[tauri::command]
pub(crate) fn set_ui_language(
    window: WebviewWindow,
    state: State<'_, NativeUi>,
    language: String,
) -> Result<(), String> {
    verify_frontend_window(&window)?;
    let language = match language.as_str() {
        "zh-CN" => "zh-CN",
        "en-US" => "en-US",
        "ja-JP" => "ja-JP",
        _ => return Err("Unsupported UI language".into()),
    };
    let mut current = state
        .language
        .lock()
        .map_err(|_| "UI language state unavailable")?;
    let text = native_text(language);
    state.show.set_text(text[0]).map_err(|e| e.to_string())?;
    state.exit.set_text(text[1]).map_err(|e| e.to_string())?;
    *current = language;
    Ok(())
}

pub(super) fn current_native_text(app: &tauri::AppHandle) -> [&'static str; 6] {
    let language = app
        .try_state::<NativeUi>()
        .and_then(|s| s.language.lock().ok().map(|v| *v))
        .unwrap_or("zh-CN");
    native_text(language)
}

pub(super) fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
pub(super) fn request_exit(app: tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        app.exit(0);
        return;
    };
    if state.exiting.swap(true, Ordering::AcqRel) {
        return;
    }
    let coord = state.coordinator.clone();
    tauri::async_runtime::spawn(async move {
        let c = coord.clone();
        let snap = spawn_blocking(move || c.snapshot()).await;
        let active = match snap {
            Ok(Ok(s)) => s.running,
            _ => true,
        };
        if active {
            let text = current_native_text(&app);
            let choice = spawn_blocking(move || {
                MessageDialog::new()
                    .set_title(text[2])
                    .set_description(text[3])
                    .set_level(MessageLevel::Info)
                    .set_buttons(MessageButtons::YesNo)
                    .show()
            })
            .await;
            if !matches!(choice, Ok(MessageDialogResult::Yes)) {
                if let Some(s) = app.try_state::<AppState>() {
                    s.exiting.store(false, Ordering::Release);
                }
                return;
            }
            let c = coord.clone();
            let _ = spawn_blocking(move || c.cancel(None)).await;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
            loop {
                let c = coord.clone();
                let status = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    spawn_blocking(move || c.snapshot()),
                )
                .await;
                if matches!(status,Ok(Ok(Ok(s))) if !s.running&& !s.jobs.iter().any(|j|j.status=="stopping"))
                {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    if let Some(s) = app.try_state::<AppState>() {
                        s.exiting.store(false, Ordering::Release);
                    }
                    let text = current_native_text(&app);
                    let _ = spawn_blocking(move || {
                        MessageDialog::new()
                            .set_title(text[4])
                            .set_description(text[5])
                            .set_level(MessageLevel::Warning)
                            .show()
                    })
                    .await;
                    show_main(&app);
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
        }
        let sequence = app
            .try_state::<AppState>()
            .and_then(|s| s.clipboard_sequence.lock().ok().and_then(|v| *v));
        if let Some(sequence) = sequence {
            let _ = spawn_blocking(move || clipboard::clear_if_unchanged(sequence)).await;
        }
        single_instance::remove_wake_token(&coord.data);
        app.exit(0);
    });
}
pub(super) fn install_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };
    let show = MenuItem::with_id(app, "show", "打开 OpenUnpack", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &exit])?;
    let mut tray = TrayIconBuilder::with_id("openunpack-tray")
        .tooltip("OpenUnpack")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "exit" => request_exit(app.clone()),
            _ => (),
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } | TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(NativeUi {
        language: Mutex::new("zh-CN"),
        show,
        exit,
    });
    Ok(())
}
