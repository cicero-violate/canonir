use canon_agent_v2::dag::{task_graph_resolve_ready, ExecutionGraph, ExecutionNode, NodeStatus};
use canon_agent_v2::state_snapshot;
use canon_agent_v2::capability_types::PipelineCapability;
use canon_agent_v2::graph_algo::{compute_graph_features_parallel, graph_analysis_compute_graph_signals};
use canon_agent_v2::planner_update::{apply_graph_patch, GraphPatch, PlannerUpdateRewriteSpec};
use canon_types::{
    EventDelta, KernelState, RuntimeConsumer, RuntimeEmitterHandle, RuntimeEvent, RuntimeEventFilter,
};
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
                self.apply_result(event);
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
            return;
        };
        let Some(args) = build_capability_args(&node, capability_name) else {
            let _ = self.graph.update_status(&node.id, NodeStatus::Failed);
            if let Some(node_mut) = self.graph.get_node_mut(&node.id) {
                node_mut.error = Some("missing capability args".to_string());
            }
            return;
        };
        let request_id = format!("node-{}-{}", node.id, self.last_tick);
        let _ = self.graph.update_status(&node.id, NodeStatus::Running);
        self.pending.insert(request_id.clone(), node.id.clone());
        emitter.emit(RuntimeEvent::CapabilityRequested(
            canon_types::CapabilityRequested {
                request_id,
                name: capability_name.to_string(),
                args,
            },
        ));
    }

    fn apply_result(&mut self, event: RuntimeEvent) {
        let (request_id, success, stdout, stderr) = match event {
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
                (payload.request_id, success, stdout, stderr)
            }
            RuntimeEvent::CapabilityFailed(payload) => {
                (payload.request_id, false, String::new(), payload.error)
            }
            _ => return,
        };
        let Some(node_id) = self.pending.remove(&request_id) else {
            return;
        };
        let new_status = if success {
            NodeStatus::Completed
        } else {
            NodeStatus::Failed
        };
        let _ = self.graph.update_status(&node_id, new_status);
        if let Some(node) = self.graph.get_node_mut(&node_id) {
            if success {
                if !stdout.is_empty() {
                    node.result = Some(stdout);
                }
            } else if !stderr.is_empty() {
                node.error = Some(stderr);
            }
        }
    }

    fn plan_if_stalled(&mut self) -> bool {
        let _signals = graph_analysis_compute_graph_signals(&self.graph);
        let _features = compute_graph_features_parallel(&self.graph);

        let mut update = GraphPatch {
            new_nodes: Vec::new(),
            new_edges: Vec::new(),
            retract_nodes: Vec::new(),
            rewrite_nodes: Vec::new(),
        };

        if self.graph.nodes.is_empty() {
            self.seed_orchestration(&mut update);
        } else if let Some(node) = self
            .graph
            .nodes
            .iter()
            .find(|n| n.status == NodeStatus::Failed)
            .cloned()
        {
            let retries = self.retry_counts.entry(node.id.clone()).or_insert(0);
            if *retries < 1 {
                *retries += 1;
                update.rewrite_nodes.push(PlannerUpdateRewriteSpec {
                    id: node.id,
                    new_description: node.description,
                    new_capabilities: node.required_capabilities,
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
        // No auto-seed: avoid flooding tlog with failing bash nodes when no
        // external graph is provided. The agent is driven by external events only.
        let _ = update;
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
            _ => {}
        }
    }
    None
}

fn build_capability_args(node: &ExecutionNode, capability: &str) -> Option<serde_json::Value> {
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
