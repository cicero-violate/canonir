use anyhow::anyhow;
use canon_capability::{CapabilityExecutionContext, CapabilityRegistry, CapabilityExecutionResult};
use canon_event::{CapabilityFailed, EventConsumer, EventEmitterHandle, CanonEvent, EventFilter};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct CapabilityExecutor {
    registry: Arc<Mutex<CapabilityRegistry>>,
    workspace: PathBuf,
    emitter: Option<EventEmitterHandle>,
}

impl CapabilityExecutor {
    pub fn new(registry: Arc<Mutex<CapabilityRegistry>>, workspace: PathBuf) -> Self {
        Self {
            registry,
            workspace,
            emitter: None,
        }
    }
}

impl EventConsumer for CapabilityExecutor {
    fn filter(&self) -> EventFilter {
        EventFilter::CapabilityOnly
    }

    fn on_event(&mut self, event: &CanonEvent) {
        let CanonEvent::CapabilityRequested(request) = event else {
            return;
        };
        let ctx = CapabilityExecutionContext {
            workspace: self.workspace.clone(),
            event: CanonEvent::CapabilityRequested(request.clone()),
            emitter: self.emitter.clone(),
        };
        let result = match self.registry.lock() {
            Ok(registry) => registry.execute(&request.name, ctx),
            Err(err) => Err(anyhow!("capability registry lock poisoned: {err}")),
        };

        let outcome = match result {
            Ok(result) => result,
            Err(err) => CapabilityExecutionResult::Emit(CanonEvent::CapabilityFailed(CapabilityFailed {
                request_id: request.request_id.clone(),
                name: request.name.clone(),
                error: err.to_string(),
            })),
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
