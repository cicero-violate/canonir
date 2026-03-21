use anyhow::anyhow;
use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityRegistry};
use canon_event::{new_error_occurred, CanonEvent, EventConsumer, EventEmitterHandle, EventFilter};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct CapabilityExecutor {
    registry: Arc<Mutex<CapabilityRegistry>>,
    workspace: PathBuf,
    emitter: Option<EventEmitterHandle>,
}

impl CapabilityExecutor {
    pub fn new(registry: Arc<Mutex<CapabilityRegistry>>, workspace: PathBuf) -> Self {
        Self { registry, workspace, emitter: None }
    }
}

impl EventConsumer for CapabilityExecutor {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn on_event(&mut self, event: &CanonEvent) {
        let is_cap_event = matches!(event, CanonEvent::Edit(_) | CanonEvent::Cargo(_) | CanonEvent::File(_) | CanonEvent::Bash(_) | CanonEvent::Llm(_) | CanonEvent::Analysis(_));
        if !is_cap_event {
            return;
        }
        let ctx = CapabilityExecutionContext { workspace: self.workspace.clone(), event: event.clone(), emitter: self.emitter.clone() };
        let result = match self.registry.lock() {
            Ok(registry) => registry.route(ctx),
            Err(err) => Err(anyhow!("capability registry lock poisoned: {err}")),
        };

        let outcome = match result {
            Ok(result) => result,
            Err(err) => {
                let error_event = CanonEvent::ErrorOccurred(new_error_occurred(
                    "capability_execution",
                    "capability_executor",
                    err.to_string(),
                    "error",
                    serde_json::json!({ "event": format!("{:?}", event) }),
                    None,
                ));
                CapabilityExecutionResult::Emit(error_event)
            }
        };

        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        match outcome {
            CapabilityExecutionResult::Emit(event) => {
                emitter.emit(event);
            }
            CapabilityExecutionResult::EmitMany(events) => {
                for event in events {
                    emitter.emit(event);
                }
            }
            CapabilityExecutionResult::Deferred | CapabilityExecutionResult::NoOp => {}
        }
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }
}
