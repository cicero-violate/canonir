use canon_capability::{CapabilityHandler, CapabilityExecutionContext, CapabilityExecutionResult};
use canon_event::CanonEvent;

pub struct AnalysisRunCapability;

impl CapabilityHandler for AnalysisRunCapability {
    fn name(&self) -> &'static str {
        "analysis.run"
    }

    fn execute(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let outcome = crate::capabilities::runner::run_full_analysis(&request.args)?;
        let (status, crate_root) = match outcome {
            crate::capabilities::runner::RunOutcome::Ran(root) => ("complete", root),
            crate::capabilities::runner::RunOutcome::Skipped(root) => ("skipped", root),
        };
        crate::capabilities::events::emit_analysis_event(
            &canon_event::resolve_tlog_path(None, Some("CANON_REPORTS_TLOG")),
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
        Ok(CapabilityExecutionResult::NoOp)
    }
}
