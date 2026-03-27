use crate::hooks::{hook_denied_event, HookChain, HookDecision};
use canon_invariant::{decision_trace_payload, invariant_violation_delta, invariant_violation_state};
use canon_event::{Code, DebugEvent, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventMask, EventOutcome, RuntimeEvent, RustcEvent};
use crossbeam_channel::{bounded, Sender};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

#[derive(Clone)]
pub struct EventMessage {
    pub event: RuntimeEvent,
    pub event_id: EventId,
}

pub struct ConsumerEntry {
    pub filter: EventFilter,
    pub sender: Sender<EventMessage>,
}

pub struct SyncConsumerEntry {
    pub name: String,
    pub filter: EventFilter,
    pub consumer: Mutex<Box<dyn EventConsumer>>,
    pub emitter: EventEmitterHandle,
}

pub struct EventBus {
    consumers: Vec<ConsumerEntry>,
    sync_consumers: Vec<SyncConsumerEntry>,
    queue_size: usize,
    hooks: Arc<HookChain>,
}

fn is_error_event(event: &RuntimeEvent) -> bool {
    match event {
        RuntimeEvent::ErrorOccurred(_) => true,
        RuntimeEvent::CapabilityFailed(_) => true,
        RuntimeEvent::NodeFailed(_) => true,
        RuntimeEvent::LoopActed(payload) => !payload.success,
        RuntimeEvent::LoopVerified(_) => false,
        RuntimeEvent::VerifierPolicyUpdated(payload) => payload.actionable_failure,
        RuntimeEvent::LoopRewarded(payload) => payload.halt,
        RuntimeEvent::Code(canon_event::Code { delta, .. }) => matches!(delta.event, RustcEvent::PanicCaptured(_) | RustcEvent::InvariantViolation(_)),
        _ => false,
    }
}

fn is_control_event(event: &RuntimeEvent) -> bool {
    matches!(
        event,
        RuntimeEvent::RouteSelected(_)
            | RuntimeEvent::LoopObserved(_)
            | RuntimeEvent::PlanningCompleted(_)
            | RuntimeEvent::LoopActed(_)
            | RuntimeEvent::LoopVerified(_)
            | RuntimeEvent::VerifierPolicyUpdated(_)
            | RuntimeEvent::LoopRewarded(_)
    )
}

fn should_count_as_noop_violation(consumer: &str, reason: &str) -> bool {
    match consumer {
        // Only the control-owning executors should contribute to noop-based invariant failures.
        "route_executor" => !matches!(
            reason,
            "route_executor_idle_dispatch"
                | "route_executor_plan_dispatch"
                | "route_executor_planned_to_act"
                | "route_executor_continue_act"
                | "route_executor_bootstrap_refresh"
                | "route_executor_batch_settled"
                | "route_executor_done_verify"
                | "route_executor_missing_observed_context"
                | "route_executor_completion"
                | "route_executor_failure_reroute"
                | "route_executor_unrelated_completion"
                | "route_executor_unrelated_failure"
                | "route_executor_noop"
        ),
        "loop_stage_executor" => !matches!(
            reason,
            "loop_stage_not_stage_event"
                | "loop_stage_async"
                | "loop_stage_halted"
                | "loop_stage_no_emitter"
        ),
        _ => false,
    }
}

impl EventBus {
    pub fn new(queue_size: usize, hooks: Arc<HookChain>) -> Self {
        Self { consumers: Vec::new(), sync_consumers: Vec::new(), queue_size: queue_size.max(1), hooks }
    }

    pub fn set_hooks(&mut self, hooks: Arc<HookChain>) {
        self.hooks = hooks;
    }

