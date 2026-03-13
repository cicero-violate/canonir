use canon_capability::{Capability, CapabilityContext, CapabilityRegistry, CapabilityResult};
use canon_types::{CapabilityCompleted, CapabilityFailed, CapabilityRequested, RuntimeEvent};
use serde_json::json;
use std::process::Command;

pub fn register_capabilities(registry: &mut CapabilityRegistry) {
    registry.register(std::sync::Arc::new(CargoBuild));
    registry.register(std::sync::Arc::new(CargoCheck));
    registry.register(std::sync::Arc::new(FileRead));
    registry.register(std::sync::Arc::new(FileWrite));
    registry.register(std::sync::Arc::new(Bash));
    registry.register(std::sync::Arc::new(LlmCall));
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

struct CargoBuild;

impl Capability for CargoBuild {
    fn name(&self) -> &'static str {
        "cargo.build"
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        let manifest_path = req.args.get("manifest_path").and_then(|v| v.as_str());
        let mut cmd = Command::new("cargo");
        cmd.arg("build");
        if let Some(path) = manifest_path {
            cmd.arg("--manifest-path").arg(path);
        }
        cmd.current_dir(&ctx.workspace);
        let output = cmd.output()?;
        let result = json!({
            "status": output.status.code().unwrap_or(-1),
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        });
        Ok(emit_completed(&req, result))
    }
}

struct CargoCheck;

impl Capability for CargoCheck {
    fn name(&self) -> &'static str {
        "cargo.check"
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        let manifest_path = req.args.get("manifest_path").and_then(|v| v.as_str());
        let mut cmd = Command::new("cargo");
        cmd.arg("check");
        if let Some(path) = manifest_path {
            cmd.arg("--manifest-path").arg(path);
        }
        cmd.current_dir(&ctx.workspace);
        let output = cmd.output()?;
        let result = json!({
            "status": output.status.code().unwrap_or(-1),
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        });
        Ok(emit_completed(&req, result))
    }
}

struct FileRead;

impl Capability for FileRead {
    fn name(&self) -> &'static str {
        "file.read"
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        let Some(path) = req.args.get("path").and_then(|v| v.as_str()) else {
            return Ok(emit_failed(&req, "missing path"));
        };
        let content = std::fs::read_to_string(path)?;
        Ok(emit_completed(&req, json!({ "path": path, "content": content })))
    }
}

struct FileWrite;

impl Capability for FileWrite {
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
        std::fs::write(path, content)?;
        Ok(emit_completed(&req, json!({ "path": path, "bytes": content.len() })))
    }
}

struct Bash;

impl Capability for Bash {
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
        let result = json!({
            "status": output.status.code().unwrap_or(-1),
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        });
        Ok(emit_completed(&req, result))
    }
}

struct LlmCall;

impl Capability for LlmCall {
    fn name(&self) -> &'static str {
        "llm.call"
    }

    fn execute(&self, ctx: CapabilityContext) -> anyhow::Result<CapabilityResult> {
        let req = request_from_ctx(&ctx)?;
        Ok(emit_failed(&req, "llm.call capability not wired in runtime"))
    }
}
