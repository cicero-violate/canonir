use canon_event::{RuntimeEvent, LoopRewarded, LoopVerified, RouteSelected};

use crate::{context::LoopContext, result::LoopStageResult};

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
    let reward = compute_reward(ctx, &v);
    let semantic_progress = latest_semantic_progress(ctx);
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
        reward,
        halt,
        goodness: ctx.goodness.unwrap_or(0.0),
        delta_g: ctx.delta_g.unwrap_or(0.0),
        trace_id: v.trace_id.clone(),
        execution_id: v.execution_id.clone(),
    };
    if v.compiler_clean || semantic_progress {
        ctx.stagnant_ticks = 0;
    } else {
        ctx.stagnant_ticks = ctx.stagnant_ticks.saturating_add(1);
        // Compiler failed — the LLM's "done" was premature. Force replanning.
        ctx.last_done_goal = None;
    }
    Ok(LoopStageResult::Emit(RuntimeEvent::LoopRewarded(rewarded)))
}

fn compute_reward(ctx: &LoopContext, v: &LoopVerified) -> f32 {
    let mut reward = if v.compiler_clean { 1.0_f32 } else { -1.0_f32 };
    if !ctx.last_action_success {
        reward -= 0.2;
    }
    if ctx.last_action_kind == "done" {
        reward += 0.5;
    }
    if latest_semantic_progress(ctx) {
        reward += 0.4;
    } else if latest_no_semantic_progress(ctx) {
        reward -= 0.4;
    }
    reward
}

fn latest_semantic_progress(ctx: &LoopContext) -> bool {
    ctx.recent_execution_results
        .iter()
        .rev()
        .next()
        .is_some_and(|result| result.semantic_progress)
}

fn latest_no_semantic_progress(ctx: &LoopContext) -> bool {
    ctx.recent_execution_results
        .iter()
        .rev()
        .next()
        .is_some_and(|result| !result.semantic_progress)
}

#[cfg(test)]
mod tests {
    use super::{compute_reward, execute};
    use crate::context::LoopContext;
    use canon_event::LoopVerified;
    use canon_semantic_state::SemanticExecutionResultRecord;
    use std::path::PathBuf;

    fn base_verified() -> LoopVerified {
        LoopVerified {
            tick: 0,
            passed: false,
            compiler_clean: false,
            diagnostics: vec!["error".into()],
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            done_action: false,
            system_satisfied: false,
        }
    }

    #[test]
    fn semantic_progress_improves_reward() {
        let mut ctx = LoopContext::new(PathBuf::from("/tmp"), PathBuf::from("/tmp/tlog"));
        let verified = base_verified();
        let base = compute_reward(&ctx, &verified);
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new(
            "module_created",
            "module file created",
            vec!["/tmp/src/index.rs".into()],
            true,
        ));
        assert!(compute_reward(&ctx, &verified) > base);
    }

    #[test]
    fn semantic_progress_resets_stagnation_on_failed_verify() {
        let mut ctx = LoopContext::new(PathBuf::from("/tmp"), PathBuf::from("/tmp/tlog"));
        ctx.stagnant_ticks = 3;
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new(
            "module_created",
            "module file created",
            vec!["/tmp/src/index.rs".into()],
            true,
        ));
        let verified = base_verified();
        let _ = execute(verified, &mut ctx).unwrap();
        assert_eq!(ctx.stagnant_ticks, 0);
    }
}
