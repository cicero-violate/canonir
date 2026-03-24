use std::path::Path;
use std::process::Command;

use canon_event::{RuntimeEvent, LoopVerified, RouteSelected};
use canon_goal::parse_agent_goal_markdown;

use crate::{context::LoopContext, result::LoopStageResult};

pub fn execute(rs: RouteSelected, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let trace_id = Some(uuid::Uuid::new_v4().to_string());
    let execution_id = Some(uuid::Uuid::new_v4().to_string());
    ctx.last_verify_trace_id = trace_id.clone();
    ctx.last_verify_execution_id = execution_id.clone();

    let mut diagnostics: Vec<String> = Vec::new();
    let mut passed = true;

    // Determine the target workspace from the goal spec, not the agent's own workspace.
    let target_path = ctx
        .goal_text
        .as_deref()
        .and_then(|text| {
            let spec = parse_agent_goal_markdown(text);
            spec.target_path
        })
        .unwrap_or_else(|| ctx.workspace.clone());

    // Always run cargo check on the target — even when the last action failed,
    // we need accurate verification state.
    let (ok, stderr) = run_cargo_check(&target_path)?;
    if !ok {
        passed = false;
        diagnostics.push("cargo_check_failed".into());
        diagnostics.push(stderr);
    }

    // basic file_written check stub: ensure last acted not empty
    if ctx.last_acted.is_none() {
        passed = false;
        diagnostics.push("no_actions_executed".into());
    }

    let verified = LoopVerified {
        tick: rs.tick,
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
    Ok(LoopStageResult::Emit(RuntimeEvent::LoopVerified(verified)))
}

fn run_cargo_check(workspace: &Path) -> anyhow::Result<(bool, String)> {
    if !workspace.exists() {
        return Ok((false, format!("target path does not exist: {}", workspace.display())));
    }
    let output = Command::new("cargo").arg("check").current_dir(workspace).output()?;
    let success = output.status.success();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((success, stderr))
}
