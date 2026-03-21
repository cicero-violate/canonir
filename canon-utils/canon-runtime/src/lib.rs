use anyhow::Result;
pub mod bootstrap;
mod bus;
pub mod consumers;

use bus::EventBus;
use canon_capability::{CapabilityExecutionContext, CapabilityExecutionResult, CapabilityRegistry};
use canon_event::{
    new_error_occurred, AgentRegistered, CanonEvent, CapabilityCompleted, CapabilityFailed, CapabilityInvoked, CapabilityRequested, CapabilityResolved, Code, DebugEvent, ErrorOccurred, EventConsumer,
    EventDelta, EventEmitter, EventEmitterHandle, GoalEdgeDefined, GoalGraphCheckpointed, GoalNodeCreated, GoalNodeRetracted, GoalNodeRewritten, GoalSelected, PolicyBaselineUpdated, PromptLoaded,
    RuntimeStateUpdated, RustcEvent, RustcState, SystemConfigLoaded, Tick, ToolCall, ToolResult,
};
use canon_event::{BinarySegmentWriter, TlogEvent};
use canon_event::{EdgeDefined, EdgeRemoved, FileSeen, NodeDefined, NodeRemoved, NodeUpdated};
use canon_event_store::{extract_capability_request, extract_edit_event, extract_rustc_event, extract_supervisor_event, read_any_events_from_path, AnyEvent};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

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
    emitter: EventEmitterHandle,
    emitter_rx: crossbeam_channel::Receiver<CanonEvent>,
    observed_events: Vec<CanonEvent>,
}

pub fn register_default_capabilities(registry: &mut CapabilityRegistry) {
    canon_editor::register_editor_capabilities(registry);
    canon_analysis::register_analysis_capabilities(registry);
    canon_builder::register_build_capabilities(registry);
}

impl EventRuntime {
    pub fn new(consumers: Vec<Box<dyn EventConsumer>>) -> Self {
        let registry = std::sync::Arc::new(std::sync::Mutex::new(CapabilityRegistry::new()));
        Self::new_with_registry(consumers, registry)
    }

    pub fn new_with_registry(consumers: Vec<Box<dyn EventConsumer>>, registry: std::sync::Arc<std::sync::Mutex<CapabilityRegistry>>) -> Self {
        let queue_size = std::env::var("CANON_EVENT_BUS_QUEUE").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1024);
        let mut bus = EventBus::new(queue_size);
        let (emitter_tx, emitter_rx) = crossbeam_channel::unbounded();
        let emitter: EventEmitterHandle = Arc::new(RuntimeEmitterImpl { sender: emitter_tx });
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

