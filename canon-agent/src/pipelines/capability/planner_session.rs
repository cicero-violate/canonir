use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm_provider::JsonExtractor;
use crate::ws_server::WsBridge;

use super::config::LlmEndpoint;
use super::dag::TaskGraph;
use super::dag::Status;
use super::decompose::TaskSpec;
use super::endpoint_worker;
use super::graph_algo::GraphSignals;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
}
use super::tab_management::TabsHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerUpdate {
    #[serde(default)]
    pub new_nodes: Vec<TaskSpec>,
    #[serde(default)]
    pub new_edges: Vec<EdgeSpec>,
}

pub struct PlannerSession {
    endpoint_id: String,
    url: String,
    role_schema: String,
    goal: String,
    history: Vec<String>,
    stateful: bool,
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
        }
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
                    "result": n.result,
                })
            })
            .collect();

        let history_tail = self.history.iter().rev().take(6).cloned().collect::<Vec<_>>();
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
Goal:\n{}\n\n\
Graph Nodes:\n{}\n\n\
Graph Signals:\n{}\n\n\
Recent History:\n{}\n\n\
Return JSON only with schema:\n{{\n  \"new_nodes\": [{{\"id\":\"...\",\"description\":\"...\",\"deps\":[],\"required_capabilities\":[],\"node_type\":\"analysis|render\"}}],\n  \"new_edges\": [{{\"from\":\"id\",\"to\":\"id\"}}]\n}}",
            planner_max_new_nodes,
            planner_max_new_edges,
            expandable.join(", "),
            ready_nodes.join(", "),
            unreachable_nodes.join(", "),
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
                        &PlannerUpdate { new_nodes: Vec::new(), new_edges: Vec::new() },
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
