use anyhow::{anyhow, Result};
use canon_event_consumers::build_consumers;
use canon_event_runtime::EventRuntime;
use canon_tlog_replay::detect_tlog_format;
use canon_event_log::{info, warn, error};
use std::env;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

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
    let mut runtime = EventRuntime::new(build_consumers());
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

        for event in events.iter().skip(processed) {
            if let canon_tlog_replay::AnyEvent::Canon(canon) = event {
                if let Some(kernel) = canon_tlog_replay::extract_kernel_event(canon) {
                    runtime.handle_kernel_event(kernel)?;
                }
            }
        }
        processed = events.len();

        if once {
            break;
        }

        sleep(Duration::from_millis(poll_ms));
    }

    Ok(())
}
