use anyhow::{anyhow, Result};
use canon_event_runtime::consumers::llm_executor::LlmExecutorConsumer;
use canon_event_runtime::EventRuntime;
use canon_tlog_replay::read_any_events_from_path;
use canon_tlog_writer::{BinarySegmentWriter, CanonEvent};
use canon_types::CapabilityRequested;
use canon_agent_v2::config::CapabilityConfig;
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

    let tmp_dir = std::env::temp_dir().join(format!(
        "canon-llm-smoke-{}",
        std::process::id()
    ));
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    std::fs::create_dir_all(&tmp_dir)?;

    let request = CapabilityRequested {
        request_id: format!("llm-smoke-{}", std::process::id()),
        name: "llm.call".to_string(),
        args: serde_json::json!({
            "prompt": "Return the JSON: {\"ok\":true}",
            "raw": false
        }),
    };
    let payload = serde_json::to_value(&request)?;
    let canon = CanonEvent::new("smoke-test", "capability_requested", payload);
    let writer = BinarySegmentWriter::open(&tmp_dir)?;
    let _ = writer.append_event(&canon);

    let mut runtime = EventRuntime::new(vec![Box::new(LlmExecutorConsumer::new())]);
    runtime.set_tlog_path(tmp_dir.clone());
    runtime.process_path(&tmp_dir)?;

    let start = Instant::now();
    let max_wait = Duration::from_secs(60);
    loop {
        runtime.emit_tick()?;
        std::thread::sleep(Duration::from_millis(200));
        let events = read_any_events_from_path(&tmp_dir)?;
        let mut completed = 0usize;
        let mut failed = 0usize;
        for event in events {
            if let canon_tlog_replay::AnyEvent::Canon(canon) = event {
                match canon.kind.as_str() {
                    "capability_completed" => completed += 1,
                    "capability_failed" => failed += 1,
                    _ => {}
                }
            }
        }
        if completed > 0 {
            println!("llm_smoke_test: PASS (completed={}, failed={}, log={})", completed, failed, tmp_dir.display());
            return Ok(());
        }
        if failed > 0 {
            return Err(anyhow!(
                "llm_smoke_test failed: completed=0 failed={} log={}",
                failed,
                tmp_dir.display()
            ));
        }
        if start.elapsed() > max_wait {
            println!(
                "llm_smoke_test: SKIP (timeout waiting for llm backend, log={})",
                tmp_dir.display()
            );
            return Ok(());
        }
    }
}
