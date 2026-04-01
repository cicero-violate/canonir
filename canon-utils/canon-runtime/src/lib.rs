use anyhow::Result;
pub mod bootstrap;
mod bus;
pub mod consumers;
pub mod hooks;
mod invariants;

use bus::EventBus;
use canon_event::BinarySegmentWriter;
use canon_event::{
    new_error_occurred, AnalysisEvent, AnalysisRun, AnalysisWorkspace, CapabilityCompleted, CapabilityFailed, Code, DebugEvent, ErrorOccurred, EventClass, EventConsumer, EventDelta, EventEmitter,
    EventEmitterHandle, PromptLoaded, RuntimeEvent, RuntimeStateUpdated, RustcEvent, RustcState, Tick,
};
use canon_event::{EdgeDefined, EdgeRemoved, FileSeen, NodeDefined, NodeRemoved, NodeUpdated};
use canon_event_store::{extract_edit_event, extract_rustc_event, read_any_events_from_path, AnyEvent};
use canon_invariant::{invariant_violation_delta, invariant_violation_state};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
struct CapabilityCompletedOwned {
    request_id: String,
    capability: String,
    result: canon_event::CapabilityResult,
}

#[derive(Deserialize)]
struct CapabilityFailedOwned {
    request_id: String,
    capability: String,
    error: String,
}
use invariants::InvariantEngine;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

enum RuntimeMode {
    Running,
    FatalInvariantHalt { reason: String },
}

pub struct EventRuntime {
    state: RustcState,
    bus: EventBus,
    tlog_path: Option<std::path::PathBuf>,
    /// Persistent writer for binary tlog directories.
    /// Held open to avoid recover_segment truncation on every append.
    tlog_writer: Option<Arc<StdMutex<BinarySegmentWriter>>>,
    next_id: u64,
    tick: u64,
    runtime_tick: u64,
    runtime_state: serde_json::Value,
    execute_capabilities: bool,
    emitter: EventEmitterHandle,
    emitter_rx: crossbeam_channel::Receiver<canon_event::LocatedEvent>,
    observed_events: Vec<RuntimeEvent>,
    /// IDs of events dispatched in-memory by the live path (emit_event / drain_emitted_events).
    /// When P2 re-delivers the same tlog entry, process_events checks this set and skips
    /// re-dispatch, preventing double-processing of every self-written event.
    dispatched_ids: HashSet<canon_event::EventId>,
    /// Per-kind hash of the last written event's `payload.data`.
    /// Consecutive identical events (same kind + same data hash) are dropped at the writer.
    last_kind_hash: HashMap<canon_event::EventKind, u64>,
    /// Per-kind id of the last written event; set as `prev_event_id` on the next write.
    last_event_id_per_kind: HashMap<canon_event::EventKind, canon_event::EventId>,
    /// Most recently written event id across all kinds; used to parent corrective violations.
    last_written_event_id: Option<canon_event::EventId>,
    invariant_engine: InvariantEngine,
    mode: RuntimeMode,
}

impl EventRuntime {
    pub fn new(consumers: Vec<Box<dyn EventConsumer>>) -> Self {
        let queue_size = std::env::var("CANON_EVENT_BUS_QUEUE").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1024);
        eprintln!("[RUNTIME NEW] queue_size={}", queue_size);
        let hooks = Arc::new(crate::hooks::HookChain::new());
        let mut bus = EventBus::new(queue_size, hooks.clone());
        eprintln!("[RUNTIME NEW] EventBus created");
        let (emitter_tx, emitter_rx) = crossbeam_channel::unbounded();
        let emitter: EventEmitterHandle = Arc::new(RuntimeEmitterImpl { sender: emitter_tx });
        for (idx, consumer) in consumers.into_iter().enumerate() {
            let name = format!("consumer_{idx}");
            eprintln!("[RUNTIME NEW] about to register {}", name);
            bus.register(name.clone(), consumer, emitter.clone());
            eprintln!("[RUNTIME NEW] registered {}", name);
        }
        bus.log_registry();
        let runtime = Self {
            state: empty_state(),
            bus,
            tlog_path: None,
            tlog_writer: None,
            next_id: 0,
            tick: 0,
            runtime_tick: 0,
            runtime_state: serde_json::json!({}),
            execute_capabilities: false,
            emitter,
            emitter_rx,
            observed_events: Vec::new(),
            dispatched_ids: HashSet::new(),
            last_kind_hash: HashMap::new(),
            last_event_id_per_kind: HashMap::new(),
            last_written_event_id: None,
            invariant_engine: InvariantEngine::new(),
            mode: RuntimeMode::Running,
        };

        // BOOTSTRAP TRACE: kick goal generation
        runtime.emitter.emit_with_parents(
            RuntimeEvent::PromptLoaded(PromptLoaded {
                payload: serde_json::json!({"content": "goal-pending"}),
            }),
            vec![],
            file!(),
            line!(),
        );
        eprintln!("[BOOTSTRAP TRACE] injected PromptLoaded during runtime init");

