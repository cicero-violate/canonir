use crate::hooks::{is_protected_control_event, HookChain, HookDecision};
use canon_event::{new_error_occurred, DebugEvent, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RuntimeEvent};
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

    fn emit_bus_debug(&self, parent_id: &EventId, kind: &str, payload: serde_json::Value) {
        if let Some(entry) = self.sync_consumers.first() {
            entry.emitter.emit_with_parents(
                RuntimeEvent::Debug(DebugEvent {
                    source: "event_bus".to_string(),
                    kind: kind.to_string(),
                    payload,
                }),
                vec![parent_id.clone()],
                file!(),
                line!(),
            );
        } else {
            eprintln!(
                "[event_bus] unable to emit debug kind={} without registered consumers",
                kind
            );
        }
    }

    fn emit_bus_error(&self, parent_id: &EventId, kind: &str, message: String, payload: serde_json::Value) {
        if let Some(entry) = self.sync_consumers.first() {
            entry.emitter.emit_with_parents(
                RuntimeEvent::ErrorOccurred(new_error_occurred(
                    kind,
                    "event_bus",
                    message,
                    "error",
                    payload,
                    None,
                )),
                vec![parent_id.clone()],
                file!(),
                line!(),
            );
        } else {
            eprintln!(
                "[event_bus] unable to emit error kind={} without registered consumers",
                kind
            );
        }
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
        let event_kind = canon_event::event_kind_str(&event).to_string();
        let hook_report = self.hooks.run_pre(&event);
        match &hook_report.decision {
            HookDecision::Allow => {}
            HookDecision::Deny { reason } => {
                let protected_control = is_protected_control_event(&event);
                self.emit_bus_debug(
                    &event_id,
                    "hook_pre_decision",
                    serde_json::json!({
                        "event_id": event_id.to_string(),
                        "event_kind": event_kind,
                        "hook_name": hook_report.hook_name,
                        "decision": "deny",
                        "reason": reason,
                        "protected_control": protected_control,
                    }),
                );
                if protected_control {
                    self.emit_bus_error(
                        &event_id,
                        "hook_control_violation",
                        format!(
                            "hook {} requested deny for protected control event {}",
                            hook_report.hook_name.unwrap_or("unknown"),
                            canon_event::event_kind_str(&event)
                        ),
                        serde_json::json!({
                            "event_id": event_id.to_string(),
                            "event_kind": canon_event::event_kind_str(&event),
                            "hook_name": hook_report.hook_name,
                            "requested_decision": "deny",
                            "reason": reason,
                        }),
                    );
                    panic!(
                        "protected control hook violation: hook {} denied event {}",
                        hook_report.hook_name.unwrap_or("unknown"),
                        canon_event::event_kind_str(&event)
                    );
                }
            }
            HookDecision::Mutate { replacement } => {
                let protected_control = is_protected_control_event(&event);
                self.emit_bus_debug(
                    &event_id,
                    "hook_pre_decision",
                    serde_json::json!({
                        "event_id": event_id.to_string(),
                        "event_kind": canon_event::event_kind_str(&event),
                        "hook_name": hook_report.hook_name,
                        "decision": "mutate",
                        "replacement_kind": canon_event::event_kind_str(replacement),
                        "protected_control": protected_control,
                    }),
                );
                if protected_control {
                    self.emit_bus_error(
                        &event_id,
                        "hook_control_violation",
                        format!(
                            "hook {} requested mutation for protected control event {}",
                            hook_report.hook_name.unwrap_or("unknown"),
                            canon_event::event_kind_str(&event)
                        ),
                        serde_json::json!({
                            "event_id": event_id.to_string(),
                            "event_kind": canon_event::event_kind_str(&event),
                            "hook_name": hook_report.hook_name,
                            "requested_decision": "mutate",
                            "replacement_kind": canon_event::event_kind_str(replacement),
                        }),
                    );
                    panic!(
                        "protected control hook violation: hook {} mutated event {}",
                        hook_report.hook_name.unwrap_or("unknown"),
                        canon_event::event_kind_str(&event)
                    );
                }
            }
        }
        let base_event = event;

        let attempted = self.sync_consumers.len();
        let mut delivered = 0usize;
        let mut receipts = Vec::with_capacity(attempted);

        for consumer in &self.sync_consumers {
            if let Ok(mut locked) = consumer.consumer.lock() {
                let outcome = locked.on_event(&base_event, event_id.clone());
                delivered = delivered.saturating_add(1);
                let outcome_kind = match &outcome {
                    EventOutcome::NoOp(reason) => format!("noop:{reason}"),
                    EventOutcome::Error { .. } => "error".to_string(),
                };
                receipts.push(serde_json::json!({
                    "event_id": event_id.to_string(),
                    "consumer": consumer.name,
                    "status": "delivered",
                    "outcome": outcome_kind,
                }));
                self.hooks.run_post(&base_event, &outcome);

                match outcome {
                    EventOutcome::NoOp(_) => {}
                    EventOutcome::Error { event, file, line } => {
                        consumer.emitter.emit_with_parents(event, vec![event_id.clone()], file, line);
                    }
                }
            } else {
                receipts.push(serde_json::json!({
                    "event_id": event_id.to_string(),
                    "consumer": consumer.name,
                    "status": "lock_failed",
                }));
                eprintln!(
                    "[canon-runtime] ERROR: failed to lock consumer name={} for event kind={}",
                    consumer.name,
                    canon_event::event_kind_str(&base_event)
                );
                self.emit_bus_error(
                    &event_id,
                    "dispatch_consumer_lock_failed",
                    format!(
                        "failed to lock consumer {} for event {}",
                        consumer.name,
                        canon_event::event_kind_str(&base_event)
                    ),
                    serde_json::json!({
                        "event_id": event_id.to_string(),
                        "event_kind": canon_event::event_kind_str(&base_event),
                        "consumer": consumer.name,
                        "attempted_consumers": attempted,
                        "delivered_consumers": delivered,
                    }),
                );
                panic!(
                    "dispatch consumer lock failure detected for event {} consumer {}",
                    canon_event::event_kind_str(&base_event),
                    consumer.name
                );
            }
        }

        if delivered != attempted {
            self.emit_bus_debug(
                &event_id,
                "dispatch_delivery_gap",
                serde_json::json!({
                    "event_id": event_id.to_string(),
                    "event_kind": canon_event::event_kind_str(&base_event),
                    "attempted_consumers": attempted,
                    "delivered_consumers": delivered,
                    "receipts": receipts,
                }),
            );
            panic!(
                "dispatch delivery gap detected for event {}: delivered {} of {} consumers",
                canon_event::event_kind_str(&base_event),
                delivered,
                attempted
            );
        }

        delivered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{HookChain, HookDecision, PreHook};
    use canon_event::{ErrorOccurred, EventEmitter, EventFilter, RuntimeEvent, Tick};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct CollectingEmitter {
        emitted: Mutex<Vec<RuntimeEvent>>,
    }

    impl CollectingEmitter {
        fn take(&self) -> Vec<RuntimeEvent> {
            std::mem::take(&mut *self.emitted.lock().unwrap())
        }
    }

    impl EventEmitter for CollectingEmitter {
        fn emit_with_parents(&self, event: RuntimeEvent, _parents: Vec<EventId>, _file: &'static str, _line: u32) {
            self.emitted.lock().unwrap().push(event);
        }

        fn emit_located(&self, event: RuntimeEvent, _file: &'static str, _line: u32) {
            self.emitted.lock().unwrap().push(event);
        }
    }

    struct CountingConsumer {
        hits: Arc<AtomicUsize>,
    }

    impl EventConsumer for CountingConsumer {
        fn filter(&self) -> EventFilter {
            EventFilter::All
        }

        fn on_event(&mut self, _event: &RuntimeEvent, _trigger_id: EventId) -> EventOutcome {
            self.hits.fetch_add(1, Ordering::SeqCst);
            EventOutcome::NoOp("counting_consumer_observed")
        }

        fn consumer_name(&self) -> &'static str {
            "counting_consumer"
        }
    }

    struct DenyTickHook;

    struct MutateTickHook;

    impl PreHook for DenyTickHook {
        fn name(&self) -> &'static str {
            "deny_tick_hook"
        }

        fn on_pre(&self, event: &RuntimeEvent) -> HookDecision {
            if matches!(event, RuntimeEvent::Tick(_)) {
                HookDecision::Deny {
                    reason: "deny_tick_for_test".to_string(),
                }
            } else {
                HookDecision::Allow
            }
        }
    }

    impl PreHook for MutateTickHook {
        fn name(&self) -> &'static str {
            "mutate_tick_hook"
        }

        fn on_pre(&self, event: &RuntimeEvent) -> HookDecision {
            if matches!(event, RuntimeEvent::Tick(_)) {
                HookDecision::Mutate {
                    replacement: RuntimeEvent::Debug(DebugEvent {
                        source: "mutate_tick_hook".to_string(),
                        kind: "mutated_tick".to_string(),
                        payload: serde_json::json!({"mutated": true}),
                    }),
                }
            } else {
                HookDecision::Allow
            }
        }
    }

    #[test]
    fn dispatch_fails_closed_on_consumer_lock_failure() {
        let collector = Arc::new(CollectingEmitter::default());
        let emitter: EventEmitterHandle = collector.clone();
        let hits = Arc::new(AtomicUsize::new(0));

        let mut bus = EventBus::new(16, Arc::new(HookChain::new()));
        bus.register(
            "healthy".to_string(),
            Box::new(CountingConsumer { hits: hits.clone() }),
            emitter.clone(),
        );

        let poisoned_consumer = Arc::new(Mutex::new(Box::new(CountingConsumer {
            hits: Arc::new(AtomicUsize::new(0)),
        }) as Box<dyn EventConsumer>));
        {
            let poisoned_consumer_for_thread = poisoned_consumer.clone();
            let _ = std::thread::spawn(move || {
                let _guard = poisoned_consumer_for_thread.lock().unwrap();
                panic!("poison consumer lock for delivery-gap audit test");
            })
            .join();
        }
        let poisoned_consumer = match Arc::try_unwrap(poisoned_consumer) {
            Ok(mutex) => mutex,
            Err(_) => panic!("expected unique poisoned consumer mutex"),
        };
        assert!(poisoned_consumer.lock().is_err(), "expected poisoned consumer mutex");

        bus.sync_consumers.push(SyncConsumerEntry {
            name: "poisoned".to_string(),
            filter: EventFilter::All,
            consumer: poisoned_consumer,
            emitter: emitter.clone(),
        });

        let dispatch = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bus.dispatch(
                RuntimeEvent::Debug(DebugEvent {
                    source: "bus_test".to_string(),
                    kind: "delivery_gap_probe".to_string(),
                    payload: serde_json::json!({"probe": true}),
                }),
                EventId::new("dispatch-gap-test".to_string()),
            )
        }));

        assert!(dispatch.is_err(), "consumer lock failure must halt dispatch");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "healthy consumer should still observe the event");

        let emitted = collector.take();
        assert!(
            emitted.iter().any(|event| matches!(
                event,
                RuntimeEvent::ErrorOccurred(ErrorOccurred { kind, .. }) if kind == "dispatch_consumer_lock_failed"
            )),
            "expected dispatch_consumer_lock_failed audit event"
        );
    }

    #[test]
    fn dispatch_emits_hook_audit_for_protected_control_event() {
        let collector = Arc::new(CollectingEmitter::default());
        let emitter: EventEmitterHandle = collector.clone();
        let hits = Arc::new(AtomicUsize::new(0));

        let mut hooks = HookChain::new();
        hooks.add_pre(Box::new(DenyTickHook));
        let mut bus = EventBus::new(16, Arc::new(hooks));
        bus.register(
            "healthy".to_string(),
            Box::new(CountingConsumer { hits: hits.clone() }),
            emitter.clone(),
        );

        let dispatch = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bus.dispatch(
                RuntimeEvent::Tick(Tick {
                    tick: 1,
                    emitted: true,
                }),
                EventId::new("hook-control-test".to_string()),
            )
        }));

        assert!(dispatch.is_err(), "protected control deny must halt dispatch");
        assert_eq!(hits.load(Ordering::SeqCst), 0, "consumer must not receive denied protected control event");

        let emitted = collector.take();
        assert!(
            emitted.iter().any(|event| matches!(
                event,
                RuntimeEvent::Debug(DebugEvent { kind, payload, .. })
                    if kind == "hook_pre_decision"
                        && payload.get("decision") == Some(&serde_json::json!("deny"))
                        && payload.get("protected_control") == Some(&serde_json::json!(true))
            )),
            "expected hook_pre_decision audit event for denied protected control event"
        );
        assert!(
            emitted.iter().any(|event| matches!(
                event,
                RuntimeEvent::ErrorOccurred(ErrorOccurred { kind, .. }) if kind == "hook_control_violation"
            )),
            "expected hook_control_violation error event"
        );
    }

    #[test]
    fn dispatch_emits_hook_audit_for_mutated_protected_control_event() {
        let collector = Arc::new(CollectingEmitter::default());
        let emitter: EventEmitterHandle = collector.clone();
        let hits = Arc::new(AtomicUsize::new(0));

        let mut hooks = HookChain::new();
        hooks.add_pre(Box::new(MutateTickHook));
        let mut bus = EventBus::new(16, Arc::new(hooks));
        bus.register(
            "healthy".to_string(),
            Box::new(CountingConsumer { hits: hits.clone() }),
            emitter.clone(),
        );

        let dispatch = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bus.dispatch(
                RuntimeEvent::Tick(Tick {
                    tick: 1,
                    emitted: true,
                }),
                EventId::new("hook-control-mutate-test".to_string()),
            )
        }));

        assert!(dispatch.is_err(), "protected control mutate must halt dispatch");
        assert_eq!(hits.load(Ordering::SeqCst), 0, "consumer must not receive mutated protected control event");

        let emitted = collector.take();
        assert!(
            emitted.iter().any(|event| matches!(
                event,
                RuntimeEvent::Debug(DebugEvent { kind, payload, .. })
                    if kind == "hook_pre_decision"
                        && payload.get("decision") == Some(&serde_json::json!("mutate"))
                        && payload.get("protected_control") == Some(&serde_json::json!(true))
            )),
            "expected hook_pre_decision audit event for mutated protected control event"
        );
        assert!(
            emitted.iter().any(|event| matches!(
                event,
                RuntimeEvent::ErrorOccurred(ErrorOccurred { kind, .. }) if kind == "hook_control_violation"
            )),
            "expected hook_control_violation error event for mutated protected control event"
        );
    }
}
