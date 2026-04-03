use canon_event::{events::VerifierPolicyUpdated, LoopRewarded, LoopVerified, RouteSelected, RuntimeEvent};
use canon_semantic_state::{latest_graph_proof_failed, latest_graph_proof_verified, latest_no_semantic_progress, latest_semantic_progress};

use crate::{context::LoopContext, result::LoopStageResult};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RewardSemantics {
    pub reward: f32,
    pub resets_stagnation: bool,
}

pub fn execute_conclude(_rs: RouteSelected, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let rewarded = LoopRewarded {
        tick: ctx.current_tick,
        errors_before: ctx.errors_before,
        errors_after: ctx.error_count,
        stagnant_ticks: ctx.stagnant_ticks,
        span_id: None,
        parent_span_id: None,
        reward: 1.0_f32,
        halt: true,
        goodness: ctx.goodness.unwrap_or(0.0),
        delta_g: ctx.delta_g.unwrap_or(0.0),
        trace_id: None,
        execution_id: None,
    };
    Ok(LoopStageResult::Emit(RuntimeEvent::LoopRewarded(rewarded)))
}

pub fn execute(v: LoopVerified, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    ctx.last_reward_trace_id = v.trace_id.clone();
    ctx.last_reward_execution_id = v.execution_id.clone();
    let semantics = evaluate_reward_semantics(ctx, &v);
    // Verify/reward is evaluative, not terminal: ordinary compiler failures should replan,
    // not halt the loop. Terminal halts only come from explicit conclude routing.
    let halt = false;
    let rewarded = LoopRewarded {
        tick: v.tick,
        errors_before: ctx.errors_before,
        errors_after: ctx.error_count,
        stagnant_ticks: ctx.stagnant_ticks,
        span_id: v.span_id.clone(),
        parent_span_id: v.parent_span_id.clone(),
        reward: semantics.reward,
        halt,
        goodness: ctx.goodness.unwrap_or(0.0),
        delta_g: ctx.delta_g.unwrap_or(0.0),
        trace_id: v.trace_id.clone(),
        execution_id: v.execution_id.clone(),
    };
    if semantics.resets_stagnation {
        ctx.stagnant_ticks = 0;
    } else {
        ctx.stagnant_ticks = ctx.stagnant_ticks.saturating_add(1);
        // Compiler failed — the LLM's "done" was premature. Force replanning.
        ctx.last_done_goal = None;
    }
    Ok(LoopStageResult::Emit(RuntimeEvent::LoopRewarded(rewarded)))
}

pub fn execute_from_policy(policy: VerifierPolicyUpdated, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let verified = ctx.last_verified.clone().expect("LoopVerified must be observed before VerifierPolicyUpdated reward evaluation");
    ctx.last_reward_trace_id = policy.trace_id.clone().or_else(|| verified.trace_id.clone());
    ctx.last_reward_execution_id = policy.execution_id.clone().or_else(|| verified.execution_id.clone());
    let semantics = evaluate_reward_semantics(ctx, &verified);
    let halt = false;
    let rewarded = LoopRewarded {
        tick: verified.tick,
        errors_before: ctx.errors_before,
        errors_after: ctx.error_count,
        stagnant_ticks: ctx.stagnant_ticks,
        span_id: verified.span_id.clone(),
        parent_span_id: verified.parent_span_id.clone(),
        reward: semantics.reward,
        halt,
        goodness: ctx.goodness.unwrap_or(0.0),
        delta_g: ctx.delta_g.unwrap_or(0.0),
        trace_id: ctx.last_reward_trace_id.clone(),
        execution_id: ctx.last_reward_execution_id.clone(),
    };
    if semantics.resets_stagnation {
        ctx.stagnant_ticks = 0;
    } else {
        ctx.stagnant_ticks = ctx.stagnant_ticks.saturating_add(1);
        ctx.last_done_goal = None;
    }
    Ok(LoopStageResult::Emit(RuntimeEvent::LoopRewarded(rewarded)))
}

