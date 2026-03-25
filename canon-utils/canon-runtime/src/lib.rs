use anyhow::Result;
pub mod bootstrap;
mod bus;
pub mod consumers;
pub mod hooks;

use bus::EventBus;
use canon_event::BinarySegmentWriter;
use canon_event::{
    new_error_occurred, AnalysisEvent, AnalysisRun, AnalysisWorkspace, CapabilityCompleted, CapabilityFailed, Code, DebugEvent, ErrorOccurred, EventConsumer, EventDelta, EventEmitter,
    EventEmitterHandle, PromptLoaded, RuntimeEvent, RuntimeStateUpdated, RustcEvent, RustcState, Tick,
};
use canon_event::{EdgeDefined, EdgeRemoved, FileSeen, NodeDefined, NodeRemoved, NodeUpdated};
use canon_event_store::{extract_edit_event, extract_rustc_event, read_any_events_from_path, AnyEvent};
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
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

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
}

impl EventRuntime {
    pub fn new(consumers: Vec<Box<dyn EventConsumer>>) -> Self {
        let queue_size = std::env::var("CANON_EVENT_BUS_QUEUE").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1024);
        let hooks = Arc::new(crate::hooks::HookChain::new());
        let mut bus = EventBus::new(queue_size, hooks.clone());
        let (emitter_tx, emitter_rx) = crossbeam_channel::unbounded();
        let emitter: EventEmitterHandle = Arc::new(RuntimeEmitterImpl { sender: emitter_tx });
        for (idx, consumer) in consumers.into_iter().enumerate() {
            bus.register(format!("consumer_{idx}"), consumer, emitter.clone());
        }
        bus.log_registry();
        Self {
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
        }
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
                if let Some(kernel) = extract_rustc_event(canon) {
                    self.handle_kernel_event(kernel)?;
                    self.drain_emitted_events()?;
                } else if let Some(edit) = extract_edit_event(canon) {
                    self.handle_runtime_event(RuntimeEvent::Edit(edit))?;
                    self.drain_emitted_events()?;
                } else {
                    let data = canon.payload.data.clone();
                    let actor = canon.actor.as_str();
                    match canon.kind.as_str() {
                        "runtime_state.updated" => {
                            self.handle_runtime_event(RuntimeEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload: data.clone() }))?;
                            self.drain_emitted_events()?;
                        }
                        "prompt_loaded" if actor != "event-runtime" => {
                            let payload = data.get("data").unwrap_or(&data).clone();
                            self.handle_runtime_event(RuntimeEvent::PromptLoaded(PromptLoaded { payload }))?;
                            self.drain_emitted_events()?;
                        }
                        "capability_completed" if actor != "event-runtime" => {
                            if let Ok(payload_owned) = serde_json::from_value::<CapabilityCompletedOwned>(data.clone()) {
                                let payload =
                                    CapabilityCompleted { request_id: payload_owned.request_id, capability: Box::leak(payload_owned.capability.into_boxed_str()), result: payload_owned.result };
                                self.handle_runtime_event(RuntimeEvent::CapabilityCompleted(payload))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "capability_failed" if actor != "event-runtime" => {
                            if let Ok(payload_owned) = serde_json::from_value::<CapabilityFailedOwned>(data.clone()) {
                                let payload = CapabilityFailed { request_id: payload_owned.request_id, capability: Box::leak(payload_owned.capability.into_boxed_str()), error: payload_owned.error };
                                self.handle_runtime_event(RuntimeEvent::CapabilityFailed(payload))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "error_occurred" if actor != "event-runtime" => {
                            if let Ok(payload) = serde_json::from_value::<ErrorOccurred>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::ErrorOccurred(payload))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "loop_observed" if actor != "observe" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::LoopObserved>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::LoopObserved(decoded))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "loop_planned" if actor != "plan" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::LoopPlanned>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::LoopPlanned(decoded))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "loop_acted" if actor != "act" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::LoopActed>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::LoopActed(decoded))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "loop_verified" if actor != "verify" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::LoopVerified>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::LoopVerified(decoded))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "loop_rewarded" if actor != "reward" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::LoopRewarded>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::LoopRewarded(decoded))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "agent_registered" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::AgentRegistered>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::AgentRegistered(decoded))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "request_dispatch" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::RequestDispatch>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::RequestDispatch(decoded))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "sub_task_result" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::SubTaskResult>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::SubTaskResult(decoded))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "route_tick" if actor != "supervisor" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::RouteTick>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::RouteTick(decoded))?;
                                self.drain_emitted_events()?;
                            }
                        }
                        "route_selected" if actor != "supervisor" => {
                            if let Ok(decoded) = serde_json::from_value::<canon_event::RouteSelected>(data.clone()) {
                                self.handle_runtime_event(RuntimeEvent::RouteSelected(decoded))?;
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
            let run = AnalysisRun { request_id: format!("analysis-{}-{}", crate_name, self.tick), crate_name, batch_id: None };
            self.handle_runtime_event(RuntimeEvent::Analysis(AnalysisEvent::Run(run)))?;
            let workspace = AnalysisWorkspace { request_id: format!("analysis-workspace-{}", self.tick) };
            self.handle_runtime_event(RuntimeEvent::Analysis(AnalysisEvent::Workspace(workspace)))?;
        }
        Ok(())
    }

    pub fn emit_tick(&mut self) -> Result<()> {
        self.runtime_tick = self.runtime_tick.saturating_add(1);
        self.handle_runtime_event_located(RuntimeEvent::Tick(Tick { tick: self.runtime_tick }), "", 0)?;
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
        self.handle_runtime_event_located(event, "", 0)?;
        self.drain_emitted_events()?;
        Ok(())
    }

    fn handle_runtime_event(&mut self, event: RuntimeEvent) -> Result<()> {
        self.handle_runtime_event_located(event, "", 0)
    }

    fn handle_runtime_event_located(&mut self, event: RuntimeEvent, file: &'static str, line: u32) -> Result<()> {
        self.observed_events.push(event.clone());
        self.bus.dispatch(event.clone());
        if !matches!(event, RuntimeEvent::RuntimeStateUpdated(_)) {
            self.append_runtime_event(&event, file, line);
        }
        match event {
            RuntimeEvent::CapabilityFailed(payload) => {
                let error_event = RuntimeEvent::ErrorOccurred(new_error_occurred(
                    "capability_failed",
                    "event-runtime",
                    payload.error.clone(),
                    "error",
                    serde_json::json!({
                        "request_id": payload.request_id,
                        "capability": payload.capability,
                    }),
                    None,
                ));
                self.bus.dispatch(error_event.clone());
                self.append_runtime_event(&error_event, "", 0);
            }
            RuntimeEvent::NodeFailed(payload) => {
                let error_event = RuntimeEvent::ErrorOccurred(new_error_occurred(
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
                ));
                self.bus.dispatch(error_event.clone());
                self.append_runtime_event(&error_event, "", 0);
            }
            RuntimeEvent::Code(Code { delta, .. }) => match &delta.event {
                RustcEvent::PanicCaptured(payload) => {
                    let error_event = RuntimeEvent::ErrorOccurred(new_error_occurred(
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
                    ));
                    self.bus.dispatch(error_event.clone());
                    self.append_runtime_event(&error_event, "", 0);
                }
                RustcEvent::InvariantViolation(payload) => {
                    let error_event = RuntimeEvent::ErrorOccurred(new_error_occurred("invariant_violation", "rustc", payload.message.clone(), "error", serde_json::json!({}), None));
                    self.bus.dispatch(error_event.clone());
                    self.append_runtime_event(&error_event, "", 0);
                }
                _ => {}
            },
            RuntimeEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload }) => {
                self.runtime_state = payload;
            }
            _ => {}
        }
        Ok(())
    }

    fn drain_emitted_events(&mut self) -> Result<()> {
        while let Ok(located) = self.emitter_rx.try_recv() {
            self.handle_runtime_event_located(located.event, located.file, located.line)?;
        }
        Ok(())
    }

    fn append_runtime_event(&mut self, event: &RuntimeEvent, _file: &'static str, _line: u32) {
        let Some(path) = self.tlog_path.clone() else {
            return;
        };
        let Some(wire) = runtime_event_to_wire(event) else {
            return;
        };
        if is_segment_dir_path(&path) {
            if let Some(writer_arc) = self.tlog_writer.as_ref() {
                let needs_reopen = if let Ok(w) = writer_arc.lock() {
                    if w.write_canon_event(&wire).is_err() {
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if needs_reopen {
                    if let Ok(fresh) = BinarySegmentWriter::open(&path) {
                        if let Ok(mut w) = writer_arc.lock() {
                            *w = fresh;
                            let _ = w.write_canon_event(&wire);
                        }
                    }
                }
            }
            return;
        }

        let _ = canon_event::write_canon_event_auto(&path, &wire);
    }
}

fn payload_from_shape<T: canon_event::CanonPayloadShape>(val: &T) -> canon_event::CanonPayload {
    canon_event::CanonPayload {
        input: val.payload_input(),
        output: val.payload_output(),
        delta: val.payload_delta(),
        meta: canon_event::CanonPayloadMeta { file: String::new(), line: 0 },
        data: val.payload_data(),
    }
}

fn runtime_event_to_wire(event: &RuntimeEvent) -> Option<canon_event::CanonEvent> {
    let (kind, payload) = match event {
        RuntimeEvent::LoopObserved(p) => (canon_event::EventKind::LoopObserved, payload_from_shape(p)),
        RuntimeEvent::LoopPlanned(p) => (canon_event::EventKind::LoopPlanned, payload_from_shape(p)),
        RuntimeEvent::LoopActed(p) => (canon_event::EventKind::LoopActed, payload_from_shape(p)),
        RuntimeEvent::LoopVerified(p) => (canon_event::EventKind::LoopVerified, payload_from_shape(p)),
        RuntimeEvent::LoopRewarded(p) => (canon_event::EventKind::LoopRewarded, payload_from_shape(p)),
        RuntimeEvent::RouteTick(p) => (canon_event::EventKind::RouteTick, payload_from_shape(p)),
        RuntimeEvent::RouteSelected(p) => (canon_event::EventKind::RouteSelected, payload_from_shape(p)),
        RuntimeEvent::CapabilityCompleted(p) => (canon_event::EventKind::CapabilityCompleted, payload_from_shape(p)),
        RuntimeEvent::CapabilityFailed(p) => (canon_event::EventKind::CapabilityFailed, payload_from_shape(p)),
        RuntimeEvent::CapabilityInvoked(p) => (canon_event::EventKind::CapabilityInvoked, payload_from_shape(p)),
        RuntimeEvent::CapabilityResolved(p) => (canon_event::EventKind::CapabilityResolved, payload_from_shape(p)),
        RuntimeEvent::ErrorOccurred(p) => (canon_event::EventKind::ErrorOccurred, payload_from_shape(p)),
        RuntimeEvent::Debug(d) => (canon_event::EventKind::Debug, payload_from_shape(d)),
        RuntimeEvent::PromptLoaded(p) => (canon_event::EventKind::PromptLoaded, payload_from_shape(p)),
        RuntimeEvent::RuntimeStateUpdated(p) => (canon_event::EventKind::RuntimeStateUpdated, payload_from_shape(p)),
        RuntimeEvent::ToolCall(p) => (canon_event::EventKind::ToolCall, payload_from_shape(p)),
        RuntimeEvent::ToolResult(p) => (canon_event::EventKind::ToolResult, payload_from_shape(p)),
        RuntimeEvent::GoalNodeCreated(p) => (canon_event::EventKind::GoalNodeCreated, payload_from_shape(p)),
        RuntimeEvent::GoalNodeRetracted(p) => (canon_event::EventKind::GoalNodeRetracted, payload_from_shape(p)),
        RuntimeEvent::GoalNodeRewritten(p) => (canon_event::EventKind::GoalNodeRewritten, payload_from_shape(p)),
        RuntimeEvent::GoalEdgeDefined(p) => (canon_event::EventKind::GoalEdgeDefined, payload_from_shape(p)),
        RuntimeEvent::GoalGraphCheckpointed(p) => (canon_event::EventKind::GoalGraphCheckpointed, payload_from_shape(p)),
        RuntimeEvent::GoodnessSnapshot(p) => (canon_event::EventKind::GoodnessSnapshot, payload_from_shape(p)),
        RuntimeEvent::Llm(p) => (canon_event::EventKind::LlmCall, payload_from_shape(p)),
        _ => return None,
    };
    let actor = match event {
        RuntimeEvent::LoopObserved(_) => "observe",
        RuntimeEvent::LoopPlanned(_) => "plan",
        RuntimeEvent::LoopActed(_) => "act",
        RuntimeEvent::LoopVerified(_) => "verify",
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
        _ => "event-runtime",
    };
    Some(canon_event::CanonEvent::new(
        canon_event::EventId::new(canon_event::new_event_id()),
        Vec::new(),
        actor.to_string(),
        kind,
        now_ms(),
        payload,
        true,
    ))
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
    fn emit_located(&self, event: RuntimeEvent, file: &'static str, line: u32) {
        let _ = self.sender.send(canon_event::LocatedEvent { event, file, line });
    }

    fn emit_with_parents(&self, event: RuntimeEvent, _parents: Vec<canon_event::EventId>, file: &'static str, line: u32) {
        self.emit_located(event, file, line);
    }
}

fn compute_invariant_hash(node_count: u64, edge_count: u64, schema_version: u64) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    node_count.hash(&mut hasher);
    edge_count.hash(&mut hasher);
    schema_version.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
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
        RustcEvent::NodeRemoved(NodeRemoved { symbol }) => {
            state.known_symbols.remove(symbol);
            state.removed_symbols.insert(symbol.clone());
        }
        RustcEvent::EdgeDefined(EdgeDefined { src, dst, kind }) => {
            state.known_edges.push((src.clone(), dst.clone(), kind.clone()));
            state.removed_edges.retain(|e| e != &(src.clone(), dst.clone(), kind.clone()));
        }
        RustcEvent::EdgeRemoved(EdgeRemoved { src, dst, kind }) => {
            state.known_edges.retain(|e| e != &(src.clone(), dst.clone(), kind.clone()));
            state.removed_edges.push((src.clone(), dst.clone(), kind.clone()));
        }
        RustcEvent::FileSeen(FileSeen { path }) => {
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
                            })));
                            canon_meta::canon_emit_meta!(emitter; Analysis(AnalysisEvent::Workspace(AnalysisWorkspace {
                                request_id: format!("analysis-workspace-k-{}", tick),
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
