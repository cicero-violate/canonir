use canon_planner::planner::dag::{task_graph_resolve_ready, GoalGraph, GoalNode, NodeStatus};
use canon_planner::planner::state_snapshot::PipelineSnapshot;
use canon_planner::planner::capability_types::PipelineCapability;
use canon_planner::planner::decompose::{DecomposeNodeType, DecomposeTaskSpec};
use canon_planner::planner::goal::GoalSpec;
use canon_planner::planner::graph_algo::{
    compute_graph_features_parallel, graph_analysis_compute_graph_signals,
};
use canon_planner::planner::objectives::{
    goal_raw_with_artifact, load_goal_from_reports, maybe_write_baseline,
    objective_task_hints, ObjectiveWeights,
};
use canon_planner::planner::planner_update::{apply_graph_patch, GoalGraphEvent, GoalGraphPatch, PlannerUpdateRewriteSpec};
use canon_event::{
    CapabilityCompleted, CapabilityFailed, CapabilityRequested, EventDelta, RustcState,
    NodeCompleted, NodeFailed, NodeReady, NodeStarted, RuntimeConsumer, RuntimeEmitterHandle,
    RuntimeEvent, RuntimeEventFilter,
};
use canon_event::emit_debug::{info, warn};
use canon_event_store::{read_any_events_from_path_with_start_seq, replay_goal_graph_from_tlog, replay_capability_graph_from_tlog, AnyEvent};
use serde_json::json;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::collections::HashMap;

pub struct AgentConsumer {
    emitter: Arc<Mutex<Option<RuntimeEmitterHandle>>>,
    work_tx: Sender<AgentWork>,
}

impl AgentConsumer {
    pub fn new() -> Self {
        let (work_tx, work_rx) = mpsc::channel();
        let emitter = Arc::new(Mutex::new(None));
        let emitter_handle = Arc::clone(&emitter);
        thread::Builder::new()
            .name("agent_consumer_worker".to_string())
            .spawn(move || {
                let mut state = AgentWorkerState::new();
                for job in work_rx.iter() {
                    state.handle(job, &emitter_handle);
                }
            })
            .expect("agent consumer worker thread");
        Self {
            emitter,
            work_tx,
        }
    }

    fn on_kernel_event(&mut self, _event: &RuntimeEvent) {
        if let RuntimeEvent::Kernel { delta, state } = _event {
            let _ = self.work_tx.send(AgentWork::Kernel {
                delta: delta.clone(),
                state: state.clone(),
            });
        }
    }

    fn on_runtime_event(&mut self, event: &RuntimeEvent) {
        if let RuntimeEvent::Tick { tick } = event {
            let _ = self.work_tx.send(AgentWork::Tick(*tick));
            return;
        }
        if let RuntimeEvent::RuntimeStateUpdated { payload } = event {
            let _ = self.work_tx.send(AgentWork::RuntimeState(payload.clone()));
        }
    }

    fn on_capability_result(&mut self, event: &RuntimeEvent) {
        let _ = self
            .work_tx
            .send(AgentWork::CapabilityResult(event.clone()));
    }
}

impl RuntimeConsumer for AgentConsumer {
    fn filter(&self) -> RuntimeEventFilter {
        RuntimeEventFilter::All
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::Kernel { .. } => self.on_kernel_event(event),
            RuntimeEvent::CapabilityCompleted(_) | RuntimeEvent::CapabilityFailed(_) => {
                self.on_capability_result(event)
            }
            _ => self.on_runtime_event(event),
        }
    }

    fn set_emitter(&mut self, emitter: RuntimeEmitterHandle) {
        if let Ok(mut slot) = self.emitter.lock() {
            *slot = Some(emitter);
        }
    }
}

#[derive(Debug, Clone)]
enum AgentWork {
    Tick(u64),
    CapabilityResult(RuntimeEvent),
    RuntimeState(serde_json::Value),
    Kernel { delta: EventDelta, state: RustcState },
}

struct ExecutorState {
    role: String,
    pending_count: usize,
    results: Vec<(usize, String)>,
}

struct AgentWorkerState {
    graph: GoalGraph,
    pending: HashMap<String, String>,
    last_tick: u64,
    retry_counts: HashMap<String, u32>,
    /// delta_request_id → (node_id, delta_idx, delta_kind)
    delta_to_node: HashMap<String, (String, usize, String)>,
    /// node_id → in-flight executor context
    executor_state: HashMap<String, ExecutorState>,
}

impl AgentWorkerState {
    fn new() -> Self {
        Self {
            graph: GoalGraph::new(),
            pending: HashMap::new(),
            last_tick: 0,
            retry_counts: HashMap::new(),
            delta_to_node: HashMap::new(),
            executor_state: HashMap::new(),
        }
    }

