use super::{
    AppState, CoordinatorVaultStore, NativeAccess, acquire_instance_lock, canonicalize_existing,
    local_api, logging, resolve_data_directory, resolve_worker_paths_standalone,
    setup_process_environment,
};
use openunpack_core::coordinator::Coordinator;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

pub(crate) fn collect_flag_values(args: &[String], flag: &str) -> Vec<String> {
    args.iter()
        .enumerate()
        .filter(|(_, arg)| arg.as_str() == flag)
        .filter_map(|(index, _)| args.get(index + 1))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn canonical_api_roots(raw: &[String], kind: &str) -> Vec<String> {
    raw.iter()
        .map(|value| {
            let canonical = canonicalize_existing(value).unwrap_or_else(|error| {
                eprintln!("[Error] --{kind}-root '{value}' is not usable: {error}");
                std::process::exit(1);
            });
            if !canonical.is_dir() {
                eprintln!("[Error] --{kind}-root '{value}' must be an existing directory.");
                std::process::exit(1);
            }
            canonical.to_string_lossy().into_owned()
        })
        .collect()
}

pub(crate) fn mask_token(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 10 {
        return "***".to_string();
    }
    let head: String = chars[..6].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

pub(crate) fn open_coordinator_on_runtime(
    data: PathBuf,
    worker: PathBuf,
    engine: PathBuf,
    runtime: &tokio::runtime::Handle,
) -> Result<Arc<Coordinator>, String> {
    // Both host entry points initialize on a plain OS thread. Core captures
    // this handle so its folder watcher can dispatch without a Tokio context.
    let _entered = runtime.enter();
    Coordinator::open(data, worker, engine)
}

pub fn run_headless() {
    println!("==================================================");
    println!(" OpenUnpack Daemon (Headless Mode)");
    println!("==================================================");
    let args: Vec<String> = std::env::args().collect();

    // Check for --data-dir override
    if let Some(pos) = args.iter().position(|a| a == "--data-dir")
        && let Some(val) = args.get(pos + 1)
    {
        unsafe {
            std::env::set_var("OPENUNPACK_DATA_DIR", val);
        }
    }
    // Check for --port override
    let custom_port: Option<u16> = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|pos| args.get(pos + 1))
        .and_then(|val| val.parse().ok());

    // Check for --trust-all (headless only). This is process state, never an
    // inherited environment variable, so a parent process cannot disable path
    // authorization just by launching the daemon.
    if args.iter().any(|a| a == "--trust-all") {
        openunpack_local_api::set_trusted_all_paths(true);
        eprintln!(
            "[Warning] --trust-all enabled: local API path authorization is bypassed for this process."
        );
    }
    // Check for --on-complete or --on-extract-complete
    let on_complete_script: Option<String> = args
        .iter()
        .position(|a| a == "--on-complete" || a == "--on-extract-complete")
        .and_then(|pos| args.get(pos + 1))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    // Check for --watch-dir or --watch
    let watch_dirs: Vec<PathBuf> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--watch-dir" || *a == "--watch")
        .filter_map(|(i, _)| args.get(i + 1).map(PathBuf::from))
        .collect();

    // Credentials and path grants are opt-in: a plain headless boot must neither
    // print a full token into container logs nor force operators to choose
    // between "no file access" and "--trust-all".
    let init_token = args.iter().any(|a| a == "--init-token");
    let read_roots = canonical_api_roots(&collect_flag_values(&args, "--read-root"), "read");
    let write_roots = canonical_api_roots(&collect_flag_values(&args, "--write-root"), "write");

    let data_dir = match resolve_data_directory() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("[Error] Failed to resolve data directory: {err}");
            std::process::exit(1);
        }
    };
    println!("Data directory: {}", data_dir.display());
    if let Some(log_path) = logging::init(&data_dir, true) {
        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            data_dir = %data_dir.display(),
            log = %log_path.display(),
            "openunpack starting (headless)"
        );
        println!("Log file:       {}", log_path.display());
    }

    let lock_file = match acquire_instance_lock(&data_dir) {
        Ok(file) => file,
        Err(err) => {
            tracing::error!("instance lock unavailable: {err}");
            eprintln!(
                "[Error] Another instance is already running or data directory is locked: {err}"
            );
            std::process::exit(1);
        }
    };
    setup_process_environment(&data_dir);

    let (worker_exe, worker_dll) = match resolve_worker_paths_standalone() {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("[Error] Worker engine binaries missing: {err}");
            std::process::exit(1);
        }
    };
    println!("Worker engine:  {}", worker_exe.display());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to initialize Tokio runtime");
    let coordinator =
        match open_coordinator_on_runtime(data_dir.clone(), worker_exe, worker_dll, rt.handle()) {
            Ok(coord) => coord,
            Err(err) => {
                eprintln!("[Error] Failed to initialize Coordinator: {err}");
                std::process::exit(1);
            }
        };

    // `--on-complete` used to be written to an environment variable that nothing
    // read, so headless automation silently had no hook. Persist it into settings
    // instead, which is what the scheduler actually consults.
    if let Some(script) = &on_complete_script {
        let path = std::path::Path::new(script);
        if !path.is_absolute() || !path.is_file() {
            eprintln!(
                "[Error] --on-complete must point to an existing absolute script path: {script}"
            );
            std::process::exit(1);
        }
        match coordinator.snapshot() {
            Ok(snapshot) => {
                let mut settings = snapshot.settings.clone();
                settings.on_complete = Some(script.clone());
                if let Err(err) = coordinator.save_settings(settings) {
                    eprintln!("[Error] Failed to persist --on-complete hook: {err}");
                    std::process::exit(1);
                }
                println!("On-complete hook: {script}");
            }
            Err(err) => {
                eprintln!("[Error] Failed to read settings for --on-complete: {err}");
                std::process::exit(1);
            }
        }
    }

    rt.block_on(async move {
        let mut native_access = NativeAccess::default();
        if let Ok(snap) = coordinator.snapshot() {
            if !snap.settings.output_root.trim().is_empty()
                && let Ok(p) = canonicalize_existing(&snap.settings.output_root)
            {
                native_access.grant_write_dir(p);
            }
            if !snap.settings.cache_root.trim().is_empty()
                && let Ok(p) = canonicalize_existing(&snap.settings.cache_root)
            {
                native_access.grant_write_dir(p);
            }
            for j in &snap.jobs {
                if let Ok(p) = canonicalize_existing(&j.source.path) {
                    native_access.grant_read(p, false);
                }
                for v in &j.source.volumes {
                    if let Ok(p) = canonicalize_existing(v) {
                        native_access.grant_read(p, false);
                    }
                }
            }
        }

        let access_state = Arc::new(Mutex::new(native_access));
        let vault_store = Arc::new(CoordinatorVaultStore(coordinator.clone()));
        let vault_use_cases = Arc::new(openunpack_application::VaultUseCases::new(vault_store));
        let sys_env: Arc<dyn openunpack_application::SystemEnvironment> =
            Arc::new(openunpack_infrastructure::WindowsSystemEnvironment::new());

        let app_state = Arc::new(AppState {
            coordinator: coordinator.clone(),
            vault_use_cases,
            sys_env,
            access: access_state,
            _lock_file: lock_file,
            scans: Mutex::new(HashMap::new()),
            clipboard_sequence: Mutex::new(None),
            exiting: AtomicBool::new(false),
            minimize_to_tray: AtomicBool::new(false),
        });

        // Register any CLI watch directories
        for w in &watch_dirs {
            let p_str = w.to_string_lossy().to_string();
            if let Ok(canon) = canonicalize_existing(&p_str) {
                let canon_str = canon.to_string_lossy().to_string();
                println!("Registering watch directory: {}", canon_str);
                if let Ok(mut lock) = app_state.access.lock() {
                    lock.grant_read(canon.clone(), true);
                    lock.grant_write_dir(canon);
                }
                if let Ok(snap) = coordinator.snapshot() {
                    let mut s = snap.settings.clone();
                    if !s.watched_folders.contains(&canon_str) {
                        s.watched_folders.push(canon_str);
                        let _ = coordinator.save_settings(s);
                    }
                }
            } else {
                eprintln!(
                    "[Warning] Watch directory does not exist or inaccessible: {}",
                    p_str
                );
            }
        }

        // Initialize background folder watcher
        coordinator.init_folder_watcher();

        // Start Local API server
        let api_manager = local_api::ApiManager::open(&coordinator.data);
        if let Ok(store) = api_manager.store() {
            let mut settings = store.settings().unwrap_or_default();
            settings.enabled = true;
            if let Some(port) = custom_port {
                settings.port = port;
            }
            let _ = store.save_settings(settings.clone());
            let _ = api_manager.restore_standalone(app_state.clone()).await;

            // Ensure at least one client exists for local operation. The default
            // grant is deliberately least-privilege: vault and settings writes
            // must be requested explicitly, and the token expires.
            let clients = store.clients().unwrap_or_default();
            if clients.is_empty() || init_token {
                let scopes = [
                    openunpack_local_api::Scope::JobsRead,
                    openunpack_local_api::Scope::JobsWrite,
                    openunpack_local_api::Scope::FilesRead,
                    openunpack_local_api::Scope::FilesWrite,
                    openunpack_local_api::Scope::SettingsRead,
                ]
                .into_iter()
                .collect();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let name = if clients.is_empty() {
                    "default-cli".to_string()
                } else {
                    format!("cli-{now}")
                };
                let default_client = openunpack_local_api::ApiClientInput {
                    name,
                    scopes,
                    read_roots: read_roots.clone(),
                    write_roots: write_roots.clone(),
                    expires_at: Some((now + 30 * 24 * 3600).to_string()),
                };
                match store.create_client(default_client) {
                    Ok(created) => {
                        println!("--------------------------------------------------");
                        println!(" Created API Client: {}", created.client.name);
                        if init_token {
                            println!(" Token: {}", created.token);
                            println!(" Store it now; the full value is never shown again.");
                        } else {
                            println!(
                                " Token: {} (pass --init-token to print a new full token)",
                                mask_token(&created.token)
                            );
                        }
                        if let Some(expires) = &created.client.expires_at {
                            println!(" Expires at (unix seconds): {expires}");
                        }
                    }
                    Err(error) => {
                        eprintln!("[Warning] Failed to create the default API client: {error}");
                    }
                }
            }
        }

        println!("--------------------------------------------------");
        if let Ok(status) = api_manager.status().await
            && let Some(url) = status.base_url
        {
            println!(" Local API listening at: {}", url);
        }
        println!(" OpenUnpack daemon is active. Press Ctrl+C to stop.");
        println!("==================================================");

        let _ = tokio::signal::ctrl_c().await;
        println!("\nReceived shutdown signal, stopping OpenUnpack daemon...");
        let _ = coordinator.cancel(None);
        let _ = coordinator.pause();
    });
}
