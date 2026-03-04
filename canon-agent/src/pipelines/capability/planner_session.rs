use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm_provider::JsonExtractor;
use crate::ws_server::WsBridge;

use super::config::LlmEndpoint;
use super::dag::TaskGraph;
use super::dag::Status;
use super::decompose::TaskSpec;
use super::capability::Capability;
use super::endpoint_worker;
use super::graph_algo::GraphSignals;
use super::graph_algo::graph_features;
use super::policy;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
}
use super::tab_management::TabsHandle;

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

pub struct PlannerSession {
    endpoint_id: String,
    url: String,
    role_schema: String,
    goal: String,
    history: Vec<String>,
    stateful: bool,
    reward_context: Option<RewardContext>,
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
        }
    }

    pub fn set_reward_context(&mut self, ctx: RewardContext) {
        self.reward_context = Some(ctx);
    }

    pub async fn planner_iteration(
        &mut self,
        graph: &TaskGraph,
        signals: &GraphSignals,
        bridge: &WsBridge,
        tabs: &TabsHandle,
        max_tabs: usize,
        tab_cooldown_ms: u64,
        retries: u32,
        delay_secs: u64,
        log_dir: &std::path::Path,
        iter: u64,
        planner_max_new_nodes: usize,
        planner_max_new_edges: usize,
    ) -> Result<PlannerUpdate> {
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
        let mut features = graph_features(graph);
        if let Some(ctx) = self.reward_context.as_ref() {
            features = features.with_reward_history(&ctx.recent_rewards);
        }
        let bias = policy::PolicyModel::load_default().predict(&features);
        let bias_text = policy::format_bias(&bias);
        let prompt = format!(
            "You are a planner. Maintain continuity across iterations.\n\
Planner limits: max_new_nodes={}, max_new_edges={}\n\
Expandable nodes: {}\n\
Ready nodes: {}\n\
Unreachable nodes: {}\n\
Rules:\n\
1) Prefer expanding root or blocked nodes.\n\
2) Avoid expanding nodes already expanded.\n\
3) Prefer nodes with unsatisfied dependencies.\n\
4) Never exceed planner_max_new_nodes.\n\
5) Retract nodes that are Pending or Failed with no dependents.\n\
6) Rewrite nodes that are Pending with an imprecise description.\n\
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
            bias_text,
            reward_section,
            self.goal,
            serde_json::to_string_pretty(&nodes_json).unwrap_or_default(),
            serde_json::to_string_pretty(&signals_json).unwrap_or_default(),
            history_tail.join("\n")
        );

        let attempts = retries.max(1);
        for attempt in 1..=attempts {
            let allow_mismatch = attempt > 1 && self.history.is_empty();
            let raw = endpoint_worker::send_request(
                bridge,
                &self.endpoint_id,
                &self.url,
                self.stateful,
                &prompt,
                &self.role_schema,
                None,
                None,
                allow_mismatch,
                "planner",
                tabs,
                max_tabs,
                tab_cooldown_ms,
            )
            .await;

            let raw = match raw {
                Ok(v) => v,
                Err(e) => {
                    if e.to_string().contains("req_id mismatch") {
                        self.history.clear();
                        let retry_raw = endpoint_worker::send_request(
                            bridge,
                            &self.endpoint_id,
                            &self.url,
                            self.stateful,
                            &prompt,
                            &self.role_schema,
                            None,
                            None,
                            true,
                            "planner",
                            tabs,
                            max_tabs,
                            tab_cooldown_ms,
                        )
                        .await?;
                        retry_raw
                    } else {
                        return Err(e);
                    }
                }
            };

            self.history.push(raw.clone());
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }

            let parsed = JsonExtractor::extract(&raw)
                .or_else(|_| try_parse_loose_json(&raw).ok_or_else(|| anyhow::anyhow!("planner json extract error")))
                .and_then(|payload| serde_json::from_value(payload).context("planner update did not match schema"));

            match parsed {
                Ok(update) => {
                    log_planner_iteration(log_dir, iter, graph.nodes.len(), signals, &update, &raw, None);
                    return Ok(update);
                }
                Err(e) => {
                    log_planner_iteration(
                        log_dir,
                        iter,
                        graph.nodes.len(),
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
                    if attempt < attempts {
                        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        Err(anyhow::anyhow!("planner retries exhausted"))
    }
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
