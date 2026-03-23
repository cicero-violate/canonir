use canon_event::{RuntimeEvent, LoopRewarded, LoopVerified, RouteSelected};

use crate::{context::LoopContext, result::LoopStageResult};

pub fn execute_conclude(_rs: RouteSelected, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let rewarded = LoopRewarded {
        tick: 0,
        errors_before: ctx.errors_before,
        errors_after: ctx.error_count,
        stagnant_ticks: ctx.stagnant_ticks,
        span_id: None,
        parent_span_id: None,
        reward: 1.0_f32,
        halt: true,
        goodness: ctx.goodness.unwrap_or(1.0),
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
    let halt = ctx.stagnant_ticks > 10;
    let rewarded = LoopRewarded {
        tick: v.tick,
        errors_before: ctx.errors_before,
        errors_after: ctx.error_count,
        stagnant_ticks: ctx.stagnant_ticks,
        span_id: v.span_id.clone(),
        parent_span_id: v.parent_span_id.clone(),
        reward,
        halt,
        goodness: ctx.goodness.unwrap_or(1.0),
        delta_g: ctx.delta_g.unwrap_or(0.0),
        trace_id: v.trace_id.clone(),
        execution_id: v.execution_id.clone(),
    };
    if v.compiler_clean {
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
    reward
}
