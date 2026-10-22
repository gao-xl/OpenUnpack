mod client;

use client::DaemonConnection;
use std::env;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return;
    }

    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("openunpack-cli 0.1.0 (OpenUnpack Core)");
        return;
    }

    // Global flags
    let mut custom_data_dir: Option<PathBuf> = None;
    let mut custom_token: Option<String> = None;
    let mut custom_port: Option<u16> = None;
    let mut json_output = false;

    let mut filtered_args = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--data-dir" && i + 1 < args.len() {
            custom_data_dir = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else if arg == "--token" && i + 1 < args.len() {
            custom_token = Some(args[i + 1].clone());
            i += 2;
        } else if arg == "--port" && i + 1 < args.len() {
            custom_port = args[i + 1].parse().ok();
            i += 2;
        } else if arg == "--json" {
            json_output = true;
            i += 1;
        } else {
            filtered_args.push(arg.clone());
            i += 1;
        }
    }

    if filtered_args.is_empty() {
        print_help();
        return;
    }

    let mut conn = match DaemonConnection::discover(custom_data_dir.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[Error] Daemon discovery failed: {e}");
            std::process::exit(1);
        }
    };

    if let Some(t) = custom_token {
        conn.token = t;
    }
    if let Some(p) = custom_port {
        conn.port = p;
        conn.base_url = format!("http://127.0.0.1:{p}");
    }

    let subcommand = filtered_args[0].as_str();
    let sub_args = &filtered_args[1..];

    match subcommand {
        "status" => {
            cmd_status(&conn, json_output).await;
        }
        "unpack" | "extract" | "add" => {
            cmd_unpack(&conn, sub_args, json_output).await;
        }
        "list" | "jobs" | "ls" => {
            cmd_list(&conn, json_output).await;
        }
        "pause" => {
            cmd_pause(&conn).await;
        }
        "resume" => {
            cmd_resume(&conn).await;
        }
        "cancel" => {
            if sub_args.is_empty() {
                eprintln!("[Error] Usage: openunpack cancel <JOB_ID>");
                std::process::exit(1);
            }
            cmd_cancel(&conn, &sub_args[0]).await;
        }
        "retry" => {
            if sub_args.is_empty() {
                eprintln!("[Error] Usage: openunpack retry <JOB_ID> [--password <PWD>]");
                std::process::exit(1);
            }
            let job_id = &sub_args[0];
            let password = sub_args
                .iter()
                .position(|a| a == "--password" || a == "-p")
                .and_then(|idx| sub_args.get(idx + 1))
                .map(|s| s.as_str());
            cmd_retry(&conn, job_id, password).await;
        }
        _ => {
            eprintln!("[Error] Unknown subcommand: {subcommand}");
            print_help();
            std::process::exit(1);
        }
    }
}

async fn cmd_status(conn: &DaemonConnection, json: bool) {
    match conn.get_status().await {
        Ok(status_val) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&status_val).unwrap());
            } else {
                println!("==================================================");
                println!(" OpenUnpack Daemon Status");
                println!("==================================================");
                println!(" Connection:   {}", conn.base_url);
                println!(" Service:      Online");
                if let Some(running) = status_val.get("running").and_then(|v| v.as_bool()) {
                    println!(
                        " Queue:        {}",
                        if running { "Running" } else { "Idle" }
                    );
                }
                if let Some(port) = status_val.get("port").and_then(|p| p.as_u64()) {
                    println!(" API Port:     {}", port);
                }
                if let Some(clients) = status_val.get("clients").and_then(|c| c.as_array()) {
                    println!(" Authorized:   {} client(s)", clients.len());
                }

                // Check active jobs snapshot
                if let Ok(snap) = conn.get_snapshot().await {
                    let total = snap.jobs.len();
                    let running = snap.jobs.iter().filter(|j| j.status == "running").count();
                    let queued = snap.jobs.iter().filter(|j| j.status == "queued").count();
                    let failed = snap.jobs.iter().filter(|j| j.status == "failed").count();
                    let completed = snap.jobs.iter().filter(|j| j.status == "completed").count();
                    println!("--------------------------------------------------");
                    println!(" Jobs Total:   {}", total);
                    println!(
                        " Running:      {} | Queued: {} | Completed: {} | Failed: {}",
                        running, queued, completed, failed
                    );
                }
                println!("==================================================");
            }
        }
        Err(e) => {
            eprintln!("[Error] Could not connect to OpenUnpack daemon: {e}");
            std::process::exit(1);
        }
    }
}

async fn cmd_unpack(conn: &DaemonConnection, args: &[String], json: bool) {
    let mut paths = Vec::new();
    let mut output_dir: Option<String> = None;
    let mut password: Option<String> = None;
    let mut wait = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if (arg == "--output" || arg == "-o") && i + 1 < args.len() {
            output_dir = Some(args[i + 1].clone());
            i += 2;
        } else if (arg == "--password" || arg == "-p") && i + 1 < args.len() {
            password = Some(args[i + 1].clone());
            i += 2;
        } else if arg == "--wait" || arg == "-w" {
            wait = true;
            i += 1;
        } else if !arg.starts_with('-') {
            paths.push(arg.clone());
            i += 1;
        } else {
            i += 1;
        }
    }

    if paths.is_empty() {
        eprintln!("[Error] Please provide at least one archive file or directory to unpack.");
        eprintln!(
            "Usage: openunpack unpack <PATH...> [--output <DIR>] [--password <PWD>] [--wait]"
        );
        std::process::exit(1);
    }

    let mut all_added = Vec::new();
    for p in &paths {
        match conn
            .add_archive(p, output_dir.as_deref(), password.as_deref(), true)
            .await
        {
            Ok(added) => {
                if !json {
                    println!("+ Added {} job(s) from '{}'", added.len(), p);
                    for j in &added {
                        println!(
                            "  - [{}] {} ({:.1} MB)",
                            j.id,
                            j.source.display_name,
                            j.source.size as f64 / 1_048_576.0
                        );
                    }
                }
                all_added.extend(added);
            }
            Err(e) => {
                eprintln!("[Error] Failed to add archive '{}': {e}", p);
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&all_added).unwrap());
    }

    if wait && !all_added.is_empty() {
        let job_ids: Vec<String> = all_added.into_iter().map(|j| j.id).collect();
        wait_for_jobs(conn, &job_ids).await;
    }
}

