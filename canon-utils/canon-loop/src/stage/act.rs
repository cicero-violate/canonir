use canon_event::{CapabilityCompleted, CapabilityFailed, DebugEvent};

use crate::{context::LoopContext, result::LoopStageResult};

pub fn execute_dispatch(_d: DebugEvent, _ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    Ok(LoopStageResult::Noop)
}

pub fn execute_complete(_c: CapabilityCompleted, _ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    Ok(LoopStageResult::Noop)
}

pub fn execute_failed(_f: CapabilityFailed, _ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    Ok(LoopStageResult::Noop)
}
