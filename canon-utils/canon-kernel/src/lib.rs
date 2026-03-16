use anyhow::Result;
mod bus;
pub mod consumers;

use canon_capability::{CapabilityContext, CapabilityRegistry, CapabilityResult};
use canon_event::emit_debug::{error, info};
use canon_event_store::writer::{append_event_json, BinarySegmentWriter, CanonEvent};
use std::sync::Mutex as StdMutex;
use canon_event_store::{
    extract_capability_request,
    extract_supervisor_event,
    extract_edit_event,
    extract_rustc_event,
    read_any_events_from_path,
    AnyEvent,
};
use canon_event::{
    CapabilityCompleted,
    CapabilityFailed,
    CapabilityRequested,
    EventDelta,
    RustcEvent,
    RustcState,
    RuntimeConsumer,
    RuntimeEmitter,
    RuntimeEmitterHandle,
    RuntimeEvent,
};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use bus::EventBus;

pub struct EventRuntime {
    state: RustcState,
    bus: EventBus,
    registry: std::sync::Arc<std::sync::Mutex<CapabilityRegistry>>,
    tlog_path: Option<std::path::PathBuf>,
    /// Persistent writer for binary tlog directories.
    /// Held open to avoid recover_segment truncation on every append.
    tlog_writer: Option<Arc<StdMutex<BinarySegmentWriter>>>,
    next_id: u64,
    tick: u64,
    runtime_tick: u64,
    runtime_state: serde_json::Value,
    execute_capabilities: bool,
    emitter_rx: crossbeam_channel::Receiver<RuntimeEvent>,
}

pub fn register_default_capabilities(registry: &mut CapabilityRegistry) {
    canon_editor::register_editor_capabilities(registry);
    canon_planner::register_analysis_capabilities(registry);
    canon_supervisor::register_build_capabilities(registry);
}

impl EventRuntime {
    pub fn new(consumers: Vec<Box<dyn RuntimeConsumer>>) -> Self {
        let registry = std::sync::Arc::new(std::sync::Mutex::new(CapabilityRegistry::new()));
        Self::new_with_registry(consumers, registry)
    }

    pub fn new_with_registry(
        consumers: Vec<Box<dyn RuntimeConsumer>>,
        registry: std::sync::Arc<std::sync::Mutex<CapabilityRegistry>>,
    ) -> Self {
        let queue_size = std::env::var("CANON_EVENT_BUS_QUEUE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1024);
        let mut bus = EventBus::new(queue_size);
        let (emitter_tx, emitter_rx) = crossbeam_channel::unbounded();
        let emitter: RuntimeEmitterHandle = Arc::new(RuntimeEmitterImpl { sender: emitter_tx });
        for (idx, consumer) in consumers.into_iter().enumerate() {
            bus.register(format!("consumer_{idx}"), consumer, emitter.clone());
        }
        bus.log_registry();
        Self {
            state: empty_state(),
            bus,
            registry,
            tlog_path: None,
            tlog_writer: None,
            next_id: 1,
            tick: 0,
            runtime_tick: 0,
            runtime_state: serde_json::json!({}),
            execute_capabilities: false,
            emitter_rx,
        }
    }

    pub fn registry_mut(&self) -> std::sync::MutexGuard<'_, CapabilityRegistry> {
        self.registry.lock().expect("capability registry lock")
    }

    pub fn registry_handle(&self) -> std::sync::Arc<std::sync::Mutex<CapabilityRegistry>> {
        std::sync::Arc::clone(&self.registry)
    }

    pub fn set_execute_capabilities(&mut self, enabled: bool) {
        self.execute_capabilities = enabled;
    }