    fn handle(&mut self, work: AgentWork, emitter: &Arc<Mutex<Option<RuntimeEmitterHandle>>>) {
        match work {
            AgentWork::Tick(tick) => {
                self.last_tick = tick;
                if self.graph.nodes.is_empty() {
                    self.try_load_snapshot();
                }
                self.schedule_next(emitter);
                self.emit_state(emitter);
            }
            AgentWork::CapabilityResult(event) => {
                self.apply_result(event, emitter);
                self.schedule_next(emitter);
                self.emit_state(emitter);
            }
            AgentWork::RuntimeState(payload) => {
                self.apply_runtime_state(payload);
                // Do NOT call emit_state here: emitting RuntimeStateUpdated in response to
                // receiving one creates a feedback loop that fills AgentConsumer's bounded
                // channel and permanently blocks the EventBus dispatch thread.
            }
            AgentWork::Kernel { delta, state } => {
                self.observe_kernel(delta, state);
                self.schedule_next(emitter);
                self.emit_state(emitter);
            }
        }
    }

    fn try_load_snapshot(&mut self) {
        let tlog_path = resolve_runtime_tlog_path();
        if !tlog_path.exists() {
            return;
        }
        let events = match read_any_events_from_path_with_start_seq(&tlog_path, 0) {
            Ok(events) => events,
            Err(_) => return,
        };
        // Find the latest agent_state event in the tlog.
        let latest = events.iter().rev().find_map(|e| {
            let AnyEvent::Canon(canon) = e else { return None };
            if canon.kind != "agent_state" { return None }
            serde_json::from_value::<PipelineSnapshot>(canon.payload.clone()).ok()
        });
        let Some(snapshot) = latest else { return };
        self.graph = snapshot.graph;
        self.graph.rebuild_index();
        self.last_tick = snapshot.iteration;
        for node in self.graph.nodes.iter_mut() {
            if node.status == NodeStatus::Running {
                node.status = NodeStatus::Pending;
            }
        }
        self.graph.rebuild_index();
        self.replay_runtime_events_since(snapshot.runtime_start_seq);
    }

    fn apply_runtime_state(&mut self, payload: serde_json::Value) {
        if !self.graph.nodes.is_empty() {
            return;
        }
        let Ok(mut graph) = serde_json::from_value::<GoalGraph>(payload) else {
            return;
        };
        graph.rebuild_index();
        self.graph = graph;
    }

    fn observe_kernel(&mut self, _delta: EventDelta, state: RustcState) {
        // Minimal observe: sync tick and load snapshot if graph is empty.
        if state.tick > self.last_tick {
            self.last_tick = state.tick;
        }
        if self.graph.nodes.is_empty() {
            self.try_load_snapshot();
        }
    }

    fn schedule_next(&mut self, emitter: &Arc<Mutex<Option<RuntimeEmitterHandle>>>) {
        task_graph_resolve_ready(&mut self.graph);
        let mut maybe_node = self.graph.ready_nodes().into_iter().next().cloned();
        if maybe_node.is_none() {
            if self.plan_if_stalled(emitter) {
                task_graph_resolve_ready(&mut self.graph);
                maybe_node = self.graph.ready_nodes().into_iter().next().cloned();
            }
        }
        let Some(node) = maybe_node else {
            return;
        };
        let Some(emitter) = emitter.lock().ok().and_then(|slot| slot.clone()) else {
            return;
        };
        let Some(capability_name) = capability_name_for_node(&node) else {
            let _ = self.graph.update_status(&node.id, NodeStatus::Failed);
            if let Some(node_mut) = self.graph.get_node_mut(&node.id) {
                node_mut.error = Some("unsupported capability".to_string());
            }
            warn(
                "agent_consumer",
                "node_failed_unsupported_capability",
                serde_json::json!({ "node_id": node.id }),
            );
            return;
        };
        let Some(args) = build_capability_args(&node, capability_name) else {
            let _ = self.graph.update_status(&node.id, NodeStatus::Failed);
            if let Some(node_mut) = self.graph.get_node_mut(&node.id) {
                node_mut.error = Some("missing capability args".to_string());
            }
            warn(
                "agent_consumer",
                "node_failed_missing_args",
                serde_json::json!({ "node_id": node.id, "capability": capability_name }),
            );
            return;
        };
        let request_id = format!("node-{}-{}", node.id, self.last_tick);
        let _ = self.graph.update_status(&node.id, NodeStatus::Running);
        emitter.emit(RuntimeEvent::NodeReady(NodeReady {
            node_id: node.id.clone(),
            capability: capability_name.to_string(),
            request_id: request_id.clone(),
            args: args.clone(),
        }));
        emitter.emit(RuntimeEvent::NodeStarted(NodeStarted {
            node_id: node.id.clone(),
            capability: capability_name.to_string(),
            request_id: request_id.clone(),
        }));
        emitter.emit(RuntimeEvent::CapabilityInvoked {
            capability_id: request_id.clone(),
            name: capability_name.to_string(),
            node_id: node.id.clone(),
        });
        info(
            "agent_consumer",
            "node_started",
            serde_json::json!({ "node_id": node.id, "capability": capability_name }),
        );
        self.pending.insert(request_id.clone(), node.id.clone());
        // NOTE: CapabilityRequested is emitted by EventLoopConsumer in response to NodeReady.
        // Do NOT emit it here — doing so would cause duplicate processing in LlmExecutorConsumer.
    }

