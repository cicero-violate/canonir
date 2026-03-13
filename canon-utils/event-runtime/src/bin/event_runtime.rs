use anyhow::{anyhow, Result};
use canon_event_consumers::build_consumers;
use canon_event_runtime::EventRuntime;
use canon_tlog_replay::detect_tlog_format;
use canon_event_log::{info, warn, error};
use canon_agent_v2::runtime_capabilities::register_capabilities;
use canon_editor::register_editor_capabilities;
use canon_analysis::register_analysis_capabilities;
use canon_build_runtime::register_build_capabilities;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

struct LockGuard {
    path: PathBuf,
    _file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn parse_pid(lock_contents: &str) -> Option<u32> {
    lock_contents
        .lines()
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|value| value.trim().parse::<u32>().ok())
}

fn pid_is_alive(pid: u32) -> Result<bool> {
    let stat_path = PathBuf::from("/proc").join(pid.to_string()).join("stat");
    let mut file = match File::open(&stat_path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let close_paren = match contents.rfind(')') {
        Some(idx) => idx,
        None => return Ok(true),
    };
    let state = contents[close_paren + 1..]
        .trim_start()
        .chars()
        .next()
        .unwrap_or(' ');
    Ok(state != 'Z')
}

fn acquire_lock(path: &Path) -> Result<Option<LockGuard>> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            let _ = file.write_all(format!("pid={}\n", std::process::id()).as_bytes());
            return Ok(Some(LockGuard {
                path: path.to_path_buf(),
                _file: file,
            }));
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err.into()),
    }

    let mut contents = String::new();
    if let Ok(mut file) = File::open(path) {
        let _ = file.read_to_string(&mut contents);
    }
    let Some(pid) = parse_pid(&contents) else {
        eprintln!(
            "[event_runtime] another instance is running (lock: {})",
            path.display()
        );
        return Ok(None);
    };
    let alive = pid_is_alive(pid)?;
    if alive {
        eprintln!(
            "[event_runtime] another instance is running (lock: {})",
            path.display()
        );
        return Ok(None);
    }

    let _ = fs::remove_file(path);
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let _ = file.write_all(format!("pid={}\n", std::process::id()).as_bytes());
    Ok(Some(LockGuard {
        path: path.to_path_buf(),
        _file: file,
    }))
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let mut tlog_path: Option<PathBuf> = None;
    let mut poll_ms: u64 = 500;
    let mut once = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--tlog" => {
                i += 1;
                tlog_path = args.get(i).map(PathBuf::from);
            }
            "--poll-ms" => {
                i += 1;
                if let Some(val) = args.get(i) {
                    poll_ms = val.parse().unwrap_or(poll_ms);
                }
            }
            "--once" => {
                once = true;
            }
            _ => {}
        }
        i += 1;
    }

    let tlog_path = tlog_path.ok_or_else(|| anyhow!("missing --tlog"))?;
    let lock_path = env::var("CANON_EVENT_RUNTIME_LOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/state/event_runtime.lock"));
    let _lock_guard = match acquire_lock(&lock_path)? {
        Some(guard) => guard,
        None => return Ok(()),
    };
    let mut runtime = EventRuntime::new(build_consumers());
    register_capabilities(runtime.registry_mut());
    register_editor_capabilities(runtime.registry_mut());
    register_analysis_capabilities(runtime.registry_mut());
    register_build_capabilities(runtime.registry_mut());
    runtime.set_tlog_path(tlog_path.clone());
    let mut processed: usize = 0;
    info(
        "event_runtime",
        "runtime_start",
        serde_json::json!({ "tlog": tlog_path.display().to_string(), "once": once }),
    );

    loop {
        if !tlog_path.exists() {
            if once {
                error(
                    "event_runtime",
                    "tlog_missing",
                    serde_json::json!({ "tlog": tlog_path.display().to_string() }),
                );
                return Err(anyhow!("tlog not found: {}", tlog_path.display()));
            }
            sleep(Duration::from_millis(poll_ms));
            continue;
        }

        let _format = detect_tlog_format(&tlog_path);

        let events = canon_tlog_replay::read_any_events_from_path(&tlog_path)?;
        if events.len() < processed {
            warn(
                "event_runtime",
                "event_count_reset",
                serde_json::json!({ "prev": processed, "next": events.len() }),
            );
            runtime.reset();
            processed = 0;
        }

        if processed < events.len() {
            runtime.process_events(&events[processed..])?;
        }
        processed = events.len();

        if once {
            break;
        }

        sleep(Duration::from_millis(poll_ms));
    }

    Ok(())
}
