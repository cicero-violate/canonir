use canon_event::{DebugEvent, LoopVerified};

use crate::{context::LoopContext, result::LoopStageResult};

pub fn execute_conclude(_d: DebugEvent, _ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    Ok(LoopStageResult::Noop)
}

pub fn execute(_v: LoopVerified, _ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    Ok(LoopStageResult::Noop)
}
