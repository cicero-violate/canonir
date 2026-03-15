use canon_capability::{Capability, CapabilityContext, CapabilityResult};
use canon_event_log::info;
use canon_types::RuntimeEvent;

pub struct AnalysisRunCapability;

impl Capability for AnalysisRunCapability {
    fn name(&self) -> &'static str {
        "analysis.run"
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let RuntimeEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        info(
            "analysis_capability",
            "analysis_run",
            serde_json::json!({ "args": request.args }),
        );
        let outcome = crate::capabilities::runner::run_full_analysis(&request.args)?;
        let (status, crate_root) = match outcome {
            crate::capabilities::runner::RunOutcome::Ran(root) => ("complete", root),
            crate::capabilities::runner::RunOutcome::Skipped(root) => ("skipped", root),
        };
        crate::capabilities::events::emit_analysis_event(
            &canon_event_emit::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG")),
            "analysis.completed",
            serde_json::json!({
                "crate": request
                    .args
                    .get("crate")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
                "status": status,
                "crate_root": crate_root.display().to_string(),
                "batch_id": request
                    .args
                    .get("batch_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            }),
        )?;
        Ok(CapabilityResult::NoOp)
    }
}
