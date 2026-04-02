use canon_event::{events::VerifierPolicyUpdated, CapabilityCompleted, CapabilityFailed, EventId, RouteSelected, RuntimeEvent};

use crate::{context::LoopContext, result::LoopStageResult};

pub mod act;
pub mod decompose;
pub mod observe;
pub mod plan;
pub mod reward;
pub mod verify;

pub enum LoopStageEvent {
    Scan(RouteSelected),
    PlanTrigger(RouteSelected),
    ActDispatch(RouteSelected),
    VerifyTrigger(RouteSelected),
    Decompose(RouteSelected),
    Conclude(RouteSelected),
    CapabilityDone(CapabilityCompleted),
    CapabilityFail(CapabilityFailed),
    RewardPolicy(VerifierPolicyUpdated),
}

impl LoopStageEvent {
    pub fn execute(self, ctx: &mut LoopContext, trigger_id: EventId) -> anyhow::Result<LoopStageResult> {
        match self {
            LoopStageEvent::Scan(_rs) => observe::execute_forced(ctx),
            LoopStageEvent::PlanTrigger(d) => plan::execute_trigger(d, ctx, trigger_id),
            LoopStageEvent::ActDispatch(d) => act::execute_dispatch(d, ctx, trigger_id.clone()),
            LoopStageEvent::VerifyTrigger(d) => verify::execute(d, ctx),
            LoopStageEvent::Decompose(d) => decompose::execute(d, ctx, trigger_id),
            LoopStageEvent::Conclude(d) => reward::execute_conclude(d, ctx),
            LoopStageEvent::CapabilityDone(c) => dispatch_capability_done(c, ctx, trigger_id),
            LoopStageEvent::CapabilityFail(f) => dispatch_capability_fail(f, ctx, trigger_id),
            LoopStageEvent::RewardPolicy(v) => reward::execute_from_policy(v, ctx),
        }
    }
}

fn dispatch_capability_done(c: CapabilityCompleted, ctx: &mut LoopContext, trigger_id: EventId) -> anyhow::Result<LoopStageResult> {
    // DEBUG: trace dispatch into completion pipeline
    {
        let emitter = &ctx.emitter;
        emitter.emit_child(
            RuntimeEvent::Debug(canon_event::DebugEvent {
                source: "loop_stage_dispatch".to_string(),
                kind: "dispatch_capability_done_entry".to_string(),
                payload: serde_json::json!({
                    "scheduler_empty": ctx.scheduler.is_empty(),
                    "pending_act": ctx.pending_act.is_some(),
                }),
            }),
            vec![trigger_id.clone()],
            file!(),
            line!(),
        );
    }
    let decompose_result = decompose::execute_complete(c.clone(), ctx)?;
    if !matches!(decompose_result, LoopStageResult::Noop) {
        return Ok(decompose_result);
    }
    let plan_result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        plan::execute_complete(c.clone(), ctx, trigger_id.clone())
    })) {
        Ok(res) => res?,
        Err(_) => {
            eprintln!("[WARN][loop] execute_complete(plan) panicked; suppressing");
            LoopStageResult::Noop
        }
    };
    if !matches!(plan_result, LoopStageResult::Noop) {
        return Ok(plan_result);
    }
    // TEMP FIX: guard act::execute_complete which may panic
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        act::execute_complete(c, ctx, trigger_id)
    })) {
        Ok(res) => res,
        Err(_) => {
            eprintln!("[WARN][loop] execute_complete panicked; suppressing to keep runtime alive");
            Ok(LoopStageResult::Noop)
        }
    }
}

fn dispatch_capability_fail(f: CapabilityFailed, ctx: &mut LoopContext, trigger_id: EventId) -> anyhow::Result<LoopStageResult> {
    let plan_result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        plan::execute_failed(f.clone(), ctx, trigger_id.clone())
    })) {
        Ok(res) => res?,
        Err(_) => {
            eprintln!("[WARN][loop] execute_failed(plan) panicked; suppressing");
            LoopStageResult::Noop
        }
    };
    if !matches!(plan_result, LoopStageResult::Noop) {
        return Ok(plan_result);
    }

    // TEMP FIX: guard act::execute_failed which may panic
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        act::execute_failed(f, ctx, trigger_id)
    })) {
        Ok(res) => res,
        Err(_) => {
            eprintln!("[WARN][loop] execute_failed panicked; suppressing to keep runtime alive");
            Ok(LoopStageResult::Noop)
        }
    }
}

impl TryFrom<RuntimeEvent> for LoopStageEvent {
    type Error = RuntimeEvent;
    fn try_from(e: RuntimeEvent) -> Result<Self, RuntimeEvent> {
        match e {
            // FIX: RouteSelected must originate from canonical decision() pipeline
            // Reject direct routing based on event payload to enforce semantic-state authority
            RuntimeEvent::RouteSelected(rs) => {
                // NOTE: was crashing runtime; downgrade to warning and ignore
                eprintln!("[WARN][route] non-canonical RouteSelected received: {}", rs.approved_route);
                return Err(RuntimeEvent::RouteSelected(rs));
            },
            RuntimeEvent::CapabilityCompleted(c) => Ok(LoopStageEvent::CapabilityDone(c)),
            RuntimeEvent::CapabilityFailed(f) => Ok(LoopStageEvent::CapabilityFail(f)),
            RuntimeEvent::VerifierPolicyUpdated(v) => Ok(LoopStageEvent::RewardPolicy(v)),
            other => Err(other),
        }
    }
}
