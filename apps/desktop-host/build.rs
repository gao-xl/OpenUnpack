fn main() {
    let mut attributes = tauri_build::Attributes::new();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Tauri's resource helper links its manifest only to binary targets.
        // The library unit-test executable also imports Common Controls v6.
        // Embed the same dependency through the linker for every host target,
        // and omit Tauri's duplicate manifest while retaining icons/version info.
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        let manifest = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
            .join("common-controls.manifest");
        std::fs::write(
            &manifest,
            r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency><dependentAssembly><assemblyIdentity type="win32"
    name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
    processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"
  /></dependentAssembly></dependency>
</assembly>"#,
        )
        .expect("无法生成 Windows Common Controls 清单");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::try_build(
        attributes.app_manifest(tauri_build::AppManifest::new().commands(&[
            "set_ui_language",
            "api_status",
            "api_configure",
            "api_create_client",
            "api_revoke_client",
            "api_open_docs",
            "snapshot",
            "queue_snapshot",
            "snapshot_update",
            "history_page",
            "job_details",
            "scan_sources",
            "cancel_scan",
            "preview_exclusions",
            "cancel_preview_exclusions",
            "add_jobs",
            "start_jobs",
            "pause_jobs",
            "cancel_jobs",
            "retry_job",
            "retry_node",
            "remove_job",
            "clear_completed",
            "passwords",
            "save_password",
            "delete_password",
            "reveal_password",
            "copy_password",
            "bulk_password_action",
            "preview_import",
            "confirm_import",
            "export_backup",
            "preview_restore_backup",
            "confirm_restore_backup",
            "save_settings",
            "history",
            "clear_history",
            "recover_job",
            "inspect_archive",
            "cancel_inspect_archive",
            "extract_selective",
            "open_path",
            "pick_files",
            "pick_directory",
            "pick_save_backup",
            "pick_open_backup",
            "read_import_file",
            "preview_toolbox_operation",
            "execute_toolbox_operation",
            "execute_post_action",
            "get_shell_context_menu_status",
            "set_shell_context_menu_status",
        ])),
    )
    .expect("无法生成桌面权限清单");
}
