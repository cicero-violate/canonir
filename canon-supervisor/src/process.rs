use crate::config::{ProcessConfig, RestartStrategy};
use anyhow::Result;
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
        let mut cmd = Command::new(&cfg.bin);
        cmd.args(&cfg.args);
        if resume && !cfg.args.iter().any(|arg| arg == "--resume") {
            cmd.arg("--resume");
        }
        let child = cmd.spawn()?;
        self.children.insert(cfg.name.clone(), child);
        Ok(())
    }

    pub fn restart(&mut self, cfg: &ProcessConfig, log_root: Option<&Path>) -> Result<()> {
        let resume = matches!(cfg.restart, RestartStrategy::Drain);
        if let Some(mut child) = self.children.remove(&cfg.name) {
            match cfg.restart {
                RestartStrategy::Kill => {
                    terminate_child(&mut child, cfg.drain_timeout_ms)?;
                }
                RestartStrategy::Drain => {
                    if let Some(root) = log_root {
                        write_recovery_signal(root);
                    }
                    if !wait_for_exit(&mut child, cfg.drain_timeout_ms) {
                        let _ = child.kill();
                    }
                }
            }
        }
        self.spawn(cfg, resume)?;
        Ok(())
    }

    pub fn shutdown_all(&mut self, timeout_ms: u64) {
        for (_name, mut child) in self.children.drain() {
            let _ = terminate_child(&mut child, timeout_ms);
        }
    }
}

fn terminate_child(child: &mut Child, timeout_ms: u64) -> Result<()> {
    send_sigterm(child);
    if !wait_for_exit(child, timeout_ms) {
        let _ = child.kill();
    }
    Ok(())
}

fn wait_for_exit(child: &mut Child, timeout_ms: u64) -> bool {
    let start = Instant::now();
    loop {
        if let Ok(Some(_status)) = child.try_wait() {
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
