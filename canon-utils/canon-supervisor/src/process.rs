use crate::config::{ProcessConfig, RestartStrategy};
use crate::events::wrap_event;
use anyhow::Result;
use canon_event_emit::{emit_event, resolve_tlog_path};
use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

pub struct ProcessManager {
    children: HashMap<String, Child>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            children: HashMap::new(),
        }
    }

    pub fn spawn(&mut self, cfg: &ProcessConfig, resume: bool) -> Result<()> {
        if self.children.contains_key(&cfg.name) {
            let payload = wrap_event(
                "process_spawn_skipped",
                serde_json::json!({
                    "name": cfg.name,
                    "reason": "already_running",
                }),
            );
            let tlog_path = resolve_tlog_path(None, None);
            let _ = emit_event("canon-supervisor", "supervisor_event", payload, &tlog_path);
            return Ok(());
        }
        let mut cmd = Command::new(&cfg.bin);
        cmd.args(&cfg.args);
        for (key, value) in &cfg.env {
            cmd.env(key, value);
        }
        if resume && !cfg.args.iter().any(|arg| arg == "--resume") {
            cmd.arg("--resume");
        }
        let child = cmd.spawn()?;
        self.children.insert(cfg.name.clone(), child);
        let payload = wrap_event(
            "process_spawned",
            serde_json::json!({
                "name": cfg.name,
                "bin": cfg.bin,
                "args": cfg.args,
                "resume": resume,
            }),
        );
        let tlog_path = resolve_tlog_path(None, None);
        let _ = emit_event("canon-supervisor", "supervisor_event", payload, &tlog_path);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn restart(&mut self, cfg: &ProcessConfig, log_root: Option<&Path>) -> Result<()> {
        let resume = matches!(cfg.restart, RestartStrategy::Drain);
        if let Some(mut child) = self.children.remove(&cfg.name) {
            let payload = wrap_event(
                "process_restarted",
                serde_json::json!({
                    "name": cfg.name,
                    "strategy": format!("{:?}", cfg.restart),
                }),
            );
            let tlog_path = resolve_tlog_path(None, None);
            let _ = emit_event("canon-supervisor", "supervisor_event", payload, &tlog_path);
            match cfg.restart {
                RestartStrategy::Kill => {
                    terminate_child(&mut child, &cfg.name, cfg.drain_timeout_ms)?;
                }
                RestartStrategy::Drain => {
                    if let Some(root) = log_root {
                        write_recovery_signal(root);
                    }
                    if !wait_for_exit(&mut child, &cfg.name, cfg.drain_timeout_ms) {
                        let _ = child.kill();
                    }
                }
            }
        }
        self.spawn(cfg, resume)?;
        Ok(())
    }

    pub fn shutdown_all(&mut self, timeout_ms: u64) {
        for (name, mut child) in self.children.drain() {
            let payload = wrap_event(
                "process_exit",
                serde_json::json!({
                    "name": name,
                    "reason": "shutdown",
                }),
            );
            let tlog_path = resolve_tlog_path(None, None);
            let _ = emit_event("canon-supervisor", "supervisor_event", payload, &tlog_path);
            let _ = terminate_child(&mut child, &name, timeout_ms);
        }
    }
}

fn terminate_child(child: &mut Child, name: &str, timeout_ms: u64) -> Result<()> {
    send_sigterm(child);
    if !wait_for_exit(child, name, timeout_ms) {
        let _ = child.kill();
    }
    Ok(())
}

fn wait_for_exit(child: &mut Child, name: &str, timeout_ms: u64) -> bool {
    let start = Instant::now();
    loop {
        if let Ok(Some(_status)) = child.try_wait() {
            let payload = wrap_event(
                "process_exit",
                serde_json::json!({
                    "name": name,
                    "reason": "exit",
                }),
            );
            let tlog_path = resolve_tlog_path(None, None);
            let _ = emit_event("canon-supervisor", "supervisor_event", payload, &tlog_path);
            return true;
        }
        if start.elapsed() >= Duration::from_millis(timeout_ms) {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn send_sigterm(child: &Child) {
    let pid = child.id() as i32;
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
}

#[allow(dead_code)]
fn write_recovery_signal(log_root: &Path) {
    let payload = serde_json::json!({ "reason": "supervisor_restart" });
    let path = log_root.join("recovery_signal.json");
    if let Ok(text) = serde_json::to_string_pretty(&payload) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }
}