    fn apply_result(
        &mut self,
        event: RuntimeEvent,
        emitter: &Arc<Mutex<Option<RuntimeEmitterHandle>>>,
    ) {
        self.apply_result_with_options(event, emitter, true);
    }

    fn apply_result_replay(&mut self, event: RuntimeEvent) {
        self.apply_result_with_options(event, &Arc::new(Mutex::new(None)), false);
    }

    fn apply_result_with_options(
        &mut self,
        event: RuntimeEvent,
        emitter: &Arc<Mutex<Option<RuntimeEmitterHandle>>>,
        plan_and_persist: bool,
    ) {
        let (request_id, capability_name, success, stdout, stderr, result_value) = match event {
            RuntimeEvent::CapabilityCompleted(payload) => {
                let success = payload
                    .result
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let stdout = payload
                    .result
                    .get("stdout")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let stderr = payload
                    .result
                    .get("stderr")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let result_value = payload.result.get("result").cloned();
                (
                    payload.request_id,
                    payload.name,
                    success,
                    stdout,
                    stderr,
                    result_value,
                )
            }
            RuntimeEvent::CapabilityFailed(payload) => (
                payload.request_id,
                payload.name,
                false,
                String::new(),
                payload.error,
                None,
            ),
            _ => return,
        };
        // --- Delta tool-call completion ---
        if let Some((orig_node_id, delta_idx, delta_kind)) = self.delta_to_node.remove(&request_id) {
            let output_text = if success { stdout.clone() } else { stderr.clone() };
            if let Some(emit) = emitter.lock().ok().and_then(|s| s.clone()) {
                emit.emit(RuntimeEvent::ToolResult {
                    node_id: orig_node_id.clone(),
                    request_id: request_id.clone(),
                    kind: delta_kind,
                    output: serde_json::json!({ "stdout": output_text, "success": success }),
                    success,
                });
            }
            if let Some(exec) = self.executor_state.get_mut(&orig_node_id) {
                exec.results.push((delta_idx, output_text));
                if exec.results.len() >= exec.pending_count {
                    // All tool calls done — build follow-up prompt and re-call LLM
                    let mut exec = self.executor_state.remove(&orig_node_id).unwrap();
                    exec.results.sort_by_key(|(idx, _)| *idx);
                    let mut follow_up = String::from("[TOOL RESULTS]\n");
                    for (i, out) in &exec.results {
                        follow_up.push_str(&format!("\nResult {}:\n{}\n", i, out));
                    }
                    follow_up.push_str("\nBased on the above results, provide your final analysis and return your json block with empty deltas.");
                    let followup_id = format!("exec-followup-{}-{}", orig_node_id, self.last_tick);
                    self.pending.insert(followup_id.clone(), orig_node_id.clone());
                    if let Some(emit) = emitter.lock().ok().and_then(|s| s.clone()) {
                        emit.emit(RuntimeEvent::CapabilityRequested(canon_event::CapabilityRequested {
                            request_id: followup_id,
                            name: "llm.call".to_string(),
                            args: serde_json::json!({ "prompt": follow_up, "role": exec.role, "raw": true }),
                        }));
                    }
                }
            }
            return;
        }

        let node_id = match self.pending.remove(&request_id) {
            Some(node_id) => node_id,
            None => {
                let Some(parsed) = parse_node_id_from_request_id(&request_id) else {
                    return;
                };
                if self.graph.get_node(&parsed).is_some() {
                    parsed
                } else {
                    return;
                }
            }
        };
        // Guard against double-processing (both live and replay paths)
        if let Some(node) = self.graph.get_node(&node_id) {
            if matches!(node.status, NodeStatus::Completed | NodeStatus::Failed) {
                return;
            }
        }
        let new_status = if success {
            NodeStatus::Completed
        } else {
            NodeStatus::Failed
        };
        let _ = self.graph.update_status(&node_id, new_status);
        if plan_and_persist {
            info(
                "agent_consumer",
                if success { "node_completed" } else { "node_failed" },
                serde_json::json!({ "node_id": node_id, "capability": capability_name }),
            );
        }
        if plan_and_persist {
            if let Some(emitter) = emitter.lock().ok().and_then(|slot| slot.clone()) {
                if success {
                    emitter.emit(RuntimeEvent::NodeCompleted(NodeCompleted {
                        node_id: node_id.clone(),
                        capability: capability_name.clone(),
                        request_id: request_id.clone(),
                    }));
                } else {
                    emitter.emit(RuntimeEvent::NodeFailed(NodeFailed {
                        node_id: node_id.clone(),
                        capability: capability_name.clone(),
                        error: Some(stderr.clone()),
                        request_id: request_id.clone(),
                    }));
                }
                emitter.emit(RuntimeEvent::CapabilityResolved {
                    capability_id: request_id.clone(),
                    success,
                    duration_ms: 0,
                });
            }
        }
        // --- Executor delta dispatch ---
        // If the LLM returned executor-format JSON (results array with deltas), dispatch
        // each delta as a sub-capability and keep the node Running.
        if success && capability_name == "llm.call" {
            if let Some(result) = result_value.as_ref() {
                let text = result.get("text").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(deltas) = parse_executor_deltas(text) {
                    if !deltas.is_empty() {
                        let _ = self.graph.update_status(&node_id, NodeStatus::Running);
                        let role = self.graph.get_node(&node_id)
                            .map(|n| if matches!(n.node_type, canon_planner::planner::decompose::DecomposeNodeType::Analysis) { "planner" } else { "exec" })
                            .unwrap_or("exec")
                            .to_string();
                        let exec_state = ExecutorState {
                            role,
                            pending_count: deltas.len(),
                            results: Vec::new(),
                        };
                        self.executor_state.insert(node_id.clone(), exec_state);
                        if let Some(emit) = emitter.lock().ok().and_then(|s| s.clone()) {
                            for (idx, delta) in deltas.iter().enumerate() {
                                let kind = delta.get("type").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                                let delta_id = format!("delta-{}-{}-{}", node_id, idx, self.last_tick);
                                let cap_args = delta_to_cap_args(delta);
                                self.delta_to_node.insert(delta_id.clone(), (node_id.clone(), idx, kind.clone()));
                                emit.emit(RuntimeEvent::ToolCall {
                                    node_id: node_id.clone(),
                                    request_id: delta_id.clone(),
                                    kind,
                                    payload: delta.clone(),
                                });
                                emit.emit(RuntimeEvent::CapabilityRequested(canon_event::CapabilityRequested {
                                    request_id: delta_id,
                                    name: "bash".to_string(),
                                    args: cap_args,
                                }));
                            }
                        }
                        if plan_and_persist {
                            self.persist_snapshot(emitter);
                        }
                        return;
                    }
                }
            }
        }

        let patch_to_apply = if success && capability_name == "llm.call" {
            result_value
                .as_ref()
                .and_then(extract_graph_patch_from_llm_result)
        } else {
            None
        };
        if let Some(node) = self.graph.get_node_mut(&node_id) {
            if success {
                if capability_name == "llm.call" {
                    node.result = result_value.as_ref().and_then(|v| serde_json::to_string(v).ok());
                    // Log the LLM response to the reports directory for observability
                    if plan_and_persist {
                        // raw=true: result={"text":"..."}, raw=false: result is parsed JSON directly
                        let text = result_value.as_ref().map(|v| {
                            v.get("text")
                                .and_then(|t| t.as_str())
                                .map(str::to_string)
                                .unwrap_or_else(|| serde_json::to_string_pretty(v).unwrap_or_default())
                        }).unwrap_or_default();
                        append_llm_response_log(&node_id, &request_id, &text);
                    }
                } else if !stdout.is_empty() {
                    node.result = Some(stdout);
                }
            } else if !stderr.is_empty() {
                node.error = Some(stderr);
            }
        }
        if let Some(patch) = patch_to_apply {
            if let Ok(graph_events) = apply_graph_patch(&mut self.graph, patch) {
                self.graph.rebuild_index();
                if plan_and_persist {
                    if let Some(emitter_handle) = emitter.lock().ok().and_then(|s| s.clone()) {
                        emit_goal_graph_events(&emitter_handle, graph_events);
                    }
                }
            } else {
                self.graph.rebuild_index();
            }
        }
        if plan_and_persist {
            let _ = self.plan_if_stalled(emitter);
            self.persist_snapshot(emitter);
        }
    }

