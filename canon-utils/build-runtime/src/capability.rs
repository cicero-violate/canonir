use crate::{emit_build_completed, emit_build_started, run_cargo_build, BuildRequest};
use canon_capability::{Capability, CapabilityContext, CapabilityRegistry, CapabilityResult};
use canon_event_log::{error, info};
use canon_types::RuntimeEvent;

pub const CAP_BUILD_CARGO: &str = "build.cargo";

pub fn register_build_capabilities(registry: &mut CapabilityRegistry) {
    registry.register(std::sync::Arc::new(BuildCargoCapability));
}

struct BuildCargoCapability;

impl Capability for BuildCargoCapability {
    fn name(&self) -> &'static str {
        CAP_BUILD_CARGO
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let RuntimeEvent::CapabilityRequested(request) = ctx.event else {
            anyhow::bail!("capability context missing request");
        };
        let crate_name = request
            .args
            .get("crate")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing crate arg"))?;

        info(
            "build_capability",
            "build_started",
            serde_json::json!({ "crate": crate_name }),
        );
        if let Err(err) = emit_build_started(crate_name) {
            error(
                "build_capability",
                "emit_build_started_failed",
                serde_json::json!({ "crate": crate_name, "error": err.to_string() }),
            );
        }

        let result = run_cargo_build(&BuildRequest {
            crate_name: crate_name.to_string(),
        })?;

        if let Err(err) = emit_build_completed(&result.crate_name, result.success, result.duration_ms) {
            error(
                "build_capability",
                "emit_build_completed_failed",
                serde_json::json!({ "crate": crate_name, "error": err.to_string() }),
            );
        }

        Ok(CapabilityResult::NoOp)
    }
}
