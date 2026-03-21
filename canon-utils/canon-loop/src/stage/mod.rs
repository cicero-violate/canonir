use canon_event::{CapabilityCompleted, CapabilityFailed, CanonEvent, DebugEvent, LoopVerified, Tick};

use crate::{context::LoopContext, result::LoopStageResult};

pub mod observe;
pub mod plan;
pub mod act;
pub mod verify;
pub mod reward;

pub enum LoopStageEvent {
    Observe(Tick),
    PlanTrigger(DebugEvent),
    ActDispatch(DebugEvent),
    VerifyTrigger(DebugEvent),
    Conclude(DebugEvent),
    CapabilityDone(CapabilityCompleted),
    CapabilityFail(CapabilityFailed),
    Reward(LoopVerified),
}

impl LoopStageEvent {
    pub fn execute(self, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
        match self {
            LoopStageEvent::Observe(t) => observe::execute(t, ctx),
            LoopStageEvent::PlanTrigger(d) => plan::execute_trigger(d, ctx),
            LoopStageEvent::ActDispatch(d) => act::execute_dispatch(d, ctx),
            LoopStageEvent::VerifyTrigger(d) => verify::execute(d, ctx),
            LoopStageEvent::Conclude(d) => reward::execute_conclude(d, ctx),
            LoopStageEvent::CapabilityDone(c) => dispatch_capability_done(c, ctx),
            LoopStageEvent::CapabilityFail(f) => dispatch_capability_fail(f, ctx),
            LoopStageEvent::Reward(v) => reward::execute(v, ctx),
        }
    }
}

fn dispatch_capability_done(c: CapabilityCompleted, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    plan::execute_complete(c.clone(), ctx).or_else(|_| act::execute_complete(c, ctx))
}

fn dispatch_capability_fail(f: CapabilityFailed, ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    plan::execute_failed(f.clone(), ctx).or_else(|_| act::execute_failed(f, ctx))
}

impl TryFrom<CanonEvent> for LoopStageEvent {
    type Error = CanonEvent;
    fn try_from(e: CanonEvent) -> Result<Self, CanonEvent> {
        fn route_lane(d: &DebugEvent) -> &str {
            if d.kind != "route_selected" {
                return "";
            }
            d.payload
                .get("approved_route")
                .or_else(|| d.payload.get("lane"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
        }
        match e {
            CanonEvent::Tick(t) => Ok(LoopStageEvent::Observe(t)),
            CanonEvent::Debug(d) if route_lane(&d) == "shape" => Ok(LoopStageEvent::PlanTrigger(d)),
            CanonEvent::Debug(d) if route_lane(&d) == "execute" => Ok(LoopStageEvent::ActDispatch(d)),
            CanonEvent::Debug(d) if route_lane(&d) == "validate" => Ok(LoopStageEvent::VerifyTrigger(d)),
            CanonEvent::Debug(d) if route_lane(&d) == "conclude" => Ok(LoopStageEvent::Conclude(d)),
            CanonEvent::CapabilityCompleted(c) => Ok(LoopStageEvent::CapabilityDone(c)),
            CanonEvent::CapabilityFailed(f) => Ok(LoopStageEvent::CapabilityFail(f)),
            CanonEvent::LoopVerified(v) => Ok(LoopStageEvent::Reward(v)),
            other => Err(other),
        }
    }
}