    pub fn register(&mut self, name: String, mut consumer: Box<dyn EventConsumer>, emitter: EventEmitterHandle) {
        if consumer.is_synchronous() {
            let consumer_name = consumer.consumer_name().to_string();
            consumer.set_emitter(emitter.clone());
            let filter = consumer.filter();
            self.sync_consumers.push(SyncConsumerEntry { name: consumer_name, filter, consumer: Mutex::new(consumer), emitter });
            return;
        }
        let consumer_name = consumer.consumer_name().to_string();
        let emitter_for_loop = emitter.clone();
        consumer.set_emitter(emitter);
        let hooks = self.hooks.clone();
        let filter = consumer.filter();
        let (tx, rx) = bounded::<EventMessage>(self.queue_size);
        let thread_name = format!("event_consumer_{name}");
        let _ = thread::Builder::new().name(thread_name.clone()).spawn(move || {
            let mut consumer = consumer;
            for msg in rx.iter() {
                let parent_id = msg.event_id.clone();
                let outcome = consumer.on_event(&msg.event, parent_id.clone());
                hooks.run_post(&msg.event, &outcome);
                match outcome {
                    EventOutcome::Emit { event, file, line } => {
                        emitter_for_loop.emit_with_parents(event, vec![parent_id], file, line);
                    }
                    EventOutcome::EmitMany { events, file, line } => {
                        for event in events {
                            emitter_for_loop.emit_with_parents(event, vec![parent_id.clone()], file, line);
                        }
                    }
                    EventOutcome::NoOp(reason) => {
                        if is_control_event(&msg.event) {
                            emitter_for_loop.emit_with_parents(
                                RuntimeEvent::Debug(DebugEvent {
                                    source: consumer_name.clone(),
                                    kind: "control_noop".to_string(),
                                    payload: decision_trace_payload(
                                        reason,
                                        serde_json::json!({
                                            "event_kind": canon_event::event_kind_str(&msg.event),
                                            "event_id": parent_id.to_string(),
                                        }),
                                    ),
                                }),
                                vec![parent_id],
                                file!(),
                                line!(),
                            );
                        }
                    }
                    EventOutcome::Error { event, file, line } => {
                        emitter_for_loop.emit_with_parents(event, vec![parent_id], file, line);
                    }
                }
            }
        });
        self.consumers.push(ConsumerEntry { filter, sender: tx });
    }

    /// Dispatch an event to all matching consumers. Returns the number of consumers that received it.
    pub fn dispatch(&self, event: RuntimeEvent, event_id: EventId) -> usize {
        let base_event = match self.hooks.run_pre(&event) {
            HookDecision::Allow => event,
            HookDecision::Mutate { replacement } => replacement,
            HookDecision::Deny { reason } => {
                self.hooks.run_post(&event, &EventOutcome::error(hook_denied_event(&reason), file!(), line!()));
                return 0;
            }
        };
        let reliable = is_control_event(&base_event);
        let mut delivered = 0usize;
        let mut noop_reasons: Vec<String> = Vec::new();
        for consumer in &self.sync_consumers {
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
            if let Ok(mut locked) = consumer.consumer.lock() {
                let outcome = locked.on_event(&base_event, event_id.clone());
                self.hooks.run_post(&base_event, &outcome);
                match outcome {
                    EventOutcome::Emit { event, file, line } => {
                        consumer.emitter.emit_with_parents(event, vec![event_id.clone()], file, line);
                    }
                    EventOutcome::EmitMany { events, file, line } => {
                        for event in events {
                            consumer.emitter.emit_with_parents(event, vec![event_id.clone()], file, line);
                        }
                    }
                    EventOutcome::NoOp(reason) => {
                        if is_control_event(&base_event) && should_count_as_noop_violation(&consumer.name, reason) {
                            noop_reasons.push(format!("{}:{}", consumer.name, reason));
                        }
                    }
                    EventOutcome::Error { event, file, line } => {
                        consumer.emitter.emit_with_parents(event, vec![event_id.clone()], file, line);
                    }
                }
                delivered += 1;
            }
        }
        if !noop_reasons.is_empty() && is_control_event(&base_event) {
            if let Some(first) = self.sync_consumers.first() {
                first.emitter.emit_with_parents(
                    RuntimeEvent::Code(Code {
                        delta: invariant_violation_delta(format!(
                            "noop_spam; parent={}; kind={}; reasons={}; count={}",
                            event_id,
                            canon_event::event_kind_str(&base_event),
                            noop_reasons.join(","),
                            noop_reasons.len()
                        )),
                        state: invariant_violation_state(),
                    }),
                    vec![event_id.clone()],
                    file!(),
                    line!(),
                );
            }
        }
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
            let sent = if reliable {
                consumer.sender.send(EventMessage { event: base_event.clone(), event_id: event_id.clone() }).is_ok()
            } else {
                consumer.sender.try_send(EventMessage { event: base_event.clone(), event_id: event_id.clone() }).is_ok()
            };
            if sent {
                delivered += 1;
            }
        }
        if delivered == 0 && is_control_event(&base_event) {
            if let Some(first) = self.sync_consumers.first() {
                first.emitter.emit_with_parents(
                    RuntimeEvent::Code(Code {
                        delta: invariant_violation_delta(format!(
                            "control event delivered to 0 consumers; kind={}; event_id={}",
                            canon_event::event_kind_str(&base_event),
                            event_id
                        )),
                        state: invariant_violation_state(),
                    }),
                    vec![event_id],
                    file!(),
                    line!(),
                );
            }
        }
        delivered
    }

    pub fn log_registry(&self) {}
}