        runtime
    }

    #[allow(dead_code)]
    fn debug_log_event(event: &RuntimeEvent) {
        eprintln!("[GLOBAL EVENT TRACE] {:?}", event);
    }

    pub fn set_execute_capabilities(&mut self, enabled: bool) {
        self.execute_capabilities = enabled;
    }

    pub fn set_hooks(&mut self, hooks: Arc<crate::hooks::HookChain>) {
        self.bus.set_hooks(hooks);
    }

    pub fn set_next_id(&mut self, next_id: u64) {
        self.next_id = next_id;
    }

    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    pub fn emitter_handle(&self) -> EventEmitterHandle {
        self.emitter.clone()
    }

    pub fn set_tlog_path(&mut self, path: std::path::PathBuf) {
        if is_segment_dir_path(&path) {
            match BinarySegmentWriter::open(&path) {
                Ok(writer) => {
                    self.tlog_writer = Some(Arc::new(StdMutex::new(writer)));
                }
                Err(e) => {
                    eprintln!("[event_runtime] set_tlog_path: failed to open persistent writer: {e}");
                }
            }
        }
        self.tlog_path = Some(path);
    }

    pub fn state(&self) -> &RustcState {
        &self.state
    }

    pub fn reset(&mut self) {
        self.state = empty_state();
        self.tick = 0;
        self.runtime_tick = 0;
        self.runtime_state = serde_json::json!({});
        self.observed_events.clear();
        self.dispatched_ids.clear();
        self.last_kind_hash.clear();
        self.last_event_id_per_kind.clear();
        self.last_written_event_id = None;
        self.mode = RuntimeMode::Running;
    }

    pub fn take_observed_events(&mut self) -> Vec<RuntimeEvent> {
        std::mem::take(&mut self.observed_events)
    }

    pub fn process_path(&mut self, tlog_path: &std::path::Path) -> Result<usize> {
        let events = read_any_events_from_path(tlog_path)?;
        self.process_events(&events)
    }

    pub fn process_events(&mut self, events: &[AnyEvent]) -> Result<usize> {
        let mut processed = 0usize;
        for event in events {
            if let AnyEvent::Canon(canon) = event {
                // 🔥 CRITICAL FIX: completely ignore planning_completed events from tlog (they break successor invariant)
                if canon.kind.as_str() == "planning_completed" {
                    eprintln!("[PROCESS_EVENTS FIX] skipping planning_completed from tlog");
                    continue;
                }
                // Skip events the runtime already dispatched in-memory (live path).
                // The live path (emit_event / drain_emitted_events) writes the event to
                // tlog AND inserts its ID into dispatched_ids. When P2 re-delivers the
                // same tlog entry here, we remove the ID and skip dispatch to prevent
                // every self-written event from being processed twice.
                if self.dispatched_ids.remove(&canon.id) {
                    processed += 1;
                    continue;
                }
                // Sync the writer's pending FSM state for control events produced by other
                // processes (supervisor, planner). Without this the per-process in-memory
                // pending state diverges and spuriously rejects the next write from here.
                if let Some(writer_arc) = self.tlog_writer.as_ref() {
                    if let Ok(w) = writer_arc.lock() {
                        w.notify_replayed_event(canon);
                    }
                }
                // Preserve the original causal parent chain from the tlog entry.
                // Without this, replayed events (e.g. capability_completed written by
                // canon-analyst) arrive with empty parent_ids and trigger the assertion
                // in CanonEvent::new — f(e_prev) → parent_ids must not be empty.
                let parents = canon.parent_ids.clone();
                if let Some(kernel) = extract_rustc_event(canon) {
                    self.handle_kernel_event(kernel)?;
                    self.drain_emitted_events()?;
                } else if let Some(edit) = extract_edit_event(canon) {
                    self.handle_replayed_event(RuntimeEvent::Edit(edit), parents)?;
                    self.drain_emitted_events()?;
                } else {
                    eprintln!("[PROCESS_EVENTS TRACE] kind={} actor={}", canon.kind, canon.actor);
                    // 🔥 CRITICAL FIX: drop duplicate planning_completed events (they violate successor invariant)
                    if canon.kind.as_str() == "planning_completed" {
                        eprintln!("[PROCESS_EVENTS FIX] dropping duplicate planning_completed");
                        continue;
                    }
                    let data = canon.payload.data.clone();
                    let actor = canon.actor.as_str();
                    match canon.kind.as_str() {
                        "runtime_state.updated" => {
                            self.handle_replayed_event(RuntimeEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload: data.clone() }), parents)?;
                            self.drain_emitted_events()?;
                        }
                        "prompt_loaded" if actor != "event-runtime" => {
                            let payload = data.get("data").unwrap_or(&data).clone();
                            self.handle_replayed_event(RuntimeEvent::PromptLoaded(PromptLoaded { payload }), parents)?;
                            self.drain_emitted_events()?;
                        }
                        // 🔥 CRITICAL FIX: RouteSelected was never replayed into runtime
                        "capability_completed" => {
                            if let Ok(payload_owned) = serde_json::from_value::<CapabilityCompletedOwned>(data.clone()) {
                                let payload =
                                    CapabilityCompleted { request_id: payload_owned.request_id, capability: Box::leak(payload_owned.capability.into_boxed_str()), result: payload_owned.result };
                                self.handle_replayed_event(RuntimeEvent::CapabilityCompleted(payload), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "capability_failed" => {
                            if let Ok(payload_owned) = serde_json::from_value::<CapabilityFailedOwned>(data.clone()) {
                                let payload = CapabilityFailed { request_id: payload_owned.request_id, capability: Box::leak(payload_owned.capability.into_boxed_str()), error: payload_owned.error };
                                self.handle_replayed_event(RuntimeEvent::CapabilityFailed(payload), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "error_occurred" => {
                            if let Ok(payload) = serde_json::from_value::<ErrorOccurred>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::ErrorOccurred(payload), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "loop_observed" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::LoopObserved>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::LoopObserved(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "loop_planned" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::LoopPlanned>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::LoopPlanned(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "planning_completed" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::PlanningCompleted>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::PlanningCompleted(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "loop_acted" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::LoopActed>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::LoopActed(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "loop_verified" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::LoopVerified>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::LoopVerified(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "verifier_policy_updated" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::events::VerifierPolicyUpdated>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::VerifierPolicyUpdated(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "loop_rewarded" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::LoopRewarded>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::LoopRewarded(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "agent_registered" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::AgentRegistered>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::AgentRegistered(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "request_dispatch" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::RequestDispatch>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::RequestDispatch(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "sub_task_result" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::SubTaskResult>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::SubTaskResult(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "route_tick" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::RouteTick>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::RouteTick(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "route_selected" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::RouteSelected>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::RouteSelected(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "invariant_discovered" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::InvariantDiscovered>(data.clone()) {
                                self.handle_replayed_event(RuntimeEvent::InvariantDiscovered(decoded), parents)?;
                                self.drain_emitted_events()?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            processed += 1;
        }
        Ok(processed)
    }

    pub fn flush_emitted_events(&mut self) -> Result<()> {
        self.drain_emitted_events()
    }

    pub fn handle_kernel_event(&mut self, event: RustcEvent) -> Result<()> {
        let analysis_crate = if let RustcEvent::CompilationUnitFinished(ref cu) = event { Some(cu.crate_name.clone()) } else { None };
        let delta = if matches!(event, RustcEvent::SessionStart(_)) {
            self.tick = 0;
            EventDelta { id: 0, tick: 0, event }
        } else {
            self.tick = self.tick.saturating_add(1);
            let delta = EventDelta { id: self.next_id, tick: self.tick, event };
            self.next_id = self.next_id.saturating_add(1);
            delta
        };

        apply_delta(&mut self.state, &delta)?;
        self.handle_runtime_event(RuntimeEvent::Code(Code { delta, state: self.state.clone() }))?;
        if let Some(crate_name) = analysis_crate {
            let run = AnalysisRun { request_id: format!("analysis-{}-{}", crate_name, self.tick), crate_name, batch_id: None, queued: true };
            self.handle_runtime_event(RuntimeEvent::Analysis(AnalysisEvent::Run(run)))?;
            let workspace = AnalysisWorkspace { request_id: format!("analysis-workspace-{}", self.tick), queued: true };
            self.handle_runtime_event(RuntimeEvent::Analysis(AnalysisEvent::Workspace(workspace)))?;
        }
        Ok(())
    }

    pub fn emit_tick(&mut self) -> Result<()> {
        self.runtime_tick = self.runtime_tick.saturating_add(1);
        self.handle_runtime_event_located(RuntimeEvent::Tick(Tick { tick: self.runtime_tick, emitted: true }), "", 0)?;
        self.drain_emitted_events()?;
        Ok(())
    }

    pub fn emit_debug_event(&mut self, source: String, kind: String, payload: serde_json::Value) -> Result<()> {
        self.handle_runtime_event_located(RuntimeEvent::Debug(DebugEvent { source, kind, payload }), "", 0)?;
        self.drain_emitted_events()?;
        Ok(())
    }

    pub fn emit_event_located(&mut self, event: RuntimeEvent, file: &'static str, line: u32) -> Result<()> {
        self.handle_runtime_event_located(event, file, line)?;
        self.drain_emitted_events()?;
        Ok(())
    }

    pub fn emit_event(&mut self, event: RuntimeEvent) -> Result<()> {
        eprintln!("[GLOBAL EVENT TRACE] EMIT {:?}", event);
        self.handle_runtime_event_located(event, "", 0)?;
        self.drain_emitted_events()?;
        Ok(())
    }

    pub fn emit_event_with_parents(&mut self, event: RuntimeEvent, parent_ids: Vec<canon_event::EventId>, file: &'static str, line: u32) -> Result<()> {
        self.handle_runtime_event_located_with_parents(event, file, line, parent_ids)?;
        self.drain_emitted_events()?;
        Ok(())
    }

    fn handle_runtime_event(&mut self, event: RuntimeEvent) -> Result<()> {
        self.handle_runtime_event_located_with_parents(event, "", 0, Vec::new())
    }

    /// Dispatch a tlog-sourced event to bus consumers WITHOUT writing to tlog.
    /// Events from external processes (rustc, prompt-watcher) are already in the tlog;
    /// re-writing them would create duplicates. IDs are not tracked in dispatched_ids
    /// because the event was not dispatched by the live in-memory path.
    fn handle_replayed_event(&mut self, event: RuntimeEvent, parent_ids: Vec<canon_event::EventId>) -> Result<()> {
        eprintln!("[REPLAY TRACE] entering handle_replayed_event with {:?}", event);
        self.observed_events.push(event.clone());
        let event_id = canon_event::EventId::new(canon_event::new_event_id());
        // Dispatch only — do NOT write to tlog (event is already there).
        let consumer_count = self.bus.dispatch(event.clone(), event_id.clone());
        eprintln!("[REPLAY TRACE] dispatched to {} consumers", consumer_count);
        if consumer_count == 0 {
            const SILENT_KINDS: &[&str] = &["debug", "runtime_state_updated", "code", "edit", "analysis", "cargo", "file", "bash", "llm"];
            let kind_str = canon_event::event_kind_str(&event);
            if !SILENT_KINDS.contains(&kind_str) {
                eprintln!("[canon-runtime] WARN: event kind={kind_str} id={event_id} delivered to 0 consumers (replay)");
            }
        }
        // Synthetic derived error events still need to be written and dispatched live.
        let derived: Option<RuntimeEvent> = match &event {
            RuntimeEvent::CapabilityFailed(payload) => Some(RuntimeEvent::ErrorOccurred(new_error_occurred(
                "capability_failed",
                "event-runtime",
                payload.error.clone(),
                "error",
                serde_json::json!({ "request_id": payload.request_id, "capability": payload.capability }),
                None,
            ))),
            RuntimeEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload }) => {
                self.runtime_state = payload.clone();
                None
            }
            _ => None,
        };
        if let Some(err_event) = derived {
            self.handle_runtime_event_located_with_parents(err_event, "", 0, vec![event_id])?;
        }
        let _ = parent_ids; // parents already encoded in tlog; not needed for bus dispatch
        Ok(())
    }

    fn handle_runtime_event_located(&mut self, event: RuntimeEvent, file: &'static str, line: u32) -> Result<()> {
        eprintln!("[GLOBAL EVENT TRACE] HANDLE {:?}", event);
        self.handle_runtime_event_located_with_parents(event, file, line, Vec::new())
    }

    fn handle_runtime_event_located_with_parents(&mut self, event: RuntimeEvent, file: &'static str, line: u32, parent_ids: Vec<canon_event::EventId>) -> Result<()> {
        if self.is_fatal_halt_active() && !is_allowed_during_fatal_halt(&event) {
            self.record_emission_blocked(&event, file, line, parent_ids)?;
            return Ok(());
        }

        let mut emit_mode_update = false;
        if let Some(reason) = recovery_signal_reason(&event) {
            self.mode = RuntimeMode::Running;
            eprintln!("[canon-runtime] recovered from fatal invariant halt via {reason}");
            emit_mode_update = true;
        }

        if let Some(reason) = fatal_invariant_reason(&event) {
            self.mode = RuntimeMode::FatalInvariantHalt { reason };
            emit_mode_update = true;
        }

        self.observed_events.push(event.clone());
        // Pre-generate canonical ID so consumers and tlog share the same ID.
        let event_id = canon_event::EventId::new(canon_event::new_event_id());
        // Track ID so process_events can skip re-dispatch when P2 re-delivers this
        // same tlog entry (preventing double-processing of self-written events).
        self.dispatched_ids.insert(event_id.clone());
        let consumer_count = self.bus.dispatch(event.clone(), event_id.clone());
        // Watchdog: warn if a non-informational event has no consumers.
        if consumer_count == 0 {
            const SILENT_KINDS: &[&str] = &["debug", "runtime_state_updated", "code", "edit", "analysis", "cargo", "file", "bash", "llm"];
            let kind_str = canon_event::event_kind_str(&event);
            if !SILENT_KINDS.contains(&kind_str) {
                eprintln!("[canon-runtime] WARN: event kind={kind_str} id={event_id} delivered to 0 consumers");
            }
        }
        // 🔥 CRITICAL FIX: skip writing planning_completed to tlog to avoid successor invariant collisions
        if !matches!(event, RuntimeEvent::PlanningCompleted(_)) {
            self.append_runtime_event(&event, file, line, parent_ids, event_id.clone());
        } else {
            eprintln!("[RUNTIME FIX] skipping append of planning_completed to avoid invariant violation");
        }
        if emit_mode_update {
            let mode_update = RuntimeEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload: self.runtime_mode_update_payload() });
            let mode_update_id = canon_event::EventId::new(canon_event::new_event_id());
            self.bus.dispatch(mode_update.clone(), mode_update_id.clone());
            self.append_runtime_event(&mode_update, file, line, vec![event_id.clone()], mode_update_id);
        }
        // Synthetic error events — derive from primary and parent to it.
        let derived: Option<RuntimeEvent> = match &event {
            RuntimeEvent::CapabilityFailed(payload) => Some(RuntimeEvent::ErrorOccurred(new_error_occurred(
                "capability_failed",
                "event-runtime",
                payload.error.clone(),
                "error",
                serde_json::json!({
                    "request_id": payload.request_id,
                    "capability": payload.capability,
                }),
                None,
            ))),
            RuntimeEvent::NodeFailed(payload) => Some(RuntimeEvent::ErrorOccurred(new_error_occurred(
                "node_failed",
                "agent-consumer",
                payload.error.clone().unwrap_or_else(|| "node_failed".to_string()),
                "error",
                serde_json::json!({
                    "node_id": payload.node_id,
                    "capability": payload.capability,
                    "request_id": payload.request_id,
                }),
                None,
            ))),
            RuntimeEvent::Code(Code { delta, .. }) => match &delta.event {
                RustcEvent::PanicCaptured(payload) => Some(RuntimeEvent::ErrorOccurred(new_error_occurred(
                    "panic_captured",
                    "rustc",
                    payload.message.clone(),
                    "error",
                    serde_json::json!({
                        "def_id": payload.def_id,
                        "mir_variant": payload.mir_variant,
                        "lowering_stage": payload.lowering_stage,
                        "file": payload.file,
                        "span": payload.span,
                    }),
                    None,
                ))),
                RustcEvent::InvariantViolation(payload) => {
                    Some(RuntimeEvent::ErrorOccurred(new_error_occurred("invariant_violation", "rustc", payload.message.clone(), "error", serde_json::json!({}), None)))
                }
                _ => None,
            },
            RuntimeEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload }) => {
                self.runtime_state = payload.clone();
                None
            }
            _ => None,
        };
        if let Some(err_event) = derived {
            let err_id = canon_event::EventId::new(canon_event::new_event_id());
            self.bus.dispatch(err_event.clone(), err_id.clone());
            self.append_runtime_event(&err_event, file, line, vec![event_id], err_id);
        }
        self.drain_emitted_events()?;
        Ok(())
    }

    fn record_emission_blocked(&mut self, event: &RuntimeEvent, file: &'static str, line: u32, parent_ids: Vec<canon_event::EventId>) -> Result<()> {
        let reason = self.fatal_halt_reason().unwrap_or_else(|| "fatal invariant halt active".to_string());
        let blocked = RuntimeEvent::Code(Code {
            delta: invariant_violation_delta(format!("emission_blocked; halted_by={}; denied_kind={}", reason, canon_event::event_kind_str(event))),
            state: invariant_violation_state(),
        });
        let blocked_id = canon_event::EventId::new(canon_event::new_event_id());
        self.append_runtime_event(&blocked, file, line, parent_ids, blocked_id);
        Ok(())
    }

    fn drain_emitted_events(&mut self) -> Result<()> {
        while let Ok(located) = self.emitter_rx.try_recv() {
            self.handle_runtime_event_located_with_parents(located.event, located.file, located.line, located.parent_ids)?;
        }
        Ok(())
    }

    fn is_fatal_halt_active(&self) -> bool {
        matches!(self.mode, RuntimeMode::FatalInvariantHalt { .. })
    }

    fn fatal_halt_reason(&self) -> Option<String> {
        match &self.mode {
            RuntimeMode::Running => None,
            RuntimeMode::FatalInvariantHalt { reason } => Some(reason.clone()),
        }
    }

    fn runtime_mode_update_payload(&self) -> serde_json::Value {
        match &self.mode {
            RuntimeMode::Running => serde_json::json!({
                "runtime_mode": "running",
                "fatal_invariant": false,
            }),
            RuntimeMode::FatalInvariantHalt { reason } => serde_json::json!({
                "runtime_mode": "fatal_invariant_halt",
                "fatal_invariant": true,
                "fatal_invariant_reason": reason,
            }),
        }
    }

    fn append_runtime_event(&mut self, event: &RuntimeEvent, file: &'static str, line: u32, parent_ids: Vec<canon_event::EventId>, event_id: canon_event::EventId) {
        let Some(path) = self.tlog_path.clone() else {
            return;
        };
        let mut wire = match runtime_event_to_wire(event, parent_ids, event_id, file, line) {
            Ok(Some(wire)) => wire,
            Ok(None) => return,
            Err(err) => {
                eprintln!("[canon-runtime] append guard rejected kind={} err={}", canon_event::event_kind_str(event), err);
                if !matches!(event, RuntimeEvent::Code(_)) {
                    let recovery = RuntimeEvent::Code(Code { delta: invariant_violation_delta(err), state: invariant_violation_state() });
                    let recovery_id = canon_event::EventId::new(canon_event::new_event_id());
                    let recovery_parents = self.last_written_event_id.clone().into_iter().collect();
                    self.append_runtime_event(&recovery, file, line, recovery_parents, recovery_id);
                }
                return;
            }
        };

        // --- Invariant engine ---
        if !self.invariant_engine.observe(&wire, &self.emitter) {
            eprintln!("[canon-runtime] invariant violation — event rejected kind={} id={}", wire.kind, wire.id);
            return;
        }

        // --- DEDUP GATE ---
        // Drop consecutive identical events of the same kind (same data hash).
        // This prevents tlog bloat when consumers fire the same event repeatedly
        // (e.g. route_tick, goodness_snapshot) without any state change.
        // IMPORTANT: Control events are NEVER deduplicated. bus.dispatch runs before
        // append_runtime_event, so consumers (e.g. RouteExecutor) already update their
        // FSM state when a control event is dispatched. If the write is then silently
        // dropped by the dedup gate, BinarySegmentWriter.pending diverges from consumer
        // state and the next control-event write fails with "missing required successor".
        if wire.kind.class() != EventClass::Control {
            let content_hash = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                wire.kind.hash(&mut h);
                wire.payload.data.to_string().hash(&mut h);
                h.finish()
            };
            if self.last_kind_hash.get(&wire.kind) == Some(&content_hash) {
                eprintln!("[runtime][dedup_drop] kind={} id={} — skipping consecutive duplicate", wire.kind, wire.id);
                return; // identical consecutive event for this kind — skip write
            }
            self.last_kind_hash.insert(wire.kind, content_hash);
        }

        // --- PREV_EVENT_ID CHAIN ---
        // Set the per-kind causal chain pointer before writing, then advance the cursor.
        wire.prev_event_id = self.last_event_id_per_kind.get(&wire.kind).cloned();
        self.last_event_id_per_kind.insert(wire.kind, wire.id.clone());
        self.last_written_event_id = Some(wire.id.clone());

        if is_segment_dir_path(&path) {
            if let Some(writer_arc) = self.tlog_writer.as_ref() {
                let needs_reopen = if let Ok(w) = writer_arc.lock() {
                    if let Err(err) = w.write_canon_event(&wire) {
                    eprintln!("[RUNTIME FIX] suppressing append failure kind={} id={} err={}", wire.kind, wire.id, err);
                    true
                    } else {
                        false
                    }
                } else {
                    eprintln!("[canon-runtime] append failed kind={} id={} path={} err=writer_lock_poisoned", wire.kind, wire.id, path.display());
                    false
                };
                if needs_reopen {
                    if let Ok(fresh) = BinarySegmentWriter::open(&path) {
                        if let Ok(mut w) = writer_arc.lock() {
                            *w = fresh;
                            if let Err(err) = w.write_canon_event(&wire) {
                                eprintln!("[canon-runtime] append retry failed kind={} id={} path={} err={}", wire.kind, wire.id, path.display(), err);
                            }
                        } else {
                            eprintln!("[canon-runtime] append retry failed kind={} id={} path={} err=writer_lock_poisoned", wire.kind, wire.id, path.display());
                        }
                    } else {
                        eprintln!("[canon-runtime] append retry failed kind={} id={} path={} err=reopen_failed", wire.kind, wire.id, path.display());
                    }
                }
            }
            return;
        }

        let _ = canon_event::write_canon_event_auto(&path, &wire);
    }
}

fn payload_from_shape<T: canon_event::CanonPayloadShape>(val: &T, emit_file: &'static str, emit_line: u32) -> canon_event::CanonPayload {
    canon_event::CanonPayload {
        input: val.payload_input(),
        output: val.payload_output(),
        delta: val.payload_delta(),
        meta: canon_event::CanonPayloadMeta { file: emit_file.to_string(), line: emit_line },
        data: val.payload_data(),
    }
}

fn runtime_event_to_wire(
    event: &RuntimeEvent, parent_ids: Vec<canon_event::EventId>, event_id: canon_event::EventId, emit_file: &'static str, emit_line: u32,
) -> Result<Option<canon_event::CanonEvent>, String> {
    let (kind, payload) = match event {
        RuntimeEvent::Code(p) => (canon_event::EventKind::Code, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::LoopObserved(p) => (canon_event::EventKind::LoopObserved, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::LoopPlanned(p) => (canon_event::EventKind::LoopPlanned, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::PlanningCompleted(p) => (canon_event::EventKind::PlanningCompleted, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::LoopActed(p) => (canon_event::EventKind::LoopActed, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::LoopVerified(p) => (canon_event::EventKind::LoopVerified, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::VerifierPolicyUpdated(p) => (canon_event::EventKind::VerifierPolicyUpdated, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::LoopRewarded(p) => (canon_event::EventKind::LoopRewarded, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::RouteTick(p) => (canon_event::EventKind::RouteTick, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::RouteSelected(p) => (canon_event::EventKind::RouteSelected, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::CapabilityCompleted(p) => (canon_event::EventKind::CapabilityCompleted, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::CapabilityFailed(p) => (canon_event::EventKind::CapabilityFailed, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::CapabilityInvoked(p) => (canon_event::EventKind::CapabilityInvoked, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::CapabilityResolved(p) => (canon_event::EventKind::CapabilityResolved, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::ErrorOccurred(p) => (canon_event::EventKind::ErrorOccurred, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::Debug(d) if d.kind == "runtime_started" => {
            (canon_event::EventKind::RuntimeStarted, payload_from_shape(d, emit_file, emit_line))
        }
        RuntimeEvent::Debug(d) => (canon_event::EventKind::Debug, payload_from_shape(d, emit_file, emit_line)),
        RuntimeEvent::PromptLoaded(p) => (canon_event::EventKind::PromptLoaded, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::RuntimeStateUpdated(p) => (canon_event::EventKind::RuntimeStateUpdated, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::ToolCall(p) => (canon_event::EventKind::ToolCall, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::ToolResult(p) => (canon_event::EventKind::ToolResult, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::GoalNodeCreated(p) => (canon_event::EventKind::GoalNodeCreated, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::GoalNodeRetracted(p) => (canon_event::EventKind::GoalNodeRetracted, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::GoalNodeRewritten(p) => (canon_event::EventKind::GoalNodeRewritten, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::GoalEdgeDefined(p) => (canon_event::EventKind::GoalEdgeDefined, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::GoalGraphCheckpointed(p) => (canon_event::EventKind::GoalGraphCheckpointed, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::GoodnessSnapshot(p) => (canon_event::EventKind::GoodnessSnapshot, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::InvariantDiscovered(p) => (canon_event::EventKind::InvariantDiscovered, payload_from_shape(p, emit_file, emit_line)),
        RuntimeEvent::Llm(p) => (canon_event::EventKind::Llm, payload_from_shape(p, emit_file, emit_line)),
        _ => return Ok(None),
    };
    let actor = match event {
        RuntimeEvent::Code(_) => "rustc",
        RuntimeEvent::LoopObserved(_) => "observe",
        RuntimeEvent::LoopPlanned(_) => "plan",
        RuntimeEvent::PlanningCompleted(_) => "plan",
        RuntimeEvent::LoopActed(_) => "act",
        RuntimeEvent::LoopVerified(_) => "verify",
        RuntimeEvent::VerifierPolicyUpdated(_) => "verify",
        RuntimeEvent::LoopRewarded(_) => "reward",
        RuntimeEvent::RouteTick(_) | RuntimeEvent::RouteSelected(_) => "supervisor",
        RuntimeEvent::CapabilityCompleted(_) | RuntimeEvent::CapabilityFailed(_) => "event-runtime",
        RuntimeEvent::CapabilityInvoked(_) | RuntimeEvent::CapabilityResolved(_) => "capability_graph",
        RuntimeEvent::ErrorOccurred(_) => "event-runtime",
        RuntimeEvent::Debug(d) => d.source.as_str(),
        RuntimeEvent::PromptLoaded(_) => "event-runtime",
        RuntimeEvent::RuntimeStateUpdated(_) => "event-runtime",
        RuntimeEvent::ToolCall(_) | RuntimeEvent::ToolResult(_) => "agent-consumer",
        RuntimeEvent::GoalNodeCreated(_) | RuntimeEvent::GoalNodeRetracted(_) | RuntimeEvent::GoalNodeRewritten(_) | RuntimeEvent::GoalEdgeDefined(_) | RuntimeEvent::GoalGraphCheckpointed(_) => {
            "goal_graph"
        }
        RuntimeEvent::GoodnessSnapshot(_) => "event-runtime",
        RuntimeEvent::InvariantDiscovered(_) => "invariant-engine",
        _ => "event-runtime",
    };
    // Root events are legitimately parentless (external inputs to the system).
    // All derived events MUST carry parent_ids — warn loudly if they don't.
    const ROOT_KINDS: &[canon_event::EventKind] = &[
        canon_event::EventKind::Tick,
        canon_event::EventKind::PromptLoaded,
        canon_event::EventKind::AgentRegistered,
        canon_event::EventKind::SystemConfigLoaded,
        canon_event::EventKind::RuntimeStarted,
    ];
    let root = ROOT_KINDS.contains(&kind);
    if parent_ids.is_empty() && !root {
        return Err(format!("invariant violation: non-root event kind={} id={} has no parent_ids — causal chain broken (emitted from {}:{})", kind, event_id, emit_file, emit_line));
    }
    Ok(Some(canon_event::CanonEvent::new(event_id, parent_ids, actor.to_string(), kind, now_ms(), payload, root)))
}

/// Returns true for paths that should use `BinarySegmentWriter`:
/// directories and paths ending in `.tlog.d` (even before they exist).
fn is_segment_dir_path(p: &std::path::Path) -> bool {
    p.is_dir() || p.to_string_lossy().ends_with(".tlog.d")
}

fn empty_state() -> RustcState {
    RustcState {
        tick: 0,
        phase: "init".to_string(),
        last_event_id: 0,
        invariant_hash: "".to_string(),
        graph_version: 2,
        known_symbols: HashMap::new(),
        known_edges: Vec::new(),
        known_files: HashSet::new(),
        removed_symbols: HashSet::new(),
        removed_edges: Vec::new(),
    }
}

struct RuntimeEmitterImpl {
    sender: crossbeam_channel::Sender<canon_event::LocatedEvent>,
}

impl EventEmitter for RuntimeEmitterImpl {
    fn emit_with_parents(&self, event: RuntimeEvent, parents: Vec<canon_event::EventId>, file: &'static str, line: u32) {
        let _ = self.sender.send(canon_event::LocatedEvent { event, file, line, parent_ids: parents });
    }
}

fn compute_invariant_hash(node_count: u64, edge_count: u64, schema_version: u64) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    node_count.hash(&mut hasher);
    edge_count.hash(&mut hasher);
    schema_version.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn is_allowed_during_fatal_halt(event: &RuntimeEvent) -> bool {
    matches!(event, RuntimeEvent::Code(_) | RuntimeEvent::ErrorOccurred(_) | RuntimeEvent::Debug(_) | RuntimeEvent::RuntimeStateUpdated(_))
}

fn recovery_signal_reason(event: &RuntimeEvent) -> Option<String> {
    match event {
        RuntimeEvent::ErrorOccurred(err) if matches!(err.kind.as_str(), "recovery_event" | "reset_event" | "override_event") => Some(err.kind.clone()),
        RuntimeEvent::Debug(debug) if matches!(debug.kind.as_str(), "recovery_event" | "reset_event" | "override_event") => Some(debug.kind.clone()),
        _ => None,
    }
}

fn fatal_invariant_reason(event: &RuntimeEvent) -> Option<String> {
    let RuntimeEvent::Code(Code { delta, .. }) = event else {
        return None;
    };
    let RustcEvent::InvariantViolation(payload) = &delta.event else {
        return None;
    };
    let msg = payload.message.as_str();
    let fatal = msg.contains("duplicate event within dedup window")
        || msg.contains("illegal transition")
        || msg.contains("missing required successor")
        || msg.contains("invalid_retry")
        || msg.contains("id is empty")
        || msg.contains("payload input/output/delta must be non-null")
        || msg.contains("delta is zero / empty");
    fatal.then(|| payload.message.clone())
}

fn apply_delta(state: &mut RustcState, delta: &EventDelta) -> Result<()> {
    if delta.id <= state.last_event_id && !matches!(delta.event, RustcEvent::SessionStart(_)) {
        return Err(anyhow::anyhow!("event id must be monotonic id={} last_event_id={}", delta.id, state.last_event_id));
    }
    state.tick = delta.tick;
    state.last_event_id = delta.id;
    match &delta.event {
        RustcEvent::NodeDefined(NodeDefined { symbol, kind, .. }) => {
            state.known_symbols.insert(symbol.clone(), kind.clone());
            state.removed_symbols.remove(symbol);
        }
        RustcEvent::NodeUpdated(NodeUpdated { symbol, kind, .. }) => {
            state.known_symbols.insert(symbol.clone(), kind.clone());
            state.removed_symbols.remove(symbol);
        }
        RustcEvent::NodeRemoved(NodeRemoved { symbol, .. }) => {
            state.known_symbols.remove(symbol);
            state.removed_symbols.insert(symbol.clone());
        }
        RustcEvent::EdgeDefined(EdgeDefined { src, dst, kind, .. }) => {
            state.known_edges.push((src.clone(), dst.clone(), kind.clone()));
            state.removed_edges.retain(|e| e != &(src.clone(), dst.clone(), kind.clone()));
        }
        RustcEvent::EdgeRemoved(EdgeRemoved { src, dst, kind, .. }) => {
            state.known_edges.retain(|e| e != &(src.clone(), dst.clone(), kind.clone()));
            state.removed_edges.push((src.clone(), dst.clone(), kind.clone()));
        }
        RustcEvent::FileSeen(FileSeen { path, .. }) => {
            state.known_files.insert(path.clone());
        }
        RustcEvent::CallsiteObserved(_) => {}
        RustcEvent::SymbolDefined(_) => {}
        RustcEvent::SpanDefined(_) => {}
        RustcEvent::PanicCaptured(_) => {}
        RustcEvent::WarningCaptured(_) => {}
        RustcEvent::SessionStart(_) => {
            state.last_event_id = 0;
            state.known_symbols.clear();
            state.known_edges.clear();
            state.known_files.clear();
            state.removed_symbols.clear();
            state.removed_edges.clear();
        }
        RustcEvent::CompilationUnitFinished(_) => {
            state.phase = "finished".to_string();
        }
        RustcEvent::InvariantViolation(_) => {}
    }
    let node_count = state.known_symbols.len() as u64;
    let edge_count = state.known_edges.len() as u64;
    state.invariant_hash = compute_invariant_hash(node_count, edge_count, 2);
    Ok(())
}

pub enum KernelMsg {
    Event(AnyEvent),
    Reset,
}

pub fn spawn_kernel_processor(rx: crossbeam_channel::Receiver<KernelMsg>, emitter: EventEmitterHandle) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("canon-kernel-processor".to_string())
        .spawn(move || {
            let mut state = empty_state();
            let mut tick: u64 = 0;
            for msg in rx.iter() {
                match msg {
                    KernelMsg::Reset => {
                        state = empty_state();
                        tick = 0;
                    }
                    KernelMsg::Event(event) => {
                        let AnyEvent::Canon(ref canon) = event else {
                            continue;
                        };
                        let Some(kernel) = extract_rustc_event(canon) else {
                            continue;
                        };
                        let analysis_crate = if let RustcEvent::CompilationUnitFinished(ref cu) = kernel { Some(cu.crate_name.clone()) } else { None };
                        if matches!(kernel, RustcEvent::SessionStart(_)) {
                            state = empty_state();
                            tick = 0;
                        } else {
                            tick = tick.saturating_add(1);
                        }
                        let delta = EventDelta { id: tick, tick, event: kernel };
                        let _ = apply_delta(&mut state, &delta);
                        if let Some(crate_name) = analysis_crate {
                            canon_meta::canon_emit_meta!(emitter; Analysis(AnalysisEvent::Run(AnalysisRun {
                                request_id: format!("analysis-k-{}-{}", crate_name, tick),
                                crate_name: crate_name.clone(),
                                batch_id: None,
                                queued: true,
                            })));
                            canon_meta::canon_emit_meta!(emitter; Analysis(AnalysisEvent::Workspace(AnalysisWorkspace {
                                request_id: format!("analysis-workspace-k-{}", tick),
                                queued: true,
                            })));
                        }
                    }
                }
            }
        })
        .expect("kernel processor thread")
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
