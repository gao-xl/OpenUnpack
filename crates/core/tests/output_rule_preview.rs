#![cfg(windows)]
use openunpack_core::worker::WorkerClient;
use std::path::PathBuf;

#[tokio::test]
async fn actual_worker_rule_preview_matches_utf16_and_explicit_directories() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let executable = std::env::var_os("OPENUNPACK_TEST_WORKER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.join("src/OpenUnpack.Worker/bin/Release/net10.0-windows/OpenUnpack.Worker.exe")
        });
    let engine = root.join("tools/7zip/7z.dll");
    let worker = WorkerClient { executable, engine };
    let paths = vec![
        "😀.txt".into(),
        "ς.txt".into(),
        "temp/".into(),
        "temp/child.bin".into(),
        "temporary/file.bin".into(),
        "ı.txt".into(),
    ];
    assert!(
        !worker
            .preview_output_rules(&paths, &["?.txt".into()])
            .await
            .unwrap()[0]
    );
    assert!(
        worker
            .preview_output_rules(&paths, &["??.txt".into()])
            .await
            .unwrap()[0]
    );
    assert!(
        worker
            .preview_output_rules(&paths, &["σ.txt".into()])
            .await
            .unwrap()[1]
    );
    assert!(
        !worker
            .preview_output_rules(&paths, &["I.txt".into()])
            .await
            .unwrap()[5]
    );
    assert_eq!(
        worker
            .preview_output_rules(&paths, &["temp".into()])
            .await
            .unwrap(),
        vec![false, false, true, true, false, false]
    );
    assert!(
        !worker
            .preview_output_rules(&["temp/file.bin".into()], &["temp".into()])
            .await
            .unwrap()[0]
    );
    assert!(
        worker
            .preview_output_rules(&paths, &["a".repeat(513)])
            .await
            .is_err()
    );
    assert!(
        worker
            .preview_output_rules(&vec!["a".repeat(32768); 40], &[])
            .await
            .is_err()
    );
}