    fn plan_if_stalled(&mut self, emitter: &Arc<Mutex<Option<RuntimeEmitterHandle>>>) -> bool {
        let signals = graph_analysis_compute_graph_signals(&self.graph);
        let features = compute_graph_features_parallel(&self.graph);

        let mut update = GoalGraphPatch {
            new_nodes: Vec::new(),
            new_edges: Vec::new(),
            retract_nodes: Vec::new(),
            rewrite_nodes: Vec::new(),
        };

        if self.graph.nodes.is_empty() {
            self.seed_orchestration(&mut update, emitter);
        } else {
            let failed_nodes: Vec<GoalNode> = self
                .graph
                .nodes
                .iter()
                .filter(|n| n.status == NodeStatus::Failed)
                .cloned()
                .collect();
            let has_retry_left = failed_nodes.iter().any(|n| {
                self.retry_counts
                    .get(&n.id)
                    .copied()
                    .unwrap_or(0)
                    < 1
            });
            if let Some(node) = failed_nodes.first() {
                let retries = self.retry_counts.entry(node.id.clone()).or_insert(0);
                if *retries < 1 {
                    *retries += 1;
                    update.rewrite_nodes.push(PlannerUpdateRewriteSpec {
                        id: node.id.clone(),
                        new_description: node.description.clone(),
                        new_capabilities: node.required_capabilities.clone(),
                    });
                }
            }
            let all_blocked = self
                .graph
                .nodes
                .iter()
                .all(|n| n.status == NodeStatus::Blocked);
            let all_failed = !self.graph.nodes.is_empty()
                && failed_nodes.len() == self.graph.nodes.len();
            let stalled = all_blocked
                || (all_failed && !has_retry_left)
                || (features.ready_fraction == 0.0
                    && features.blocked_fraction > 0.0
                    && signals.has_cycle);
            if stalled {
                let reason = if all_blocked {
                    "all nodes blocked".to_string()
                } else if all_failed && !has_retry_left {
                    "all nodes failed".to_string()
                } else if self.graph.nodes.is_empty() {
                    "empty graph".to_string()
                } else {
                    "deadlock detected".to_string()
                };
                let id = unique_node_id("stall_replan", &self.graph);
                update.new_nodes.push(DecomposeTaskSpec {
                    id,
                    description: format!(
                        "Graph stalled ({reason}). Analyze current state and propose next steps."
                    ),
                    deps: Vec::new(),
                    required_capabilities: vec![PipelineCapability::Llm],
                    node_type: DecomposeNodeType::Analysis,
                    priority: 1,
                    budget: None,
                    reasoning_trace: Some("AUTO_REPLAN: stalled graph".to_string()),
                });
            }
        }

        if update.new_nodes.is_empty()
            && update.new_edges.is_empty()
            && update.retract_nodes.is_empty()
            && update.rewrite_nodes.is_empty()
        {
            return false;
        }

        if let Ok(_graph_events) = apply_graph_patch(&mut self.graph, update) {
            self.graph.rebuild_index();
            return true;
        }
        false
    }

