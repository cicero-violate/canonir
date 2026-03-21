use super::{Executable, ExecutionContext, ExecutionResult};
use canon_event::{CanonEvent, EditEvent};

impl Executable for EditEvent {
    fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        Ok(ExecutionResult::Emit(CanonEvent::Edit(self)))
    }
}
