use canon_event_log::info;
use canon_types::KernelEvent;
use std::path::Path;

const ANALYSIS_CAPS: &[&str] = &["analysis.run"];

pub fn dispatch_for_event(
    event: &KernelEvent,
    workspace: &Path,
    _tlog_path: &Path,
) -> anyhow::Result<String> {
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
        info("canon-analysis", "capability_requested", payload);
    }
    info(
        "analysis_dispatcher",
        "capabilities_requested",
        serde_json::json!({ "crate": crate_name, "count": ANALYSIS_CAPS.len() }),
    );
    Ok(batch_id)
}
