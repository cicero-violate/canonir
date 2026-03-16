use anyhow::anyhow;
use canon_capability::{CapabilityContext, CapabilityRegistry, CapabilityResult};
use canon_event::emit_debug::error;
use canon_event::{
    CapabilityFailed, RuntimeConsumer, RuntimeEmitterHandle, RuntimeEvent, RuntimeEventFilter,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct CapabilityExecutor {
    registry: Arc<Mutex<CapabilityRegistry>>,
    workspace: PathBuf,
    emitter: Option<RuntimeEmitterHandle>,
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

impl RuntimeConsumer for CapabilityExecutor {
    fn filter(&self) -> RuntimeEventFilter {
        RuntimeEventFilter::CapabilityOnly
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        let RuntimeEvent::CapabilityRequested(request) = event else {
            return;
        };
        if request.name == "llm.call" {
            return;
        }
        let ctx = CapabilityContext {
            workspace: self.workspace.clone(),
            event: RuntimeEvent::CapabilityRequested(request.clone()),
        };
        let result = match self.registry.lock() {
            Ok(registry) => registry.execute(&request.name, ctx),
            Err(err) => Err(anyhow!("capability registry lock poisoned: {err}")),
        };

        let outcome = match result {
            Ok(result) => result,
            Err(err) => {
                error(
                    "capability_executor",
                    "capability_failed",
                    serde_json::json!({
                        "name": request.name,
                        "request_id": request.request_id,
                        "error": err.to_string()
                    }),
                );
                CapabilityResult::Emit(RuntimeEvent::CapabilityFailed(CapabilityFailed {
                    request_id: request.request_id.clone(),
                    name: request.name.clone(),
                    error: err.to_string(),
                }))
            }
        };

        let Some(emitter) = self.emitter.as_ref() else {
            return;
        };
        match outcome {
            CapabilityResult::Emit(event) => {
                emitter.emit(event);
            }
            CapabilityResult::EmitMany(events) => {
                for event in events {
                    emitter.emit(event);
                }
            }
            CapabilityResult::NoOp => {}
        }
    }

    fn set_emitter(&mut self, emitter: RuntimeEmitterHandle) {
        self.emitter = Some(emitter);
    }
}
