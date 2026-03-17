use canon_agent::task_graph::{task_graph_resolve_ready, TaskGraph, TaskNode, NodeStatus};
use canon_agent::capability_types::PipelineCapability;
use canon_agent::decompose::{DecomposeNodeType, DecomposeTaskSpec};
use canon_agent::graph_algo::{
    compute_graph_features_parallel, graph_analysis_compute_graph_signals,
};
use canon_agent::objectives::{
    goal_raw_with_artifact, load_goal_from_reports, maybe_write_baseline,
    objective_task_hints, ObjectiveWeights,
};
use canon_agent::task_graph_patch::{apply_graph_patch, TaskGraphPatch, PlannerUpdateRewriteSpec};
use canon_event::{
    EventDelta, RustcState,
    NodeCompleted, NodeFailed, NodeReady, NodeStarted, EventConsumer, EventEmitterHandle,
    CanonEvent, EventFilter,
};
use canon_event::emit_debug::{info, warn};
use canon_event_store::{replay_goal_graph_from_tlog, GoalGraphState};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::collections::HashMap;

mod executor;
mod reports;
mod patch;

use self::executor::*;
use self::reports::*;
use self::patch::*;

pub struct AgentConsumer {
    emitter: Arc<Mutex<Option<EventEmitterHandle>>>,
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

    fn on_kernel_event(&mut self, _event: &CanonEvent) {
        if let CanonEvent::Kernel { delta, state } = _event {
            let _ = self.work_tx.send(AgentWork::Kernel {
                delta: delta.clone(),
                state: state.clone(),
            });
        }
    }

    fn on_runtime_event(&mut self, event: &CanonEvent) {
        if let CanonEvent::Tick { tick } = event {
            let _ = self.work_tx.send(AgentWork::Tick(*tick));
            return;
        }
        if let CanonEvent::RuntimeStateUpdated { payload } = event {
            let _ = self.work_tx.send(AgentWork::RuntimeState(payload.clone()));
        }
    }

    fn on_capability_result(&mut self, event: &CanonEvent) {
        let _ = self
            .work_tx
            .send(AgentWork::CapabilityResult(event.clone()));
    }
}

impl EventConsumer for AgentConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn on_event(&mut self, event: &CanonEvent) {
        match event {
            CanonEvent::Kernel { .. } => self.on_kernel_event(event),
            CanonEvent::CapabilityCompleted(_) | CanonEvent::CapabilityFailed(_) => {
                self.on_capability_result(event)
            }
            _ => self.on_runtime_event(event),
        }
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        if let Ok(mut slot) = self.emitter.lock() {
            *slot = Some(emitter);
        }
    }
}

#[derive(Debug, Clone)]
enum AgentWork {
    Tick(u64),
    CapabilityResult(CanonEvent),
    RuntimeState(serde_json::Value),
    Kernel { delta: EventDelta, state: RustcState },
}

struct ExecutorState {
    role: String,
    pending_count: usize,
    results: Vec<(usize, String)>,
}

