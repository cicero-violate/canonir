use canon_capability::{Capability, CapabilityContext, CapabilityResult};
use canon_event_log::info;
use canon_types::RuntimeEvent;

pub struct DependencyCyclesCapability;

impl Capability for DependencyCyclesCapability {
    fn name(&self) -> &'static str {
        "analysis.dependency_cycles"
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let RuntimeEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        info(
            "analysis_capability",
            "dependency_cycles",
            serde_json::json!({ "args": request.args }),
        );
        let outcome = crate::capabilities::runner::run_full_analysis(&request.args)?;
        let (status, crate_root) = match outcome {
            crate::capabilities::runner::RunOutcome::Ran(root) => ("complete", root),
            crate::capabilities::runner::RunOutcome::Skipped(root) => ("skipped", root),
        };
        crate::capabilities::events::emit_analysis_event(
            &crate::capabilities::events::resolve_tlog_path(),
            "analysis.dependency_cycles",
            serde_json::json!({ "status": status, "crate_root": crate_root.display().to_string() }),
        )?;
        Ok(CapabilityResult::NoOp)
    }
}
