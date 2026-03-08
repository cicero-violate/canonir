use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use super::capability::{
    capability_model_assert_class_disjoint, PipelineCapability, CapabilityMode,
};
use super::config::CapabilityConfigLlmEndpoint;
use super::dag::NodeStatus;
use super::dag::ExecutionGraph;
use super::decompose::DecomposeTaskSpec;
use super::failure_store::FailureStore;
use super::graph_algo::{self, GraphAnalysis};
use super::planner_update::{
    apply_graph_patch, PlannerUpdateEdgeSpec, GraphPatch, PlannerUpdateRetractSpec,
    PlannerUpdateRewriteSpec,
};
use crate::llm_provider::JsonExtractor;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerControllerRewardContext {
    pub recent_rewards: Vec<f64>,
    pub plateaued: bool,
    pub best_reward: f64,
    pub stored_reward: f64,
    pub bootstrap_seed: Option<PlannerControllerBootstrapSeed>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerControllerBootstrapSeed {
    pub goal: String,
    pub similarity_score: f64,
    pub reward: f64,
    pub node_summaries: Vec<String>,
    pub capability_set: Vec<String>,
    pub node_count: usize,
    pub edge_count: usize,
}
pub(crate) struct PlannerControllerRepairReport {
    pub count: u64,
    pub ids: Vec<String>,
}
pub struct PlannerController {
    endpoint_id: String,
    url: String,
    role_schema: String,
    goal: String,
    history: Vec<String>,
    stateful: bool,
    reward_context: Option<PlannerControllerRewardContext>,
}
const MAX_HISTORY: usize = 5;
impl PlannerController {
    pub fn new(endpoint: &CapabilityConfigLlmEndpoint, goal: String) -> Self {
        Self {
            endpoint_id: endpoint.id.clone(),
            url: endpoint.url.clone(),
            role_schema: endpoint.role_markdown.clone(),
            goal,
            history: Vec::new(),
            stateful: endpoint.stateful,
            reward_context: None,
        }
    }
    pub fn set_reward_context(&mut self, ctx: PlannerControllerRewardContext) {
        self.reward_context = Some(ctx);
    }
    pub fn reward_context(&self) -> Option<&PlannerControllerRewardContext> {
        self.reward_context.as_ref()
    }
    pub fn build_prompt(
        &mut self,
        graph: &ExecutionGraph,
        signals: &GraphAnalysis,
        features: &graph_algo::GraphFeatureVector,
        cost_summary: &str,
        rewrite_requests: &[String],
        bias_text: &str,
        planner_max_new_nodes: usize,
        planner_max_new_edges: usize,
    ) -> String {
        let ids: Vec<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
        let signals_json = signals.to_json(&ids);
        let expandable = planner_controller_expandable_nodes(graph);
        let ready_nodes = graph
            .ready_nodes()
            .iter()
            .map(|n| n.id.clone())
            .collect::<Vec<_>>();
        let unreachable_nodes = signals
            .unreachable
            .iter()
            .filter_map(|&idx| ids.get(idx).cloned())
            .collect::<Vec<_>>();
        let rewrite_text = if rewrite_requests.is_empty() {
            "Rewrite requests: none\n".to_string()
        } else {
            format!("Rewrite requests: {}\n", rewrite_requests.join(", "))
        };
        let nodes_json: Vec<Value> = graph
            .nodes
            .iter()
            .map(|n| {
                serde_json::json!(
                    { "id" : n.id, "description" : n.description, "deps" : n.deps,
                    "status" : n.status, "node_type" : n.node_type,
                    "required_capabilities" : n.required_capabilities, "priority" : n
                    .priority, "budget" : n.budget, "reasoning_trace" : n
                    .reasoning_trace, "result" : n.result, }
                )
            })
            .collect();
        let history_tail = self
            .history
            .iter()
            .rev()
            .take(6)
            .cloned()
            .collect::<Vec<_>>();
        let reward_section = match &self.reward_context {
            None => String::new(),
            Some(r) => {
                let trend = r
                    .recent_rewards
                    .iter()
                    .map(|v| format!("{:.3}", v))
                    .collect::<Vec<_>>()
                    .join(", ");
                let seed_section = match r.bootstrap_seed.as_ref() {
                    None => String::new(),
                    Some(seed) => {
                        format!(
                            "Bootstrap seed (similar prior goal, similarity={:.2}, reward={:.3}):\n\
Prior goal: {}\n\
Prior graph had {} nodes, {} edges.\n\
Capabilities used: {}\n\
Node summaries:\n{}\n\
Consider reusing this structure as a starting point, adapting it to the current goal.\n",
                            seed.similarity_score, seed.reward, seed.goal, seed
                            .node_count, seed.edge_count, seed.capability_set.join(", "),
                            seed.node_summaries.join("\n"),
                        )
                    }
                };
                let base = if r.plateaued {
                    let signals_str = super::graph_algo::graph_analysis_planner_signals_for_graph(
                        graph,
                    );
                    format!(
                        "Reward history (last {} runs): [{}]\n\
Best recorded reward: {:.3}\n\
STATUS: PLATEAUED. The current graph structure is not improving.\n\
You MUST propose a structurally different graph: different node decomposition, \
different capability assignments, or different dependency topology. Do not make incremental edits.\n\
Current graph topology: {}\n\
Use these signals to identify structural bottlenecks before proposing changes.\n",
                        r.recent_rewards.len(), trend, r.best_reward, signals_str
                    )
                } else {
                    format!(
                        "Reward history (last {} runs): [{}]\n\
Best recorded reward: {:.3}\n\
Continue refining the current graph.\n",
                        r.recent_rewards.len(), trend, r.best_reward
                    )
                };
                base + &seed_section
            }
        };
        let mut features = features.clone();
        if let Some(ctx) = self.reward_context.as_ref() {
            features = features.with_reward_history(&ctx.recent_rewards);
        }
        let metrics_text = format!(
            "Metrics:\n\
nodes={} edges={} depth={} scc_count={}\n\
roots={} leaves={} avg_out_deg={:.2} avg_in_deg={:.2} branching={:.2}\n\
verify/mutate={:.2} observe/mutate={:.2} entropy={:.2}\n\
priority_avg={:.2} budget_avg={:.2}\n\
blocked_frac={:.2} ready_frac={:.2} failed_frac={:.2}\n\
completion_velocity={:.3} retry_rate={:.3}\n\
failure_pattern_rate={:.3} cycle_freq={:.3} deadlock_rate={:.3}\n",
            features.nodes, features.edges, features.depth, features.scc_count, features
            .root_count, features.leaf_count, features.avg_out_degree, features
            .avg_in_degree, features.branching_factor, features.verify_to_mutate_ratio,
            features.observe_to_mutate_ratio, features.node_type_entropy, features
            .avg_node_priority, features.avg_node_budget, features.blocked_fraction,
            features.ready_fraction, features.failed_fraction, features
            .completion_velocity, features.retry_rate, features.failure_pattern_rate,
            features.cycle_frequency, features.deadlock_rate,
        );
        let prompt = format!(
            "You are a planner. Maintain continuity across iterations.\n\
Planner limits: max_new_nodes={}, max_new_edges={}\n\
Expandable nodes: {}\n\
Ready nodes: {}\n\
Unreachable nodes: {}\n\
{}\
CAPABILITY SCHEMA (snake_case, do not mix classes in one node)\n\
Observe: file_read, read_structural_surface, read_dag, compute_delta, radius_budget_eval, reward_signal_compute, stateless_invoke, goal_to_subgoals, schedule_ready, constraint_attach, prompt_contract_enforce, stdout_capture\n\
Verify: detect_failures, invariant_check, boundary_guard, parse_orchestration_report, status_update_only, update_status\n\
Mutate: file_write, apply_patch, bash, cargo_build, cargo_check, create_node, add_edge, refine_node, dependency_rewrite\n\
If a task needs both Verify and Mutate, split into two nodes with a dependency edge.\n\
Rules:\n\
1) Prefer expanding root or blocked nodes.\n\
2) Avoid expanding nodes already expanded.\n\
3) Prefer nodes with unsatisfied dependencies.\n\
4) Never exceed planner_max_new_nodes.\n\
5) Retract nodes that are Pending or Failed with no dependents.\n\
6) Rewrite nodes that are Pending with an imprecise description.\n\
7) If rewrite requests are listed, include them in rewrite_nodes.\n\
8) If the graph is empty, include a node that runs `cargo run --bin orchestration -- --all` with mutate-only capability (`bash` or `cargo_build`).\n\
9) Do not avoid Mutate tasks; execute Mutate steps early if they unblock diagnostics.\n\
POLICY BIAS\n\
{}\n\
SYSTEM GRAPH METRICS\n\
{}\n\
CAPABILITY COSTS (highest)\n\
{}\n\
{}\n\
Goal:\n{}\n\n\
Graph Nodes:\n{}\n\n\
Graph Signals:\n{}\n\n\
Recent History:\n{}\n\n\
Return JSON only with schema:\n{{\n  \"new_nodes\": [{{\"id\":\"...\",\"description\":\"...\",\"deps\":[],\"required_capabilities\":[],\"node_type\":\"analysis|render\",\"priority\":0,\"budget\":3,\"reasoning_trace\":\"...\"}}],\n  \"new_edges\": [{{\"from\":\"id\",\"to\":\"id\"}}],\n  \"retract_nodes\": [{{\"id\":\"...\"}}],\n  \"rewrite_nodes\": [{{\"id\":\"...\",\"new_description\":\"...\",\"new_capabilities\":[]}}]\n}}",
            planner_max_new_nodes, planner_max_new_edges, expandable.join(", "),
            ready_nodes.join(", "), unreachable_nodes.join(", "), rewrite_text,
            bias_text, metrics_text, cost_summary, reward_section, self.goal,
            serde_json::to_string_pretty(& nodes_json).unwrap_or_default(),
            serde_json::to_string_pretty(& signals_json).unwrap_or_default(),
            history_tail.join("\n")
        );
        prompt
    }
    pub fn apply_raw_response(
        &mut self,
        raw: String,
        log_dir: &std::path::Path,
        iter: u64,
        graph_nodes_len: usize,
        signals: &GraphAnalysis,
    ) -> Result<GraphPatch> {
        self.history.push(raw.clone());
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
        let parsed = JsonExtractor::extract(&raw)
            .or_else(|_| {
                try_parse_lenient_json(&raw)
                    .ok_or_else(|| anyhow::anyhow!("planner json extract error"))
            })
            .and_then(|payload| {
                serde_json::from_value(payload)
                    .context("planner update did not match schema")
            });
        match parsed {
            Ok(update) => {
                planner_controller_log_planner_iteration(
                    log_dir,
                    iter,
                    graph_nodes_len,
                    signals,
                    &update,
                    &raw,
                    None,
                );
                Ok(update)
            }
            Err(e) => {
                planner_controller_log_planner_iteration(
                    log_dir,
                    iter,
                    graph_nodes_len,
                    signals,
                    &GraphPatch {
                        new_nodes: Vec::new(),
                        new_edges: Vec::new(),
                        retract_nodes: Vec::new(),
                        rewrite_nodes: Vec::new(),
                    },
                    &raw,
                    Some(&e.to_string()),
                );
                Err(e)
            }
        }
    }
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
    pub fn is_history_empty(&self) -> bool {
        self.history.is_empty()
    }
}
pub(crate) fn planner_controller_validate_planner_update(
    graph: &ExecutionGraph,
    update: &GraphPatch,
    planner_max_new_nodes: usize,
    planner_max_new_edges: usize,
    failure_store: &mut FailureStore,
    iteration: u64,
    failure_constraint_threshold: usize,
    max_constraints: usize,
) -> Result<()> {
    planner_controller_ensure(
        update.new_nodes.len() <= planner_max_new_nodes,
        "planner expansion limit exceeded",
    )?;
    planner_controller_ensure(
        update.new_edges.len() <= planner_max_new_edges,
        "planner edge limit exceeded",
    )?;
    let mut existing: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut status_by_id: std::collections::HashMap<String, NodeStatus> = std::collections::HashMap::new();
    for (idx, node) in graph.nodes.iter().enumerate() {
        existing.insert(node.id.clone(), idx);
        status_by_id.insert(node.id.clone(), node.status);
    }
    let mut new_ids: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    update
        .new_nodes
        .iter()
        .try_for_each(|spec| {
            planner_controller_ensure(
                !spec.id.trim().is_empty(),
                "planner node id empty",
            )?;
            planner_controller_ensure(
                !spec.description.trim().is_empty(),
                "planner node description empty",
            )?;
            planner_controller_ensure(
                !spec
                    .required_capabilities
                    .iter()
                    .any(|c| matches!(c, PipelineCapability::Unknown)),
                "planner node has unknown capability",
            )?;
            planner_controller_ensure(
                !(existing.contains_key(&spec.id) || new_ids.contains_key(&spec.id)),
                &format!("duplicate node id {}", spec.id),
            )?;
            new_ids.insert(spec.id.clone(), new_ids.len());
            Ok::<(), anyhow::Error>(())
        })?;
    update
        .new_edges
        .iter()
        .try_for_each(|edge| {
            planner_controller_ensure(
                !edge.from.trim().is_empty() && !edge.to.trim().is_empty(),
                "planner edge endpoints empty",
            )?;
            let from_ok = existing.contains_key(&edge.from)
                || new_ids.contains_key(&edge.from);
            let to_ok = existing.contains_key(&edge.to)
                || new_ids.contains_key(&edge.to);
            planner_controller_ensure(
                from_ok && to_ok,
                "planner edge references unknown node",
            )
        })?;
    update
        .retract_nodes
        .iter()
        .try_for_each(|spec| {
            let status = status_by_id
                .get(&spec.id)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("retract references unknown node"))?;
            planner_controller_ensure(
                matches!(status, NodeStatus::Pending | NodeStatus::Failed),
                "retract node must be pending or failed",
            )
        })?;
    update
        .rewrite_nodes
        .iter()
        .try_for_each(|spec| {
            let status = status_by_id
                .get(&spec.id)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("rewrite references unknown node"))?;
            planner_controller_ensure(
                matches!(status, NodeStatus::Pending),
                "rewrite node must be pending",
            )?;
            planner_controller_ensure(
                !spec
                    .new_capabilities
                    .iter()
                    .any(|c| matches!(c, PipelineCapability::Unknown)),
                "rewrite node has unknown capability",
            )?;
            let caps: std::collections::HashSet<_> = spec
                .new_capabilities
                .iter()
                .copied()
                .collect();
            capability_model_assert_class_disjoint(&caps).map_err(|e| anyhow::anyhow!(e))
        })?;
    let mut test_graph = graph.clone();
    apply_graph_patch(&mut test_graph, update.clone())?;
    if let Err(e) = test_graph.validate() {
        let msg = e.to_string();
        if msg.contains("cycle detected") {
            failure_store.record_graph("cycle", &test_graph, iteration);
        } else if msg.contains("capability class") {
            failure_store.record_graph("invalid_authority", &test_graph, iteration);
        }
        return Err(anyhow::anyhow!(e));
    }
    let constraints = failure_store
        .constraints(failure_constraint_threshold, max_constraints);
    if !constraints.is_empty() {
        let signals = graph_algo::graph_analysis_compute_graph_signals(&test_graph);
        for c in constraints {
            if let Some(err) = planner_controller_check_constraint(
                &test_graph,
                &signals,
                &c,
            ) {
                return Err(anyhow::anyhow!(err));
            }
        }
    }
    Ok(())
}
fn planner_controller_check_constraint(
    graph: &ExecutionGraph,
    signals: &graph_algo::GraphAnalysis,
    constraint: &super::failure_store::FailureStoreConstraint,
) -> Option<String> {
    use super::failure_store::FailureStoreConstraintRule;
    match &constraint.rule {
        FailureStoreConstraintRule::NoCycle => {
            signals.has_cycle.then(|| "constraint violated: NoCycle".to_string())
        }
        FailureStoreConstraintRule::NoUnreachable => {
            (!signals.unreachable.is_empty())
                .then(|| "constraint violated: NoUnreachable".to_string())
        }
        FailureStoreConstraintRule::CapabilityConflict => None,
        FailureStoreConstraintRule::InvalidDependency => None,
        FailureStoreConstraintRule::PatternRewrite { pattern, .. } => {
            let bad = graph
                .nodes
                .iter()
                .any(|n| n.description.to_lowercase().contains(pattern));
            bad.then(|| format!("constraint violated: PatternRewrite({})", pattern))
        }
        FailureStoreConstraintRule::SignatureBan => {
            let sig = graph_algo::hash_graph_structure(graph);
            (sig == constraint.signature)
                .then(|| "constraint violated: SignatureBan".to_string())
        }
    }
}
pub(crate) fn planner_controller_auto_repair_planner_update(
    graph: &ExecutionGraph,
    update: &mut GraphPatch,
) -> PlannerControllerRepairReport {
    let mut used: std::collections::HashSet<String> = graph
        .nodes
        .iter()
        .map(|n| n.id.clone())
        .collect();
    for spec in &update.new_nodes {
        used.insert(spec.id.clone());
    }
    for spec in &update.rewrite_nodes {
        used.insert(spec.id.clone());
    }
    let mut repairs = 0u64;
    let mut ids = Vec::new();
    let mut repaired_nodes: Vec<DecomposeTaskSpec> = Vec::new();
    for mut spec in update.new_nodes.drain(..) {
        planner_controller_normalize_capabilities(
            &mut spec.required_capabilities,
            &spec.description,
        );
        let (observe, verify, mutate) = planner_controller_split_caps(
            &spec.required_capabilities,
        );
        if !mutate.is_empty() && !verify.is_empty() {
            repairs += 1;
            ids.push(spec.id.clone());
            let verify_id = planner_controller_unique_id(
                format!("{}_verify", spec.id),
                &mut used,
            );
            let mut verify_caps = verify;
            verify_caps.extend(observe);
            let verify_node = DecomposeTaskSpec {
                id: verify_id.clone(),
                description: format!(
                    "Verify preconditions for {}: {}", spec.id, spec.description
                ),
                deps: spec.deps.clone(),
                required_capabilities: verify_caps,
                node_type: super::decompose::DecomposeNodeType::Analysis,
                priority: spec.priority,
                budget: spec.budget,
                reasoning_trace: Some("AUTO_REPAIR: split verify/mutate".to_string()),
            };
            let mut mutate_deps = spec.deps.clone();
            if !mutate_deps.contains(&verify_id) {
                mutate_deps.push(verify_id.clone());
            }
            let mutate_node = DecomposeTaskSpec {
                id: spec.id,
                description: spec.description,
                deps: mutate_deps,
                required_capabilities: mutate,
                node_type: spec.node_type,
                priority: spec.priority,
                budget: spec.budget,
                reasoning_trace: spec.reasoning_trace,
            };
            let mutate_id = mutate_node.id.clone();
            repaired_nodes.push(verify_node);
            repaired_nodes.push(mutate_node);
            update
                .new_edges
                .push(PlannerUpdateEdgeSpec {
                    from: verify_id,
                    to: mutate_id,
                });
        } else {
            let mut caps = if !mutate.is_empty() { mutate } else { verify };
            if !caps.is_empty() {
                caps.extend(observe);
                let mut spec = spec;
                spec.required_capabilities = caps;
                repaired_nodes.push(spec);
            } else {
                repaired_nodes.push(spec);
            }
        }
    }
    for spec in update.rewrite_nodes.iter_mut() {
        planner_controller_normalize_capabilities(
            &mut spec.new_capabilities,
            &spec.new_description,
        );
        let (observe, verify, mutate) = planner_controller_split_caps(
            &spec.new_capabilities,
        );
        if !mutate.is_empty() && !verify.is_empty() {
            repairs += 1;
            ids.push(spec.id.clone());
            let verify_id = planner_controller_unique_id(
                format!("{}_verify", spec.id),
                &mut used,
            );
            let mut verify_caps = verify;
            verify_caps.extend(observe);
            update
                .new_nodes
                .push(DecomposeTaskSpec {
                    id: verify_id.clone(),
                    description: format!(
                        "Verify preconditions for {}: {}", spec.id, spec.new_description
                    ),
                    deps: Vec::new(),
                    required_capabilities: verify_caps,
                    node_type: super::decompose::DecomposeNodeType::Analysis,
                    priority: 5,
                    budget: None,
                    reasoning_trace: Some(
                        "AUTO_REPAIR: split rewrite verify/mutate".to_string(),
                    ),
                });
            update
                .new_edges
                .push(PlannerUpdateEdgeSpec {
                    from: verify_id,
                    to: spec.id.clone(),
                });
            spec.new_capabilities = mutate;
        } else if !mutate.is_empty() {
            let mut caps = mutate;
            caps.extend(observe);
            spec.new_capabilities = caps;
        } else {
            let mut caps = verify;
            caps.extend(observe);
            spec.new_capabilities = caps;
        }
    }
    update.new_nodes.extend(repaired_nodes);
    planner_controller_seed_orchestration_node_if_empty(graph, update);
    PlannerControllerRepairReport {
        count: repairs,
        ids,
    }
}
fn planner_controller_normalize_capabilities(
    caps: &mut Vec<PipelineCapability>,
    description: &str,
) {
    let lower = description.to_lowercase();
    if caps.iter().any(|c| matches!(c, PipelineCapability::StatelessInvoke)) {
        let replacement = if lower.contains("cargo check") {
            Some(PipelineCapability::CargoCheck)
        } else if lower.contains("cargo build") || lower.contains("cargo run") {
            Some(PipelineCapability::CargoBuild)
        } else if lower.contains("bash") || lower.contains("shell") {
            Some(PipelineCapability::Bash)
        } else {
            None
        };
        if let Some(rep) = replacement {
            *caps = caps
                .iter()
                .filter(|c| !matches!(c, PipelineCapability::StatelessInvoke))
                .copied()
                .collect();
            caps.push(rep);
        }
    }
}
fn planner_controller_seed_orchestration_node_if_empty(
    graph: &ExecutionGraph,
    update: &mut GraphPatch,
) {
    if !graph.nodes.is_empty() {
        return;
    }
    let has_orch = update
        .new_nodes
        .iter()
        .any(|n| {
            let d = n.description.to_lowercase();
            d.contains("orchestration -- --all")
                || d.contains("cargo run --bin orchestration")
        });
    if has_orch {
        return;
    }
    let mut used: std::collections::HashSet<String> = update
        .new_nodes
        .iter()
        .map(|n| n.id.clone())
        .collect();
    let id = planner_controller_unique_id(
        "run_orchestration_build".to_string(),
        &mut used,
    );
    update
        .new_nodes
        .push(DecomposeTaskSpec {
            id,
            description: "Run `cargo run --bin orchestration -- --all` to reproduce the pipeline and capture diagnostics."
                .to_string(),
            deps: Vec::new(),
            required_capabilities: vec![PipelineCapability::Bash],
            node_type: super::decompose::DecomposeNodeType::Analysis,
            priority: 10,
            budget: Some(3),
            reasoning_trace: Some(
                "AUTO_SEED: ensure orchestration build runs when graph is empty"
                    .to_string(),
            ),
        });
}
fn planner_controller_split_caps(
    caps: &[PipelineCapability],
) -> (Vec<PipelineCapability>, Vec<PipelineCapability>, Vec<PipelineCapability>) {
    let mut observe = Vec::new();
    let mut verify = Vec::new();
    let mut mutate = Vec::new();
    for &cap in caps {
        match cap.class() {
            CapabilityMode::Observe => observe.push(cap),
            CapabilityMode::Verify => verify.push(cap),
            CapabilityMode::Mutate => mutate.push(cap),
        }
    }
    (observe, verify, mutate)
}
fn planner_controller_unique_id(
    base: String,
    used: &mut std::collections::HashSet<String>,
) -> String {
    if !used.contains(&base) {
        used.insert(base.clone());
        return base;
    }
    let mut idx = 1u32;
    loop {
        let candidate = format!("{}_{}", base, idx);
        if !used.contains(&candidate) {
            used.insert(candidate.clone());
            return candidate;
        }
        idx = idx.saturating_add(1);
    }
}
fn planner_controller_ensure(cond: bool, msg: &str) -> Result<()> {
    cond.then_some(()).ok_or_else(|| anyhow::anyhow!("{}", msg))
}
fn planner_controller_expandable_nodes(graph: &ExecutionGraph) -> Vec<String> {
    let mut has_children = std::collections::HashSet::new();
    for node in &graph.nodes {
        for dep in &node.deps {
            has_children.insert(dep.clone());
        }
    }
    graph
        .nodes
        .iter()
        .filter(|n| {
            n.status == NodeStatus::Pending
                && n.node_type == super::decompose::DecomposeNodeType::Analysis
                && !has_children.contains(&n.id)
        })
        .map(|n| n.id.clone())
        .collect()
}
fn try_parse_lenient_json(raw: &str) -> Option<Value> {
    let start = raw.find('{').or_else(|| raw.find('['))?;
    let end = raw.rfind('}').or_else(|| raw.rfind(']'))?;
    if end <= start {
        return None;
    }
    let slice = raw[start..=end].trim();
    serde_json::from_str(slice).ok()
}
fn planner_controller_log_planner_iteration(
    log_dir: &std::path::Path,
    iter: u64,
    graph_nodes: usize,
    signals: &GraphAnalysis,
    update: &GraphPatch,
    raw: &str,
    error: Option<&str>,
) {
    let _ = std::fs::create_dir_all(log_dir);
    let payload = serde_json::json!(
        { "iter" : iter, "graph_nodes" : graph_nodes, "signals" : signals,
        "planner_output" : update, "error" : error, "raw" : raw, }
    );
    let path = log_dir.join(format!("planner_iter_{:04}.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, pretty);
    }
}