    fn emit_state(&self, emitter: &Arc<Mutex<Option<RuntimeEmitterHandle>>>) {
        let Some(emitter) = emitter.lock().ok().and_then(|slot| slot.clone()) else {
            return;
        };
        if let Ok(payload) = serde_json::to_value(&self.graph) {
            emitter.emit(RuntimeEvent::RuntimeStateUpdated { payload });
        }
    }

    fn seed_orchestration(&mut self, update: &mut GoalGraphPatch, emitter: &Arc<Mutex<Option<RuntimeEmitterHandle>>>) {
        if !update.new_nodes.is_empty() {
            return;
        }
        let mut description =
            "Analyse system state and produce initial task decomposition".to_string();
        if let Some(selection) = load_goal_from_reports(ObjectiveWeights::default(), Some(&self.graph)) {
            maybe_write_baseline(&selection);
            if let Ok(payload) = serde_json::to_value(&selection.artifact) {
                if let Some(emit) = emitter.lock().ok().and_then(|slot| slot.clone()) {
                    emit.emit(RuntimeEvent::GoalSelected { payload: payload.clone() });
                    emit.emit(RuntimeEvent::PolicyBaselineUpdated { payload });
                }
            }
            let mut goal = goal_raw_with_artifact("", &selection.artifact);
            let hints = objective_task_hints(&selection.artifact);
            if !hints.is_empty() {
                goal.push_str("\n\nTASK_HINTS:\n");
                for hint in hints {
                    goal.push_str("- ");
                    goal.push_str(&hint);
                    goal.push('\n');
                }
            }
            description = format!("{description}\n\n{goal}");
        }
        update.new_nodes.push(DecomposeTaskSpec {
            id: "seed_0".to_string(),
            description,
            deps: Vec::new(),
            required_capabilities: vec![PipelineCapability::Llm],
            node_type: DecomposeNodeType::Analysis,
            priority: 1,
            budget: None,
            reasoning_trace: Some("AUTO_SEED: empty graph".to_string()),
        });
    }

    fn persist_snapshot(&self, emitter: &Arc<Mutex<Option<RuntimeEmitterHandle>>>) {
        let tlog_seq = latest_segment_seq(&resolve_runtime_tlog_path()).unwrap_or(0);
        let snapshot = PipelineSnapshot {
            graph: self.graph.clone(),
            iteration: self.last_tick,
            runtime_start_seq: tlog_seq,
            goal: GoalSpec::new(String::new(), 0),
        };
        if let Ok(payload) = serde_json::to_value(&snapshot) {
            if let Some(emitter_handle) = emitter.lock().ok().and_then(|slot| slot.clone()) {
                emitter_handle.emit(RuntimeEvent::AgentState { payload });
                emitter_handle.emit(RuntimeEvent::GoalGraphCheckpointed { tlog_seq });
            }
        }
        write_graph_report(&self.graph, self.last_tick);
    }