    pub fn registry_mut(&self) -> std::sync::MutexGuard<'_, CapabilityRegistry> {
        self.registry.lock().expect("capability registry lock")
    }

    pub fn registry_handle(&self) -> std::sync::Arc<std::sync::Mutex<CapabilityRegistry>> {
        std::sync::Arc::clone(&self.registry)
    }

    pub fn set_execute_capabilities(&mut self, enabled: bool) {
        self.execute_capabilities = enabled;
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

    pub fn take_observed_events(&mut self) -> Vec<CanonEvent> {
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
                    self.handle_runtime_event(CanonEvent::Edit(edit))?;
                    self.drain_emitted_events()?;
                } else if let Some(request) = extract_capability_request(canon) {
                    self.handle_runtime_event(CanonEvent::CapabilityRequested(request))?;
                    self.drain_emitted_events()?;
                } else if canon.kind == "runtime_state.updated" {
                    self.handle_runtime_event(CanonEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload: canon.payload.clone() }))?;
                    self.drain_emitted_events()?;
                } else if let Some(supervisor_event) = extract_supervisor_event(canon) {
                    if supervisor_event.kind == "workspace.changed" {
                        let payload = serde_json::json!({
                            "workspace_dirty": true,
                            "crate": supervisor_event.payload.get("crate").cloned().unwrap_or(serde_json::Value::Null),
                            "reason": "workspace_changed",
                        });
                        self.handle_runtime_event(CanonEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload }))?;
                        self.drain_emitted_events()?;
                    }
                } else if canon.kind == "crate_compiled" {
                    // Old-format crate_compiled events (byte_offset/schema) from pre-CompilationUnitFinished
                    // rustc. Trigger analysis for any crate not already handled via rustc_event.
                    if let Some(crate_name) = canon.payload.get("crate").and_then(|v| v.as_str()) {
                        // Skip build scripts and the new duplicate event already paired with
                        // a CompilationUnitFinished rustc_event above.
                        if canon.payload.get("byte_offset").is_some() && crate_name != "build_script_build" {
                            let request = CapabilityRequested {
                                request_id: format!("analysis-{}-{}", crate_name, self.tick),
                                name: "analysis.run".to_string(),
                                args: serde_json::json!({ "crate": crate_name }),
                            };
                            self.handle_runtime_event(CanonEvent::CapabilityRequested(request))?;
                            self.drain_emitted_events()?;
                            let workspace_request =
                                CapabilityRequested { request_id: format!("analysis-workspace-{}", self.tick), name: "analysis.workspace".to_string(), args: serde_json::json!({}) };
                            self.handle_runtime_event(CanonEvent::CapabilityRequested(workspace_request))?;
                            self.drain_emitted_events()?;
                        }
                    }
                } else if canon.kind == "prompt_loaded" && canon.source != "event-runtime" {
                    // Guard mirrors capability_completed: skip W's own re-emission
                    // (source="event-runtime") to break the write-back feedback loop.
                    self.handle_runtime_event(CanonEvent::PromptLoaded(PromptLoaded { payload: canon.payload.clone() }))?;
                    self.drain_emitted_events()?;
                } else if canon.kind == "capability_completed" && canon.source != "event-runtime" {
                    // External capability result written directly to tlog (e.g. by an external executor).
                    // Internal results already flow through the emitter path — skip those to avoid
                    // double-dispatch.
                    if let Ok(payload) = serde_json::from_value::<CapabilityCompleted>(canon.payload.clone()) {
                        self.handle_runtime_event(CanonEvent::CapabilityCompleted(payload))?;
                        self.drain_emitted_events()?;
                    }
                } else if canon.kind == "capability_failed" && canon.source != "event-runtime" {
                    if let Ok(payload) = serde_json::from_value::<CapabilityFailed>(canon.payload.clone()) {
                        self.handle_runtime_event(CanonEvent::CapabilityFailed(payload))?;
                        self.drain_emitted_events()?;
                    }
                } else if canon.kind == "error_occurred" && canon.source != "event-runtime" {
                    if let Ok(payload) = serde_json::from_value::<ErrorOccurred>(canon.payload.clone()) {
                        self.handle_runtime_event(CanonEvent::ErrorOccurred(payload))?;
                        self.drain_emitted_events()?;
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
        self.handle_runtime_event(CanonEvent::Code(Code { delta, state: self.state.clone() }))?;
        if let Some(crate_name) = analysis_crate {
            let request = CapabilityRequested { request_id: format!("analysis-{}-{}", crate_name, self.tick), name: "analysis.run".to_string(), args: serde_json::json!({ "crate": crate_name }) };
            self.handle_runtime_event(CanonEvent::CapabilityRequested(request))?;
            let workspace_request = CapabilityRequested { request_id: format!("analysis-workspace-{}", self.tick), name: "analysis.workspace".to_string(), args: serde_json::json!({}) };
            self.handle_runtime_event(CanonEvent::CapabilityRequested(workspace_request))?;
        }
        Ok(())
    }

    pub fn emit_tick(&mut self) -> Result<()> {
        self.runtime_tick = self.runtime_tick.saturating_add(1);
        self.handle_runtime_event(CanonEvent::Tick(Tick { tick: self.runtime_tick }))?;
        self.drain_emitted_events()?;
        Ok(())
    }

    pub fn emit_debug_event(&mut self, source: String, kind: String, payload: serde_json::Value) -> Result<()> {
        self.handle_runtime_event(CanonEvent::Debug(DebugEvent { source, kind, payload }))?;
        self.drain_emitted_events()?;
        Ok(())
    }

    fn handle_runtime_event(&mut self, event: CanonEvent) -> Result<()> {
        self.observed_events.push(event.clone());
        self.bus.dispatch(event.clone());
        if !matches!(event, CanonEvent::RuntimeStateUpdated(_)) {
            self.append_runtime_event(&event);
        }
        match event {
            CanonEvent::CapabilityFailed(payload) => {
                let error_event = CanonEvent::ErrorOccurred(new_error_occurred(
                    "capability_failed",
                    "event-runtime",
                    payload.error.clone(),
                    "error",
                    serde_json::json!({
                        "request_id": payload.request_id,
                        "capability": payload.name,
                    }),
                    None,
                ));
                self.bus.dispatch(error_event.clone());
                self.append_runtime_event(&error_event);
            }
            CanonEvent::NodeFailed(payload) => {
                let error_event = CanonEvent::ErrorOccurred(new_error_occurred(
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
                self.append_runtime_event(&error_event);
            }
            CanonEvent::Code(Code { delta, .. }) => match &delta.event {
                RustcEvent::PanicCaptured(payload) => {
                    let error_event = CanonEvent::ErrorOccurred(new_error_occurred(
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
                    self.append_runtime_event(&error_event);
                }
                RustcEvent::InvariantViolation(payload) => {
                    let error_event = CanonEvent::ErrorOccurred(new_error_occurred("invariant_violation", "rustc", payload.message.clone(), "error", serde_json::json!({}), None));
                    self.bus.dispatch(error_event.clone());
                    self.append_runtime_event(&error_event);
                }
                _ => {}
            },
            CanonEvent::RuntimeStateUpdated(RuntimeStateUpdated { payload }) => {
                self.runtime_state = payload;
            }
            CanonEvent::CapabilityRequested(request) => {
                // execute_capabilities provides a direct synchronous execution path for
                // capabilities NOT handled asynchronously by bus consumers (e.g. CapabilityExecutor).
                // Skip "llm.call" here: it is always handled asynchronously by LlmCapabilityHandler
                // via the consumer bus. Running it here too would double-enqueue every LLM request.
                if self.execute_capabilities && request.name != "llm.call" && self.registry.lock().ok().and_then(|r| r.lookup(&request.name)).is_some() {
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
        let ctx = CapabilityExecutionContext {
            workspace: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            event: CanonEvent::CapabilityRequested(request.clone()),
            emitter: Some(self.emitter.clone()),
        };
        let execute_result = {
            let registry = self.registry.lock().map_err(|_| anyhow::anyhow!("capability registry lock poisoned"))?;
            registry.execute(&request.name, ctx)
        };
        let result = match execute_result {
            Ok(result) => result,
            Err(err) => {
                let error_event = CanonEvent::ErrorOccurred(new_error_occurred(
                    "capability_execution",
                    "event-runtime",
                    err.to_string(),
                    "error",
                    serde_json::json!({
                        "request_id": request_id.clone(),
                        "capability": request_name.clone(),
                    }),
                    Some(request_id.clone()),
                ));
                self.bus.dispatch(error_event.clone());
                self.append_runtime_event(&error_event);
                let failed = CanonEvent::CapabilityFailed(CapabilityFailed { request_id: request_id.clone(), name: request_name.clone(), error: err.to_string() });
                self.bus.dispatch(failed.clone());
                self.append_runtime_event(&failed);
                return Ok(());
            }
        };

        let mut terminal_emitted = false;
        let mut deferred = false;
        match result {
            CapabilityExecutionResult::Emit(event) => {
                terminal_emitted = matches!(event, CanonEvent::CapabilityCompleted(_) | CanonEvent::CapabilityFailed(_));
                if terminal_emitted {
                    self.append_runtime_event(&event);
                }
                self.bus.dispatch(event);
            }
            CapabilityExecutionResult::EmitMany(events) => {
                for event in events {
                    let is_terminal = matches!(event, CanonEvent::CapabilityCompleted(_) | CanonEvent::CapabilityFailed(_));
                    if is_terminal {
                        terminal_emitted = true;
                        self.append_runtime_event(&event);
                    }
                    self.bus.dispatch(event);
                }
            }
            CapabilityExecutionResult::Deferred => {
                deferred = true;
            }
            CapabilityExecutionResult::NoOp => {}
        }

        if !terminal_emitted && !deferred {
            let completed = CanonEvent::CapabilityCompleted(CapabilityCompleted { request_id: request_id.clone(), name: request_name.clone(), result: serde_json::json!({ "status": "ok" }) });
            self.bus.dispatch(completed.clone());
            self.append_runtime_event(&completed);
        }
        Ok(())
    }

    fn append_runtime_event(&mut self, event: &CanonEvent) {
        let Some(path) = self.tlog_path.clone() else {
            return;
        };
        let mut canon = match event {
            CanonEvent::CapabilityCompleted(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("event-runtime", "capability_completed", val)
            }
            CanonEvent::CapabilityFailed(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("event-runtime", "capability_failed", val)
            }
            CanonEvent::NodeStarted(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("agent-consumer", "node_started", val)
            }
            CanonEvent::NodeCompleted(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("agent-consumer", "node_completed", val)
            }
            CanonEvent::NodeFailed(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("agent-consumer", "node_failed", val)
            }
            CanonEvent::PolicyBaselineUpdated(PolicyBaselineUpdated { payload }) => TlogEvent::new("agent-consumer", "policy_baseline_updated", payload.clone()),
            CanonEvent::GoalSelected(GoalSelected { payload }) => TlogEvent::new("agent-consumer", "goal_selected", payload.clone()),
            CanonEvent::SystemConfigLoaded(SystemConfigLoaded { payload }) => TlogEvent::new("bootstrap", "system_config_loaded", payload.clone()),
            CanonEvent::AgentRegistered(AgentRegistered { payload }) => TlogEvent::new("bootstrap", "agent_registered", payload.clone()),
            CanonEvent::PromptLoaded(PromptLoaded { payload }) => {
                // Use "event-runtime" source so process_events can distinguish W's
                // re-emission from the original bootstrap write and skip it (Rule 9).
                TlogEvent::new("event-runtime", "prompt_loaded", payload.clone())
            }
            CanonEvent::ToolCall(ToolCall { node_id, tool_call_id, request_id, kind, payload }) => TlogEvent::new(
                "agent-consumer",
                "tool_call",
                serde_json::json!({
                    "node_id": node_id,
                    "tool_call_id": tool_call_id,
                    "request_id": request_id,
                    "kind": kind,
                    "payload": payload,
                }),
            ),
            CanonEvent::ToolResult(ToolResult { node_id, tool_call_id, tool_result_id, request_id, kind, output, success }) => TlogEvent::new(
                "agent-consumer",
                "tool_result",
                serde_json::json!({
                    "node_id": node_id,
                    "tool_call_id": tool_call_id,
                    "tool_result_id": tool_result_id,
                    "request_id": request_id,
                    "kind": kind,
                    "output": output,
                    "success": success,
                }),
            ),
            CanonEvent::GoalNodeCreated(GoalNodeCreated { node_id, description, deps, caps, node_type, priority, budget }) => TlogEvent::new(
                "goal_graph",
                "goal_node_created",
                serde_json::json!({
                    "node_id": node_id,
                    "description": description,
                    "deps": deps,
                    "caps": caps,
                    "node_type": node_type,
                    "priority": priority,
                    "budget": budget,
                }),
            ),
            CanonEvent::GoalNodeRetracted(GoalNodeRetracted { node_id }) => TlogEvent::new("goal_graph", "goal_node_retracted", serde_json::json!({ "node_id": node_id })),
            CanonEvent::GoalNodeRewritten(GoalNodeRewritten { node_id, new_description, new_caps }) => TlogEvent::new(
                "goal_graph",
                "goal_node_rewritten",
                serde_json::json!({
                    "node_id": node_id,
                    "new_description": new_description,
                    "new_caps": new_caps,
                }),
            ),
            CanonEvent::GoalEdgeDefined(GoalEdgeDefined { from_node_id, to_node_id }) => TlogEvent::new(
                "goal_graph",
                "goal_edge_defined",
                serde_json::json!({
                    "from_node_id": from_node_id,
                    "to_node_id": to_node_id,
                }),
            ),
            CanonEvent::GoalGraphCheckpointed(GoalGraphCheckpointed { tlog_seq }) => TlogEvent::new("goal_graph", "goal_graph_checkpointed", serde_json::json!({ "tlog_seq": tlog_seq })),
            CanonEvent::CapabilityInvoked(CapabilityInvoked { capability_id, name, node_id }) => TlogEvent::new(
                "capability_graph",
                "capability_invoked",
                serde_json::json!({
                    "capability_id": capability_id,
                    "name": name,
                    "node_id": node_id,
                }),
            ),
            CanonEvent::CapabilityResolved(CapabilityResolved { capability_id, success, duration_ms }) => TlogEvent::new(
                "capability_graph",
                "capability_resolved",
                serde_json::json!({
                    "capability_id": capability_id,
                    "success": success,
                    "duration_ms": duration_ms,
                }),
            ),
            CanonEvent::LoopObserved(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("observe", "loop_observed", val)
            }
            CanonEvent::LoopPlanned(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("plan", "loop_planned", val)
            }
            CanonEvent::LoopActed(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("act", "loop_acted", val)
            }
            CanonEvent::LoopVerified(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("verify", "loop_verified", val)
            }
            CanonEvent::LoopRewarded(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("reward", "loop_rewarded", val)
            }
            CanonEvent::Debug(DebugEvent { source, kind, payload }) => TlogEvent::new(source, kind, payload.clone()),
            CanonEvent::ErrorOccurred(payload) => {
                let val = serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({}));
                TlogEvent::new("event-runtime", "error_occurred", val)
            }
            _ => {
                return;
            }
        };
        canon.event_id = Some(self.next_id);
        self.next_id = self.next_id.saturating_add(1);

        if is_segment_dir_path(&path) {
            if let Some(writer_arc) = self.tlog_writer.as_ref() {
                let needs_reopen = if let Ok(w) = writer_arc.lock() {
                    if w.write_event(&canon).is_err() {
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
                            let _ = w.write_event(&canon);
                        }
                    }
                }
            }
            return;
        }

        let _ = canon_event::write_event_auto(&path, &canon);
    }
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
    sender: crossbeam_channel::Sender<CanonEvent>,
}

impl EventEmitter for RuntimeEmitterImpl {
    fn emit(&self, event: CanonEvent) {
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
                            canon_meta::canon_emit_meta!(emitter; CapabilityRequested(CapabilityRequested {
                                request_id: format!("analysis-k-{}-{}", crate_name, tick),
                                name: "analysis.run".to_string(),
                                args: serde_json::json!({ "crate": crate_name }),
                            }));
                            canon_meta::canon_emit_meta!(emitter; CapabilityRequested(CapabilityRequested {
                                request_id: format!("analysis-workspace-k-{}", tick),
                                name: "analysis.workspace".to_string(),
                                args: serde_json::json!({}),
                            }));
                        }
                    }
                }
            }
        })
        .expect("kernel processor thread")
}
