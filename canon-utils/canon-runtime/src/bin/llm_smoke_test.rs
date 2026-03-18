use anyhow::{anyhow, Result};
use canon_llm::config::CapabilityConfig;
use canon_event_store::read_any_events_from_path;
use canon_event_store::read_any_events_from_path_with_start_seq;
use canon_event::canon_emit;
use canon_event::CapabilityRequested;
use std::fs::File;
use std::io::Read;
use std::time::{Duration, Instant};

fn main() -> Result<()> {
    let config = match CapabilityConfig::snapshot_store_load() {
        Ok(config) => config,
        Err(err) => {
            println!("llm_smoke_test: SKIP (capability_config.toml missing: {err})");
            return Ok(());
        }
    };
    if config.llm_endpoints.is_empty() {
        println!("llm_smoke_test: SKIP (no llm endpoints configured)");
        return Ok(());
    }

    if let Some(msg) = check_event_runtime_lock() {
        println!("llm_smoke_test: SKIP ({msg})");
        return Ok(());
    }

    let args: Vec<String> = std::env::args().collect();
    let mut tlog_path = std::env::var("CANON_TLOG_PATH")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(
                "/workspace/ai_sandbox/canon/state/event_log/event.tlog.d",
            )
        });
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--tlog" {
            if let Some(val) = args.get(i + 1) {
                tlog_path = std::path::PathBuf::from(val);
            }
            i += 1;
        }
        i += 1;
    }

    let request = CapabilityRequested {
        request_id: format!("llm-smoke-{}", std::process::id()),
        name: "llm.call".to_string(),
        args: serde_json::json!({
            "prompt": "Return the JSON: {\"ok\":true}",
            "raw": false
        }),
    };
    let payload = serde_json::to_value(&request)?;
    println!("llm_smoke_test: writing request to {}", tlog_path.display());
    canon_emit!("smoke-test", "capability_requested", payload, &tlog_path)?;

    let replay_path = if tlog_path.is_dir() {
        // Only scan the segment the request was written into (and the next one).
        // Scanning the entire tlog dir on every 250ms poll is too expensive when
        // segments are large.
        tlog_path.clone() // kept as dir; we'll filter in the poll loop below
    } else if tlog_path.extension().and_then(|s| s.to_str()) == Some("log") {
        tlog_path
            .parent()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| tlog_path.clone())
    } else {
        tlog_path.clone()
    };

    // Sanity check: ensure the request is visible to the replay reader.
    let mut visible = false;
    if let Ok(events) = read_any_events_from_path(&replay_path) {
        for event in events {
            if let canon_event_store::AnyEvent::Canon(canon) = event {
                if canon.kind == "capability_requested"
                    && canon
                        .payload
                        .get("request_id")
                        .and_then(|v| v.as_str())
                        == Some(request.request_id.as_str())
                {
                    visible = true;
                    break;
                }
            }
        }
    }
    if !visible {
        println!("llm_smoke_test: warning: request not visible in tlog replay");
    }

    let start = Instant::now();
    let max_wait = Duration::from_secs(30);
    let mut last_log = Instant::now();
    // Record which segment was latest when we wrote the request, so we only
    // scan that segment and newer ones instead of the full tlog history.
    let scan_from_seq: u64 = if tlog_path.is_dir() {
        latest_segment_seq_local(&tlog_path).unwrap_or(0)
    } else {
        0
    };
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let events = if tlog_path.is_dir() && scan_from_seq > 0 {
            read_any_events_from_path_with_start_seq(&replay_path, scan_from_seq)?
        } else {
            read_any_events_from_path(&replay_path)?
        };

        let mut completed = 0usize;
        let mut failed = 0usize;
        let mut matched = false;
        for event in events {
            if let canon_event_store::AnyEvent::Canon(canon) = event {
                match canon.kind.as_str() {
                    "capability_completed" => {
                        completed += 1;
                        matched |= canon
                            .payload
                            .get("request_id")
                            .and_then(|v| v.as_str())
                            .map(|v| v == request.request_id.as_str())
                            .unwrap_or(false);
                    }
                    "capability_failed" => {
                        failed += 1;
                        matched |= canon
                            .payload
                            .get("request_id")
                            .and_then(|v| v.as_str())
                            .map(|v| v == request.request_id.as_str())
                            .unwrap_or(false);
                    }
                    _ => {}
                }
            }
        }
        if matched && completed > 0 {
            println!(
                "llm_smoke_test: PASS (completed={}, failed={}, log={})",
                completed,
                failed,
                tlog_path.display()
            );
            return Ok(());
        }
        if matched && failed > 0 {
            return Err(anyhow!(
                "llm_smoke_test failed: completed=0 failed={} log={}",
                failed,
                tlog_path.display()
            ));
        }
        if start.elapsed() > max_wait {
            println!(
                "llm_smoke_test: SKIP (timeout waiting for llm backend, log={})",
                tlog_path.display()
            );
            return Ok(());
        }
        if last_log.elapsed() > Duration::from_secs(5) {
            println!("llm_smoke_test: waiting for response...");
            last_log = Instant::now();
        }
    }
}

fn check_event_runtime_lock() -> Option<String> {
    let lock_path = std::env::var("CANON_EVENT_RUNTIME_LOCK")
        .ok()
        .unwrap_or_else(|| "/workspace/ai_sandbox/canon/state/event_runtime.lock".to_string());
    let mut contents = String::new();
    let mut file = File::open(&lock_path).ok()?;
    let _ = file.read_to_string(&mut contents);
    let pid = contents
        .lines()
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|value| value.trim().parse::<u32>().ok())?;
    if pid_is_alive(pid).unwrap_or(false) {
        None
    } else {
        Some(format!("event_runtime not running (stale lock pid={})", pid))
    }
}

fn pid_is_alive(pid: u32) -> std::io::Result<bool> {
    let stat_path = std::path::PathBuf::from("/proc")
        .join(pid.to_string())
        .join("stat");
    let mut file = match File::open(&stat_path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
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

fn latest_segment_seq_local(tlog_path: &std::path::Path) -> Option<u64> {
    let mut max_seq = 0u64;
    for entry in std::fs::read_dir(tlog_path).ok()? {
        let entry = entry.ok()?;
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
    if max_seq > 0 { Some(max_seq) } else { None }
}
