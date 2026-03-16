use anyhow::{anyhow, Result};
use canon_supervisor::register_build_capabilities;
use canon_kernel::EventRuntime;
use canon_event_store::read_any_events_from_path;
use canon_event::{BinarySegmentWriter, CanonEvent};
use canon_event::CapabilityRequested;

fn main() -> Result<()> {
    let tmp_dir = std::env::temp_dir().join(format!("canon-capability-smoke-{}", std::process::id()));
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    std::fs::create_dir_all(&tmp_dir)?;

    let request = CapabilityRequested {
        request_id: format!("smoke-{}", std::process::id()),
        name: "file.read".to_string(),
        args: serde_json::json!({
            "path": "/workspace/ai_sandbox/canon/canon-utils/README.md"
        }),
    };
    let payload = serde_json::to_value(&request)?;
    let canon = CanonEvent::new("smoke-test", "capability_requested", payload);
    let writer = BinarySegmentWriter::open(&tmp_dir)?;
    let _ = writer.write_event(&canon);

    let mut runtime = EventRuntime::new(Vec::new());
    register_build_capabilities(&mut runtime.registry_mut());
    runtime.set_tlog_path(tmp_dir.clone());
    runtime.process_path(&tmp_dir)?;

    let events = read_any_events_from_path(&tmp_dir)?;
    let mut completed = 0usize;
    let mut failed = 0usize;
    for event in events {
        if let canon_event_store::AnyEvent::Canon(canon) = event {
            match canon.kind.as_str() {
                "capability_completed" => completed += 1,
                "capability_failed" => failed += 1,
                _ => {}
            }
        }
    }

    if completed == 0 {
        return Err(anyhow!(
            "capability_smoke_test failed: completed=0 failed={failed} log={}",
            tmp_dir.display()
        ));
    }

    println!(
        "capability_smoke_test: PASS (completed={}, failed={}, log={})",
        completed,
        failed,
        tmp_dir.display()
    );
    Ok(())
}
