use anyhow::Result;
mod bus;

use canon_capability::{CapabilityContext, CapabilityRegistry, CapabilityResult};
use canon_event_log::{error, info};
use canon_tlog_writer::{append_event_json, BinarySegmentWriter, CanonEvent};
use canon_tlog_replay::{
    extract_capability_request,
    extract_edit_event,
    extract_kernel_event,
    read_any_events_from_path,
    AnyEvent,
};
use canon_types::{
    CapabilityCompleted,
    CapabilityFailed,
    CapabilityRequested,
    EventDelta,
    KernelEvent,
    KernelState,
    RuntimeConsumer,
    RuntimeEvent,
};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use bus::EventBus;

pub struct EventRuntime {
    state: KernelState,
    bus: EventBus,
    registry: CapabilityRegistry,
    tlog_path: Option<std::path::PathBuf>,
    next_id: u64,
    tick: u64,
}

impl EventRuntime {
    pub fn new(consumers: Vec<Box<dyn RuntimeConsumer>>) -> Self {
        let queue_size = std::env::var("CANON_EVENT_BUS_QUEUE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1024);
        let mut bus = EventBus::new(queue_size);
        for (idx, consumer) in consumers.into_iter().enumerate() {
            bus.register(format!("consumer_{idx}"), consumer);
        }
        bus.log_registry();
        Self {
            state: empty_state(),
            bus,
            registry: CapabilityRegistry::new(),
            tlog_path: None,
            next_id: 1,
            tick: 0,
        }
    }

    pub fn registry_mut(&mut self) -> &mut CapabilityRegistry {
        &mut self.registry
    }

    pub fn set_tlog_path(&mut self, path: std::path::PathBuf) {
        self.tlog_path = Some(path);
    }

    pub fn state(&self) -> &KernelState {
        &self.state
    }

    pub fn reset(&mut self) {
        self.state = empty_state();
        self.next_id = 1;
        self.tick = 0;
    }

    pub fn process_path(&mut self, tlog_path: &std::path::Path) -> Result<usize> {
        let events = read_any_events_from_path(tlog_path)?;
        self.process_events(&events)
    }

    pub fn process_events(&mut self, events: &[AnyEvent]) -> Result<usize> {
        let mut processed = 0usize;
        for event in events {
            if let AnyEvent::Canon(canon) = event {
                if let Some(kernel) = extract_kernel_event(canon) {
                    self.handle_kernel_event(kernel)?;
                } else if let Some(edit) = extract_edit_event(canon) {
                    self.handle_runtime_event(RuntimeEvent::Edit(edit))?;
                } else if let Some(request) = extract_capability_request(canon) {
                    self.handle_runtime_event(RuntimeEvent::CapabilityRequested(request))?;
                }
            }
            processed += 1;
        }
        Ok(processed)
    }

    pub fn handle_kernel_event(&mut self, event: KernelEvent) -> Result<()> {
        let delta = if matches!(event, KernelEvent::SessionStart { .. }) {
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

    fn handle_runtime_event(&mut self, event: RuntimeEvent) -> Result<()> {
        self.bus.dispatch(event.clone());
        match event {
            RuntimeEvent::CapabilityRequested(request) => {
                self.handle_capability_request(request)?;
            }
            _ => {}
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
        let result = match self.registry.execute(&request.name, ctx) {
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
            _ => {
                return;
            }
        };

        if path.is_dir() {
            if let Ok(writer) = BinarySegmentWriter::open(path) {
                let _ = writer.append_event(&canon);
            }
            return;
        }

        let _ = append_event_json(path, "event-runtime", canon.kind, canon.payload);
    }
}

fn empty_state() -> KernelState {
    KernelState {
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

fn compute_invariant_hash(node_count: u64, edge_count: u64, schema_version: u64) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    node_count.hash(&mut hasher);
    edge_count.hash(&mut hasher);
    schema_version.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn apply_delta(state: &mut KernelState, delta: &EventDelta) -> Result<()> {
    if delta.id <= state.last_event_id
        && !matches!(delta.event, KernelEvent::SessionStart { .. })
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
        KernelEvent::NodeDefined { symbol, kind, .. } => {
            state.known_symbols.insert(symbol.clone(), kind.clone());
            state.removed_symbols.remove(symbol);
        }
        KernelEvent::NodeUpdated { symbol, kind, .. } => {
            state.known_symbols.insert(symbol.clone(), kind.clone());
            state.removed_symbols.remove(symbol);
        }
        KernelEvent::NodeRemoved { symbol } => {
            state.known_symbols.remove(symbol);
            state.removed_symbols.insert(symbol.clone());
        }
        KernelEvent::EdgeDefined { src, dst, kind } => {
            state
                .known_edges
                .push((src.clone(), dst.clone(), kind.clone()));
            state
                .removed_edges
                .retain(|e| e != &(src.clone(), dst.clone(), kind.clone()));
        }
        KernelEvent::EdgeRemoved { src, dst, kind } => {
            state
                .known_edges
                .retain(|e| e != &(src.clone(), dst.clone(), kind.clone()));
            state
                .removed_edges
                .push((src.clone(), dst.clone(), kind.clone()));
        }
        KernelEvent::FileSeen { path } => {
            state.known_files.insert(path.clone());
        }
        KernelEvent::CallsiteObserved { .. } => {}
        KernelEvent::SymbolDefined { .. } => {}
        KernelEvent::SpanDefined { .. } => {}
        KernelEvent::PanicCaptured { .. } => {}
        KernelEvent::WarningCaptured { .. } => {}
        KernelEvent::SessionStart { .. } => {
            state.last_event_id = 0;
            state.known_symbols.clear();
            state.known_edges.clear();
            state.known_files.clear();
            state.removed_symbols.clear();
            state.removed_edges.clear();
        }
        KernelEvent::CompilationUnitFinished { .. } => {
            state.phase = "finished".to_string();
        }
        KernelEvent::InvariantViolation { .. } => {}
    }
    let node_count = state.known_symbols.len() as u64;
    let edge_count = state.known_edges.len() as u64;
    state.invariant_hash = compute_invariant_hash(node_count, edge_count, 2);
    Ok(())
}
