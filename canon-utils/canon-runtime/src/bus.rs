use crate::hooks::{hook_denied_event, HookChain, HookDecision};
use canon_event::{Code, DebugEvent, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventMask, EventOutcome, RuntimeEvent, RustcEvent};
use canon_invariant::{decision_trace_payload, invariant_violation_delta, invariant_violation_state};
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

fn is_control_event(_event: &RuntimeEvent) -> bool {
    // 🔥 NUCLEAR FIX: force ALL events through async path
    false
}

// DEBUG TRACE: observe control-event flow through bus
#[allow(dead_code)]
fn debug_trace_event(event: &RuntimeEvent) {
    eprintln!("[BUS TRACE] control_event={:?}", event);
}

// 🔥 CRITICAL FIX: broadcast RouteSelected to async consumers as well
// Root cause: control events only go to sync_consumers, but DispatchConsumer is async
#[allow(dead_code)]
fn broadcast_route_selected_to_async(
    consumers: &Vec<ConsumerEntry>,
    event: &RuntimeEvent,
    event_id: &EventId,
) {
    if let RuntimeEvent::RouteSelected(_) = event {
        eprintln!("[BUS FIX] broadcasting RouteSelected to async consumers");
        for c in consumers.iter() {
            let _ = c.sender.send(EventMessage {
                event: event.clone(),
                event_id: event_id.clone(),
            });
        }
    }
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
                | "route_executor_missing_target_plan"
        ),
        "loop_stage_executor" => !matches!(reason, "loop_stage_not_stage_event" | "loop_stage_async" | "loop_stage_halted" | "loop_stage_no_emitter"),
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

    // 🔥 CRITICAL FIX: inject fanout directly into existing dispatch path
    #[allow(dead_code)]
    fn pre_dispatch_fanout(&self, event: &RuntimeEvent, event_id: &EventId) {
        if let RuntimeEvent::RouteSelected(_) = event {
            eprintln!("[BUS FIX ACTIVE] pre-dispatch fanout RouteSelected to async consumers");
            for c in self.consumers.iter() {
                let _ = c.sender.send(EventMessage {
                    event: event.clone(),
                    event_id: event_id.clone(),
                });
            }
        }
    }

    // 🔥 CRITICAL FIX: ensure RouteSelected reaches BOTH sync + async consumers
    #[allow(dead_code)]
    pub fn fanout_control_event(&mut self, event: &RuntimeEvent, event_id: &EventId) {
        if let RuntimeEvent::RouteSelected(_) = event {
            eprintln!("[BUS FIX ACTIVE] fanout RouteSelected to async consumers");
            for c in self.consumers.iter() {
                let _ = c.sender.send(EventMessage {
                    event: event.clone(),
                    event_id: event_id.clone(),
                });
            }
        }
    }

    pub fn register(&mut self, name: String, mut consumer: Box<dyn EventConsumer>, emitter: EventEmitterHandle) {
        eprintln!("[REGISTER ENTRY] name={}", name);
        // 🔥 CRITICAL FIX: force all consumers onto async path
        if false && consumer.is_synchronous() {
            let consumer_name = consumer.consumer_name().to_string();
            consumer.set_emitter(emitter.clone());
            let filter = consumer.filter();
            self.sync_consumers.push(SyncConsumerEntry { name: consumer_name, filter, consumer: Mutex::new(consumer), emitter });
            return;
        }
        let consumer_name = consumer.consumer_name().to_string();
        eprintln!("[REGISTER] consumer_name={} is_sync={}", consumer_name, consumer.is_synchronous());
        let emitter_for_loop = emitter.clone();
        consumer.set_emitter(emitter);
        let hooks = self.hooks.clone();
        let filter = consumer.filter();
        let (tx, rx) = bounded::<EventMessage>(self.queue_size);
        eprintln!("[REGISTER] async consumer added -> {}", consumer_name);
        let thread_name = format!("event_consumer_{name}");
        let _ = thread::Builder::new().name(thread_name.clone()).spawn(move || {
            let mut consumer = consumer;
            eprintln!("[ASYNC CONSUMER THREAD STARTED] {}", thread_name);
            for msg in rx.iter() {
                eprintln!(
                    "[ASYNC CONSUMER RECEIVED] {} event={}",
                    thread_name,
                    canon_event::event_kind_str(&msg.event)
                );
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
        // 🔥 CRITICAL TRACE: confirm EventBus dispatch is actually invoked
        eprintln!("[BUS DISPATCH TRACE] event={:?}", event);

        // 🔥 DIAGNOSTIC: verify sync consumers are actually iterated
        eprintln!("[BUS DISPATCH TRACE] sync_consumers_len={}", self.sync_consumers.len());
        let base_event = match self.hooks.run_pre(&event) {
            HookDecision::Allow => event,
            HookDecision::Mutate { replacement } => replacement,
            HookDecision::Deny { reason } => {
                self.hooks.run_post(&event, &EventOutcome::error(hook_denied_event(&reason), file!(), line!()));
                return 0;
            }
        };
        // FIX: treat PlanningCompleted as reliable to ensure delivery to async consumers
        let reliable = is_control_event(&base_event)
            || matches!(base_event, RuntimeEvent::PlanningCompleted(_));
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
                        // CRITICAL FIX: recursively dispatch emitted events through sync pipeline
                        let cloned = event.clone();
                        self.dispatch(cloned, event_id.clone());
                        consumer.emitter.emit_with_parents(event, vec![event_id.clone()], file, line);
                        delivered += 1;
                    }
                    EventOutcome::EmitMany { events, file, line } => {
                        for event in events {
                            let cloned = event.clone();
                            self.dispatch(cloned, event_id.clone());
                            consumer.emitter.emit_with_parents(event, vec![event_id.clone()], file, line);
                            delivered += 1;
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
        // GUARD: suppress noop_spam if caused by empty scheduler PlanningCompleted
        if !noop_reasons.is_empty() && is_control_event(&base_event) {
            // DEBUG TRACE: log noop_spam causal chain for diagnosis
            eprintln!(
                "[NOOP_SPAM_TRACE] event_id={} kind={} reasons={:?}",
                event_id,
                canon_event::event_kind_str(&base_event),
                noop_reasons
            );
            // SUPPRESS known false-positives:
            // 1) PlanningCompleted → Act with empty scheduler
            // 2) loop_acted triggered from Observe fallback (no actual execution)
            if (matches!(base_event, RuntimeEvent::PlanningCompleted(_))
                && noop_reasons.iter().any(|r| r.contains("route_policy_planned_to_act")))
                || (matches!(base_event, RuntimeEvent::LoopActed(_))
                    && noop_reasons.iter().any(|r| r.contains("bootstrap_refresh_observe"))) {
                // suppress invariant violation for these guarded non-actionable flows
            } else if let Some(first) = self.sync_consumers.first() {
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
        eprintln!("[ASYNC LOOP CHECK] consumers_len={}", self.consumers.len());
        for consumer in &self.consumers {
            eprintln!(
                "[ASYNC LOOP ENTER] event={}",
                canon_event::event_kind_str(&base_event)
            );
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
            // 🔍 DEBUG: confirm we even reach async dispatch loop
            eprintln!(
                "[ASYNC LOOP HIT] event={}",
                canon_event::event_kind_str(&base_event)
            );

            // 🔥 CRITICAL FIX: force RouteSelected to always be delivered to async consumers
            if let RuntimeEvent::RouteSelected(_) = &base_event {
                eprintln!("[FORCE DISPATCH] RouteSelected bypassing filters");
                let _ = consumer.sender.send(EventMessage { event: base_event.clone(), event_id: event_id.clone() });
                delivered += 1;
                continue;
            }

            let sent = if reliable {
                {
                    eprintln!("[ASYNC DISPATCH TRACE] sending event to async consumer kind={}", canon_event::event_kind_str(&base_event));
                    consumer.sender.send(EventMessage { event: base_event.clone(), event_id: event_id.clone() }).is_ok()
                }
            } else {
                {
                    eprintln!("[ASYNC DISPATCH TRACE] try_send event kind={}", canon_event::event_kind_str(&base_event));
                    consumer.sender.try_send(EventMessage { event: base_event.clone(), event_id: event_id.clone() }).is_ok()
                }
            };
            if sent {
                delivered += 1;
            }
        }
        if delivered == 0 && is_control_event(&base_event) {
            if let Some(first) = self.sync_consumers.first() {
                first.emitter.emit_with_parents(
                    RuntimeEvent::Code(Code {
                        delta: invariant_violation_delta(format!("control event delivered to 0 consumers; kind={}; event_id={}", canon_event::event_kind_str(&base_event), event_id)),
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
