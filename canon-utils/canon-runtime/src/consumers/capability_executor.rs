use canon_event::{new_error_occurred, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RuntimeEvent};
use canon_exec::{ExecutableEvent, ExecutionContext, ExecutionResult};
use canon_proc_macros::must_emit;
use std::path::PathBuf;

pub struct CapabilityExecutor {
    workspace: PathBuf,
    emitter: Option<EventEmitterHandle>,
    current_trigger: Option<EventId>,
}

impl CapabilityExecutor {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace, emitter: None, current_trigger: None }
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
    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        self.current_trigger = Some(trigger_id.clone());
        let Ok(exec) = ExecutableEvent::try_from(event.clone()) else {
            return EventOutcome::NoOp("capability_executor_not_executable");
        };
        let Some(emitter) = self.emitter.clone() else {
            return EventOutcome::NoOp("capability_executor_no_emitter");
        };
        let ctx = ExecutionContext { workspace: self.workspace.clone(), emitter: emitter.clone(), trigger_id: trigger_id.clone() };
        let event_debug = format!("{:?}", event);
        // Spawn the execution in a separate thread so the consumer thread is not blocked
        // for the duration of the LLM/bash/IO call. A blocked consumer thread fills its
        // channel (1024), causing the bus dispatch loop to stall on blocking send() for
        // control events, freezing the entire runtime.
        std::thread::spawn(move || {
            match exec.execute(ctx) {
                Ok(ExecutionResult::Emit(e)) => emitter.emit_child(e, vec![trigger_id.clone()], file!(), line!()),
                Ok(ExecutionResult::EmitMany(evs)) => evs.into_iter().for_each(|e| emitter.emit_child(e, vec![trigger_id.clone()], file!(), line!())),
                Ok(ExecutionResult::Deferred) => {}
                Err(err) => emitter.emit_child(RuntimeEvent::ErrorOccurred(new_error_occurred(
                    "capability_execution",
                    "capability_executor",
                    err.to_string(),
                    "error",
                    serde_json::json!({ "event": event_debug }),
                    None,
                )), vec![trigger_id], file!(), line!()),
            }
        });
        EventOutcome::NoOp("capability_executor_async")
    }
}
