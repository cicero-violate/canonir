mod config;
mod process;
mod tlog;
mod watcher;

use crate::config::{load_config, write_default_config, ProcessConfig};
use crate::process::ProcessManager;
use crate::watcher::{affected_crates, crate_for_path, start_watcher};
use anyhow::Result;
use canon_event_log::{error, info};
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::flag;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    let root = std::env::current_dir()?;
    let config_path = root.join("supervisor.toml");
    if !config_path.exists() {
        write_default_config(&config_path)?;
        info(
            "supervisor",
            "default_config_written",
            serde_json::json!({ "path": config_path.display().to_string() }),
        );
    }
    let config = load_config(&config_path)?;
    start_event_stream_tail();
    let mut manager = ProcessManager::new();
    for proc in &config.process {
        manager.spawn(proc, false)?;
    }

    let watch_dirs: Vec<PathBuf> = config
        .watcher
        .watch_dirs
        .iter()
        .map(|d| root.join(d))
        .collect();
    let (tx, rx) = mpsc::channel();
    let _watcher = start_watcher(tx, &watch_dirs)?;

    let term = Arc::new(AtomicBool::new(false));
    flag::register(SIGINT, term.clone())?;
    flag::register(SIGTERM, term.clone())?;

    let process_map = build_process_map(&config.process);
    let debounce = Duration::from_millis(config.watcher.debounce_ms);

    loop {
        if term.load(Ordering::Relaxed) {
            manager.shutdown_all(3000);
            break;
        }
        let first = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(path) => path,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        };
        let mut paths = vec![first];
        let start = Instant::now();
        while start.elapsed() < debounce {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(path) => paths.push(path),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }
        for path in &paths {
            let crate_name = crate_for_path(path);
            tlog::emit(
                "file_change_detected",
                serde_json::json!({
                    "path": path.display().to_string(),
                    "crate": crate_name,
                }),
            );
        }
        let affected = affected_crates(&paths);
        handle_changes(&affected, &process_map, &mut manager)?;
    }
    Ok(())
}

fn handle_changes(
    affected: &HashSet<String>,
    process_map: &HashMap<String, Vec<ProcessConfig>>,
    _manager: &mut ProcessManager,
) -> Result<()> {
    let mut to_build: HashSet<String> = affected.iter().cloned().collect();
    for procs in process_map.values() {
        for proc_cfg in procs {
            if proc_cfg.depends_on.iter().any(|dep| affected.contains(dep)) {
                let name = proc_cfg
                    .crate_name
                    .clone()
                    .unwrap_or_else(|| proc_cfg.name.clone());
                to_build.insert(name);
            }
        }
    }

    for crate_name in &to_build {
        tlog::emit(
            "workspace.changed",
            serde_json::json!({ "crate": crate_name }),
        );
    }
    Ok(())
}

fn start_event_stream_tail() {
    let tlog_path = crate::tlog::default_tlog_path();
    thread::spawn(move || {
        if let Err(err) = tail_event_stream(&tlog_path) {
            error(
                "supervisor",
                "event_stream_tail_error",
                serde_json::json!({ "error": err.to_string() }),
            );
        }
    });
}

fn tail_event_stream(path: &std::path::Path) -> anyhow::Result<()> {
    use canon_tlog_replay::{read_any_events_from_path, AnyEvent};
    let mut last_count = 0usize;
    let mut initialized = false;
    loop {
        let events = match read_any_events_from_path(path) {
            Ok(events) => events,
            Err(_) => {
                thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        if !initialized {
            last_count = events.len();
            initialized = true;
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        if events.len() > last_count {
            for event in events.iter().skip(last_count) {
                if let AnyEvent::Canon(canon) = event {
                    match canon.kind.as_str() {
                        "capability_requested" => {
                            let name = canon.payload.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                            info(
                                "supervisor",
                                "capability_requested",
                                serde_json::json!({ "name": name }),
                            );
                        }
                        "capability_completed" => {
                            let name = canon.payload.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                            info(
                                "supervisor",
                                "capability_completed",
                                serde_json::json!({ "name": name }),
                            );
                        }
                        "capability_failed" => {
                            let name = canon.payload.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                            info(
                                "supervisor",
                                "capability_failed",
                                serde_json::json!({ "name": name }),
                            );
                        }
                        "build.started" => {
                            let krate = canon.payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
                            info(
                                "supervisor",
                                "build_started",
                                serde_json::json!({ "crate": krate }),
                            );
                        }
                        "build.completed" => {
                            let krate = canon.payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
                            let ok = canon.payload.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                            info(
                                "supervisor",
                                "build_completed",
                                serde_json::json!({ "crate": krate, "success": ok }),
                            );
                        }
                        "analysis.run" => {
                            info(
                                "supervisor",
                                "analysis_requested",
                                serde_json::json!({ "name": "analysis.run" }),
                            );
                        }
                        "analysis.completed" => {
                            let krate = canon.payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
                            let status = canon.payload.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
                            info(
                                "supervisor",
                                "analysis_completed",
                                serde_json::json!({ "crate": krate, "status": status }),
                            );
                        }
                        "analysis.failed" => {
                            let krate = canon.payload.get("crate").and_then(|v| v.as_str()).unwrap_or("unknown");
                            info(
                                "supervisor",
                                "analysis_failed",
                                serde_json::json!({ "crate": krate }),
                            );
                        }
                        _ => {}
                    }
                }
            }
            last_count = events.len();
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn build_process_map(processes: &[ProcessConfig]) -> HashMap<String, Vec<ProcessConfig>> {
    let mut map: HashMap<String, Vec<ProcessConfig>> = HashMap::new();
    for proc_cfg in processes {
        let name = proc_cfg
            .crate_name
            .clone()
            .unwrap_or_else(|| proc_cfg.name.clone());
        map.entry(name).or_default().push(proc_cfg.clone());
    }
    map
}

 
