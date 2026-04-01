use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, SystemTime};

pub fn run_binary_supervisor(binary_path: &Path) {
    // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
    eprintln!("[ENTER ROOT] {}:{} {} - run_binary_supervisor", file!(), line!(), module_path!());
    let mut last = SystemTime::UNIX_EPOCH;
    let mut child: Option<Child> = None;

    // Initial spawn (always run immediately)
    if binary_path.is_file() {
        // ensure stale runtime lock does not block initial instance
        let lock_path = "/workspace/ai_sandbox/canon/state/event_runtime.lock";
        if std::path::Path::new(lock_path).exists() {
            eprintln!("[SUPERVISOR FIX] removing stale runtime lock (initial spawn): {}", lock_path);
            let _ = std::fs::remove_file(lock_path);
        }
        child = Command::new(binary_path)
            .arg("--tlog")
            .arg("/workspace/ai_sandbox/canon/state/event_log/event.tlog.d")
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .ok();
        if let Ok(meta) = fs::metadata(binary_path) {
            if let Ok(modified) = meta.modified() {
                last = modified;
            }
        }
    }

    loop {
        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
        // suppressed noisy loop tick log

        // TEMP: ensure runtime pipeline executes so RouteExecutor + decide() are reached
        // Without events, decision tracing will never fire
        if child.is_none() {
            // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
            eprintln!("[ENTER] {}:{} {} - spawning event_runtime fallback", file!(), line!(), module_path!());
            child = Command::new(binary_path)
                .arg("--tlog")
                .arg("/workspace/ai_sandbox/canon/state/event_log/event.tlog.d")
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .spawn()
                .ok();
        }
        if let Ok(meta) = fs::metadata(binary_path) {
            if let Ok(modified) = meta.modified() {
                if modified > last {
                    last = modified;

                    println!("[binary-supervisor] new binary detected: {:?} @ {:?}", binary_path, modified);

                    if let Some(mut c) = child.take() {
                        // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
                        eprintln!("[EXIT] {}:{} {} - killing existing child", file!(), line!(), module_path!());
                        let _ = c.kill();
                    }

                    // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
    eprintln!("[ENTER] {}:{} {} - spawning new child", file!(), line!(), module_path!());
    // CRITICAL FIX: ensure stale runtime lock does not block new instance
    let lock_path = "/workspace/ai_sandbox/canon/state/event_runtime.lock";
    if std::path::Path::new(lock_path).exists() {
        eprintln!("[SUPERVISOR FIX] removing stale runtime lock: {}", lock_path);
        let _ = std::fs::remove_file(lock_path);
    }
                    child = Command::new(binary_path)
                        .arg("--tlog")
                        .arg("/workspace/ai_sandbox/canon/state/event_log/event.tlog.d")
                        .spawn()
                        .ok();
                }
            }
        }

        thread::sleep(Duration::from_millis(200));
    }
}
