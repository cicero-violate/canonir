use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, SystemTime};

pub fn run_binary_supervisor(binary_path: &Path) {
    #[cfg(feature = "trace")]
    eprintln!("[TRACE] {}:{} {} - entering run_binary_supervisor", file!(), line!(), module_path!());
    let mut last = SystemTime::UNIX_EPOCH;
    let mut child: Option<Child> = None;

    // Initial spawn (always run immediately)
    if binary_path.is_file() {
        child = Command::new("cargo").args(["run", "--features", "trace", "--bin", "canon-runtime", "--", "--tlog", "/workspace/ai_sandbox/canon/state/event_log/event.tlog.d"]).spawn().ok();
        if let Ok(meta) = fs::metadata(binary_path) {
            if let Ok(modified) = meta.modified() {
                last = modified;
            }
        }
    }

    loop {
        #[cfg(feature = "trace")]
        eprintln!("[TRACE] {}:{} {} - supervisor loop tick", file!(), line!(), module_path!());

        // TEMP: ensure runtime pipeline executes so RouteExecutor + decide() are reached
        // Without events, decision tracing will never fire
        if child.is_none() {
            #[cfg(feature = "trace")]
            eprintln!("[TRACE] {}:{} {} - spawning event_runtime fallback", file!(), line!(), module_path!());
            child = Command::new("cargo").args(["run", "--bin", "canon-runtime", "--", "--tlog", "/workspace/ai_sandbox/canon/state/event_log/event.tlog.d"]).spawn().ok();
        }
        if let Ok(meta) = fs::metadata(binary_path) {
            if let Ok(modified) = meta.modified() {
                if modified > last {
                    last = modified;

                    println!("[binary-supervisor] new binary detected: {:?} @ {:?}", binary_path, modified);

                    if let Some(mut c) = child.take() {
                        #[cfg(feature = "trace")]
                        eprintln!("[TRACE] {}:{} {} - killing existing child", file!(), line!(), module_path!());
                        let _ = c.kill();
                    }

                    #[cfg(feature = "trace")]
                    eprintln!("[TRACE] {}:{} {} - spawning new child", file!(), line!(), module_path!());
                    child =
                        Command::new("cargo").args(["run", "--features", "trace", "--bin", "canon-runtime", "--", "--tlog", "/workspace/ai_sandbox/canon/state/event_log/event.tlog.d"]).spawn().ok();
                }
            }
        }

        thread::sleep(Duration::from_millis(200));
    }
}