    fn replay_runtime_events_since(&mut self, start_seq: u64) {
        let tlog_path = resolve_runtime_tlog_path();
        if !tlog_path.exists() {
            return;
        }
        let events = match read_any_events_from_path_with_start_seq(&tlog_path, start_seq) {
            Ok(events) => events,
            Err(_) => return,
        };
        for event in events {
            let AnyEvent::Canon(canon) = event else {
                continue;
            };
            match canon.kind.as_str() {
                "capability_requested" => {
                    if let Ok(req) = serde_json::from_value::<CapabilityRequested>(canon.payload.clone()) {
                        if let Some(node_id) = parse_node_id_from_request_id(&req.request_id) {
                            if self.graph.get_node(&node_id).is_some() {
                                self.pending.insert(req.request_id, node_id);
                            }
                        }
                    }
                }
                "capability_completed" => {
                    if let Ok(payload) = serde_json::from_value::<CapabilityCompleted>(canon.payload.clone()) {
                        self.apply_result_replay(RuntimeEvent::CapabilityCompleted(payload));
                    }
                }
                "capability_failed" => {
                    if let Ok(payload) = serde_json::from_value::<CapabilityFailed>(canon.payload.clone()) {
                        self.apply_result_replay(RuntimeEvent::CapabilityFailed(payload));
                    }
                }
                _ => {}
            }
        }
    }
}

fn capability_name_for_node(node: &GoalNode) -> Option<&'static str> {
    use canon_planner::planner::decompose::DecomposeNodeType;
    for cap in &node.required_capabilities {
        match cap {
            PipelineCapability::FileRead => return Some("file.read"),
            PipelineCapability::FileWrite | PipelineCapability::ApplyPatch => {
                return Some("file.write")
            }
            PipelineCapability::Bash => return Some("bash"),
            PipelineCapability::CargoBuild => return Some("cargo.build"),
            PipelineCapability::CargoCheck => return Some("cargo.check"),
            PipelineCapability::Llm | PipelineCapability::Analysis => return Some("llm.call"),
            _ => {}
        }
    }
    // Fallback: analysis nodes with any unrecognised capability set are dispatched to the LLM.
    // This covers ReadDag, ComputeDelta, StatelessInvoke, InvariantCheck, etc.
    if matches!(node.node_type, DecomposeNodeType::Analysis) {
        return Some("llm.call");
    }
    None
}

fn build_capability_args(node: &GoalNode, capability: &str) -> Option<serde_json::Value> {
    if capability == "llm.call" {
        use canon_planner::planner::decompose::DecomposeNodeType;
        let prompt = node.description.clone();
        // Analysis nodes go to the planner endpoint (returns graph patches).
        // All other nodes go to the exec endpoint (returns delta tool calls).
        let (raw, role) = if matches!(node.node_type, DecomposeNodeType::Analysis) {
            (false, "planner")
        } else {
            (true, "exec")
        };
        return Some(json!({ "prompt": prompt, "raw": raw, "role": role }));
    }
    if let Some(args) = parse_inline_json(&node.description) {
        return Some(args);
    }
    match capability {
        "file.read" => extract_path(&node.description).map(|path| json!({ "path": path })),
        "file.write" => {
            let path = extract_path(&node.description)?;
            let content = extract_field(&node.description, "content")
                .unwrap_or_else(|| String::new());
            Some(json!({ "path": path, "content": content }))
        }
        "bash" => {
            let cmd = extract_field(&node.description, "cmd")
                .unwrap_or_else(|| node.description.trim().to_string());
            Some(json!({ "cmd": cmd }))
        }
        "cargo.build" => {
            let crate_name = extract_field(&node.description, "crate")?;
            Some(json!({ "crate": crate_name }))
        }
        "cargo.check" => {
            let crate_name = extract_field(&node.description, "crate")?;
            Some(json!({ "crate": crate_name }))
        }
        _ => None,
    }
}

fn parse_inline_json(text: &str) -> Option<serde_json::Value> {
    let s = parse_inline_json_str(text)?;
    serde_json::from_str(&s).ok()
}

