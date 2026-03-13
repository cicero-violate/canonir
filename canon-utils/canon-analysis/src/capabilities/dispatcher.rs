use canon_event_log::info;
use canon_tlog_writer::{append_event_json, BinarySegmentWriter, CanonEvent};
use canon_types::KernelEvent;
use std::path::Path;

const ANALYSIS_CAPS: &[&str] = &["analysis.run"];

pub fn dispatch_for_event(event: &KernelEvent, workspace: &Path, tlog_path: &Path) -> anyhow::Result<String> {
    let KernelEvent::CompilationUnitFinished { crate_name } = event else {
        return Ok(String::new());
    };
    let batch_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string();
    for cap in ANALYSIS_CAPS {
        let payload = serde_json::json!({
            "request_id": format!("analysis-{}-{}", crate_name, cap),
            "name": cap,
            "args": {
                "crate": crate_name,
                "batch_id": batch_id,
                "workspace": workspace.display().to_string(),
                "reports_root": workspace.join("state").join("reports_out").display().to_string()
            }
        });
        let canon = CanonEvent::new("canon-analysis", "capability_requested", payload);
        append_canon_event(tlog_path, &canon)?;
    }
    info(
        "analysis_dispatcher",
        "capabilities_requested",
        serde_json::json!({ "crate": crate_name, "count": ANALYSIS_CAPS.len() }),
    );
    Ok(batch_id)
}

fn append_canon_event(tlog_path: &Path, canon: &CanonEvent) -> anyhow::Result<()> {
    if tlog_path.is_dir() {
        let writer = BinarySegmentWriter::open(tlog_path)?;
        let _ = writer.append_event(canon);
        return Ok(());
    }
    append_event_json(tlog_path, &canon.source, &canon.kind, canon.payload.clone())?;
    Ok(())
}
