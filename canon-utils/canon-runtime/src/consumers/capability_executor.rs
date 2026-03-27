use canon_event::{new_error_occurred, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RuntimeEvent};
use canon_exec::{
    evaluate_execution_policy, ExecutableEvent, ExecutionContext, ExecutionPolicyDecision,
    ExecutionResult,
};
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

    fn is_synchronous(&self) -> bool { true }

    fn consumer_name(&self) -> &'static str { "capability_executor" }

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
        let policy = evaluate_execution_policy(&exec);
        let ctx = ExecutionContext { workspace: self.workspace.clone(), emitter: emitter.clone(), trigger_id: trigger_id.clone() };
        let event_debug = format!("{:?}", event);
        if policy.decision == ExecutionPolicyDecision::Forbid {
            return EventOutcome::error(
                RuntimeEvent::ErrorOccurred(new_error_occurred(
                    "execution_policy_forbidden",
                    "capability_executor",
                    policy.reason,
                    "error",
                    serde_json::json!({
                        "risk": format!("{:?}", policy.risk),
                        "event": event_debug,
                    }),
                    None,
                )),
                file!(),
                line!(),
            );
        }
        // Spawn the execution in a separate thread so the consumer thread is not blocked
        // for the duration of the LLM/bash/IO call. A blocked consumer thread fills its
        // channel (1024), causing the bus dispatch loop to stall on blocking send() for
        // control events, freezing the entire runtime.
        std::thread::spawn(move || {
            if policy.decision == ExecutionPolicyDecision::Review {
                emitter.emit_child(
                    RuntimeEvent::Debug(canon_event::DebugEvent {
                        source: "capability_executor".to_string(),
                        kind: "execution_policy_review".to_string(),
                        payload: serde_json::json!({
                            "reason": policy.reason,
                            "risk": format!("{:?}", policy.risk),
                            "event": event_debug,
                        }),
                    }),
                    vec![trigger_id.clone()],
                    file!(),
                    line!(),
                );
            }
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