/// Extract the outermost `{...}` span from text as a String without parsing.
fn parse_inline_json_str(text: &str) -> Option<String> {
    let start = text.find('{')?;
    // Walk forward from start to find the matching closing brace
    let mut depth = 0usize;
    let mut end = None;
    let chars = text[start..].char_indices();
    for (i, c) in chars {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    Some(text[start..=end].to_string())
}

/// Parse executor-format LLM response. Returns Some(deltas) if the text contains
/// a `{"results":[{"deltas":[...]}]}` block. Returns None if it's not executor format.
/// Returns Some(empty vec) if executor format but no tool calls (done).
fn parse_executor_deltas(text: &str) -> Option<Vec<serde_json::Value>> {
    // Find a fenced JSON block first; fall back to bare JSON
    let json_str = extract_fenced_json(text).unwrap_or_else(|| text.to_string());
    let val: serde_json::Value = serde_json::from_str(json_str.trim()).ok()?;
    let results = val.get("results")?.as_array()?;
    let deltas: Vec<serde_json::Value> = results
        .iter()
        .filter_map(|r| r.get("deltas")?.as_array().cloned())
        .flatten()
        .collect();
    Some(deltas)
}

fn extract_fenced_json(text: &str) -> Option<String> {
    let start = text.find("```json")
        .map(|i| i + 7)
        .or_else(|| text.find("```\n{").map(|i| i + 3))?;
    let end = text[start..].find("```")?;
    Some(text[start..start + end].trim().to_string())
}

/// Convert an executor delta into a bash `cmd` string.
fn delta_to_cap_args(delta: &serde_json::Value) -> serde_json::Value {
    let kind = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let cmd = match kind {
        "read_file" => {
            let path = delta.get("path").and_then(|v| v.as_str()).unwrap_or("/");
            format!("cat {}", shell_quote(path))
        }
        "list_dir" => {
            let path = delta.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            format!("ls -la {}", shell_quote(path))
        }
        "read_command" => {
            let command = delta.get("command").and_then(|v| v.as_str()).unwrap_or("echo");
            let args = delta.get("args")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter()
                    .filter_map(|a| a.as_str())
                    .map(shell_quote)
                    .collect::<Vec<_>>()
                    .join(" "))
                .unwrap_or_default();
            let path = delta.get("path").and_then(|v| v.as_str());
            if let Some(p) = path {
                format!("cd {} && {} {}", shell_quote(p), command, args)
            } else {
                format!("{} {}", command, args)
            }
        }
        _ => format!("echo 'unknown delta type: {}'", kind),
    };
    serde_json::json!({ "cmd": cmd })
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn extract_graph_patch_from_llm_result(
    result: &serde_json::Value,
) -> Option<canon_planner::planner::planner_update::GoalGraphPatch> {
    use canon_planner::planner::planner_update::GoalGraphPatch;

    // raw=false (analysis/planner) path: result IS the parsed JSON value directly.
    // Try deserialising it straight into a GoalGraphPatch first.
    if result.get("text").is_none() && result.get("results").is_none() {
        if let Ok(patch) = serde_json::from_value::<GoalGraphPatch>(result.clone()) {
            return Some(patch);
        }
    }

    // raw=true (executor) path: result = {"text": "..."}. Parse the text string.
    let text = result.get("text").and_then(|v| v.as_str())?;
    // Try fenced JSON block first (```json ... ```), then fall back to brace-depth scan.
    let json_str = extract_fenced_json(text)
        .or_else(|| parse_inline_json_str(text))?;
    let parsed = serde_json::from_str::<serde_json::Value>(&json_str).ok()?;
    if parsed.get("results").is_some() {
        return None; // executor format — not a GoalGraphPatch
    }
    let patch = serde_json::from_value::<GoalGraphPatch>(parsed).ok()?;
    if patch.new_nodes.is_empty() && patch.new_edges.is_empty()
        && patch.retract_nodes.is_empty() && patch.rewrite_nodes.is_empty()
    {
        canon_event::emit_debug::warn(
            "agent_consumer",
            "graph_patch_empty",
            serde_json::json!({ "text_preview": &text[..text.len().min(200)] }),
        );
    }
    Some(patch)
}

fn extract_path(text: &str) -> Option<String> {
    extract_field(text, "path").or_else(|| {
        text.split_whitespace()
            .find(|tok| tok.starts_with('/') || tok.starts_with("./"))
            .map(|tok| tok.to_string())
    })
}

fn extract_field(text: &str, key: &str) -> Option<String> {
    let pattern = format!("{key}=");
    if let Some(idx) = text.find(&pattern) {
        let value = &text[idx + pattern.len()..];
        return Some(value.split_whitespace().next().unwrap_or("").trim().to_string());
    }
    let pattern = format!("{key}:");
    if let Some(idx) = text.find(&pattern) {
        let value = &text[idx + pattern.len()..];
        return Some(value.split_whitespace().next().unwrap_or("").trim().to_string());
    }
    None
}

fn resolve_runtime_tlog_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("CANON_TLOG_PATH") {
        return std::path::PathBuf::from(path);
    }
    let binary = std::path::PathBuf::from(
        "/workspace/ai_sandbox/canon/state/event_log/event.tlog.d",
    );
    if binary.exists() {
        return binary;
    }
    std::path::PathBuf::from("/workspace/ai_sandbox/canon/state/event_log/event.tlog")
}

