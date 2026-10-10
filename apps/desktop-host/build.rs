fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
        "snapshot","scan_sources","cancel_scan","preview_exclusions","add_jobs","start_jobs","pause_jobs","cancel_jobs","retry_job","retry_node","remove_job","clear_completed",
        "passwords","save_password","delete_password","reveal_password","copy_password","bulk_password_action","preview_import","confirm_import",
        "export_backup","preview_restore_backup","confirm_restore_backup","save_settings","history","clear_history","recover_job","open_path","pick_files","pick_directory","pick_save_backup","pick_open_backup","read_import_file",
    ]))).expect("无法生成桌面权限清单");
}
