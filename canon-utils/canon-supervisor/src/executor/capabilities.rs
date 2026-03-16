use crate::executor::{run_cargo_build, run_cargo_check, run_cargo_run, BuildRequest, CheckRequest, RunRequest};
use canon_capability::{Capability, CapabilityContext, CapabilityRegistry, CapabilityResult};
use canon_event::emit_debug::info;
use canon_event::{CapabilityCompleted, CapabilityFailed, CapabilityRequested, RuntimeEvent};
use serde_json::json;
use std::process::Command;

pub const CAP_BUILD_CARGO: &str = "cargo.build";
pub const CAP_RUN_CARGO: &str = "cargo.run";
pub const CAP_CHECK_CARGO: &str = "cargo.check";

pub fn register_build_capabilities(registry: &mut CapabilityRegistry) {
    registry.register(std::sync::Arc::new(BuildCargoCapability));
    registry.register(std::sync::Arc::new(CargoRunCapability));
    registry.register(std::sync::Arc::new(CargoCheckCapability));
    registry.register(std::sync::Arc::new(FileReadCapability));
    registry.register(std::sync::Arc::new(FileWriteCapability));
    registry.register(std::sync::Arc::new(BashCapability));
    registry.register(std::sync::Arc::new(LlmCallCapability));
}

fn request_from_ctx(ctx: &CapabilityContext) -> anyhow::Result<CapabilityRequested> {
    if let RuntimeEvent::CapabilityRequested(req) = &ctx.event {
        return Ok(req.clone());
    }
    anyhow::bail!("capability context missing request");
}

fn emit_completed(req: &CapabilityRequested, result: serde_json::Value) -> CapabilityResult {
    CapabilityResult::Emit(RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
        request_id: req.request_id.clone(),
        name: req.name.clone(),
        result,
    }))
}

fn emit_failed(req: &CapabilityRequested, error: &str) -> CapabilityResult {
    CapabilityResult::Emit(RuntimeEvent::CapabilityFailed(CapabilityFailed {
        request_id: req.request_id.clone(),
        name: req.name.clone(),
        error: error.to_string(),
    }))
}

fn runtime_log_event(kind: &str, payload: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::RuntimeStateUpdated {
        payload: json!({ "kind": kind, "payload": payload }),
    }
}

fn emit_completed_with_events(
    req: &CapabilityRequested,
    result: serde_json::Value,
    mut events: Vec<RuntimeEvent>,
) -> CapabilityResult {
    events.push(RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
        request_id: req.request_id.clone(),
        name: req.name.clone(),
        result,
    }));
    CapabilityResult::EmitMany(events)
}

fn result_payload(status: i32, success: bool, stdout: String, stderr: String) -> serde_json::Value {
    json!({
        "status": status,
        "success": success,
        "stdout": stdout,
        "stderr": stderr,
    })
}

struct BuildCargoCapability;

impl Capability for BuildCargoCapability {
    fn name(&self) -> &'static str {
        CAP_BUILD_CARGO
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        let crate_name = req
            .args
            .get("crate")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing crate arg"))?;

        info(
            "build_capability",
            "build_started",
            serde_json::json!({ "crate": crate_name }),
        );
        let mut events = vec![runtime_log_event("build.started", json!({ "crate": crate_name }))];

        let result = run_cargo_build(&BuildRequest {
            crate_name: crate_name.to_string(),
        })?;

        events.push(runtime_log_event(
            "build.completed",
            json!({
                "crate": result.crate_name,
                "success": result.success,
                "duration_ms": result.duration_ms
            }),
        ));

        Ok(emit_completed_with_events(
            &req,
            result_payload(result.status, result.success, result.stdout, result.stderr),
            events,
        ))
    }
}

struct CargoRunCapability;

