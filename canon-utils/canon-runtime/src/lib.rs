#![allow(dead_code, unused_variables)]
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
use std::sync::atomic::{AtomicBool, Ordering};
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
    last_route_tick_emitted: u64,
    execute_capabilities: bool,
    emitter: EventEmitterHandle,
    emitter_rx: crossbeam_channel::Receiver<canon_event::LocatedEvent>,
    emitter_replay_blocked: Arc<AtomicBool>,
    observed_events: Vec<RuntimeEvent>,
    /// IDs of events dispatched in-memory by the live path (emit_event / drain_emitted_events).
    /// When P2 re-delivers the same tlog entry, process_events checks this set and skips
    /// re-dispatch, preventing double-processing of every self-written event.
    dispatched_ids: HashSet<canon_event::EventId>,
    /// Per-kind hash of the last written event's `payload.data`.
    /// Consecutive identical events (same kind + same data hash) are dropped at the writer.
    /// NOTE: RouteTick must not be deduplicated (it is a per-tick driver event).
    last_kind_hash: HashMap<canon_event::EventKind, u64>,
    /// Per-kind id of the last written event; set as `prev_event_id` on the next write.
    last_event_id_per_kind: HashMap<canon_event::EventKind, canon_event::EventId>,
    /// Most recently written event id across all kinds; used to parent corrective violations.
    last_written_event_id: Option<canon_event::EventId>,
    /// Last LoopObserved tick written, used to enforce exactly-once per tick.
    last_loop_observed_tick: Option<u64>,
    /// Buffered LoopObserved emitted before routing completes
    pending_loop_observed: Option<canon_event::LoopObserved>,
    /// TRACKING: ensures RouteTick is durably appended per Tick
    route_tick_append_seen: bool,
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
        let emitter_replay_blocked = Arc::new(AtomicBool::new(false));
        let emitter: EventEmitterHandle = Arc::new(RuntimeEmitterImpl {
            sender: emitter_tx,
            replay_blocked: emitter_replay_blocked.clone(),
        });
        // FIX: do NOT inject LoopStageExecutor here; it must be explicitly ordered after RouteExecutor
        let consumers = consumers;

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
            last_route_tick_emitted: 0,
            execute_capabilities: false,
            emitter,
            emitter_rx,
            emitter_replay_blocked,
            observed_events: Vec::new(),
            dispatched_ids: HashSet::new(),
            last_kind_hash: HashMap::new(),
            last_event_id_per_kind: HashMap::new(),
            last_written_event_id: None,
            last_loop_observed_tick: None,
            pending_loop_observed: None,
            route_tick_append_seen: false,
            invariant_engine: InvariantEngine::new(),
            mode: RuntimeMode::Running,
        };

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
        eprintln!("[TLOG INIT] requested_path={:?}", path);

        if !is_segment_dir_path(&path) {
            panic!("[FATAL] Non-segment tlog path provided: {:?}. Expected *.tlog.d directory.", path);
        }

        match BinarySegmentWriter::open(&path) {
            Ok(writer) => {
                eprintln!("[TLOG INIT] BinarySegmentWriter initialized at {:?}", path);
                self.tlog_writer = Some(Arc::new(StdMutex::new(writer)));
            }
            Err(e) => {
                panic!("[FATAL] tlog writer initialization failed at {:?}: {e}", path);
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
        // reset RouteTick emission guard
        self.last_route_tick_emitted = 0;
        self.emitter_replay_blocked.store(false, Ordering::SeqCst);
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
        // CRITICAL: mark replay mode so downstream logic does not emit new events
        self.emitter_replay_blocked.store(true, Ordering::SeqCst);
        let result = self.process_events(&events);
        // restore normal mode after replay
        self.emitter_replay_blocked.store(false, Ordering::SeqCst);
        result
    }

    pub fn process_events(&mut self, events: &[AnyEvent]) -> Result<usize> {
        // FIX: ensure any pending emitted events are dispatched BEFORE processing
        // This guarantees that early emissions (e.g. LoopObserved) see registered consumers
        self.drain_emitted_events()?;
        let mut processed = 0usize;
        for event in events {
            if let AnyEvent::Canon(canon) = event {
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

                        // RouteSelected already handled by single notify_replayed_event above.
                        // Avoid duplicate replay notification which causes double dispatch downstream.
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
                                // FIX: do not buffer LoopObserved — must dispatch immediately
                                // Buffering breaks LoopObserved → decision → RouteSelected chain
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
                        "route_selected" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::RouteSelected>(data.clone()) {
                                let tick = decoded.tick;
                                self.handle_replayed_event(RuntimeEvent::RouteSelected(decoded), parents)?;
                                self.drain_emitted_events()?;

                                if let Some(loop_obs) = self.pending_loop_observed.take() {
                                    if loop_obs.tick == tick {
                                        // FIX: preserve causal chain — LoopObserved must be child of RouteSelected
                                        let parent_ids = vec![canon.id.clone()];
                                        self.handle_replayed_event(RuntimeEvent::LoopObserved(loop_obs), parent_ids)?;
                                        self.drain_emitted_events()?;
                                    } else {
                                        self.pending_loop_observed = Some(loop_obs);
                                    }
                                }
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
                        // REMOVED: request_dispatch replay path (non-canonical)
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
    // CRITICAL: drive semantic loop after processing events
    {
        use canon_loop::executor::LoopStageExecutor;

        let workspace = std::path::PathBuf::from(".");
        let tlog = self.tlog_path.clone().unwrap_or_else(|| std::path::PathBuf::from("./state/event_log"));

        let mut loop_exec = LoopStageExecutor::new(workspace, tlog);

        let drained_events: Vec<_> = self.observed_events.drain(..).collect();
        if !drained_events.is_empty() {
            // FIX: prevent replay from generating new events
            // Replay must be read-only; only execute loop stage when not replay-blocked
            if !self.emitter_replay_blocked.load(Ordering::SeqCst) {
                let trigger_id = canon_event::EventId::new(self.next_id.to_string());
                for event in &drained_events {
                    let _ = loop_exec.execute_stage_event(&trigger_id, event);
                }
                self.drain_emitted_events()?;
            }
        }
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
        // Ensure no leftover emitted events from previous cycle
        self.drain_emitted_events()?;
        self.runtime_tick = self.runtime_tick.saturating_add(1);
        // RESET: track whether RouteTick is durably appended this cycle
        self.route_tick_append_seen = false;
        eprintln!("[EMIT TICK TRACE] tick={}", self.runtime_tick);
        eprintln!("[tick_emitted] tick={}", self.runtime_tick);
        // Emit Tick and capture its event_id for parent chaining
        self.emit_event(RuntimeEvent::Tick(Tick { tick: self.runtime_tick, emitted: true }))?;
        let tick_event_id = self
            .last_written_event_id
            .clone()
            .expect("[FATAL] Tick must produce event_id");
        eprintln!("[tick_dispatched] tick={}", self.runtime_tick);

        // RESTORE: emit RouteTick immediately after Tick to ensure control-loop persistence
        self.emit_event(RuntimeEvent::RouteTick(canon_event::RouteTick {
            tick: self.runtime_tick,
            emitted: true,
        }))?;

        let route_tick_event_id = self
            .last_written_event_id
            .clone()
            .expect("[FATAL] RouteTick must produce event_id");

        // FIX: ensure RouteTick always produces a distinct event_id from Tick
        if route_tick_event_id == tick_event_id {
            eprintln!("[runtime][repair] RouteTick reused Tick event_id; re-emitting with explicit parent linkage");
            let parents: Vec<_> = vec![tick_event_id.clone()];
            self.last_written_event_id = None;
            self.emit_event_with_parents(
                RuntimeEvent::RouteTick(canon_event::RouteTick {
                    tick: self.runtime_tick,
                    emitted: true,
                }),
                parents,
                file!(),
                line!(),
            )?;
        }

        self.last_route_tick_emitted = self.runtime_tick;
        eprintln!("[routetick_emitted] tick={}", self.runtime_tick);
        // NOTE: DO NOT pre-mark LoopObserved here.
        // LoopObserved must be emitted by the loop executor within the RouteTick cycle.
        // Pre-marking causes the dedup gate to drop the real LoopObserved,
        // breaking the control chain and causing multi-tick hangs.

        // STRICT: Do NOT synthesize LoopObserved.
        // Observation must originate exclusively from the loop executor.
        // Synthetic fallback creates pre-semantic (version=0) observations and violates
        // canonical ordering: RouteTick -> observe -> decision -> RouteSelected.

        // 🔧 FIX: If PlanningCompleted indicated missing semantic context,
        // we must still produce an observation to avoid early exit.
        // (removed invalid last_planning_completed logic)

        // NOTE: RouteSelected must be emitted exclusively by semantic routing (RouteExecutor).
        // Runtime must NOT emit RouteSelected to avoid duplicate decisions per tick.

        // 🔥 CRITICAL FIX: Emit RuntimeEvent once per cycle (canonical runtime summary)
        let runtime_event = RuntimeEvent::RuntimeStateUpdated(RuntimeStateUpdated {
            payload: serde_json::json!({
                "runtime_tick": self.runtime_tick,
                "state": self.runtime_state,
            }),
        });
        eprintln!("[runtime_event_emitted] tick={}", self.runtime_tick);
        self.emit_event(runtime_event)?;
        eprintln!("[runtime_event_dispatched] tick={}", self.runtime_tick);
        // FAIL-FAST: ensure at least one RuntimeEvent observed this tick
        if self.observed_events.is_empty() {
            panic!("[FATAL] No RuntimeEvent emitted for tick {}", self.runtime_tick);
        }
        // Ensure all emitted events are flushed through the bus before returning
        self.drain_emitted_events()?;
        // HARD INVARIANT: RouteTick must be durably appended in same cycle
        if !self.route_tick_append_seen {
            panic!("[FATAL] RouteTick was not durably appended for tick {}", self.runtime_tick);
        }
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
        eprintln!("[EMIT_EVENT ENTRY] kind={}", canon_event::event_kind_str(&event));
        eprintln!("[EMIT_EVENT DIRECT PATH] kind={:?}", canon_event::event_kind_str(&event));
        // If LoopObserved arrives before RouteTick, treat it as opening the cycle
        if let RuntimeEvent::LoopObserved(ref e) = event {
            if self.last_route_tick_emitted != e.tick {
                self.last_route_tick_emitted = e.tick;
            }
            self.last_loop_observed_tick = Some(e.tick);
        }
        // CRITICAL FIX: ensure control events always go through append path
        // before any potential early-return inside handle_runtime_event_located
        // by forcing append via explicit call when emitter pipeline is bypassed
        self.handle_runtime_event_located(event, "", 0)?;
        self.drain_emitted_events()?;
        Ok(())
    }

    pub fn emit_event_with_parents(&mut self, event: RuntimeEvent, parent_ids: Vec<canon_event::EventId>, file: &'static str, line: u32) -> Result<()> {
        eprintln!("[TRACE EMIT_EVENT_WITH_PARENTS] kind={:?}", canon_event::event_kind_str(&event));
        // CRITICAL FIX: ensure append path is not skipped for control events with parents
        self.handle_runtime_event_located_with_parents(event, file, line, parent_ids)?;

        // 🔧 Ensure loop stage is executed immediately for externally injected events
        // (e.g., PlanningCompleted) so observation is not skipped this cycle
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
        // CRITICAL FIX: guard against early replay before consumers are registered
        if self.bus.sync_consumers_len() == 0 {
            return Err(anyhow::anyhow!(
                "replay dispatch failure: no consumers registered for kind={}",
                canon_event::event_kind_str(&event)
            ));
        }
        self.emitter_replay_blocked.store(true, Ordering::SeqCst);
        // Replay path must ALWAYS dispatch exactly once and must not be deduped
        // Dedup here breaks control-event invariants enforced in bus
        let consumer_count = self.bus.dispatch(event.clone(), event_id.clone());
        self.emitter_replay_blocked.store(false, Ordering::SeqCst);

        // Replay must never re-enter the live emitter pipeline.
        // These events are already persisted in the canonical tlog and should only dispatch.
        eprintln!("[REPLAY TRACE] dispatched to {} consumers", consumer_count);
        if consumer_count == 0 {
            const SILENT_KINDS: &[&str] = &["debug", "runtime_state_updated", "code", "edit", "analysis", "cargo", "file", "bash", "llm"];
            let kind_str = canon_event::event_kind_str(&event);
            if !SILENT_KINDS.contains(&kind_str) {
                return Err(anyhow::anyhow!(
                    "replay dispatch failure: event kind={kind_str} id={event_id} delivered to 0 consumers"
                ));
            }
        }
        // Replay is state reconstruction only.
        // Do not synthesize new live events while replaying already-persisted input.
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
            self.observed_events.push(RuntimeEvent::Debug(DebugEvent {
                source: "event-runtime".to_string(),
                kind: "replay_suppressed_derived_event".to_string(),
                payload: serde_json::json!({
                    "replayed_kind": canon_event::event_kind_str(&event),
                    "suppressed_kind": canon_event::event_kind_str(&err_event)
                }),
            }));
        }
        let _ = parent_ids; // parents already encoded in tlog; not needed for bus dispatch
        Ok(())
    }

    fn handle_runtime_event_located(&mut self, event: RuntimeEvent, file: &'static str, line: u32) -> Result<()> {
        eprintln!("[GLOBAL EVENT TRACE] HANDLE {:?}", event);
        self.handle_runtime_event_located_with_parents(event, file, line, Vec::new())
    }

    fn handle_runtime_event_located_with_parents(&mut self, event: RuntimeEvent, file: &'static str, line: u32, parent_ids: Vec<canon_event::EventId>) -> Result<()> {
        // DEBUG TRACE: confirm handler entry + origin discrimination
        eprintln!(
            "[HANDLE ENTRY] kind={} origin={} file={} line={}",
            canon_event::event_kind_str(&event),
            if file.is_empty() { "direct_emit" } else { "emitter_rx_or_located" },
            file,
            line
        );
        let parent_ids = if parent_ids.is_empty() {
            self.last_written_event_id.clone().into_iter().collect()
        } else {
            parent_ids
        };

        // FAIL-FAST CONTRACT: RouteTick must always have a parent (Tick)
        if let RuntimeEvent::RouteTick(ref rt) = event {
            if parent_ids.is_empty() {
                panic!("[FATAL] RouteTick emitted without parent_ids (tick={})", rt.tick);
            }
            // NOTE: do not hard-fail on duplicate here — replay + runtime interleaving
            // can legitimately re-deliver the same logical tick. Exact-once is enforced
            // at persistence + invariant layer, not pre-append.
        }
        if self.is_fatal_halt_active() && !is_allowed_during_fatal_halt(&event) {
            // DEBUG TRACE: confirm fatal halt is blocking emission
            eprintln!("[EMISSION BLOCKED - FATAL HALT] kind={:?}", canon_event::event_kind_str(&event));
            // CRITICAL FIX: allow control events to bypass fatal halt so control loop remains observable
            let is_control = matches!(event,
                RuntimeEvent::Tick(_)
                | RuntimeEvent::RouteTick(_)
                | RuntimeEvent::RouteSelected(_)
                | RuntimeEvent::LoopObserved(_)
            );

            if !is_control {
                self.record_emission_blocked(&event, file, line, parent_ids)?;
                return Ok(());
            } else {
                eprintln!("[FATAL HALT BYPASS] allowing control event despite fatal halt: kind={:?}", canon_event::event_kind_str(&event));
            }
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
        // CRITICAL FIX: allow control-flow events (Tick / RouteTick / RouteSelected)
        // to bypass invariant engine pre-append rejection. These events must ALWAYS persist.
        let is_control = matches!(event,
            RuntimeEvent::Tick(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::RouteSelected(_)
            | RuntimeEvent::LoopObserved(_)
        );

        if !is_control {
            if let Err(reason) = self.invariant_engine.validate_before_append(&event, &parent_ids) {
                eprintln!("[INVARIANT VIOLATION] rejecting event before dispatch: {}", reason);
                eprintln!("[PRE-APPEND DROP TRACE] kind={} parents={:?}", canon_event::event_kind_str(&event), parent_ids);
                self.mode = RuntimeMode::FatalInvariantHalt { reason };
                return Ok(());
            }
        } else {
            eprintln!("[CONTROL BYPASS] allowing control event despite invariant engine: kind={:?}", canon_event::event_kind_str(&event));
        }

        // Append FIRST (canonical write)
        eprintln!("[APPEND CALL] kind={} origin={}", canon_event::event_kind_str(&event), if file.is_empty() { "direct_emit" } else { "emitter_rx_or_located" });
        self.append_runtime_event(&event, file, line, parent_ids.clone(), event_id.clone());

        // Then dispatch
        if self.bus.sync_consumers_len() == 0 {
            eprintln!("[LIVE DISPATCH GUARD] skipping dispatch: no consumers registered yet");
            return Ok(());
        }
        let consumer_count = self.bus.dispatch(event.clone(), event_id.clone());
        // Watchdog: warn if a non-informational event has no consumers.
        if consumer_count == 0 {
            const SILENT_KINDS: &[&str] = &["debug", "runtime_state_updated", "code", "edit", "analysis", "cargo", "file", "bash", "llm"];
            let kind_str = canon_event::event_kind_str(&event);
            if !SILENT_KINDS.contains(&kind_str) {
                eprintln!("[canon-runtime] FATAL: event kind={kind_str} id={event_id} delivered to 0 consumers");
                return Err(anyhow::anyhow!("dispatch failure: no consumers received event"));
            }
        }
        if emit_mode_update {
            let mode_update = RuntimeEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload: self.runtime_mode_update_payload() });
            let mode_update_id = canon_event::EventId::new(canon_event::new_event_id());
            self.bus.dispatch(mode_update.clone(), mode_update_id.clone());
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
        }
        self.drain_emitted_events()?;
        Ok(())
    }

    fn record_emission_blocked(&mut self, _event: &RuntimeEvent, _file: &'static str, _line: u32, _parent_ids: Vec<canon_event::EventId>) -> Result<()> {
        let reason = self.fatal_halt_reason().unwrap_or_else(|| "fatal invariant halt active".to_string());
        let _blocked = RuntimeEvent::Code(Code {
            delta: invariant_violation_delta(format!("emission_blocked; halted_by={}; denied_kind={}", reason, canon_event::event_kind_str(_event))),
            state: invariant_violation_state(),
        });
        let _blocked_id = canon_event::EventId::new(canon_event::new_event_id());
        Ok(())
    }

    pub fn drain_emitted_events(&mut self) -> Result<()> {
        // During replay, preserve control-plane events and drop others
        if self.emitter_replay_blocked.load(std::sync::atomic::Ordering::SeqCst) {
            while let Ok(located) = self.emitter_rx.try_recv() {
                match &located.event {
                    RuntimeEvent::Tick(_)
                    | RuntimeEvent::RouteTick(_)
                    | RuntimeEvent::RouteSelected(_)
                    | RuntimeEvent::LoopObserved(_)
                    | RuntimeEvent::Debug(_) => {
                        eprintln!(
                            "[DRAIN EVENT][replay-control] kind={:?} file={} line={}",
                            canon_event::event_kind_str(&located.event),
                            located.file,
                            located.line
                        );
                        let event = located.event;
                        let file = located.file;
                        let line = located.line;
                        let parent_ids = located.parent_ids.clone();
                        self.handle_runtime_event_located_with_parents(event, file, line, parent_ids)?;
                    }
                    _ => {
                        // Drop non-control emissions during replay
                    }
                }
            }
            return Ok(());
        }
        while let Ok(located) = self.emitter_rx.try_recv() {
            eprintln!("[DRAIN EVENT] kind={:?} file={} line={}", canon_event::event_kind_str(&located.event), located.file, located.line);
            let event = located.event;
            let file = located.file;
            let line = located.line;
            let parent_ids = located.parent_ids.clone();
            eprintln!("[PRE-APPEND CALL] kind={:?}", canon_event::event_kind_str(&event));
            self.handle_runtime_event_located_with_parents(event, file, line, parent_ids)?;
        }
        Ok(())
    }

    fn discard_emitted_events(&mut self) -> usize {
        let mut dropped = 0usize;
        while let Ok(located) = self.emitter_rx.try_recv() {
            dropped = dropped.saturating_add(1);
            eprintln!(
                "[REPLAY DROP EMITTED] kind={:?} file={} line={}",
                canon_event::event_kind_str(&located.event),
                located.file,
                located.line
            );
        }
        dropped
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
        std::fs::write("/tmp/append_probe.log", format!("ENTERED {:?}\n", event)).ok();
        println!("[APPEND ENTRY STDOUT] event={:?}", event);
        // DEBUG TRACE: ensure runtime is actively attempting to write events
        eprintln!("[TLOG APPEND ATTEMPT RAW EVENT] {:?}", event);
        use std::io::Write;
        let _ = std::io::stderr().flush();
        eprintln!("[TLOG APPEND ATTEMPT KIND] kind={:?} file={} line={}", canon_event::event_kind_str(event), file, line);
        let _ = std::io::stderr().flush();
        if self.tlog_path.is_none() {
            panic!("FATAL: tlog_path is None during append_runtime_event for kind={:?}", canon_event::event_kind_str(event));
        }
        let path = self.tlog_path.clone().expect("tlog_path must be set before append");
        if self.tlog_writer.is_none() {
            panic!("FATAL: tlog_writer is None during append_runtime_event for kind={:?}", canon_event::event_kind_str(event));
        }
        eprintln!("[BEFORE WIRE CALL]");

        // HARD INVARIANT: RouteTick must have exactly one parent (Tick)
        if matches!(event, RuntimeEvent::RouteTick(_)) {
            if parent_ids.is_empty() {
                panic!("[FATAL] RouteTick missing parent_ids (must reference Tick)");
            }
            // NOTE: do NOT enforce single parent here — replay / upstream wiring may
            // include additional causal parents. The invariant is non-empty + includes Tick.
        }
        let mut wire = match runtime_event_to_wire(event, parent_ids.clone(), event_id.clone(), file, line) {
            Ok(Some(wire)) => wire,
            Ok(None) => {
                if matches!(event,
                    RuntimeEvent::Tick(_)
                    | RuntimeEvent::RouteTick(_)
                    | RuntimeEvent::RouteSelected(_)
                    | RuntimeEvent::LoopObserved(_)
                ) {
                    panic!("[FATAL] runtime_event_to_wire returned None for control event: kind={:?} parents={:?}", canon_event::event_kind_str(event), parent_ids);
                }
                return;
            }
            Err(err) => {
                if matches!(event,
                    RuntimeEvent::Tick(_)
                    | RuntimeEvent::RouteTick(_)
                    | RuntimeEvent::RouteSelected(_)
                    | RuntimeEvent::LoopObserved(_)
                ) {
                    panic!("[FATAL] runtime_event_to_wire error for control event: kind={:?} parents={:?} err={}", canon_event::event_kind_str(event), parent_ids, err);
                }
                eprintln!("[canon-runtime] append guard rejected kind={} err={}", canon_event::event_kind_str(event), err);
                if !matches!(event, RuntimeEvent::Code(_)) {
                    let _recovery = RuntimeEvent::Code(Code { delta: invariant_violation_delta(err), state: invariant_violation_state() });
                    let _recovery_id = canon_event::EventId::new(canon_event::new_event_id());
                    let _recovery_parents: Vec<_> = self.last_written_event_id.clone().into_iter().collect();
                }
                return;
            }
        };

        // --- Invariant engine ---
        if !self.invariant_engine.observe(&wire, &self.emitter) {
            eprintln!("[INVARIANT REJECT] kind={:?} id={:?}", wire.kind, wire.id);
            // CRITICAL FIX: do NOT drop control driver events — they are required for FSM progression
            if !matches!(event,
                RuntimeEvent::LoopObserved(_) |
                RuntimeEvent::RouteTick(_) |
                RuntimeEvent::Tick(_) |
                RuntimeEvent::RouteSelected(_)
            ) {
                return;
            }
            eprintln!("[INVARIANT OVERRIDE] allowing control driver event to persist despite rejection");
        }

        // --- HARD GUARD: exactly-once LoopObserved per tick ---
        if let RuntimeEvent::LoopObserved(ref lo) = event {
            if let Some(last_tick) = self.last_loop_observed_tick {
                if last_tick == lo.tick {
                    eprintln!("[runtime][drop_duplicate_loop_observed] tick={}", lo.tick);
                    return;
                }
            }
            self.last_loop_observed_tick = Some(lo.tick);
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
        // SPECIAL CASE: LoopObserved must be exactly-once per cycle
        // Apply dedup even though it is a control-like semantic event
        // CRITICAL: ensure RouteTick is NEVER subject to deduplication
        if wire.kind != canon_event::EventKind::RouteTick
            && wire.kind.class() != EventClass::Control
            && !matches!(event,
                RuntimeEvent::RouteTick(_)
                | RuntimeEvent::RouteSelected(_)
                | RuntimeEvent::Tick(_)
                | RuntimeEvent::LoopObserved(_)
            ) {
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
            let writer_arc = self.tlog_writer.as_ref().expect("writer must exist");
            let w = writer_arc.lock().expect("writer lock poisoned during append");
            eprintln!("[PRE-WRITE] kind={} id={}", wire.kind, wire.id);
            if let Err(err) = w.write_canon_event(&wire) {
                // HARD FAIL: control-event persistence must never be silently retried or recovered
                panic!(
                    "[FATAL] append failure (no retry allowed) kind={} id={} err={}",
                    wire.kind,
                    wire.id,
                    err
                );
            } else {
                eprintln!("[WRITE SUCCESS] kind={} id={}", wire.kind, wire.id);
                // MARK: successful durable append of RouteTick
                if matches!(event, RuntimeEvent::RouteTick(_)) {
                    self.route_tick_append_seen = true;
                }
            }
            return;
        }

        let _ = canon_event::write_canon_event_auto(&path, &wire);

        // MARK: successful durable append of RouteTick (non-segment path)
        if matches!(event, RuntimeEvent::RouteTick(_)) {
            self.route_tick_append_seen = true;
        }

        // FIX: Do NOT re-dispatch here.
        // This caused duplicate delivery to consumers (violating async_bus test expectations)
        // because events are already dispatched upstream before append.
        // Re-dispatch here creates re-entrancy and double-processing.
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
    println!("[WIRE ENTRY STDOUT] event={:?}", event);
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
        RuntimeEvent::Tick(p) => (canon_event::EventKind::Tick, payload_from_shape(p, emit_file, emit_line)),
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
        _ => {
            eprintln!("[WIRE DROP] dropping event kind={:?}", canon_event::event_kind_str(event));
            return Ok(None);
        }
    };
    // DEBUG: confirm we passed match and will build wire
    eprintln!("[WIRE BUILD ENTER] kind={:?}", kind);
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
        RuntimeEvent::Tick(_) => "supervisor",
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
        // CRITICAL FIX: recover causal chain instead of rejecting event
        // HARD FAIL: control events must never be synthesized with fake parents
        // Surface the failure explicitly instead of corrupting causal chain
        return Err(format!(
            "missing_parent_ids for non-root event kind={} id={} (emitted from {}:{})",
            kind, event_id, emit_file, emit_line
        ));
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
    replay_blocked: Arc<AtomicBool>,
}

impl EventEmitter for RuntimeEmitterImpl {
    fn emit_with_parents(&self, event: RuntimeEvent, parents: Vec<canon_event::EventId>, file: &'static str, line: u32) {
        // FIX: do not suppress control-plane events when replay_blocked is set
        if self.replay_blocked.load(Ordering::SeqCst) {
            match &event {
                RuntimeEvent::Tick(_)
                | RuntimeEvent::RouteTick(_)
                | RuntimeEvent::RouteSelected(_)
                | RuntimeEvent::LoopObserved(_)
                | RuntimeEvent::Debug(_) => {
                    // allow control events through
                }
                _ => {
                    return;
                }
            }
        }
        let _ = self.sender.send(canon_event::LocatedEvent { event, file, line, parent_ids: parents });
    }

    fn emit_located(&self, event: RuntimeEvent, file: &'static str, line: u32) {
        // FIX: same control-plane exception during replay_blocked
        if self.replay_blocked.load(Ordering::SeqCst) {
            match &event {
                RuntimeEvent::Tick(_)
                | RuntimeEvent::RouteTick(_)
                | RuntimeEvent::RouteSelected(_)
                | RuntimeEvent::LoopObserved(_)
                | RuntimeEvent::Debug(_) => {
                    // allow control events through
                }
                _ => {
                    return;
                }
            }
        }
        let _ = self.sender.send(canon_event::LocatedEvent { event, file, line, parent_ids: Vec::new() });
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
    matches!(event,
        RuntimeEvent::Code(_)
        | RuntimeEvent::ErrorOccurred(_)
        | RuntimeEvent::Debug(_)
        | RuntimeEvent::RuntimeStateUpdated(_)
        // Allow control-flow events to continue during halt
        | RuntimeEvent::Tick(_)
        | RuntimeEvent::RouteTick(_)
        | RuntimeEvent::RouteSelected(_)
    )
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
