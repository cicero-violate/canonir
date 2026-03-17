use canon_event::emit_debug::{error, info};
use canon_event::{EventMask, EventConsumer, EventEmitterHandle, CanonEvent, EventFilter};
use crossbeam_channel::{bounded, Sender};
use serde_json::json;
use std::thread;

#[derive(Clone)]
pub struct EventMessage {
    pub event: CanonEvent,
}

pub struct ConsumerEntry {
    pub name: String,
    pub filter: EventFilter,
    pub sender: Sender<EventMessage>,
}

pub struct EventBus {
    consumers: Vec<ConsumerEntry>,
    queue_size: usize,
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
        if let Err(err) = thread::Builder::new()
            .name(thread_name.clone())
            .spawn(move || {
                let mut consumer = consumer;
                for msg in rx.iter() {
                    consumer.on_event(&msg.event);
                }
            })
        {
            error(
                "event_bus",
                "consumer_spawn_failed",
                json!({ "name": name, "error": err.to_string() }),
            );
        }
        self.consumers.push(ConsumerEntry { name, filter, sender: tx });
    }

    pub fn dispatch(&self, event: CanonEvent) {
        for consumer in &self.consumers {
            match consumer.filter {
                EventFilter::All => {}
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
                EventFilter::Kernel(mask) => {
                    let CanonEvent::Kernel { delta, .. } = &event else {
                        continue;
                    };
                    let event_mask = EventMask::for_event(&delta.event);
                    if !mask.contains(event_mask) {
                        continue;
                    }
                }
            }
            if let Err(err) = consumer.sender.try_send(EventMessage { event: event.clone() }) {
                error(
                    "event_bus",
                    "dispatch_dropped",
                    json!({ "name": consumer.name, "error": err.to_string() }),
                );
            }
        }
    }

    pub fn log_registry(&self) {
        let names: Vec<String> = self.consumers.iter().map(|c| c.name.clone()).collect();
        info(
            "event_bus",
            "registry_ready",
            json!({ "count": names.len(), "consumers": names }),
        );
    }
}
