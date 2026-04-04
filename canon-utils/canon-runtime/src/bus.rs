use crate::hooks::{hook_denied_event, HookChain, HookDecision};
use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RuntimeEvent};
use std::sync::{Arc, Mutex};

pub struct SyncConsumerEntry {
    pub name: String,
    pub filter: EventFilter,
    pub consumer: Mutex<Box<dyn EventConsumer>>,
    pub emitter: EventEmitterHandle,
}

pub struct EventBus {
    pub sync_consumers: Vec<SyncConsumerEntry>,
    hooks: Arc<HookChain>,
}

impl EventBus {
    pub fn new(_queue_size: usize, hooks: Arc<HookChain>) -> Self {
        Self { sync_consumers: Vec::new(), hooks }
    }

    pub fn set_hooks(&mut self, hooks: Arc<HookChain>) {
        self.hooks = hooks;
    }

    pub fn log_registry(&self) {
        // minimal stub for compatibility
        eprintln!("[EventBus] registered_consumers={}", self.sync_consumers.len());
    }

    /// Expose number of registered synchronous consumers
    pub fn sync_consumers_len(&self) -> usize {
        self.sync_consumers.len()
    }

    pub fn register(&mut self, name: String, mut consumer: Box<dyn EventConsumer>, emitter: EventEmitterHandle) {
        consumer.set_emitter(emitter.clone());
        let filter = consumer.filter();
        self.sync_consumers.push(SyncConsumerEntry {
            name,
            filter,
            consumer: Mutex::new(consumer),
            emitter,
        });
        println!("[BUS REGISTER TRACE STDOUT] bus_ptr={:p} after_push_len={}", self, self.sync_consumers.len());
    }

    // FIX: restore async registration path (map to sync so consumers are not lost)
    pub fn register_async(&mut self, name: String, consumer: Box<dyn EventConsumer>, emitter: EventEmitterHandle) {
        self.register(name, consumer, emitter);
    }

    pub fn dispatch(&self, event: RuntimeEvent, event_id: EventId) -> usize {
        eprintln!("[BUS DISPATCH TRACE] bus_ptr={:p} sync_consumers_len={} event={}", self, self.sync_consumers.len(), canon_event::event_kind_str(&event));
        let base_event = match self.hooks.run_pre(&event) {
            HookDecision::Allow => event,
            HookDecision::Mutate { replacement } => replacement,
            HookDecision::Deny { reason } => {
                self.hooks.run_post(&event, &EventOutcome::error(hook_denied_event(&reason), file!(), line!()));
                return 0;
            }
        };

        let mut delivered = 0usize;

        for consumer in &self.sync_consumers {
            if let Ok(mut locked) = consumer.consumer.lock() {
                let outcome = locked.on_event(&base_event, event_id.clone());
                self.hooks.run_post(&base_event, &outcome);

                match outcome {
                    EventOutcome::NoOp(_) => {}
                    EventOutcome::Error { event, file, line } => {
                        consumer.emitter.emit_with_parents(event, vec![event_id.clone()], file, line);
                        delivered += 1;
                    }
                }
            }
        }

        delivered
    }
}
