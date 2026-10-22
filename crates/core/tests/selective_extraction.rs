#![cfg(windows)]
use openunpack_core::{
    coordinator::{Coordinator, JobExecutionPolicy},
    paths::DirectoryGuard,
    types::{JobView, Settings},
};
use openunpack_protocol::SelectiveExtractRequest;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

struct Rig {
    core: Arc<Coordinator>,
    root: PathBuf,
    archive: PathBuf,
    output: PathBuf,
    _temp: tempfile::TempDir,
}
impl Rig {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("private");
        DirectoryGuard::open(temp.path())
            .unwrap()
            .create_private_child(&root)
            .unwrap();
        let output = root.join("output");
        let payload = root.join("payload");
        for path in [
            &output,
            &payload,
            &payload.join("nested"),
            &payload.join("other"),
        ] {
            fs::create_dir(path).unwrap();
        }
        fs::write(payload.join("root.txt"), b"selected root").unwrap();
        fs::write(
            payload.join("nested/root.txt"),
            b"must not be selected by root basename",
        )
        .unwrap();
        fs::write(
            payload.join("other/root.txt"),
            b"a second colliding basename",
        )
        .unwrap();
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let archive = root.join("fixture.zip");
        assert!(
            Command::new(workspace.join("tools/7zip/7z.exe"))
                .current_dir(&payload)
                .args(["a", "-tzip", "-mx=0", "-y", "-bso0", "-bsp0", "-bse0"])
                .arg(&archive)
                .arg(".")
                .status()
                .unwrap()
                .success()
        );
        let worker = std::env::var_os("OPENUNPACK_TEST_WORKER")
            .map(PathBuf::from)
            .expect("build and set the current Worker");
        let engine = std::env::var_os("OPENUNPACK_TEST_ENGINE")
            .map(PathBuf::from)
            .unwrap_or(workspace.join("tools/7zip/7z.dll"));
        let core = Coordinator::open(root.join("data"), worker, engine).unwrap();
        core.save_settings(Settings {
            min_free_bytes: 0,
            max_expanded_bytes: 16 * 1024 * 1024,
            ..Settings::default()
        })
        .unwrap();
        Self {
            core,
            root,
            archive,
            output,
            _temp: temp,
        }
    }
    fn request(&self, entries: &[&str], flatten: bool) -> SelectiveExtractRequest {
        SelectiveExtractRequest {
            archive_path: self.archive.to_string_lossy().into_owned(),
            entry_paths: entries.iter().map(|path| (*path).into()).collect(),
            destination_dir: Some(self.output.to_string_lossy().into_owned()),
            password: None,
            flatten,
        }
    }
}
struct Policy {
    read: PathBuf,
    write: PathBuf,
}
impl JobExecutionPolicy for Policy {
    fn authorize(&self, _: &JobView) -> Result<(), String> {
        Err("ApiAuthorizationDenied".into())
    }
    fn authorize_path(&self, path: &Path, write: bool) -> Result<(), String> {
        let path = path.canonicalize().map_err(|_| "ApiAuthorizationDenied")?;
        let root = if write { &self.write } else { &self.read }
            .canonicalize()
            .map_err(|_| "ApiAuthorizationDenied")?;
        if path.starts_with(root) {
            Ok(())
        } else {
            Err("ApiAuthorizationDenied".into())
        }
    }
}

#[tokio::test]
async fn selected_root_file_does_not_expand_same_basename_and_preserves_existing_files() {
    let rig = Rig::new();
    let original = fs::read(&rig.archive).unwrap();
    fs::write(rig.output.join("root.txt"), b"existing user output").unwrap();
    let result = rig
        .core
        .extract_selective(rig.request(&["root.txt"], false))
        .await
        .unwrap();
    assert_eq!(result.extracted_files.len(), 1);
    assert_eq!(
        fs::read(Path::new(&result.output_directory).join("root.txt")).unwrap(),
        b"selected root"
    );
    assert!(!Path::new(&result.output_directory).join("nested").exists());
    assert_eq!(
        fs::read(rig.output.join("root.txt")).unwrap(),
        b"existing user output"
    );
    assert_eq!(fs::read(&rig.archive).unwrap(), original);
    assert!(rig.core.vault().unwrap().views().unwrap().is_empty());
}

