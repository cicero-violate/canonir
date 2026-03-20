use canon_event::{EventMask, EventConsumer, EventEmitterHandle, CanonEvent, EventFilter, RustcEvent};
use crossbeam_channel::{bounded, Sender};
use std::thread;

#[derive(Clone)]
pub struct EventMessage {
    pub event: CanonEvent,
}

pub struct ConsumerEntry {
    pub filter: EventFilter,
    pub sender: Sender<EventMessage>,
}

pub struct EventBus {
    consumers: Vec<ConsumerEntry>,
    queue_size: usize,
}

fn is_error_event(event: &CanonEvent) -> bool {
    match event {
        CanonEvent::ErrorOccurred(_) => true,
        CanonEvent::CapabilityFailed(_) => true,
        CanonEvent::NodeFailed(_) => true,
        CanonEvent::LoopActed(payload) => !payload.success,
        CanonEvent::LoopVerified(payload) => !payload.passed,
        CanonEvent::LoopRewarded(payload) => payload.halt,
        CanonEvent::Code(canon_event::Code { delta, .. }) => matches!(
            delta.event,
            RustcEvent::PanicCaptured(_) | RustcEvent::InvariantViolation(_)
        ),
        _ => false,
    }
}

impl EventBus {
    pub fn new(queue_size: usize) -> Self {
        Self {
            consumers: Vec::new(),
            queue_size: queue_size.max(1),
        }
    }

    pub fn register(
        &mut self,
        name: String,
        mut consumer: Box<dyn EventConsumer>,
        emitter: EventEmitterHandle,
    ) {
        consumer.set_emitter(emitter);
        let filter = consumer.filter();
        let (tx, rx) = bounded::<EventMessage>(self.queue_size);
        let thread_name = format!("event_consumer_{name}");
        let _ = thread::Builder::new()
            .name(thread_name.clone())
            .spawn(move || {
                let mut consumer = consumer;
                for msg in rx.iter() {
                    consumer.on_event(&msg.event);
                }
            });
        self.consumers.push(ConsumerEntry { filter, sender: tx });
    }

    pub fn dispatch(&self, event: CanonEvent) {
        for consumer in &self.consumers {
            match consumer.filter {
                EventFilter::All => {}
                EventFilter::ErrorOnly => {
                    if !is_error_event(&event) {
                        continue;
                    }
                }
                EventFilter::EditOnly => {
                    if !matches!(event, CanonEvent::Edit(_)) {
                        continue;
                    }
                }
                EventFilter::CapabilityOnly => {
                    if !matches!(
                        event,
                        CanonEvent::CapabilityRequested(_)
                            | CanonEvent::CapabilityCompleted(_)
                            | CanonEvent::CapabilityFailed(_)
                    ) {
                        continue;
                    }
                }
                EventFilter::Code(mask) => {
                    let CanonEvent::Code(canon_event::Code { delta, .. }) = &event else {
                        continue;
                    };
                    let event_mask = EventMask::for_event(&delta.event);
                    if !mask.contains(event_mask) {
                        continue;
                    }
                }
            }
            let _ = consumer.sender.try_send(EventMessage { event: event.clone() });
        }
    }

    pub fn log_registry(&self) {
    }
}
