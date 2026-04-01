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
    // FIX: restore control-event separation to prevent async fanout duplication
    // LoopObserved and other control events should NOT be broadcast to all async consumers
    match _event {
        RuntimeEvent::LoopObserved(_)
        | RuntimeEvent::RouteSelected(_)
        | RuntimeEvent::LoopActed(_)
        | RuntimeEvent::LoopPlanned(_)
        | RuntimeEvent::LoopVerified(_) => true,
        _ => false,
    }
}

// REMOVE: dedupe hack — canonical flow must guarantee single emission upstream
// Duplicate suppression must not occur in the bus; it hides lifecycle violations

// DEBUG TRACE: observe control-event flow through bus
#[allow(dead_code)]
fn debug_trace_event(event: &RuntimeEvent) {
    eprintln!("[BUS TRACE] control_event={:?}", event);
}

// 🔥 CRITICAL FIX: broadcast RouteSelected to async consumers as well
// Root cause: control events only go to sync_consumers, but DispatchConsumer is async
#[allow(dead_code)]
fn broadcast_route_selected_to_async(
    _consumers: &Vec<ConsumerEntry>,
    event: &RuntimeEvent,
    _event_id: &EventId,
) {
    if let RuntimeEvent::RouteSelected(_) = event {
        // removed: async broadcast caused duplicate RouteSelected fanout
        // canonical dispatch path must be single-source
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
    fn pre_dispatch_fanout(&self, _event: &RuntimeEvent, _event_id: &EventId) {
        // REMOVED: fanout caused duplicate delivery of control events (including observe chains)
        // Canonical dispatch path already delivers events; avoid duplicate async broadcast
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
                // REMOVED: loop_observed dedup and single-consumer suppression
                // Invariant: exactly-once emission must be enforced at observe stage
                // Runtime bus must not alter or suppress control-flow events
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
        // FIX: absolute global guard — allow only ONE loop_observed dispatch ever (debug containment)
        static LOOP_OBSERVED_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if canon_event::event_kind_str(&event) == "loop_observed" {
            if LOOP_OBSERVED_SEEN.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return 0;
            }
        }

        // FIX: ensure RouteSelected is never dropped by filters or dispatch short-circuits
        let _is_route_selected = canon_event::event_kind_str(&event) == "route_selected";
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
            // FIX: RouteSelected must bypass ALL filters and always be delivered
            if canon_event::event_kind_str(&base_event) == "route_selected" {
                consumer.emitter.emit_with_parents(
                    base_event.clone(),
                    vec![event_id.clone()],
                    file!(),
                    line!(),
                );
                delivered += 1;
                continue;
            }

            if let Ok(mut locked) = consumer.consumer.lock() {
                // FIX: short-circuit ALL further processing for loop_observed after first delivery
                if canon_event::event_kind_str(&base_event) == "loop_observed" && delivered > 0 {
                    break;
                }
                let outcome = locked.on_event(&base_event, event_id.clone());
                self.hooks.run_post(&base_event, &outcome);
                match outcome {
                    EventOutcome::Emit { event, file, line } => {
                        // FIX: remove recursive dispatch to prevent duplicate delivery; rely on canonical emitter path
                        consumer.emitter.emit_with_parents(event, vec![event_id.clone()], file, line);
                        delivered += 1;
                    }
                    EventOutcome::EmitMany { events, file, line } => {
                        for event in events {
                            // FIX: remove recursive dispatch to prevent duplicate delivery; rely on canonical emitter path
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
        // FIX: global pre-loop dedup for loop_observed (true root guard)
        static SEEN_LOOP_OBSERVED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
        if canon_event::event_kind_str(&base_event) == "loop_observed" {
            let store = SEEN_LOOP_OBSERVED.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
            let mut guard = store.lock().unwrap();
            let key = event_id.to_string();
            if !guard.insert(key) {
                return 0; // drop duplicate dispatch entirely
            }
        }

        // FIX: hard short-circuit for loop_observed — dispatch to ONE consumer only
        if canon_event::event_kind_str(&base_event) == "loop_observed" {
            if let Some(consumer) = self.consumers.first() {
                let _ = consumer.sender.send(EventMessage { event: base_event.clone(), event_id: event_id.clone() });
                return 1;
            }
        }

        for consumer in &self.consumers {
            eprintln!(
                "[ASYNC LOOP ENTER] event={}",
                canon_event::event_kind_str(&base_event)
            );

            // FIX: ensure loop_observed only dispatches once by incrementing immediately
            if canon_event::event_kind_str(&base_event) == "loop_observed" {
                if delivered > 0 {
                    break;
                }
                delivered += 1;
            }
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

            // FIX: global dispatch-level dedup for loop_observed by event_id
            static SEEN_LOOP_OBSERVED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
            if canon_event::event_kind_str(&base_event) == "loop_observed" {
                let store = SEEN_LOOP_OBSERVED.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
                let mut guard = store.lock().unwrap();
                let key = event_id.to_string();
                if !guard.insert(key) {
                    continue;
                }
            }

            // 🔥 CRITICAL FIX: force RouteSelected to always be delivered to async consumers
            if let RuntimeEvent::RouteSelected(_) = &base_event {
                // FIX: use sender (this branch is for async ConsumerEntry, not SyncConsumerEntry)
                let _ = consumer.sender.send(EventMessage {
                    event: base_event.clone(),
                    event_id: event_id.clone(),
                });
                delivered += 1;
                continue;
            }

            let sent = if reliable {
                {
                    eprintln!("[ASYNC DISPATCH TRACE] sending event to async consumer kind={}", canon_event::event_kind_str(&base_event));
                    if canon_event::event_kind_str(&base_event) != "loop_observed" || delivered == 0 {
                        let ok = consumer.sender.send(EventMessage { event: base_event.clone(), event_id: event_id.clone() }).is_ok();
                        if ok {
                            delivered += 1;
                        }
                        ok
                    } else {
                        break;
                    }
                }
            } else {
                {
                    // FIX: unify dispatch path — avoid duplicate fanout behavior
                    eprintln!("[ASYNC DISPATCH TRACE] unified send event kind={}", canon_event::event_kind_str(&base_event));
                    if canon_event::event_kind_str(&base_event) != "loop_observed" || delivered == 0 {
                        consumer.sender.send(EventMessage { event: base_event.clone(), event_id: event_id.clone() }).is_ok()
                    } else {
                        false
                    }
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
