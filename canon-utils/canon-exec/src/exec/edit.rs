use super::{Executable, ExecutionContext, ExecutionResult};
use canon_event::{EditEvent, RuntimeEvent};

impl Executable for EditEvent {
    fn execute(self, _ctx: ExecutionContext) -> anyhow::Result<ExecutionResult> {
        Ok(ExecutionResult::Emit(RuntimeEvent::Edit(self)))
    }
}
