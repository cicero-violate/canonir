use canon_agent_v3::dag::{task_graph_resolve_ready, ExecutionGraph, ExecutionNode, NodeStatus};
use canon_agent_v3::state_snapshot;
use canon_agent_v3::capability_types::PipelineCapability;
use canon_agent_v3::decompose::{DecomposeNodeType, DecomposeTaskSpec};
use canon_agent_v3::goal::GoalSpec;
use canon_agent_v3::graph_algo::{
    compute_graph_features_parallel, graph_analysis_compute_graph_signals,
};
use canon_agent_v3::objectives::{
    goal_raw_with_artifact, load_goal_from_reports, maybe_write_baseline,
    objective_task_hints, ObjectiveWeights,
};
use canon_agent_v3::planner_update::{apply_graph_patch, GraphPatch, PlannerUpdateRewriteSpec};
use canon_types::{
    CapabilityCompleted, CapabilityFailed, CapabilityRequested, EventDelta, KernelState,
    NodeCompleted, NodeFailed, NodeReady, NodeStarted, RuntimeConsumer, RuntimeEmitterHandle,
    RuntimeEvent, RuntimeEventFilter,
};
use canon_event_log::{info, warn};
use canon_tlog_replay::{read_any_events_from_path_with_start_seq, AnyEvent};
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
    Kernel { delta: EventDelta, state: KernelState },
}

struct AgentWorkerState {
    graph: ExecutionGraph,
    pending: HashMap<String, String>,
    last_tick: u64,
    retry_counts: HashMap<String, u32>,
}

impl AgentWorkerState {
    fn new() -> Self {
        Self {
            graph: ExecutionGraph::new(),
            pending: HashMap::new(),
            last_tick: 0,
            retry_counts: HashMap::new(),
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
                self.emit_state(emitter);
            }
            AgentWork::Kernel { delta, state } => {
                self.observe_kernel(delta, state);
                self.schedule_next(emitter);
                self.emit_state(emitter);
            }
        }
    }

    fn try_load_snapshot(&mut self) {
        let path = std::path::Path::new("/workspace/ai_sandbox/canon/agent_logs/state_snapshot.json");
        if let Some(snapshot) = state_snapshot::snapshot_store_load(path) {
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
    }

    fn apply_runtime_state(&mut self, payload: serde_json::Value) {
        if !self.graph.nodes.is_empty() {
            return;
        }
        let Ok(mut graph) = serde_json::from_value::<ExecutionGraph>(payload) else {
            return;
        };
        graph.rebuild_index();
        self.graph = graph;
    }

    fn observe_kernel(&mut self, _delta: EventDelta, state: KernelState) {
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
            if self.plan_if_stalled() {
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
        info(
            "agent_consumer",
            "node_started",
            serde_json::json!({ "node_id": node.id, "capability": capability_name }),
        );
        self.pending.insert(request_id.clone(), node.id.clone());
        emitter.emit(RuntimeEvent::CapabilityRequested(
            canon_types::CapabilityRequested {
                request_id,
                name: capability_name.to_string(),
                args,
            },
        ));
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
        if !plan_and_persist {
            if let Some(node) = self.graph.get_node(&node_id) {
                if matches!(node.status, NodeStatus::Completed | NodeStatus::Failed) {
                    return;
                }
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
                } else if !stdout.is_empty() {
                    node.result = Some(stdout);
                }
            } else if !stderr.is_empty() {
                node.error = Some(stderr);
            }
        }
        if let Some(patch) = patch_to_apply {
            let _ = apply_graph_patch(&mut self.graph, patch);
            self.graph.rebuild_index();
        }
        if plan_and_persist {
            let _ = self.plan_if_stalled();
            self.persist_snapshot();
        }
    }

    fn plan_if_stalled(&mut self) -> bool {
        let signals = graph_analysis_compute_graph_signals(&self.graph);
        let features = compute_graph_features_parallel(&self.graph);

        let mut update = GraphPatch {
            new_nodes: Vec::new(),
            new_edges: Vec::new(),
            retract_nodes: Vec::new(),
            rewrite_nodes: Vec::new(),
        };

        if self.graph.nodes.is_empty() {
            self.seed_orchestration(&mut update);
        } else {
            let failed_nodes: Vec<ExecutionNode> = self
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

        if apply_graph_patch(&mut self.graph, update).is_ok() {
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

    fn seed_orchestration(&mut self, update: &mut GraphPatch) {
        if !update.new_nodes.is_empty() {
            return;
        }
        let mut description =
            "Analyse system state and produce initial task decomposition".to_string();
        if let Some(selection) = load_goal_from_reports(ObjectiveWeights::default()) {
            maybe_write_baseline(&selection);
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

    fn persist_snapshot(&self) {
        let path = std::path::Path::new(
            "/workspace/ai_sandbox/canon/agent_logs/state_snapshot.json",
        );
        let snapshot = state_snapshot::PipelineSnapshot {
            graph: self.graph.clone(),
            iteration: self.last_tick,
            runtime_start_seq: latest_segment_seq(&resolve_runtime_tlog_path()).unwrap_or(0),
            goal: GoalSpec::new(String::new(), 0),
        };
        state_snapshot::snapshot_store_save(path, &snapshot);
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

fn capability_name_for_node(node: &ExecutionNode) -> Option<&'static str> {
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
    None
}

fn build_capability_args(node: &ExecutionNode, capability: &str) -> Option<serde_json::Value> {
    if capability == "llm.call" {
        use canon_agent_v3::decompose::DecomposeNodeType;
        let prompt = node.description.clone();
        let raw = matches!(node.node_type, DecomposeNodeType::Analysis);
        return Some(json!({ "prompt": prompt, "raw": raw }));
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
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    let slice = &text[start..=end];
    serde_json::from_str(slice).ok()
}

fn extract_graph_patch_from_llm_result(
    result: &serde_json::Value,
) -> Option<canon_agent_v3::planner_update::GraphPatch> {
    let text = result.get("text").and_then(|v| v.as_str())?;
    let json_val = parse_inline_json(text)?;
    serde_json::from_value(json_val).ok()
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
        "/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog.d",
    );
    if binary.exists() {
        return binary;
    }
    std::path::PathBuf::from("/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog")
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

fn unique_node_id(base: &str, graph: &ExecutionGraph) -> String {
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
