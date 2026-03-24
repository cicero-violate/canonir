use canon_event::{new_error_occurred, RuntimeEvent, EventConsumer, EventEmitterHandle, EventFilter, EventOutcome};
use canon_proc_macros::must_emit;
use canon_exec::{ExecutableEvent, ExecutionContext, ExecutionResult};
use std::path::PathBuf;

pub struct CapabilityExecutor {
    workspace: PathBuf,
    emitter: Option<EventEmitterHandle>,
}

impl CapabilityExecutor {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace, emitter: None }
    }
}

impl EventConsumer for CapabilityExecutor {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent) -> EventOutcome {
        let Ok(exec) = ExecutableEvent::try_from(event.clone()) else {
            return EventOutcome::NoOp("capability_executor_not_executable");
        };
        let Some(emitter) = self.emitter.clone() else {
            return EventOutcome::NoOp("capability_executor_no_emitter");
        };
        let ctx = ExecutionContext { workspace: self.workspace.clone(), emitter: emitter.clone() };
        match exec.execute(ctx) {
            Ok(ExecutionResult::Emit(e)) => emitter.emit(e),
            Ok(ExecutionResult::EmitMany(evs)) => evs.into_iter().for_each(|e| emitter.emit(e)),
            Ok(ExecutionResult::Deferred) => {}
            Err(err) => emitter.emit(RuntimeEvent::ErrorOccurred(new_error_occurred(
                "capability_execution",
                "capability_executor",
                err.to_string(),
                "error",
                serde_json::json!({ "event": format!("{:?}", event) }),
                None,
            ))),
        }
        EventOutcome::NoOp("capability_executor_async")
    }
}