impl Capability for CargoRunCapability {
    fn name(&self) -> &'static str {
        CAP_RUN_CARGO
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        let crate_name = req
            .args
            .get("crate")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing crate arg"))?;
        let bin = req
            .args
            .get("bin")
            .and_then(|v| v.as_str())
            .map(|value| value.to_string());
        let args = match req.args.get("args") {
            Some(value) => value
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("args must be an array"))?
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .map(|value| value.to_string())
                        .ok_or_else(|| anyhow::anyhow!("args must be an array of strings"))
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
            None => Vec::new(),
        };

        info(
            "build_capability",
            "run_started",
            serde_json::json!({ "crate": crate_name, "bin": bin }),
        );
        let mut events = vec![runtime_log_event(
            "run.started",
            json!({ "crate": crate_name, "bin": bin }),
        )];

        let result = run_cargo_run(&RunRequest {
            crate_name: crate_name.to_string(),
            bin: bin.clone(),
            args,
        })?;

        events.push(runtime_log_event(
            "run.completed",
            json!({
                "crate": result.crate_name,
                "bin": bin,
                "success": result.success,
                "duration_ms": result.duration_ms
            }),
        ));

        Ok(emit_completed_with_events(
            &req,
            result_payload(result.status, result.success, result.stdout, result.stderr),
            events,
        ))
    }
}

struct CargoCheckCapability;

impl Capability for CargoCheckCapability {
    fn name(&self) -> &'static str {
        CAP_CHECK_CARGO
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        let crate_name = req
            .args
            .get("crate")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing crate arg"))?;

        info(
            "build_capability",
            "check_started",
            serde_json::json!({ "crate": crate_name }),
        );
        let mut events =
            vec![runtime_log_event("check.started", json!({ "crate": crate_name }))];

        let result = run_cargo_check(&CheckRequest {
            crate_name: crate_name.to_string(),
        })?;

        events.push(runtime_log_event(
            "check.completed",
            json!({
                "crate": result.crate_name,
                "success": result.success,
                "duration_ms": result.duration_ms
            }),
        ));

        Ok(emit_completed_with_events(
            &req,
            result_payload(result.status, result.success, result.stdout, result.stderr),
            events,
        ))
    }
}

struct FileReadCapability;

impl Capability for FileReadCapability {
    fn name(&self) -> &'static str {
        "file.read"
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        let Some(path) = req.args.get("path").and_then(|v| v.as_str()) else {
            return Ok(emit_failed(&req, "missing path"));
        };
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => return Ok(emit_failed(&req, &err.to_string())),
        };
        Ok(emit_completed(
            &req,
            result_payload(0, true, content, String::new()),
        ))
    }
}

struct FileWriteCapability;

impl Capability for FileWriteCapability {
    fn name(&self) -> &'static str {
        "file.write"
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        let Some(path) = req.args.get("path").and_then(|v| v.as_str()) else {
            return Ok(emit_failed(&req, "missing path"));
        };
        let Some(content) = req.args.get("content").and_then(|v| v.as_str()) else {
            return Ok(emit_failed(&req, "missing content"));
        };
        if let Err(err) = std::fs::write(path, content) {
            return Ok(emit_failed(&req, &err.to_string()));
        }
        Ok(emit_completed(
            &req,
            result_payload(0, true, String::new(), String::new()),
        ))
    }
}

struct BashCapability;

impl Capability for BashCapability {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        let Some(cmd) = req.args.get("cmd").and_then(|v| v.as_str()) else {
            return Ok(emit_failed(&req, "missing cmd"));
        };
        let mut command = Command::new("bash");
        command.arg("-lc").arg(cmd).current_dir(&ctx.workspace);
        let output = command.output()?;
        Ok(emit_completed(
            &req,
            result_payload(
                output.status.code().unwrap_or(-1),
                output.status.success(),
                String::from_utf8_lossy(&output.stdout).to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            ),
        ))
    }
}

struct LlmCallCapability;

impl Capability for LlmCallCapability {
    fn name(&self) -> &'static str {
        "llm.call"
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        Ok(emit_failed(&req, "llm.call not wired in runtime"))
    }
}