pub fn evaluate_reward_semantics(ctx: &LoopContext, _v: &LoopVerified) -> RewardSemantics {
    let reward_bias = ctx.last_verifier_reward_bias.as_deref().expect("VerifierPolicyUpdated must be observed before reward evaluation");
    let mut reward = if reward_bias == "positive" { 1.0_f32 } else { -1.0_f32 };
    if ctx.last_action_kind == "done" {
        reward += 0.5;
    }
    if latest_semantic_progress(&ctx.recent_execution_results) {
        reward += 0.4;
    } else if latest_no_semantic_progress(&ctx.recent_execution_results) {
        reward -= 0.4;
    }
    if latest_graph_proof_verified(&ctx.recent_execution_results) {
        reward += 0.2;
    } else if latest_graph_proof_failed(&ctx.recent_execution_results) {
        reward -= 0.4;
    }
    if ctx.objective_trend_state.repair_resolution_rate() > 0.5 {
        reward += 0.2;
    }
    if ctx.objective_trend_state.repeated_stall_count > 0 && ctx.objective_trend_state.current_no_progress_streak > 0 {
        reward -= 0.3;
    }
    RewardSemantics { reward, resets_stagnation: reward_bias == "positive" || (latest_semantic_progress(&ctx.recent_execution_results) && !latest_graph_proof_failed(&ctx.recent_execution_results)) }
}

#[cfg(test)]
mod tests {
    use super::{evaluate_reward_semantics, execute};
    use crate::context::LoopContext;
    use canon_event::LoopVerified;
    use canon_semantic_state::SemanticExecutionResultRecord;
    use std::path::PathBuf;

    fn base_verified() -> LoopVerified {
        LoopVerified {
            tick: 0,
            passed: false,
            compiler_clean: false,
            tlog_clean: true,
            error_count: 1,
            diagnostics: vec!["error".into()],
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
        }
    }

    #[test]
    fn semantic_progress_improves_reward() {
        let mut ctx = LoopContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp/tlog"),
            {
                use std::sync::Arc;
                struct N;
                impl canon_event::EventEmitter for N {
                    fn emit_with_parents(&self, _event: canon_event::RuntimeEvent, _parents: Vec<canon_event::EventId>, _file: &'static str, _line: u32) {}
                }
                Arc::new(N)
            }
        );
        ctx.last_verifier_reward_bias = Some("negative".into());
        let verified = base_verified();
        let base = evaluate_reward_semantics(&ctx, &verified).reward;
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("module_created", "module file created", vec!["/tmp/src/index.rs".into()], true));
        assert!(evaluate_reward_semantics(&ctx, &verified).reward > base);
    }

    #[test]
    fn semantic_progress_resets_stagnation_on_failed_verify() {
        let mut ctx = LoopContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp/tlog"),
            {
                use std::sync::Arc;
                struct N;
                impl canon_event::EventEmitter for N {
                    fn emit_with_parents(&self, _event: canon_event::RuntimeEvent, _parents: Vec<canon_event::EventId>, _file: &'static str, _line: u32) {}
                }
                Arc::new(N)
            }
        );
        ctx.stagnant_ticks = 3;
        ctx.last_verifier_reward_bias = Some("negative".into());
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("module_created", "module file created", vec!["/tmp/src/index.rs".into()], true));
        let verified = base_verified();
        let _ = execute(verified, &mut ctx).unwrap();
        assert_eq!(ctx.stagnant_ticks, 0);
    }

    #[test]
    fn graph_proof_failure_penalizes_reward() {
        let mut ctx = LoopContext::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp/tlog"),
            {
                use std::sync::Arc;
                struct N;
                impl canon_event::EventEmitter for N {
                    fn emit_with_parents(&self, _event: canon_event::RuntimeEvent, _parents: Vec<canon_event::EventId>, _file: &'static str, _line: u32) {}
                }
                Arc::new(N)
            }
        );
        ctx.last_verifier_reward_bias = Some("negative".into());
        let verified = base_verified();
        let base = evaluate_reward_semantics(&ctx, &verified).reward;
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("graph_proof_failed", "semantic graph proof failed", Vec::new(), false));
        assert!(evaluate_reward_semantics(&ctx, &verified).reward < base);
    }
}
