use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, SystemTime};

pub fn run_binary_supervisor(binary_path: &Path) {
    let mut last = SystemTime::UNIX_EPOCH;
    let mut child: Option<Child> = None;

    // Initial spawn (always run immediately)
    if binary_path.is_file() {
        child = Command::new(binary_path)
            .arg("--tlog")
            .arg("/workspace/ai_sandbox/canon/state/event_log/event.tlog.d")
            .spawn()
            .ok();
        if let Ok(meta) = fs::metadata(binary_path) {
            if let Ok(modified) = meta.modified() {
                last = modified;
            }
        }
    }

    loop {
        if let Ok(meta) = fs::metadata(binary_path) {
            if let Ok(modified) = meta.modified() {
                if modified > last {
                    last = modified;

                    println!(
                        "[binary-supervisor] new binary detected: {:?} @ {:?}",
                        binary_path,
                        modified
                    );

                    if let Some(mut c) = child.take() {
                        let _ = c.kill();
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
