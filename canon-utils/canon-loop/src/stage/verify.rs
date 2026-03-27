use std::path::Path;
use std::process::Command;

use canon_event::{events::VerifierPolicyUpdated, LoopVerified, RouteSelected, RuntimeEvent};
use canon_invariant::meta_invariant_all_results_update_policy;
use canon_goal::parse_agent_goal_markdown;
use canon_semantic_state::{FailureClassKind, FailureScopeKind};

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
        let hints = crate::compiler_hints::planner_lines(&[serde_json::Value::String(stderr.clone())]);
        let failure_class = hints
            .iter()
            .find_map(|hint| hint.kind_enum().map(|kind| kind.as_str().to_string()))
            .unwrap_or_else(|| FailureClassKind::GenericCompilerFailure.as_str().to_string());
        let failure_scope = hints
            .iter()
            .filter_map(|hint| hint.failure_scope_enum())
            .find(|scope| *scope != FailureScopeKind::None)
            .map(|scope| scope.as_str().to_string())
            .unwrap_or_else(|| FailureScopeKind::None.as_str().to_string());
        diagnostics.push(format!("failure_class={failure_class}"));
        diagnostics.push(format!("failure_scope={failure_scope}"));
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
    let policy_update = meta_invariant_all_results_update_policy(
        verified.passed,
        verified.compiler_clean,
        &verified.diagnostics,
    );
    let verifier_policy_updated = VerifierPolicyUpdated {
        tick: rs.tick,
        verifier_outcome: policy_update.verifier_outcome.as_str().to_string(),
        retry_policy: policy_update.retry_policy.to_string(),
        reward_bias: policy_update.reward_bias.to_string(),
        actionable_failure: policy_update.actionable_failure,
        trace_id: verified.trace_id.clone(),
        execution_id: verified.execution_id.clone(),
        span_id: verified.span_id.clone(),
        parent_span_id: verified.parent_span_id.clone(),
    };
    ctx.last_verify_execution_id = verified.execution_id.clone();
    ctx.last_verify_trace_id = verified.trace_id.clone();
    Ok(LoopStageResult::EmitMany(vec![
        RuntimeEvent::LoopVerified(verified),
        RuntimeEvent::VerifierPolicyUpdated(verifier_policy_updated),
    ]))
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

#[cfg(test)]
mod tests {
    use super::execute;
    use crate::{context::LoopContext, result::LoopStageResult};
    use canon_event::{RouteSelected, RuntimeEvent};
    use std::path::PathBuf;

    #[test]
    fn verify_emits_verifier_policy_updated_event() {
        let workspace = std::env::temp_dir().join(format!("canon_verify_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"verify_smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(
            workspace.join("src/main.rs"),
            "fn main() { println!(\"ok\"); }\n",
        )
        .unwrap();

        let mut ctx = LoopContext::new(workspace.clone(), PathBuf::from("/tmp/test.tlog"));
        ctx.last_acted = Some(canon_event::LoopActed {
            tick: 0,
            action_kind: "run_command".to_string(),
            capability_request_id: "req".to_string(),
            tool_call_id: None,
            tool_result_id: None,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
            duration_ms: 0,
            success: true,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
        });

        let rs = RouteSelected {
            tick: 1,
            suggested_route: "verify".to_string(),
            prompt: String::new(),
            approved_route: "verify".to_string(),
            rationale: "test".to_string(),
            confidence: None,
            gate_note: String::new(),
            gate_rules_fired: Vec::new(),
            gate_changed: false,
            gate_should_stop: false,
            model_json: String::new(),
        };
        match execute(rs, &mut ctx).unwrap() {
            LoopStageResult::EmitMany(events) => {
                assert!(events.iter().any(|event| matches!(event, RuntimeEvent::VerifierPolicyUpdated(_))));
            }
            other => panic!("expected EmitMany, got {:?}", std::mem::discriminant(&other)),
        }
        let _ = std::fs::remove_dir_all(&workspace);
    }
}