struct AgentWorkerState {
    graph: TaskGraph,
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
            graph: TaskGraph::new(),
            pending: HashMap::new(),
            last_tick: 0,
            retry_counts: HashMap::new(),
            delta_to_node: HashMap::new(),
            executor_state: HashMap::new(),
        }
    }

    fn handle(&mut self, work: AgentWork, emitter: &Arc<Mutex<Option<EventEmitterHandle>>>) {
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
        let gs = match replay_goal_graph_from_tlog(&tlog_path) {
            Ok(gs) => gs,
            Err(_) => return,
        };
        if gs.nodes.is_empty() {
            return;
        }
        self.graph = goal_graph_from_state(&gs);
        self.graph.rebuild_index();
        // Reset running→pending: those nodes were interrupted by a previous shutdown.
        for node in self.graph.nodes.iter_mut() {
            if node.status == NodeStatus::Running {
                node.status = NodeStatus::Pending;
            }
        }
        self.graph.rebuild_index();
    }

    fn apply_runtime_state(&mut self, payload: serde_json::Value) {
        if !self.graph.nodes.is_empty() {
            return;
        }
        let Ok(mut graph) = serde_json::from_value::<TaskGraph>(payload) else {
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

    fn schedule_next(&mut self, emitter: &Arc<Mutex<Option<EventEmitterHandle>>>) {
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
        emitter.emit(CanonEvent::NodeReady(NodeReady {
            node_id: node.id.clone(),
            capability: capability_name.to_string(),
            request_id: request_id.clone(),
            args: args.clone(),
        }));
        emitter.emit(CanonEvent::NodeStarted(NodeStarted {
            node_id: node.id.clone(),
            capability: capability_name.to_string(),
            request_id: request_id.clone(),
        }));
        emitter.emit(CanonEvent::CapabilityInvoked {
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
        event: CanonEvent,
        emitter: &Arc<Mutex<Option<EventEmitterHandle>>>,
    ) {
        self.apply_result_with_options(event, emitter, true);
    }


    fn apply_result_with_options(
        &mut self,
        event: CanonEvent,
        emitter: &Arc<Mutex<Option<EventEmitterHandle>>>,
        plan_and_persist: bool,
    ) {
        let (request_id, capability_name, success, stdout, stderr, result_value) = match event {
            CanonEvent::CapabilityCompleted(payload) => {
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
            CanonEvent::CapabilityFailed(payload) => (
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
                emit.emit(CanonEvent::ToolResult {
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
                        emit.emit(CanonEvent::CapabilityRequested(canon_event::CapabilityRequested {
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
                    emitter.emit(CanonEvent::NodeCompleted(NodeCompleted {
                        node_id: node_id.clone(),
                        capability: capability_name.clone(),
                        request_id: request_id.clone(),
                    }));
                } else {
                    emitter.emit(CanonEvent::NodeFailed(NodeFailed {
                        node_id: node_id.clone(),
                        capability: capability_name.clone(),
                        error: Some(stderr.clone()),
                        request_id: request_id.clone(),
                    }));
                }
                emitter.emit(CanonEvent::CapabilityResolved {
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
                            .map(|n| if matches!(n.node_type, canon_agent::decompose::DecomposeNodeType::Analysis) { "planner" } else { "exec" })
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
                                emit.emit(CanonEvent::ToolCall {
                                    node_id: node_id.clone(),
                                    request_id: delta_id.clone(),
                                    kind,
                                    payload: delta.clone(),
                                });
                                emit.emit(CanonEvent::CapabilityRequested(canon_event::CapabilityRequested {
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

    fn plan_if_stalled(&mut self, emitter: &Arc<Mutex<Option<EventEmitterHandle>>>) -> bool {
        let signals = graph_analysis_compute_graph_signals(&self.graph);
        let features = compute_graph_features_parallel(&self.graph);

        let mut update = TaskGraphPatch {
            new_nodes: Vec::new(),
            new_edges: Vec::new(),
            retract_nodes: Vec::new(),
            rewrite_nodes: Vec::new(),
        };

        if self.graph.nodes.is_empty() {
            self.seed_orchestration(&mut update, emitter);
        } else {
            let failed_nodes: Vec<TaskNode> = self
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

    fn emit_state(&self, emitter: &Arc<Mutex<Option<EventEmitterHandle>>>) {
        let Some(emitter) = emitter.lock().ok().and_then(|slot| slot.clone()) else {
            return;
        };
        if let Ok(payload) = serde_json::to_value(&self.graph) {
            emitter.emit(CanonEvent::RuntimeStateUpdated { payload });
        }
    }

    fn seed_orchestration(&mut self, update: &mut TaskGraphPatch, emitter: &Arc<Mutex<Option<EventEmitterHandle>>>) {
        if !update.new_nodes.is_empty() {
            return;
        }
        let mut description =
            "Analyse system state and produce initial task decomposition".to_string();
        if let Some(selection) = load_goal_from_reports(ObjectiveWeights::default(), Some(&self.graph)) {
            maybe_write_baseline(&selection);
            if let Ok(payload) = serde_json::to_value(&selection.artifact) {
                if let Some(emit) = emitter.lock().ok().and_then(|slot| slot.clone()) {
                    emit.emit(CanonEvent::GoalSelected { payload: payload.clone() });
                    emit.emit(CanonEvent::PolicyBaselineUpdated { payload });
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

    fn persist_snapshot(&self, emitter: &Arc<Mutex<Option<EventEmitterHandle>>>) {
        let tlog_seq = latest_segment_seq(&resolve_runtime_tlog_path()).unwrap_or(0);
        if let Some(emitter_handle) = emitter.lock().ok().and_then(|slot| slot.clone()) {
            emitter_handle.emit(CanonEvent::GoalGraphCheckpointed { tlog_seq });
        }
        write_graph_report(&self.graph, self.last_tick);
    }

}

fn goal_graph_from_state(gs: &GoalGraphState) -> TaskGraph {
    let mut graph = TaskGraph::new();
    for pn in gs.nodes.values() {
        let status = match pn.status.as_str() {
            "running" => NodeStatus::Running,
            "completed" => NodeStatus::Completed,
            "failed" => NodeStatus::Failed,
            _ => NodeStatus::Pending,
        };
        let node_type = serde_json::from_str::<DecomposeNodeType>(&format!("\"{}\"", pn.node_type))
            .unwrap_or_default();
        let required_capabilities = pn
            .caps
            .iter()
            .filter_map(|cap| {
                serde_json::from_str::<PipelineCapability>(&format!("\"{}\"", cap)).ok()
            })
            .collect();
        graph.nodes.push(TaskNode {
            id: pn.node_id.clone(),
            description: pn.description.clone(),
            status,
            deps: pn.deps.clone(),
            required_capabilities,
            node_type,
            priority: pn.priority,
            budget: pn.budget,
            reasoning_trace: None,
            result: None,
            error: None,
            readonly_fail_count: 0,
            repair_attempts: 0,
            completed_iter: None,
        });
    }
    graph
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
