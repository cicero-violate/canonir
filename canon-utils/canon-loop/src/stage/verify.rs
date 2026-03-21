use std::path::Path;
use std::process::Command;

use canon_event::{CanonEvent, DebugEvent, LoopVerified};

use crate::{context::LoopContext, result::LoopStageResult};

pub fn execute(d: DebugEvent, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let lane = d
        .payload
        .get("approved_route")
        .or_else(|| d.payload.get("lane"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if lane != "validate" {
        return Ok(LoopStageResult::Noop);
    }

    let trace_id = Some(uuid::Uuid::new_v4().to_string());
    let execution_id = Some(uuid::Uuid::new_v4().to_string());
    ctx.last_verify_trace_id = trace_id.clone();
    ctx.last_verify_execution_id = execution_id.clone();

    let mut diagnostics: Vec<String> = Vec::new();
    let mut passed = true;

    // Check tlog cleanliness (stub: if workspace_dirty flag set by runtime state updates)
    if ctx.last_acted.as_ref().map(|a| a.success).unwrap_or(true) {
        // run cargo check as in VerifyConsumer
        let (ok, stderr) = run_cargo_check(&ctx.workspace)?;
        if !ok {
            passed = false;
            diagnostics.push("cargo_check_failed".into());
            diagnostics.push(stderr);
        }
    }

    // basic file_written check stub: ensure last acted not empty
    if ctx.last_acted.is_none() {
        passed = false;
        diagnostics.push("no_actions_executed".into());
    }

    let verified = LoopVerified {
        tick: d.payload.get("tick").and_then(|v| v.as_u64()).unwrap_or(0),
        compiler_clean: passed,
        tlog_clean: true,
        error_count: ctx.error_count,
        trace_id,
        execution_id,
        span_id: ctx.last_act_span_id.clone(),
        parent_span_id: None,
        diagnostics,
        passed,
    };
    ctx.last_verify_execution_id = verified.execution_id.clone();
    ctx.last_verify_trace_id = verified.trace_id.clone();
    Ok(LoopStageResult::Emit(CanonEvent::LoopVerified(verified)))
}

fn run_cargo_check(workspace: &Path) -> anyhow::Result<(bool, String)> {
    let output = Command::new("cargo").arg("check").current_dir(workspace).output()?;
    let success = output.status.success();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((success, stderr))
}
