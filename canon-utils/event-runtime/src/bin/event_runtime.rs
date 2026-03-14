use anyhow::{anyhow, Result};
use canon_event_runtime::consumers::agent_consumer::AgentConsumer;
use canon_event_runtime::consumers::capability_executor::CapabilityExecutor;
use canon_event_runtime::consumers::event_loop::EventLoopConsumer;
use canon_event_runtime::consumers::llm_executor::LlmExecutorConsumer;
use canon_event_runtime::EventRuntime;
use canon_tlog_replay::detect_tlog_format;
use canon_tlog_replay::read_any_events_from_path_with_start_seq;
use canon_event_log::{info, warn, error};
use canon_editor::register_editor_capabilities;
use canon_analysis::register_analysis_capabilities;
use canon_capability_runtime::register_build_capabilities;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    let start_at_tail = env::var("CANON_EVENT_RUNTIME_START_AT_TAIL")
        .ok()
        .map(|v| v != "0" && v.to_lowercase() != "false")
        .unwrap_or(false);
    let cursor_path = env::var("CANON_EVENT_RUNTIME_CURSOR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/state/event_runtime.cursor.json"));

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
    let event_execution_enabled = std::env::var("CANON_EVENT_EXECUTION")
        .ok()
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);
    let lock_path = env::var("CANON_EVENT_RUNTIME_LOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/state/event_runtime.lock"));
    let _lock_guard = match acquire_lock(&lock_path)? {
        Some(guard) => guard,
        None => return Ok(()),
    };
    let registry = std::sync::Arc::new(std::sync::Mutex::new(
        canon_capability::CapabilityRegistry::new(),
    ));
    let mut consumers: Vec<Box<dyn canon_types::RuntimeConsumer>> = vec![
        Box::new(AgentConsumer::new()),
        Box::new(CapabilityExecutor::new(
            registry.clone(),
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        )),
        Box::new(LlmExecutorConsumer::new()),
    ];
    if event_execution_enabled {
        consumers.push(Box::new(EventLoopConsumer::new()));
    }
    let mut runtime = EventRuntime::new_with_registry(consumers, registry.clone());
    {
        let mut registry = registry.lock().expect("capability registry lock");
        register_editor_capabilities(&mut registry);
        register_analysis_capabilities(&mut registry);
        register_build_capabilities(&mut registry);
    }
    runtime.set_execute_capabilities(false);
    runtime.set_tlog_path(tlog_path.clone());
    let mut start_seq: u64 = load_cursor_seq(&cursor_path, &tlog_path).unwrap_or(0);
    let mut processed: usize = load_cursor(&cursor_path, &tlog_path).unwrap_or(0);
    // If no cursor exists or start_seq is 0, jump to the latest segment to avoid
    // loading the entire tlog history into memory on first boot.
    if start_seq == 0 {
        if let Ok(latest) = latest_segment_seq(&tlog_path) {
            if latest > 0 {
                info(
                    "event_runtime",
                    "tlog_tail_start",
                    serde_json::json!({ "start_seq": latest }),
                );
                start_seq = latest;
                processed = 0;
            }
        }
    }
    let mut did_fast_forward = false;
    let mut last_len: usize = 0;
    let mut last_saved = Instant::now();
    let mut last_saved_processed = processed;
    info(
        "event_runtime",
        "runtime_start",
        serde_json::json!({ "tlog": tlog_path.display().to_string(), "once": once, "event_execution": event_execution_enabled }),
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

        // Only read from start_seq forward to avoid re-scanning the entire tlog every tick.
        let events = read_any_events_from_path_with_start_seq(&tlog_path, start_seq)?;

        if start_at_tail && !once && !did_fast_forward && processed == 0 && !events.is_empty() {
            processed = events.len();
            did_fast_forward = true;
            info(
                "event_runtime",
                "tlog_fast_forward",
                serde_json::json!({ "skipped": processed }),
            );
        }
        if events.len() != last_len {
            info(
                "event_runtime",
                "tlog_len_changed",
                serde_json::json!({ "prev": last_len, "next": events.len() }),
            );
            last_len = events.len();
        }
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


        if processed != last_saved_processed && last_saved.elapsed() >= Duration::from_secs(1) {
            if let Err(err) = save_cursor(&cursor_path, &tlog_path, processed, start_seq) {
                error(
                    "event_runtime",
                    "cursor_save_failed",
                    serde_json::json!({ "error": err.to_string() }),
                );
            } else {
                last_saved = Instant::now();
                last_saved_processed = processed;
            }
        }
        runtime.emit_tick()?;

        if once {
            break;
        }

        sleep(Duration::from_millis(poll_ms));
    }

    Ok(())
}

fn load_cursor(path: &Path, tlog_path: &Path) -> Option<usize> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let stored_path = value.get("tlog_path")?.as_str()?;
    if stored_path != tlog_path.display().to_string() {
        return None;
    }
    value.get("processed")?.as_u64().map(|v| v as usize)
}

fn load_cursor_seq(path: &Path, tlog_path: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let stored_path = value.get("tlog_path")?.as_str()?;
    if stored_path != tlog_path.display().to_string() {
        return None;
    }
    value.get("start_seq")?.as_u64()
}

fn save_cursor(path: &Path, tlog_path: &Path, processed: usize, start_seq: u64) -> Result<()> {
    let state = serde_json::json!({
        "tlog_path": tlog_path.display().to_string(),
        "processed": processed,
        "start_seq": start_seq,
        "updated_ms": now_ms(),
    });
    let tmp_path = path.with_extension("tmp");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string(&state)?;
    std::fs::write(&tmp_path, text)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

fn latest_segment_seq(tlog_path: &Path) -> Result<u64> {
    if !tlog_path.is_dir() {
        return Ok(0);
    }
    let mut max_seq = 0u64;
    for entry in std::fs::read_dir(tlog_path)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("log") {
            continue;
        }
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            if let Ok(seq) = stem.parse::<u64>() {
                if seq > max_seq {
                    max_seq = seq;
                }
            }
        }
    }
    Ok(max_seq)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
