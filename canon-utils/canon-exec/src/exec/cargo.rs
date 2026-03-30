use super::{Executable, ExecutionContext, ExecutionResult};
use canon_event::{CapabilityCompleted, CapabilityResult, CargoEvent, ProcessResult, RuntimeEvent, RuntimeStateUpdated};
use serde_json::json;
use std::process::Command;
use std::time::Instant;

fn runtime_log(kind: &str, payload: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload: json!({ "kind": kind, "payload": payload }) })
}

fn completed(request_id: String, capability: &'static str, output: std::process::Output) -> RuntimeEvent {
    RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
        request_id,
        capability,
        result: CapabilityResult::Process(ProcessResult {
            status: output.status.code().unwrap_or(-1),
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }),
    })
}

impl Executable for CargoEvent {
    fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        match self {
            CargoEvent::Build(ev) => {
                let crate_name = ev.crate_name.clone();
                let mut events = vec![runtime_log("build.started", json!({ "crate": crate_name }))];
                let start = Instant::now();
                let output = Command::new("cargo").args(["build", "-p", &crate_name]).output()?;
                let duration_ms = start.elapsed().as_millis();
                events.push(runtime_log("build.completed", json!({ "crate": crate_name, "success": output.status.success(), "duration_ms": duration_ms })));
                events.push(completed(ev.request_id, "cargo.build", output));
                Ok(ExecutionResult::EmitMany(events))
            }
            CargoEvent::Run(ev) => {
                let crate_name = ev.crate_name.clone();
                let bin = ev.bin.clone();
                let mut events = vec![runtime_log("run.started", json!({ "crate": crate_name, "bin": bin }))];
                let start = Instant::now();
                let mut cmd = Command::new("cargo");
                cmd.args(["run", "-p", &crate_name]);
                if let Some(b) = ev.bin.as_deref() {
                    cmd.args(["--bin", b]);
                }
                if !ev.args.is_empty() {
                    cmd.arg("--");
                    cmd.args(&ev.args);
                }
                let output = cmd.output()?;
                let duration_ms = start.elapsed().as_millis();
                events.push(runtime_log("run.completed", json!({ "crate": crate_name, "bin": bin, "success": output.status.success(), "duration_ms": duration_ms })));
                events.push(completed(ev.request_id, "cargo.run", output));
                Ok(ExecutionResult::EmitMany(events))
            }
            CargoEvent::Check(ev) => {
                let crate_name = ev.crate_name.clone();
                let mut events = vec![runtime_log("check.started", json!({ "crate": crate_name }))];
                let start = Instant::now();
                let output = Command::new("cargo").args(["check", "-p", &crate_name]).output()?;
                let duration_ms = start.elapsed().as_millis();
                events.push(runtime_log("check.completed", json!({ "crate": crate_name, "success": output.status.success(), "duration_ms": duration_ms })));
                events.push(completed(ev.request_id, "cargo.check", output));
                Ok(ExecutionResult::EmitMany(events))
            }
        }
    }
}
