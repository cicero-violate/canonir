mod builder;
mod config;
mod process;
mod tlog;
mod watcher;

use crate::builder::build_crate;
use crate::config::{load_config, write_default_config, ProcessConfig, RestartStrategy};
use crate::process::ProcessManager;
use crate::watcher::{affected_crates, crate_for_path, start_watcher};
use anyhow::Result;
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use signal_hook::flag;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    let root = std::env::current_dir()?;
    let config_path = root.join("supervisor.toml");
    if !config_path.exists() {
        write_default_config(&config_path)?;
        eprintln!(
            "[supervisor] wrote default config at {}",
            config_path.display()
        );
    }
    let config = load_config(&config_path)?;

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
    manager: &mut ProcessManager,
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
        if build_crate(crate_name).is_err() {
            eprintln!("[supervisor] build failed for {}, keeping running process", crate_name);
        }
    }

    for procs in process_map.values() {
        for proc_cfg in procs {
            let proc_name = proc_cfg
                .crate_name
                .clone()
                .unwrap_or_else(|| proc_cfg.name.clone());
            let should_restart = to_build.contains(&proc_name)
                || proc_cfg.depends_on.iter().any(|dep| affected.contains(dep));
            if !should_restart {
                continue;
            }
            let log_root = proc_cfg
                .log_root
                .as_ref()
                .map(|p| Path::new(p))
                .or_else(|| {
                    if matches!(proc_cfg.restart, RestartStrategy::Drain) {
                        Some(Path::new("/workspace/ai_sandbox/canon/agent_logs/capability"))
                    } else {
                        None
                    }
                });
            manager.restart(proc_cfg, log_root)?;
        }
    }
    Ok(())
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
