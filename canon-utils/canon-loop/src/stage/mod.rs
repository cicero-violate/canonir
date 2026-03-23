use canon_event::{CapabilityCompleted, CapabilityFailed, RuntimeEvent, LoopVerified, RouteSelected};

use crate::{context::LoopContext, result::LoopStageResult};

pub mod observe;
pub mod plan;
pub mod act;
pub mod verify;
pub mod reward;
pub mod decompose;

pub enum LoopStageEvent {
    Scan(RouteSelected),
    PlanTrigger(RouteSelected),
    ActDispatch(RouteSelected),
    VerifyTrigger(RouteSelected),
    Decompose(RouteSelected),
    Conclude(RouteSelected),
    CapabilityDone(CapabilityCompleted),
    CapabilityFail(CapabilityFailed),
    Reward(LoopVerified),
}

impl LoopStageEvent {
    pub fn execute(self, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
        match self {
            LoopStageEvent::Scan(_rs) => Ok(LoopStageResult::Noop),
            LoopStageEvent::PlanTrigger(d) => plan::execute_trigger(d, ctx),
            LoopStageEvent::ActDispatch(d) => act::execute_dispatch(d, ctx),
            LoopStageEvent::VerifyTrigger(d) => verify::execute(d, ctx),
            LoopStageEvent::Decompose(d) => decompose::execute(d, ctx),
            LoopStageEvent::Conclude(d) => reward::execute_conclude(d, ctx),
            LoopStageEvent::CapabilityDone(c) => dispatch_capability_done(c, ctx),
            LoopStageEvent::CapabilityFail(f) => dispatch_capability_fail(f, ctx),
            LoopStageEvent::Reward(v) => reward::execute(v, ctx),
        }
    }
}

fn dispatch_capability_done(c: CapabilityCompleted, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let decompose_result = decompose::execute_complete(c.clone(), ctx)?;
    if !matches!(decompose_result, LoopStageResult::Noop) {
        return Ok(decompose_result);
    }
    let plan_result = plan::execute_complete(c.clone(), ctx)?;
    if !matches!(plan_result, LoopStageResult::Noop) {
        return Ok(plan_result);
    }
    act::execute_complete(c, ctx)
}

fn dispatch_capability_fail(f: CapabilityFailed, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    let plan_result = plan::execute_failed(f.clone(), ctx)?;
    if !matches!(plan_result, LoopStageResult::Noop) {
        return Ok(plan_result);
    }
    act::execute_failed(f, ctx)
}

impl TryFrom<RuntimeEvent> for LoopStageEvent {
    type Error = RuntimeEvent;
    fn try_from(e: RuntimeEvent) -> Result<Self, RuntimeEvent> {
        match e {
            RuntimeEvent::RouteSelected(rs) => match rs.approved_route.as_str() {
                "plan" => Ok(LoopStageEvent::PlanTrigger(rs)),
                "act" => Ok(LoopStageEvent::ActDispatch(rs)),
                "verify" => Ok(LoopStageEvent::VerifyTrigger(rs)),
                "decompose" => Ok(LoopStageEvent::Decompose(rs)),
                "conclude" => Ok(LoopStageEvent::Conclude(rs)),
                "observe" => Ok(LoopStageEvent::Scan(rs)),
                _ => Err(RuntimeEvent::RouteSelected(rs)),
            },
            RuntimeEvent::CapabilityCompleted(c) => Ok(LoopStageEvent::CapabilityDone(c)),
            RuntimeEvent::CapabilityFailed(f) => Ok(LoopStageEvent::CapabilityFail(f)),
            RuntimeEvent::LoopVerified(v) => Ok(LoopStageEvent::Reward(v)),
            other => Err(other),
        }
    }
}
