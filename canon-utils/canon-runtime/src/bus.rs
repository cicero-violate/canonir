use crate::hooks::{hook_denied_event, HookChain, HookDecision};
use canon_event::{EventConsumer, EventEmitterHandle, EventFilter, EventMask, EventOutcome, RuntimeEvent, RustcEvent};
use crossbeam_channel::{bounded, Sender};
use std::sync::Arc;
use std::thread;

#[derive(Clone)]
pub struct EventMessage {
    pub event: RuntimeEvent,
}

pub struct ConsumerEntry {
    pub filter: EventFilter,
    pub sender: Sender<EventMessage>,
}

pub struct EventBus {
    consumers: Vec<ConsumerEntry>,
    queue_size: usize,
    hooks: Arc<HookChain>,
}

fn is_error_event(event: &RuntimeEvent) -> bool {
    match event {
        RuntimeEvent::ErrorOccurred(_) => true,
        RuntimeEvent::CapabilityFailed(_) => true,
        RuntimeEvent::NodeFailed(_) => true,
        RuntimeEvent::LoopActed(payload) => !payload.success,
        RuntimeEvent::LoopVerified(payload) => !payload.passed,
        RuntimeEvent::LoopRewarded(payload) => payload.halt,
        RuntimeEvent::Code(canon_event::Code { delta, .. }) => matches!(delta.event, RustcEvent::PanicCaptured(_) | RustcEvent::InvariantViolation(_)),
        _ => false,
    }
}

fn is_control_event(event: &RuntimeEvent) -> bool {
    matches!(
        event,
        RuntimeEvent::Tick(_)
            | RuntimeEvent::PromptLoaded(_)
            | RuntimeEvent::CapabilityCompleted(_)
            | RuntimeEvent::CapabilityFailed(_)
            | RuntimeEvent::LoopObserved(_)
            | RuntimeEvent::LoopPlanned(_)
            | RuntimeEvent::LoopActed(_)
            | RuntimeEvent::LoopVerified(_)
            | RuntimeEvent::LoopRewarded(_)
            | RuntimeEvent::Debug(_)
    )
}

impl EventBus {
    pub fn new(queue_size: usize, hooks: Arc<HookChain>) -> Self {
        Self { consumers: Vec::new(), queue_size: queue_size.max(1), hooks }
    }

    pub fn set_hooks(&mut self, hooks: Arc<HookChain>) {
        self.hooks = hooks;
    }

    pub fn register(&mut self, name: String, mut consumer: Box<dyn EventConsumer>, emitter: EventEmitterHandle) {
        let emitter_for_loop = emitter.clone();
        consumer.set_emitter(emitter);
        let hooks = self.hooks.clone();
        let filter = consumer.filter();
        let (tx, rx) = bounded::<EventMessage>(self.queue_size);
        let thread_name = format!("event_consumer_{name}");
        let _ = thread::Builder::new().name(thread_name.clone()).spawn(move || {
            let mut consumer = consumer;
            for msg in rx.iter() {
                let outcome = consumer.on_event(&msg.event);
                hooks.run_post(&msg.event, &outcome);
                match outcome {
                    EventOutcome::Emit(e) => emitter_for_loop.emit(e),
                    EventOutcome::EmitMany(list) => {
                        for e in list {
                            emitter_for_loop.emit(e);
                        }
                    }
                    EventOutcome::NoOp(_) => {}
                    EventOutcome::Error(e) => emitter_for_loop.emit(e),
                }
            }
        });
        self.consumers.push(ConsumerEntry { filter, sender: tx });
    }

    pub fn dispatch(&self, event: RuntimeEvent) {
        let base_event = match self.hooks.run_pre(&event) {
            HookDecision::Allow => event,
            HookDecision::Mutate { replacement } => replacement,
            HookDecision::Deny { reason } => {
                self.hooks.run_post(&event, &EventOutcome::Error(hook_denied_event(&reason)));
                return;
            }
        };
        let reliable = is_control_event(&base_event);
        for consumer in &self.consumers {
            match consumer.filter {
                EventFilter::All => {}
                EventFilter::ErrorOnly => {
                    if !is_error_event(&base_event) {
                        continue;
                    }
                }
                EventFilter::EditOnly => {
                    if !matches!(base_event, RuntimeEvent::Edit(_)) {
                        continue;
                    }
                }
                EventFilter::CapabilityOnly => {
                    if !matches!(base_event, RuntimeEvent::CapabilityCompleted(_) | RuntimeEvent::CapabilityFailed(_)) {
                        continue;
                    }
                }
                EventFilter::Code(mask) => {
                    let RuntimeEvent::Code(canon_event::Code { delta, .. }) = &base_event else {
                        continue;
                    };
                    let event_mask = EventMask::for_event(&delta.event);
                    if !mask.contains(event_mask) {
                        continue;
                    }
                }
            }
            if reliable {
                let _ = consumer.sender.send(EventMessage { event: base_event.clone() });
            } else {
                let _ = consumer.sender.try_send(EventMessage { event: base_event.clone() });
            }
        }
    }

    pub fn log_registry(&self) {}
}
