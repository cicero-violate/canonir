use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm_provider::JsonExtractor;
use super::config::LlmEndpoint;
use super::dag::TaskGraph;
use super::dag::Status;
use super::decompose::TaskSpec;
use super::capability::{assert_class_disjoint, Capability, CapabilityClass};
use super::graph_algo::{self, GraphSignals};
use super::policy;
use super::templates::apply_planner_update;
use super::failure_store::FailureStore;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardContext {
    pub recent_rewards: Vec<f64>,
    pub plateaued: bool,
    pub best_reward: f64,
    pub stored_reward: f64,
    pub bootstrap_seed: Option<BootstrapSeed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapSeed {
    pub goal: String,
    pub similarity_score: f64,
    pub reward: f64,
    pub node_summaries: Vec<String>,
    pub capability_set: Vec<String>,
    pub node_count: usize,
    pub edge_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetractSpec {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewriteSpec {
    pub id: String,
    pub new_description: String,
    pub new_capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerUpdate {
    #[serde(default)]
    pub new_nodes: Vec<TaskSpec>,
    #[serde(default)]
    pub new_edges: Vec<EdgeSpec>,
    #[serde(default)]
    pub retract_nodes: Vec<RetractSpec>,
    #[serde(default)]
    pub rewrite_nodes: Vec<RewriteSpec>,
}

pub(crate) struct RepairReport {
    pub count: u64,
    pub ids: Vec<String>,
}

pub struct PlannerSession {
    endpoint_id: String,
    url: String,
    role_schema: String,
    goal: String,
    history: Vec<String>,
    stateful: bool,
    reward_context: Option<RewardContext>,
    prev_bias: Option<policy::PolicyBias>,
}

const MAX_HISTORY: usize = 5;

impl PlannerSession {
    pub fn new(endpoint: &LlmEndpoint, goal: String) -> Self {
        Self {
            endpoint_id: endpoint.id.clone(),
            url: endpoint.url.clone(),
            role_schema: endpoint.role_markdown.clone(),
            goal,
            history: Vec::new(),
            stateful: endpoint.stateful,
            reward_context: None,
            prev_bias: None,
        }
    }

    pub fn set_reward_context(&mut self, ctx: RewardContext) {
        self.reward_context = Some(ctx);
    }

    pub fn build_prompt(
        &mut self,
        graph: &TaskGraph,
        signals: &GraphSignals,
        features: &graph_algo::FeatureVector,
        cost_summary: &str,
        rewrite_requests: &[String],
        planner_max_new_nodes: usize,
        planner_max_new_edges: usize,
        max_nodes: usize,
        max_edges: usize,
    ) -> String {
        let ids: Vec<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
        let signals_json = signals.to_json(&ids);
        let expandable = expandable_nodes(graph);
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
                serde_json::json!({
                    "id": n.id,
                    "description": n.description,
                    "deps": n.deps,
                    "status": n.status,
                    "node_type": n.node_type,
                    "required_capabilities": n.required_capabilities,
                    "priority": n.priority,
                    "budget": n.budget,
                    "reasoning_trace": n.reasoning_trace,
                    "result": n.result,
                })
            })
            .collect();

        let history_tail = self.history.iter().rev().take(6).cloned().collect::<Vec<_>>();
        let reward_section = match &self.reward_context {
            None => String::new(),
            Some(r) => {
                let trend = r.recent_rewards.iter()
                    .map(|v| format!("{:.3}", v))
                    .collect::<Vec<_>>()
                    .join(", ");
                let seed_section = match r.bootstrap_seed.as_ref() {
                    None => String::new(),
                    Some(seed) => format!(
                        "Bootstrap seed (similar prior goal, similarity={:.2}, reward={:.3}):\n\
Prior goal: {}\n\
Prior graph had {} nodes, {} edges.\n\
Capabilities used: {}\n\
Node summaries:\n{}\n\
Consider reusing this structure as a starting point, adapting it to the current goal.\n",
                        seed.similarity_score,
                        seed.reward,
                        seed.goal,
                        seed.node_count,
                        seed.edge_count,
                        seed.capability_set.join(", "),
                        seed.node_summaries.join("\n"),
                    ),
                };
                let base = if r.plateaued {
                    let signals_str = super::graph_algo::planner_signals_for_graph(graph);
                    format!(
                        "Reward history (last {} runs): [{}]\n\
Best recorded reward: {:.3}\n\
STATUS: PLATEAUED. The current graph structure is not improving.\n\
You MUST propose a structurally different graph: different node decomposition, \
different capability assignments, or different dependency topology. Do not make incremental edits.\n\
Current graph topology: {}\n\
Use these signals to identify structural bottlenecks before proposing changes.\n",
                        r.recent_rewards.len(),
                        trend,
                        r.best_reward,
                        signals_str
                    )
                } else {
                    format!(
                        "Reward history (last {} runs): [{}]\n\
Best recorded reward: {:.3}\n\
Continue refining the current graph.\n",
                        r.recent_rewards.len(),
                        trend,
                        r.best_reward
                    )
                };
                base + &seed_section
            }
        };
        let mut features = features.clone();
        if let Some(ctx) = self.reward_context.as_ref() {
            features = features.with_reward_history(&ctx.recent_rewards);
        }
        let normalized = graph_algo::normalize_features(&features, max_nodes, max_edges);
        let bias_raw = policy::PolicyModel::load_default().predict(&normalized);
        let bias_smoothed = policy::smooth_bias(self.prev_bias.as_ref(), bias_raw);
        let bias = policy::maybe_explore(bias_smoothed, 0.05);
        self.prev_bias = Some(bias.clone());
        let bias_text = policy::format_bias(&bias);
        let metrics_text = format!(
            "Metrics:\n\
nodes={} edges={} depth={} scc_count={}\n\
roots={} leaves={} avg_out_deg={:.2} avg_in_deg={:.2} branching={:.2}\n\
verify/mutate={:.2} observe/mutate={:.2} entropy={:.2}\n\
priority_avg={:.2} budget_avg={:.2}\n\
blocked_frac={:.2} ready_frac={:.2} failed_frac={:.2}\n\
completion_velocity={:.3} retry_rate={:.3}\n\
failure_pattern_rate={:.3} cycle_freq={:.3} deadlock_rate={:.3}\n",
            features.nodes,
            features.edges,
            features.depth,
            features.scc_count,
            features.root_count,
            features.leaf_count,
            features.avg_out_degree,
            features.avg_in_degree,
            features.branching_factor,
            features.verify_to_mutate_ratio,
            features.observe_to_mutate_ratio,
            features.node_type_entropy,
            features.avg_node_priority,
            features.avg_node_budget,
            features.blocked_fraction,
            features.ready_fraction,
            features.failed_fraction,
            features.completion_velocity,
            features.retry_rate,
            features.failure_pattern_rate,
            features.cycle_frequency,
            features.deadlock_rate,
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
            planner_max_new_nodes,
            planner_max_new_edges,
            expandable.join(", "),
            ready_nodes.join(", "),
            unreachable_nodes.join(", "),
            rewrite_text,
            bias_text,
            metrics_text,
            cost_summary,
            reward_section,
            self.goal,
            serde_json::to_string_pretty(&nodes_json).unwrap_or_default(),
            serde_json::to_string_pretty(&signals_json).unwrap_or_default(),
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
        signals: &GraphSignals,
    ) -> Result<PlannerUpdate> {
        self.history.push(raw.clone());
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }

        let parsed = JsonExtractor::extract(&raw)
            .or_else(|_| try_parse_loose_json(&raw).ok_or_else(|| anyhow::anyhow!("planner json extract error")))
            .and_then(|payload| serde_json::from_value(payload).context("planner update did not match schema"));

        match parsed {
            Ok(update) => {
                log_planner_iteration(log_dir, iter, graph_nodes_len, signals, &update, &raw, None);
                Ok(update)
            }
            Err(e) => {
                log_planner_iteration(
                    log_dir,
                    iter,
                    graph_nodes_len,
                    signals,
                    &PlannerUpdate {
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

pub(crate) fn validate_planner_update(
    graph: &TaskGraph,
    update: &PlannerUpdate,
    planner_max_new_nodes: usize,
    planner_max_new_edges: usize,
    failure_store: &mut FailureStore,
    iteration: u64,
    failure_constraint_threshold: usize,
    max_constraints: usize,
) -> Result<()> {
    ensure(update.new_nodes.len() <= planner_max_new_nodes, "planner expansion limit exceeded")?;
    ensure(update.new_edges.len() <= planner_max_new_edges, "planner edge limit exceeded")?;

    let mut existing: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut status_by_id: std::collections::HashMap<String, Status> = std::collections::HashMap::new();
    for (idx, node) in graph.nodes.iter().enumerate() {
        existing.insert(node.id.clone(), idx);
        status_by_id.insert(node.id.clone(), node.status);
    }
    let mut new_ids: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    update.new_nodes.iter().try_for_each(|spec| {
        ensure(!spec.id.trim().is_empty(), "planner node id empty")?;
        ensure(!spec.description.trim().is_empty(), "planner node description empty")?;
        ensure(!spec.required_capabilities.iter().any(|c| matches!(c, Capability::Unknown)), "planner node has unknown capability")?;
        ensure(!(existing.contains_key(&spec.id) || new_ids.contains_key(&spec.id)),
               &format!("duplicate node id {}", spec.id))?;
        new_ids.insert(spec.id.clone(), new_ids.len());
        Ok::<(), anyhow::Error>(())
    })?;

    update.new_edges.iter().try_for_each(|edge| {
        ensure(!edge.from.trim().is_empty() && !edge.to.trim().is_empty(), "planner edge endpoints empty")?;
        let from_ok = existing.contains_key(&edge.from) || new_ids.contains_key(&edge.from);
        let to_ok = existing.contains_key(&edge.to) || new_ids.contains_key(&edge.to);
        ensure(from_ok && to_ok, "planner edge references unknown node")
    })?;

    update.retract_nodes.iter().try_for_each(|spec| {
        let status = status_by_id.get(&spec.id).copied()
            .ok_or_else(|| anyhow::anyhow!("retract references unknown node"))?;
        ensure(matches!(status, Status::Pending | Status::Failed), "retract node must be pending or failed")
    })?;

    update.rewrite_nodes.iter().try_for_each(|spec| {
        let status = status_by_id.get(&spec.id).copied()
            .ok_or_else(|| anyhow::anyhow!("rewrite references unknown node"))?;
        ensure(matches!(status, Status::Pending), "rewrite node must be pending")?;
        ensure(!spec.new_capabilities.iter().any(|c| matches!(c, Capability::Unknown)), "rewrite node has unknown capability")?;
        let caps: std::collections::HashSet<_> = spec.new_capabilities.iter().copied().collect();
        assert_class_disjoint(&caps).map_err(|e| anyhow::anyhow!(e))
    })?;

    let mut test_graph = graph.clone();
    apply_planner_update(&mut test_graph, update.clone())?;
    if let Err(e) = test_graph.validate() {
        let msg = e.to_string();
        if msg.contains("cycle detected") {
            failure_store.record_graph("cycle", &test_graph, iteration);
        } else if msg.contains("capability class") {
            failure_store.record_graph("invalid_authority", &test_graph, iteration);
        }
        return Err(anyhow::anyhow!(e));
    }
    let constraints = failure_store.constraints(failure_constraint_threshold, max_constraints);
    if !constraints.is_empty() {
        let signals = graph_algo::compute_graph_signals(&test_graph);
        for c in constraints {
            if let Some(err) = check_constraint(&test_graph, &signals, &c) {
                return Err(anyhow::anyhow!(err));
            }
        }
    }
    Ok(())
}

fn check_constraint(
    graph: &TaskGraph,
    signals: &graph_algo::GraphSignals,
    constraint: &super::failure_store::Constraint,
) -> Option<String> {
    use super::failure_store::ConstraintRule;
    match &constraint.rule {
        ConstraintRule::NoCycle => signals.has_cycle.then(|| "constraint violated: NoCycle".to_string()),
        ConstraintRule::NoUnreachable => (!signals.unreachable.is_empty())
            .then(|| "constraint violated: NoUnreachable".to_string()),
        ConstraintRule::CapabilityConflict => None,
        ConstraintRule::InvalidDependency => None,
        ConstraintRule::PatternRewrite { pattern, .. } => {
            let bad = graph
                .nodes
                .iter()
                .any(|n| n.description.to_lowercase().contains(pattern));
            bad.then(|| format!("constraint violated: PatternRewrite({})", pattern))
        }
        ConstraintRule::SignatureBan => {
            let sig = graph_algo::graph_signature(graph);
            (sig == constraint.signature).then(|| "constraint violated: SignatureBan".to_string())
        }
    }
}

pub(crate) fn auto_repair_planner_update(graph: &TaskGraph, update: &mut PlannerUpdate) -> RepairReport {
    let mut used: std::collections::HashSet<String> =
        graph.nodes.iter().map(|n| n.id.clone()).collect();
    for spec in &update.new_nodes {
        used.insert(spec.id.clone());
    }
    for spec in &update.rewrite_nodes {
        used.insert(spec.id.clone());
    }

    let mut repairs = 0u64;
    let mut ids = Vec::new();
    let mut repaired_nodes: Vec<TaskSpec> = Vec::new();

    for mut spec in update.new_nodes.drain(..) {
        normalize_capabilities(&mut spec.required_capabilities, &spec.description);
        let (observe, verify, mutate) = split_caps(&spec.required_capabilities);
        if !mutate.is_empty() && !verify.is_empty() {
            repairs += 1;
            ids.push(spec.id.clone());
            let verify_id = unique_id(format!("{}_verify", spec.id), &mut used);
            let mut verify_caps = verify;
            verify_caps.extend(observe);
            let verify_node = TaskSpec {
                id: verify_id.clone(),
                description: format!("Verify preconditions for {}: {}", spec.id, spec.description),
                deps: spec.deps.clone(),
                required_capabilities: verify_caps,
                node_type: super::decompose::NodeType::Analysis,
                priority: spec.priority,
                budget: spec.budget,
                reasoning_trace: Some("AUTO_REPAIR: split verify/mutate".to_string()),
            };
            let mut mutate_deps = spec.deps.clone();
            if !mutate_deps.contains(&verify_id) {
                mutate_deps.push(verify_id.clone());
            }
            let mutate_node = TaskSpec {
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
            update.new_edges.push(EdgeSpec { from: verify_id, to: mutate_id });
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
        normalize_capabilities(&mut spec.new_capabilities, &spec.new_description);
        let (observe, verify, mutate) = split_caps(&spec.new_capabilities);
        if !mutate.is_empty() && !verify.is_empty() {
            repairs += 1;
            ids.push(spec.id.clone());
            let verify_id = unique_id(format!("{}_verify", spec.id), &mut used);
            let mut verify_caps = verify;
            verify_caps.extend(observe);
            update.new_nodes.push(TaskSpec {
                id: verify_id.clone(),
                description: format!("Verify preconditions for {}: {}", spec.id, spec.new_description),
                deps: Vec::new(),
                required_capabilities: verify_caps,
                node_type: super::decompose::NodeType::Analysis,
                priority: 5,
                budget: None,
                reasoning_trace: Some("AUTO_REPAIR: split rewrite verify/mutate".to_string()),
            });
            update.new_edges.push(EdgeSpec { from: verify_id, to: spec.id.clone() });
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
    seed_orchestration_node_if_empty(graph, update);
    RepairReport { count: repairs, ids }
}

fn normalize_capabilities(caps: &mut Vec<Capability>, description: &str) {
    let lower = description.to_lowercase();
    if caps.iter().any(|c| matches!(c, Capability::StatelessInvoke)) {
        let replacement = if lower.contains("cargo check") {
            Some(Capability::CargoCheck)
        } else if lower.contains("cargo build") || lower.contains("cargo run") {
            Some(Capability::CargoBuild)
        } else if lower.contains("bash") || lower.contains("shell") {
            Some(Capability::Bash)
        } else {
            None
        };
        if let Some(rep) = replacement {
            *caps = caps
                .iter()
                .filter(|c| !matches!(c, Capability::StatelessInvoke))
                .copied()
                .collect();
            caps.push(rep);
        }
    }
}

fn seed_orchestration_node_if_empty(graph: &TaskGraph, update: &mut PlannerUpdate) {
    if !graph.nodes.is_empty() {
        return;
    }
    let has_orch = update.new_nodes.iter().any(|n| {
        let d = n.description.to_lowercase();
        d.contains("orchestration -- --all") || d.contains("cargo run --bin orchestration")
    });
    if has_orch {
        return;
    }
    let mut used: std::collections::HashSet<String> =
        update.new_nodes.iter().map(|n| n.id.clone()).collect();
    let id = unique_id("run_orchestration_build".to_string(), &mut used);
    update.new_nodes.push(TaskSpec {
        id,
        description: "Run `cargo run --bin orchestration -- --all` to reproduce the pipeline and capture diagnostics."
            .to_string(),
        deps: Vec::new(),
        required_capabilities: vec![Capability::Bash],
        node_type: super::decompose::NodeType::Analysis,
        priority: 10,
        budget: Some(3),
        reasoning_trace: Some("AUTO_SEED: ensure orchestration build runs when graph is empty".to_string()),
    });
}

fn split_caps(caps: &[Capability]) -> (Vec<Capability>, Vec<Capability>, Vec<Capability>) {
    let mut observe = Vec::new();
    let mut verify = Vec::new();
    let mut mutate = Vec::new();
    for &cap in caps {
        match cap.class() {
            CapabilityClass::Observe => observe.push(cap),
            CapabilityClass::Verify => verify.push(cap),
            CapabilityClass::Mutate => mutate.push(cap),
        }
    }
    (observe, verify, mutate)
}

fn unique_id(base: String, used: &mut std::collections::HashSet<String>) -> String {
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

fn ensure(cond: bool, msg: &str) -> Result<()> {
    cond.then_some(()).ok_or_else(|| anyhow::anyhow!("{}", msg))
}

fn expandable_nodes(graph: &TaskGraph) -> Vec<String> {
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
            n.status == Status::Pending
                && n.node_type == super::decompose::NodeType::Analysis
                && !has_children.contains(&n.id)
        })
        .map(|n| n.id.clone())
        .collect()
}

fn try_parse_loose_json(raw: &str) -> Option<Value> {
    let start = raw.find('{').or_else(|| raw.find('['))?;
    let end = raw.rfind('}').or_else(|| raw.rfind(']'))?;
    if end <= start {
        return None;
    }
    let slice = raw[start..=end].trim();
    serde_json::from_str(slice).ok()
}

fn log_planner_iteration(
    log_dir: &std::path::Path,
    iter: u64,
    graph_nodes: usize,
    signals: &GraphSignals,
    update: &PlannerUpdate,
    raw: &str,
    error: Option<&str>,
) {
    let _ = std::fs::create_dir_all(log_dir);
    let payload = serde_json::json!({
        "iter": iter,
        "graph_nodes": graph_nodes,
        "signals": signals,
        "planner_output": update,
        "error": error,
        "raw": raw,
    });
    let path = log_dir.join(format!("planner_iter_{:04}.json", iter));
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, pretty);
    }
}
