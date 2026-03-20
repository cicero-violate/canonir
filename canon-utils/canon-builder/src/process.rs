use crate::config::ProcessConfig;
use crate::events::wrap_event;
use anyhow::Result;
use canon_event::{canon_emit, resolve_tlog_path};
use std::collections::HashMap;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

pub struct ProcessManager {
    children: HashMap<String, Child>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self { children: HashMap::new() }
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
            let _ = canon_emit!("canon-supervisor", "supervisor_event", payload, &tlog_path);
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
        let _ = canon_emit!("canon-supervisor", "supervisor_event", payload, &tlog_path);
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
            let _ = canon_emit!("canon-supervisor", "supervisor_event", payload, &tlog_path);
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
            let _ = canon_emit!("canon-supervisor", "supervisor_event", payload, &tlog_path);
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