#[tokio::test]
async fn selected_directory_and_flatten_use_verified_worker_output() {
    let rig = Rig::new();
    let result = rig
        .core
        .extract_selective(rig.request(&["nested"], false))
        .await
        .unwrap();
    assert_eq!(result.extracted_files.len(), 1);
    assert!(
        Path::new(&result.output_directory)
            .join("nested/root.txt")
            .is_file()
    );
    let flat = rig
        .core
        .extract_selective(rig.request(&["nested/root.txt"], true))
        .await
        .unwrap();
    assert_eq!(flat.extracted_files.len(), 1);
    assert_eq!(
        fs::read(Path::new(&flat.output_directory).join("root.txt")).unwrap(),
        b"must not be selected by root basename"
    );
}

#[tokio::test]
async fn flatten_conflicts_and_invalid_selections_fail_without_changing_user_files() {
    let rig = Rig::new();
    let original = fs::read(&rig.archive).unwrap();
    assert!(
        rig.core
            .extract_selective(rig.request(&["nested/root.txt", "other/root.txt"], true))
            .await
            .is_err()
    );
    assert!(
        rig.core
            .extract_selective(rig.request(&["../outside"], false))
            .await
            .is_err()
    );
    assert!(
        rig.core
            .extract_selective(rig.request(&["missing.txt"], false))
            .await
            .is_err()
    );
    assert_eq!(fs::read_dir(&rig.output).unwrap().count(), 0);
    assert_eq!(fs::read(&rig.archive).unwrap(), original);
}

#[tokio::test]
async fn selective_api_checks_both_read_and_effective_default_output_before_writing() {
    let rig = Rig::new();
    let policy: Arc<dyn JobExecutionPolicy> = Arc::new(Policy {
        read: rig.root.clone(),
        write: rig.output.clone(),
    });
    let mut request = rig.request(&["root.txt"], false);
    request.destination_dir = None;
    assert_eq!(
        rig.core
            .extract_selective_authorized(request, policy.clone())
            .await
            .unwrap_err(),
        "ApiAuthorizationDenied"
    );
    let result = rig
        .core
        .extract_selective_authorized(rig.request(&["root.txt"], false), policy)
        .await
        .unwrap();
    assert_eq!(result.extracted_files.len(), 1);
    let denied: Arc<dyn JobExecutionPolicy> = Arc::new(Policy {
        read: rig.root.join("outside"),
        write: rig.output.clone(),
    });
    assert_eq!(
        rig.core
            .extract_selective_authorized(rig.request(&["root.txt"], false), denied)
            .await
            .unwrap_err(),
        "ApiAuthorizationDenied"
    );
}

struct RevocablePolicy {
    paths: Policy,
    revoked: Arc<AtomicBool>,
}
impl JobExecutionPolicy for RevocablePolicy {
    fn authorize(&self, _: &JobView) -> Result<(), String> {
        Err("ApiAuthorizationDenied".into())
    }
    fn authorize_path(&self, path: &Path, write: bool) -> Result<(), String> {
        if self.revoked.load(Ordering::Acquire) {
            Err("ApiAuthorizationDenied".into())
        } else {
            self.paths.authorize_path(path, write)
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoking_access_during_worker_extraction_prevents_publication_and_preserves_source() {
    let rig = Rig::new();
    let original = fs::read(&rig.archive).unwrap();
    let revoked = Arc::new(AtomicBool::new(false));
    let policy: Arc<dyn JobExecutionPolicy> = Arc::new(RevocablePolicy {
        paths: Policy {
            read: rig.root.clone(),
            write: rig.output.clone(),
        },
        revoked: revoked.clone(),
    });
    let core = rig.core.clone();
    let request = rig.request(&["root.txt"], false);
    let operation =
        tokio::spawn(async move { core.extract_selective_authorized(request, policy).await });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if fs::read_dir(&rig.output).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".openunpack-select-")
        }) {
            revoked.store(true, Ordering::Release);
            break;
        }
        assert!(
            !operation.is_finished() && Instant::now() < deadline,
            "must observe the real in-flight workspace before revoking access"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let result = tokio::time::timeout(Duration::from_secs(30), operation)
        .await
        .expect("revoked extraction must terminate")
        .unwrap();
    assert_eq!(result.unwrap_err(), "ApiAuthorizationDenied");
    assert_eq!(fs::read_dir(&rig.output).unwrap().count(), 0);
    assert_eq!(fs::read(&rig.archive).unwrap(), original);
    assert!(rig.core.vault().unwrap().views().unwrap().is_empty());
}
