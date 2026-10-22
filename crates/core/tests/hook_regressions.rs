use openunpack_core::coordinator::trigger_on_complete_hook;
use openunpack_core::types::{JobView, Settings, SourceGroup};
use std::collections::BTreeMap;
use std::fs;
use tempfile::tempdir;

fn mock_job(id: &str, output_path: Option<String>) -> JobView {
    JobView {
        id: id.to_string(),
        source: SourceGroup {
            path: "test.zip".into(),
            display_name: "test.zip".into(),
            volumes: vec!["test.zip".into()],
            size: 1024,
            missing_volume: false,
        },
        options: Settings::default(),
        status: "completed".into(),
        stage: "completed".into(),
        progress: 1.0,
        message: "Unpacked successfully".into(),
        attempts: 1,
        output_path,
        verification_scope: "standard".into(),
        created_at: "2026-10-20T10:00:00Z".into(),
        finished_at: Some("2026-10-20T10:00:05Z".into()),
        workspace: None,
        nodes: vec![],
        stage_times: BTreeMap::new(),
        transfer_progress: None,
    }
}

#[tokio::test]
async fn test_on_complete_hook_empty_ignored() {
    let job = mock_job("test-job-empty", Some("out".into()));

    // None or empty script should return safely and not panic
    trigger_on_complete_hook(&job, None);
    trigger_on_complete_hook(&job, Some("   "));
}

#[tokio::test]
async fn test_on_complete_hook_execution() {
    let dir = tempdir().unwrap();
    let marker_path = dir.path().join("hook_output.txt");
    let script_path = dir.path().join("hook_test.bat");

    #[cfg(target_os = "windows")]
    {
        let bat_content = format!(
            "@echo off\r\necho %OPENUNPACK_JOB_ID% > \"{}\"\r\necho %OPENUNPACK_STATUS% >> \"{}\"\r\n",
            marker_path.display(),
            marker_path.display()
        );
        fs::write(&script_path, bat_content).unwrap();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let sh_content = format!(
            "#!/bin/sh\necho \"$OPENUNPACK_JOB_ID\" > \"{}\"\necho \"$OPENUNPACK_STATUS\" >> \"{}\"\n",
            marker_path.display(),
            marker_path.display()
        );
        fs::write(&script_path, sh_content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    let job = mock_job(
        "hook-job-999",
        Some(dir.path().to_string_lossy().into_owned()),
    );

    trigger_on_complete_hook(&job, Some(script_path.to_str().unwrap()));

    // Wait for the spawned async process to execute
    let mut success = false;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if marker_path.exists()
            && let Ok(content) = fs::read_to_string(&marker_path)
            && content.contains("hook-job-999")
            && content.contains("completed")
        {
            success = true;
            break;
        }
    }

    assert!(
        success,
        "Hook did not produce expected output with OPENUNPACK_JOB_ID within timeout"
    );
}