fn latest_segment_seq(tlog_path: &std::path::Path) -> anyhow::Result<u64> {
    if !tlog_path.is_dir() {
        return Ok(0);
    }
    let mut max_seq = 0u64;
    for entry in std::fs::read_dir(tlog_path)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("log") {
            continue;
        }
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            if let Ok(seq) = stem.parse::<u64>() {
                if seq > max_seq {
                    max_seq = seq;
                }
            }
        }
    }
    Ok(max_seq)
}

fn parse_node_id_from_request_id(request_id: &str) -> Option<String> {
    let rest = request_id.strip_prefix("node-")?;
    let last_dash = rest.rfind('-')?;
    if last_dash == 0 {
        return None;
    }
    Some(rest[..last_dash].to_string())
}

fn unique_node_id(base: &str, graph: &GoalGraph) -> String {
    if graph.nodes.iter().all(|n| n.id != base) {
        return base.to_string();
    }
    let mut idx = 1u32;
    loop {
        let candidate = format!("{base}_{idx}");
        if graph.nodes.iter().all(|n| n.id != candidate) {
            return candidate;
        }
        idx = idx.saturating_add(1);
    }
}

fn reports_out_dir() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("CANON_REPORTS_OUT") {
        return std::path::PathBuf::from(p);
    }
    std::path::PathBuf::from("/workspace/ai_sandbox/canon/state/reports_out")
}

/// Overwrite graph_state.json with a human-readable summary of current node statuses.
fn write_graph_report(graph: &GoalGraph, tick: u64) {
    let dir = reports_out_dir();
    let _ = std::fs::create_dir_all(&dir);

    // Write graph_state.json (goal graph node statuses)
    let path = dir.join("graph_state.json");
    let nodes: Vec<serde_json::Value> = graph.nodes.iter().map(|n| {
        let result_preview = n.result.as_deref()
            .map(|r| if r.len() > 300 { format!("{}…", &r[..300]) } else { r.to_string() });
        serde_json::json!({
            "id": n.id,
            "status": format!("{:?}", n.status).to_lowercase(),
            "type": format!("{:?}", n.node_type).to_lowercase(),
            "caps": n.required_capabilities.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>(),
            "deps": n.deps,
            "priority": n.priority,
            "result_preview": result_preview,
            "error": n.error,
        })
    }).collect();
    let report = serde_json::json!({ "tick": tick, "nodes": nodes });
    if let Ok(s) = serde_json::to_string_pretty(&report) {
        let _ = std::fs::write(&path, s);
    }

    // Write goal_graph_state.json from event log projection
    let tlog_path = resolve_runtime_tlog_path();
    if tlog_path.exists() {
        if let Ok(goal_state) = replay_goal_graph_from_tlog(&tlog_path) {
            if let Ok(s) = serde_json::to_string_pretty(&goal_state) {
                let _ = std::fs::write(dir.join("goal_graph_state.json"), s);
            }
        }
        if let Ok(cap_state) = replay_capability_graph_from_tlog(&tlog_path) {
            if let Ok(s) = serde_json::to_string_pretty(&cap_state) {
                let _ = std::fs::write(dir.join("capability_graph_state.json"), s);
            }
        }
    }
}

/// Emit GoalGraphEvent mutations as RuntimeEvents via the emitter.
fn emit_goal_graph_events(emitter: &canon_event::RuntimeEmitterHandle, events: Vec<GoalGraphEvent>) {
    for event in events {
        let runtime_event = match event {
            GoalGraphEvent::NodeCreated { node_id, description, deps, caps, node_type, priority, budget } => {
                RuntimeEvent::GoalNodeCreated { node_id, description, deps, caps, node_type, priority, budget }
            }
            GoalGraphEvent::NodeRetracted { node_id } => {
                RuntimeEvent::GoalNodeRetracted { node_id }
            }
            GoalGraphEvent::NodeRewritten { node_id, new_description, new_caps } => {
                RuntimeEvent::GoalNodeRewritten { node_id, new_description, new_caps }
            }
            GoalGraphEvent::EdgeDefined { from, to } => {
                RuntimeEvent::GoalEdgeDefined { from_node_id: from, to_node_id: to }
            }
        };
        emitter.emit(runtime_event);
    }
}

/// Append one LLM response record to llm_responses.jsonl.
fn append_llm_response_log(node_id: &str, request_id: &str, text: &str) {
    let dir = reports_out_dir();
    let path = dir.join("llm_responses.jsonl");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let entry = serde_json::json!({
        "ts_ms": ts,
        "node_id": node_id,
        "request_id": request_id,
        "text": text,
    });
    if let Ok(mut line) = serde_json::to_string(&entry) {
        line.push('\n');
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}
