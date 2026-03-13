use anyhow::Result;
use canon_tlog_replay::{extract_kernel_event, read_any_events_from_path, AnyEvent};
use canon_types::{EventDelta, EventMask, KernelEvent, KernelEventConsumer, KernelState};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

pub struct EventRuntime {
    state: KernelState,
    consumers: Vec<Box<dyn KernelEventConsumer>>,
    next_id: u64,
    tick: u64,
}

impl EventRuntime {
    pub fn new(consumers: Vec<Box<dyn KernelEventConsumer>>) -> Self {
        Self {
            state: empty_state(),
            consumers,
            next_id: 1,
            tick: 0,
        }
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
        let mask = EventMask::for_event(&delta.event);
        for consumer in &mut self.consumers {
            if consumer.mask().contains(mask) {
                consumer.on_event(&delta, &self.state);
            }
        }
        Ok(())
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
