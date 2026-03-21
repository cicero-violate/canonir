use crate::executor::{run_cargo_build, run_cargo_check, run_cargo_run, BuildRequest, CheckRequest, RunRequest};
use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityHandler, CapabilityRegistry};
use canon_event::{CanonEvent, CapabilityCompleted, CapabilityResult, CargoEvent, FileEvent, LlmCall, ProcessResult};
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
    registry.register(std::sync::Arc::new(FilePatchCapability));
    registry.register(std::sync::Arc::new(BashCapability));
    registry.register(std::sync::Arc::new(LlmCallCapability));
}

fn emit_completed(request_id: &str, capability: &'static str, result: CapabilityResult) -> CapabilityExecutionResult {
    CapabilityExecutionResult::Emit(CanonEvent::CapabilityCompleted(CapabilityCompleted { request_id: request_id.to_string(), capability, result }))
}

fn emit_completed_with_events(request_id: &str, capability: &'static str, result: CapabilityResult, mut events: Vec<CanonEvent>) -> CapabilityExecutionResult {
    events.push(CanonEvent::CapabilityCompleted(CapabilityCompleted { request_id: request_id.to_string(), capability, result }));
    CapabilityExecutionResult::EmitMany(events)
}

fn runtime_log_event(kind: &str, payload: serde_json::Value) -> CanonEvent {
    CanonEvent::RuntimeStateUpdated(canon_event::RuntimeStateUpdated { payload: json!({ "kind": kind, "payload": payload }) })
}

struct BuildCargoCapability;
impl CapabilityHandler for BuildCargoCapability {
    fn name(&self) -> &'static str {
        CAP_BUILD_CARGO
    }
    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::Cargo(CargoEvent::Build(ev)) = ctx.event else {
            return Ok(CapabilityExecutionResult::NoOp);
        };
        let crate_name = ev.crate_name;
        let mut events = vec![runtime_log_event("build.started", json!({ "crate": crate_name }))];
        let result = run_cargo_build(&BuildRequest { crate_name: crate_name.to_string() })?;
        events.push(runtime_log_event("build.completed", json!({ "crate": result.crate_name, "success": result.success, "duration_ms": result.duration_ms })));
        Ok(emit_completed_with_events(
            &ev.request_id,
            self.name(),
            CapabilityResult::Process(ProcessResult { status: result.status, success: result.success, stdout: result.stdout, stderr: result.stderr }),
            events,
        ))
    }
}

struct CargoRunCapability;
impl CapabilityHandler for CargoRunCapability {
    fn name(&self) -> &'static str {
        CAP_RUN_CARGO
    }
    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::Cargo(CargoEvent::Run(ev)) = ctx.event else {
            return Ok(CapabilityExecutionResult::NoOp);
        };
        let crate_name = ev.crate_name;
        let bin = ev.bin.clone();
        let args = ev.args.clone();
        let mut events = vec![runtime_log_event("run.started", json!({ "crate": crate_name, "bin": bin }))];
        let result = run_cargo_run(&RunRequest { crate_name: crate_name.to_string(), bin: bin.clone(), args })?;
        events.push(runtime_log_event("run.completed", json!({ "crate": result.crate_name, "bin": bin, "success": result.success, "duration_ms": result.duration_ms })));
        Ok(emit_completed_with_events(
            &ev.request_id,
            self.name(),
            CapabilityResult::Process(ProcessResult { status: result.status, success: result.success, stdout: result.stdout, stderr: result.stderr }),
            events,
        ))
    }
}

struct CargoCheckCapability;
impl CapabilityHandler for CargoCheckCapability {
    fn name(&self) -> &'static str {
        CAP_CHECK_CARGO
    }
    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::Cargo(CargoEvent::Check(ev)) = ctx.event else {
            return Ok(CapabilityExecutionResult::NoOp);
        };
        let crate_name = ev.crate_name;
        let mut events = vec![runtime_log_event("check.started", json!({ "crate": crate_name }))];
        let result = run_cargo_check(&CheckRequest { crate_name: crate_name.to_string() })?;
        events.push(runtime_log_event("check.completed", json!({ "crate": result.crate_name, "success": result.success, "duration_ms": result.duration_ms })));
        Ok(emit_completed_with_events(
            &ev.request_id,
            self.name(),
            CapabilityResult::Process(ProcessResult { status: result.status, success: result.success, stdout: result.stdout, stderr: result.stderr }),
            events,
        ))
    }
}

struct FileReadCapability;
impl CapabilityHandler for FileReadCapability {
    fn name(&self) -> &'static str {
        "file.read"
    }
    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::File(FileEvent::Read(ev)) = ctx.event else {
            return Ok(CapabilityExecutionResult::NoOp);
        };
        let content = std::fs::read_to_string(&ev.path)?;
        Ok(emit_completed(&ev.request_id, self.name(), CapabilityResult::Process(ProcessResult { status: 0, success: true, stdout: content, stderr: String::new() })))
    }
}

struct FileWriteCapability;
impl CapabilityHandler for FileWriteCapability {
    fn name(&self) -> &'static str {
        "file.write"
    }
    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::File(FileEvent::Write(ev)) = ctx.event else {
            return Ok(CapabilityExecutionResult::NoOp);
        };
        std::fs::write(&ev.path, &ev.content)?;
        Ok(emit_completed(&ev.request_id, self.name(), CapabilityResult::Empty))
    }
}

struct FilePatchCapability;
impl CapabilityHandler for FilePatchCapability {
    fn name(&self) -> &'static str {
        "file.patch"
    }
    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::File(FileEvent::Patch(ev)) = ctx.event else {
            return Ok(CapabilityExecutionResult::NoOp);
        };
        let content = std::fs::read_to_string(&ev.path)?;
        let patched = content.replace(&ev.old, &ev.new);
        std::fs::write(&ev.path, patched)?;
        Ok(emit_completed(&ev.request_id, self.name(), CapabilityResult::Empty))
    }
}

struct BashCapability;
impl CapabilityHandler for BashCapability {
    fn name(&self) -> &'static str {
        "bash"
    }
    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::Bash(ev) = ctx.event else {
            return Ok(CapabilityExecutionResult::NoOp);
        };
        let cwd = ev.cwd.clone().unwrap_or_else(|| ".".to_string());
        let mut cmd_obj = Command::new("bash");
        cmd_obj.arg("-lc").arg(&ev.cmd).current_dir(cwd);
        let output = cmd_obj.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok(emit_completed(&ev.request_id, self.name(), CapabilityResult::Process(ProcessResult { status: output.status.code().unwrap_or(-1), success: output.status.success(), stdout, stderr })))
    }
}

struct LlmCallCapability;
impl CapabilityHandler for LlmCallCapability {
    fn name(&self) -> &'static str {
        "llm.call"
    }
    fn handle(&self, ctx: CapabilityExecutionContext) -> anyhow::Result<CapabilityExecutionResult> {
        let CanonEvent::Llm(LlmCall { request_id, prompt, role }) = ctx.event else {
            return Ok(CapabilityExecutionResult::NoOp);
        };
        Ok(CapabilityExecutionResult::Emit(CanonEvent::CapabilityCompleted(CapabilityCompleted {
            request_id,
            capability: self.name(),
            result: CapabilityResult::Process(ProcessResult { status: 0, success: true, stdout: prompt, stderr: role.unwrap_or_default() }),
        })))
    }
}