async fn wait_for_jobs(conn: &DaemonConnection, job_ids: &[String]) {
    println!("--------------------------------------------------");
    println!(" Waiting for {} job(s) to finish...", job_ids.len());

    let mut last_progress_str = String::new();
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let snap = match conn.get_snapshot().await {
            Ok(s) => s,
            Err(_) => continue,
        };

        let mut all_done = true;
        let mut active_count = 0;
        let mut progress_line = String::new();

        for id in job_ids {
            if let Some(job) = snap.jobs.iter().find(|j| j.id == *id) {
                if job.status == "running"
                    || job.status == "queued"
                    || job.status == "waitingResource"
                {
                    all_done = false;
                    active_count += 1;
                    let pct = (job.progress * 100.0) as u32;
                    progress_line = format!(
                        "[{}] {}: {}% ({}) [remaining: {}]",
                        job.id, job.source.display_name, pct, job.stage, active_count
                    );
                } else if job.status == "failed" {
                    println!("! Job [{}] FAILED: {}", job.id, job.message);
                }
            }
        }

        if !progress_line.is_empty() && progress_line != last_progress_str {
            print!("\r\x1B[K{}", progress_line);
            use std::io::Write;
            let _ = std::io::stdout().flush();
            last_progress_str = progress_line;
        }

        if all_done {
            println!("\r\x1B[K✓ All {} job(s) completed.", job_ids.len());
            break;
        }
    }
}

async fn cmd_list(conn: &DaemonConnection, json: bool) {
    match conn.get_snapshot().await {
        Ok(snap) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&snap.jobs).unwrap());
            } else {
                println!(
                    "{:<14} {:<10} {:<10} {:<8} {:<30}",
                    "JOB ID", "STATUS", "STAGE", "PROGRESS", "ARCHIVE"
                );
                println!("{}", "-".repeat(76));
                for j in &snap.jobs {
                    let pct = format!("{:.0}%", j.progress * 100.0);
                    let mut name: String = j.source.display_name.chars().take(25).collect();
                    if name.chars().count() < j.source.display_name.chars().count() {
                        name.push_str("...");
                    }
                    println!(
                        "{:<14} {:<10} {:<10} {:<8} {:<30}",
                        j.id, j.status, j.stage, pct, name
                    );
                }
                println!("{}", "-".repeat(76));
                println!("Total: {} job(s)", snap.jobs.len());
            }
        }
        Err(e) => {
            eprintln!("[Error] Failed to fetch job list: {e}");
            std::process::exit(1);
        }
    }
}

async fn cmd_pause(conn: &DaemonConnection) {
    match conn.pause_jobs().await {
        Ok(_) => println!("✓ Queue paused."),
        Err(e) => eprintln!("[Error] Failed to pause queue: {e}"),
    }
}

async fn cmd_resume(conn: &DaemonConnection) {
    match conn.resume_jobs().await {
        Ok(_) => println!("✓ Queue resumed."),
        Err(e) => eprintln!("[Error] Failed to resume queue: {e}"),
    }
}

async fn cmd_cancel(conn: &DaemonConnection, job_id: &str) {
    match conn.cancel_job(job_id).await {
        Ok(_) => println!("✓ Job [{job_id}] cancelled."),
        Err(e) => eprintln!("[Error] Failed to cancel job [{job_id}]: {e}"),
    }
}

async fn cmd_retry(conn: &DaemonConnection, job_id: &str, password: Option<&str>) {
    match conn.retry_job(job_id, password).await {
        Ok(_) => println!("✓ Job [{job_id}] queued for retry."),
        Err(e) => eprintln!("[Error] Failed to retry job [{job_id}]: {e}"),
    }
}

fn print_help() {
    println!(
        r#"OpenUnpack CLI - 解压工坊轻量命令行客户端

USAGE:
    openunpack [OPTIONS] <SUBCOMMAND>

SUBCOMMANDS:
    status                 查看守护进程/服务运行状态与队列概览
    unpack <PATHS...>      添加解压任务 (支持文件、目录)
        -o, --output <DIR> 指定解压输出目录
        -p, --password <P> 提供解压密码
        -w, --wait         阻塞等待所有任务解压完成
    list, jobs, ls         列出当前所有解压作业
    pause                  暂停当前作业调度
    resume                 恢复作业调度
    cancel <JOB_ID>        取消指定作业
    retry <JOB_ID>         重试失败作业 (-p 指定新密码)

GLOBAL OPTIONS:
    --token <TOKEN>        API 访问令牌 (默认自动读取配置)
    --port <PORT>          指定 Local API 端口 (默认 47980)
    --data-dir <DIR>       指定数据目录 (默认标准系统目录)
    --json                 以 JSON 结构输出结果
    -h, --help             显示此帮助信息
    -V, --version          显示版本号
"#
    );
}
