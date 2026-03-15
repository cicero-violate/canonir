use canon_event::emit_debug::{info, warn};
use canon_event::{
    CapabilityRequested, NodeReady, RuntimeConsumer, RuntimeEmitterHandle, RuntimeEvent,
    RuntimeEventFilter,
};
use std::sync::{Arc, Mutex};

pub struct EventLoopConsumer {
    emitter: Arc<Mutex<Option<RuntimeEmitterHandle>>>,
}

impl EventLoopConsumer {
    pub fn new() -> Self {
        Self {
            emitter: Arc::new(Mutex::new(None)),
        }
    }
}

impl RuntimeConsumer for EventLoopConsumer {
    fn filter(&self) -> RuntimeEventFilter {
        RuntimeEventFilter::All
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        let RuntimeEvent::NodeReady(NodeReady { node_id, capability, request_id, args }) = event else {
            return;
        };
        let Some(emitter) = self.emitter.lock().ok().and_then(|slot| slot.clone()) else {
            return;
        };
        let request_id = if request_id.is_empty() {
            warn(
                "event_loop",
                "node_ready_missing_request_id",
                serde_json::json!({ "node_id": node_id, "capability": capability }),
            );
            return;
        } else {
            request_id.clone()
        };
        if args.is_null() {
            warn(
                "event_loop",
                "node_ready_missing_args",
                serde_json::json!({ "node_id": node_id, "capability": capability }),
            );
            return;
        }
        info(
            "event_loop",
            "capability_dispatch",
            serde_json::json!({ "node_id": node_id, "capability": capability, "request_id": request_id }),
        );
        emitter.emit(RuntimeEvent::CapabilityRequested(CapabilityRequested {
            request_id,
            name: capability.clone(),
            args: args.clone(),
        }));
    }

    fn set_emitter(&mut self, emitter: RuntimeEmitterHandle) {
        if let Ok(mut slot) = self.emitter.lock() {
            *slot = Some(emitter);
        }
    }
}
