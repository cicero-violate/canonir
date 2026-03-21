use canon_event::DebugEvent;

use crate::{context::LoopContext, result::LoopStageResult};

pub fn execute(_d: DebugEvent, _ctx: &mut LoopContext) -> anyhow::Result<LoopStageResult> {
    Ok(LoopStageResult::Noop)
}