    pub fn set_tlog_path(&mut self, path: std::path::PathBuf) {
        if path.is_dir() {
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
        self.next_id = 1;
        self.tick = 0;
        self.runtime_tick = 0;
        self.runtime_state = serde_json::json!({});
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
                } else if let Some(request) = extract_capability_request(canon) {
                    info(
                        "event_runtime",
                        "capability_requested",
                        serde_json::json!({ "name": request.name, "request_id": request.request_id }),
                    );
                    self.handle_runtime_event(RuntimeEvent::CapabilityRequested(request))?;
                    self.drain_emitted_events()?;
                } else if canon.kind == "runtime_state.updated" {
                    self.handle_runtime_event(RuntimeEvent::RuntimeStateUpdated {
                        payload: canon.payload.clone(),
                    })?;
                    self.drain_emitted_events()?;
                } else if let Some(supervisor_event) = extract_supervisor_event(canon) {
                    if supervisor_event.kind == "workspace.changed" {
                        if let Some(crate_name) = supervisor_event.payload.get("crate").and_then(|v| v.as_str()) {
                            let request = CapabilityRequested {
                                request_id: format!("build-{}-{}", crate_name, self.tick),
                                name: "cargo.build".to_string(),
                                args: serde_json::json!({ "crate": crate_name }),
                            };
                            self.handle_runtime_event(RuntimeEvent::CapabilityRequested(request))?;
                            self.drain_emitted_events()?;
                        }
                    }
                }
            }
            processed += 1;
        }
        Ok(processed)
    }

    pub fn handle_kernel_event(&mut self, event: RustcEvent) -> Result<()> {
        let delta = if matches!(event, RustcEvent::SessionStart { .. }) {
            self.next_id = 1;
            self.tick = 0;
            EventDelta {
                id: 0,
                tick: 0,
                event,
            }
        } else {
            self.tick = self.tick.saturating_add(1);
            let delta = EventDelta {
                id: self.next_id,
                tick: self.tick,
                event,
            };
            self.next_id = self.next_id.saturating_add(1);
            delta
        };

        apply_delta(&mut self.state, &delta)?;
        self.handle_runtime_event(RuntimeEvent::Kernel { delta, state: self.state.clone() })?;
        Ok(())
    }

    pub fn emit_tick(&mut self) -> Result<()> {
        self.runtime_tick = self.runtime_tick.saturating_add(1);
        self.handle_runtime_event(RuntimeEvent::Tick {
            tick: self.runtime_tick,
        })?;
        self.drain_emitted_events()?;
        Ok(())
    }

    fn handle_runtime_event(&mut self, event: RuntimeEvent) -> Result<()> {
        self.bus.dispatch(event.clone());
        if !matches!(event, RuntimeEvent::RuntimeStateUpdated { .. }) {
            self.append_runtime_event(&event);
        }
        match event {
            RuntimeEvent::RuntimeStateUpdated { payload } => {
                self.runtime_state = payload;
            }
            RuntimeEvent::CapabilityRequested(request) => {
                if self.execute_capabilities
                    && self.registry.lock().ok().and_then(|r| r.lookup(&request.name)).is_some()
                {
                    self.handle_capability_request(request)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn drain_emitted_events(&mut self) -> Result<()> {
        while let Ok(event) = self.emitter_rx.try_recv() {
            self.handle_runtime_event(event)?;
        }
        Ok(())
    }

    fn handle_capability_request(&mut self, request: CapabilityRequested) -> Result<()> {
        let request_id = request.request_id.clone();
        let request_name = request.name.clone();
        info(
            "capability_runtime",
            "capability_requested",
            serde_json::json!({ "name": request_name, "request_id": request_id }),
        );
        let ctx = CapabilityContext {
            workspace: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            event: RuntimeEvent::CapabilityRequested(request.clone()),
        };
        let result = match self
            .registry
            .lock()
            .map_err(|_| anyhow::anyhow!("capability registry lock poisoned"))?
            .execute(&request.name, ctx)
        {
            Ok(result) => result,
            Err(err) => {
                error(
                    "capability_runtime",
                    "capability_failed",
                    serde_json::json!({
                        "name": request_name,
                        "request_id": request_id,
                        "error": err.to_string()
                    }),
                );
                let failed = RuntimeEvent::CapabilityFailed(CapabilityFailed {
                    request_id: request_id.clone(),
                    name: request_name.clone(),
                    error: err.to_string(),
                });
                self.bus.dispatch(failed.clone());
                self.append_runtime_event(&failed);
                return Ok(());
            }
        };

        let mut terminal_emitted = false;
        match result {
            CapabilityResult::Emit(event) => {
                terminal_emitted = matches!(
                    event,
                    RuntimeEvent::CapabilityCompleted(_) | RuntimeEvent::CapabilityFailed(_)
                );
                if terminal_emitted {
                    self.append_runtime_event(&event);
                }
                self.bus.dispatch(event);
            }
            CapabilityResult::EmitMany(events) => {
                for event in events {
                    let is_terminal = matches!(
                        event,
                        RuntimeEvent::CapabilityCompleted(_) | RuntimeEvent::CapabilityFailed(_)
                    );
                    if is_terminal {
                        terminal_emitted = true;
                        self.append_runtime_event(&event);
                    }
                    self.bus.dispatch(event);
                }
            }
            CapabilityResult::NoOp => {}
        }

        if !terminal_emitted {
            let completed = RuntimeEvent::CapabilityCompleted(CapabilityCompleted {
                request_id: request_id.clone(),
                name: request_name.clone(),
                result: serde_json::json!({ "status": "ok" }),
            });
            self.bus.dispatch(completed.clone());
            self.append_runtime_event(&completed);
        }
        info(
            "capability_runtime",
            "capability_completed",
            serde_json::json!({ "name": request_name, "request_id": request_id }),
        );
        Ok(())
    }

    fn append_runtime_event(&self, event: &RuntimeEvent) {
        let Some(path) = self.tlog_path.as_ref() else {
            return;
        };
        let canon = match event {
        RuntimeEvent::CapabilityCompleted(payload) => {
            let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
            CanonEvent::new("event-runtime", "capability_completed", val)
        }
        RuntimeEvent::CapabilityFailed(payload) => {
            let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
            CanonEvent::new("event-runtime", "capability_failed", val)
        }
        RuntimeEvent::AgentState { payload } => {
            CanonEvent::new("agent-consumer", "agent_state", payload.clone())
        }
        _ => {
            return;
        }
        };

        if path.is_dir() {
            if let Some(writer_arc) = self.tlog_writer.as_ref() {
                let needs_reopen = if let Ok(w) = writer_arc.lock() {
                    if w.append_event(&canon).is_err() {
                        error(
                            "event_runtime",
                            "append_runtime_event_stale_writer",
                            serde_json::json!({ "path": path.display().to_string() }),
                        );
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if needs_reopen {
                    if let Ok(fresh) = BinarySegmentWriter::open(path) {
                        if let Ok(mut w) = writer_arc.lock() {
                            *w = fresh;
                            let _ = w.append_event(&canon);
                        }
                    }
                }
            }
            return;
        }

        let _ = append_event_json(path, "event-runtime", canon.kind, canon.payload);
    }
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
    sender: crossbeam_channel::Sender<RuntimeEvent>,
}

impl RuntimeEmitter for RuntimeEmitterImpl {
    fn emit(&self, event: RuntimeEvent) {
        let _ = self.sender.send(event);
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
    if delta.id <= state.last_event_id
        && !matches!(delta.event, RustcEvent::SessionStart { .. })
    {
        return Err(anyhow::anyhow!(
            "event id must be monotonic id={} last_event_id={}",
            delta.id,
            state.last_event_id
        ));
    }
    state.tick = delta.tick;
    state.last_event_id = delta.id;
    match &delta.event {
        RustcEvent::NodeDefined { symbol, kind, .. } => {
            state.known_symbols.insert(symbol.clone(), kind.clone());
            state.removed_symbols.remove(symbol);
        }
        RustcEvent::NodeUpdated { symbol, kind, .. } => {
            state.known_symbols.insert(symbol.clone(), kind.clone());
            state.removed_symbols.remove(symbol);
        }
        RustcEvent::NodeRemoved { symbol } => {
            state.known_symbols.remove(symbol);
            state.removed_symbols.insert(symbol.clone());
        }
        RustcEvent::EdgeDefined { src, dst, kind } => {
            state
                .known_edges
                .push((src.clone(), dst.clone(), kind.clone()));
            state
                .removed_edges
                .retain(|e| e != &(src.clone(), dst.clone(), kind.clone()));
        }
        RustcEvent::EdgeRemoved { src, dst, kind } => {
            state
                .known_edges
                .retain(|e| e != &(src.clone(), dst.clone(), kind.clone()));
            state
                .removed_edges
                .push((src.clone(), dst.clone(), kind.clone()));
        }
        RustcEvent::FileSeen { path } => {
            state.known_files.insert(path.clone());
        }
        RustcEvent::CallsiteObserved { .. } => {}
        RustcEvent::SymbolDefined { .. } => {}
        RustcEvent::SpanDefined { .. } => {}
        RustcEvent::PanicCaptured { .. } => {}
        RustcEvent::WarningCaptured { .. } => {}
        RustcEvent::SessionStart { .. } => {
            state.last_event_id = 0;
            state.known_symbols.clear();
            state.known_edges.clear();
            state.known_files.clear();
            state.removed_symbols.clear();
            state.removed_edges.clear();
        }
        RustcEvent::CompilationUnitFinished { .. } => {
            state.phase = "finished".to_string();
        }
        RustcEvent::InvariantViolation { .. } => {}
    }
    let node_count = state.known_symbols.len() as u64;
    let edge_count = state.known_edges.len() as u64;
    state.invariant_hash = compute_invariant_hash(node_count, edge_count, 2);
    Ok(())
}
