//! Capability-driven DAG pipeline.

pub mod capability;
pub mod dag;
pub mod config;
pub mod graph_algo;
pub mod llm;
pub mod decompose;
pub mod planner;
pub mod engine;
pub mod act;
pub mod tab_management;

use super::{Pipeline, PipelineContext, PipelineOutcome};
use crate::ir::SystemState;
use crate::layout::FileTopology;
use crate::ws_server::WsBridge;
use anyhow::Result;
use config::{CapabilityConfig, GoalSpec};
use dag::{grant_authority, resolve_ready};
use graph_algo::{emit_planned_graph, enforce_linking_constraints, planner_signals_for_graph, run_graph_algorithms};
use std::sync::Arc;
use tokio::sync::Semaphore;
use futures_util::future::join_all;
use std::path::{Path, PathBuf};
use std::collections::HashMap;

const LOG_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/capability";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    ReadFile { path: String },
    ListDir { path: String },
    ReadCommand { command: String, args: Vec<String> },
    WriteFile { path: String, content: String },
    ReplaceText { path: String, find: String, replace: String },
    DeleteFile { path: String },
}

pub struct CapabilityPipeline {
    bridge: WsBridge,
    config: CapabilityConfig,
    tabs: tokio::sync::Mutex<tab_management::TabSlots>,
    role_rr: tokio::sync::Mutex<HashMap<String, usize>>,
}

impl CapabilityPipeline {
    pub fn new(bridge: WsBridge) -> Self {
        let config = CapabilityConfig::load().expect("failed to load capability config");
        Self {
            bridge,
            config,
            tabs: tokio::sync::Mutex::new(tab_management::TabSlots::new()),
            role_rr: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    fn ensure_log_dir() {
        let _ = std::fs::create_dir_all(LOG_ROOT);
    }

    fn log_path(name: &str) -> PathBuf {
        Path::new(LOG_ROOT).join(name)
    }

    pub async fn run_capability_loop(&self, ctx: &PipelineContext) -> Result<()> {
        Self::ensure_log_dir();
        if self.config.llm_endpoints.is_empty() {
            anyhow::bail!("capability config has no llm endpoints");
        }

        let goal = GoalSpec::from_file(&self.config.goal_file)?;
        if let Ok(pretty) = serde_json::to_string_pretty(&goal) {
            let _ = std::fs::write(Self::log_path("goal_spec.json"), pretty);
        }

        let endpoint = &self.config.llm_endpoints[0];
        let retry_count = self.config.llm_retry_count;
        let retry_delay = self.config.llm_retry_delay_secs;
        let max_output_lines = self.config.max_output_lines;
        let workspace_listing = list_workspace_entries(&ctx.cwd[0], 50);
        let policy = config::CapabilityPolicy::load(&ctx.cwd[0])?;

        let decomp = decompose::decompose_goal(
            &goal,
            &self.bridge,
            &endpoint.id,
            &endpoint.url,
            "",
            &self.tabs,
            endpoint.reuse_tabs,
            endpoint.max_tabs,
            &ctx.cwd[0],
            &workspace_listing,
            Path::new(LOG_ROOT),
            self.config.llm_retry_count,
            self.config.llm_retry_delay_secs,
            self.config.tab_cooldown_ms,
        ).await?;
        eprintln!("[capability] decompose_goal tasks={}", decomp.tasks.len());

        let mut nodes: Vec<dag::TaskNode> = decomp.tasks.into_iter().map(|t| dag::TaskNode {
            id: t.id,
            description: t.description,
            status: dag::Status::Pending,
            deps: t.deps,
            required_capabilities: t.required_capabilities,
            node_type: t.node_type,
            result: None,
            error: None,
        }).collect();
        ensure_unique_node_ids(&mut nodes);

        expand_nodes(
            &mut nodes,
            &self.bridge,
            &self.config,
            &self.role_rr,
            &self.tabs,
            &ctx.cwd[0],
            &workspace_listing,
            Path::new(LOG_ROOT),
            retry_count,
            retry_delay,
            self.config.max_nodes,
            self.config.max_expand_iters,
            self.config.max_depth,
        ).await?;
        ensure_unique_node_ids(&mut nodes);
        eprintln!("[capability] expand_nodes total_nodes={}", nodes.len());

        let mut graph = build_graph_from_edges(
            &nodes,
            &self.bridge,
            &self.config,
            &self.role_rr,
            &self.tabs,
            &ctx.cwd[0],
            &workspace_listing,
            None,
            None,
            Path::new(LOG_ROOT),
            retry_count,
            retry_delay,
        ).await?;
        eprintln!("[capability] build_graph_from_edges nodes={}", graph.nodes.len());
        emit_planned_graph(&graph, Path::new(LOG_ROOT), 0);
        run_graph_algorithms(&graph, Path::new(LOG_ROOT), 0);

        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrency.max(1)));
        let mut blocked_streak = 0u32;
        for iter in 1..=self.config.max_iterations {
            resolve_ready(&mut graph);
            let status_snapshot = graph
                .nodes
                .iter()
                .map(|n| serde_json::json!({"id": n.id, "status": n.status}))
                .collect::<Vec<_>>();
            let ready_ids: Vec<String> = graph.ready_nodes().iter().map(|n| n.id.clone()).collect();
            let summary = serde_json::json!({
                "iter": iter,
                "ready": ready_ids,
                "status": status_snapshot
            });
            let _ = std::fs::write(Self::log_path(&format!("iter_{:03}_status.json", iter)), serde_json::to_string_pretty(&summary).unwrap_or_default());
            eprintln!("[capability] {}", summary);

            if graph.all_completed() {
                return Ok(());
            }
            if graph.has_failed() && graph.ready_nodes().is_empty() {
                blocked_streak += 1;
                eprintln!(
                    r#"[capability] {{"iter":{},"event":"blocked","streak":{}}}"#,
                    iter,
                    blocked_streak
                );
                if blocked_streak >= 3 {
                    anyhow::bail!("blocked");
                }
                continue;
            }
            blocked_streak = 0;
            let mut ready_ids: Vec<String> = graph.ready_nodes().iter().map(|n| n.id.clone()).collect();
            ready_ids.sort();
            let mut futures = Vec::new();
            for node_id in ready_ids {
                let node = match graph.nodes.iter().find(|n| n.id == node_id).cloned() {
                    Some(n) => n,
                    None => continue,
                };
                let auth = match grant_authority(&node) {
                    Ok(a) => a,
                    Err(e) => {
                        eprintln!(
                            r#"[capability] {{"iter":{},"event":"authority_error","node":"{}","error":"{}"}}"#,
                            iter,
                            node.id,
                            e
                        );
                        let _ = graph.update_status(&node.id, dag::Status::Ready);
                        continue;
                    }
                };
                let sem = semaphore.clone();
                let bridge = &self.bridge;
                let endpoint_id = endpoint.id.clone();
                let url = endpoint.url.clone();
                let tabs = &self.tabs;
                let workspace_root = ctx.cwd[0].clone();
                let log_dir = Path::new(LOG_ROOT).to_path_buf();
                let node_id = node.id.clone();
                let context = build_context(&graph, &node.id, self.config.context_radius);
                let fut = async move {
                    let _permit = sem.acquire().await.map_err(|_| anyhow::anyhow!("semaphore closed"))?;
                    let res = engine::call_node(
                        &node,
                        &auth,
                        bridge,
                        &endpoint_id,
                        &url,
                        "",
                        tabs,
                        endpoint.reuse_tabs,
                        endpoint.max_tabs,
                        self.config.tab_cooldown_ms,
                        &workspace_root,
                        &context,
                        &log_dir,
                        iter,
                        retry_count,
                        retry_delay,
                    )
                    .await;
                    Ok::<_, anyhow::Error>((node_id, res))
                };
                futures.push(fut);
            }

            let results = join_all(futures).await;
            for item in results {
                match item {
                    Ok((node_id, Ok(call_result))) => {
                        if let Err(e) = engine::apply_node_result(call_result, &mut graph, &ctx.cwd, max_output_lines, Path::new(LOG_ROOT), iter, &policy) {
                            eprintln!(
                                r#"[capability] {{"iter":{},"event":"apply_error","node":"{}","error":"{}"}}"#,
                                iter,
                                node_id,
                                e
                            );
                            let _ = graph.update_status(&node_id, dag::Status::Ready);
                        }
                    }
                    Ok((node_id, Err(e))) => {
                        eprintln!(
                            r#"[capability] {{"iter":{},"event":"call_error","node":"{}","error":"{}"}}"#,
                            iter,
                            node_id,
                            e
                        );
                        let _ = graph.update_status(&node_id, dag::Status::Ready);
                    }
                    Err(e) => {
                        eprintln!(
                            r#"[capability] {{"iter":{},"event":"join_error","error":"{}"}}"#,
                            iter,
                            e
                        );
                    }
                }
            }

            // Replan edges after each iteration to reflect new structure or progress.
            ensure_unique_node_ids(&mut graph.nodes);
            let planner_signals = planner_signals_for_graph(&graph);
            graph = build_graph_from_edges(
                &graph.nodes,
                &self.bridge,
                &self.config,
                &self.role_rr,
                &self.tabs,
                &ctx.cwd[0],
                &workspace_listing,
                None,
                Some(&planner_signals),
                Path::new(LOG_ROOT),
                retry_count,
                retry_delay,
            ).await?;
            let iter_u32 = u32::try_from(iter).unwrap_or(u32::MAX);
            emit_planned_graph(&graph, Path::new(LOG_ROOT), iter_u32);
            run_graph_algorithms(&graph, Path::new(LOG_ROOT), iter_u32);

            if self.config.prune_unlinked {
                prune_unlinked_nodes(&mut graph);
            }
            enforce_semantic_validations(&graph)?;
        }
        anyhow::bail!("iteration limit exceeded")
    }
}

fn list_workspace_entries(root: &Path, limit: usize) -> String {
    let mut entries: Vec<String> = std::fs::read_dir(root)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort();
    entries.truncate(limit);
    entries.join(", ")
}

fn build_context(graph: &dag::TaskGraph, node_id: &str, radius: usize) -> Vec<engine::ContextNode> {
    if radius == 0 {
        return Vec::new();
    }
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut frontier: std::collections::VecDeque<(String, usize)> = std::collections::VecDeque::new();
    frontier.push_back((node_id.to_string(), 0));
    visited.insert(node_id.to_string());

    let by_id: std::collections::HashMap<String, dag::TaskNode> =
        graph.nodes.iter().map(|n| (n.id.clone(), n.clone())).collect();

    let mut result = Vec::new();
    while let Some((current, depth)) = frontier.pop_front() {
        if depth >= radius {
            continue;
        }
        if let Some(node) = by_id.get(&current) {
            // Add parents only (toward root)
            for dep in &node.deps {
                if visited.insert(dep.clone()) {
                    frontier.push_back((dep.clone(), depth + 1));
                }
            }
        }
    }
    for id in visited.iter() {
        if let Some(n) = by_id.get(id) {
            result.push(engine::ContextNode {
                id: n.id.clone(),
                description: n.description.clone(),
                node_type: n.node_type,
                deps: n.deps.clone(),
                required_capabilities: n.required_capabilities.clone(),
                status: n.status,
            });
        }
    }
    result
}

fn prune_unlinked_nodes(graph: &mut dag::TaskGraph) {
    let mut indegree: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut outdegree: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for n in &graph.nodes {
        indegree.insert(n.id.clone(), 0);
        outdegree.insert(n.id.clone(), 0);
    }
    for n in &graph.nodes {
        for d in &n.deps {
            if let Some(v) = indegree.get_mut(&n.id) {
                *v += 1;
            }
            if let Some(v) = outdegree.get_mut(d) {
                *v += 1;
            }
        }
    }
    let keep: std::collections::HashSet<String> = graph
        .nodes
        .iter()
        .filter_map(|n| {
            let in_d = *indegree.get(&n.id).unwrap_or(&0);
            let out_d = *outdegree.get(&n.id).unwrap_or(&0);
            if in_d == 0 && out_d == 0 {
                None
            } else {
                Some(n.id.clone())
            }
        })
        .collect();
    graph.nodes.retain(|n| keep.contains(&n.id));
}

fn enforce_semantic_validations(graph: &dag::TaskGraph) -> Result<()> {
    // 1) All render nodes must be reachable from at least one analysis node.
    let analysis_ids: std::collections::HashSet<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.node_type == decompose::NodeType::Analysis)
        .map(|n| n.id.as_str())
        .collect();
    let render_nodes: Vec<&dag::TaskNode> = graph
        .nodes
        .iter()
        .filter(|n| n.node_type == decompose::NodeType::Render)
        .collect();

    for render in render_nodes {
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![render.id.as_str()];
        let mut ok = false;
        while let Some(cur) = stack.pop() {
            if !seen.insert(cur) {
                continue;
            }
            if analysis_ids.contains(cur) {
                ok = true;
                break;
            }
            if let Some(node) = graph.nodes.iter().find(|n| n.id == cur) {
                for dep in &node.deps {
                    stack.push(dep.as_str());
                }
            }
        }
        if !ok {
            return Err(anyhow::anyhow!("render node {} not reachable from analysis node", render.id));
        }
    }

    // 2) No depth beyond max_depth (approx by DFS).
    // This will be checked elsewhere by limiting expansion; kept as sanity.
    Ok(())
}

fn is_expandable(node: &dag::TaskNode) -> bool {
    node.node_type == decompose::NodeType::Analysis
        || node.required_capabilities.iter().any(|c| matches!(
            c,
            capability::Capability::GoalToSubgoals | capability::Capability::RefineNode
        ))
}

#[derive(Clone)]
struct EndpointCtx {
    id: String,
    url: String,
    reuse_tabs: bool,
    max_tabs: usize,
}

fn role_burst(config: &CapabilityConfig, role: &str) -> usize {
    let role_cfg = config.role_config(role);
    role_cfg.burst.unwrap_or_else(|| config.max_concurrency.max(1))
}

async fn select_endpoints_for_role(
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<HashMap<String, usize>>,
    role: &str,
    burst: usize,
) -> Vec<EndpointCtx> {
    let role_cfg = config.role_config(role);
    let mut weights: Vec<(usize, u32)> = Vec::new();
    let mut total = 0u32;
    for (idx, ep) in config.llm_endpoints.iter().enumerate() {
        let w = role_cfg.weights.get(&ep.id).copied().unwrap_or(0);
        if w > 0 {
            weights.push((idx, w));
            total += w;
        }
    }
    let use_default_weights = total == 0;
    if use_default_weights {
        for (idx, _ep) in config.llm_endpoints.iter().enumerate() {
            weights.push((idx, 1));
            total += 1;
        }
    }
    if weights.is_empty() {
        return Vec::new();
    }

    let mut selected = Vec::with_capacity(burst.max(1));
    for _ in 0..burst.max(1) {
        let idx = {
            let mut rr = role_rr.lock().await;
            let entry = rr.entry(role.to_string()).or_insert(0);
            let sel = *entry % (total as usize);
            *entry = entry.wrapping_add(1);
            sel
        };
        let mut acc = 0usize;
        let mut chosen = weights[0].0;
        for (ep_idx, w) in &weights {
            acc += *w as usize;
            if idx < acc {
                chosen = *ep_idx;
                break;
            }
        }
        let ep = &config.llm_endpoints[chosen];
        selected.push(EndpointCtx {
            id: ep.id.clone(),
            url: ep.url.clone(),
            reuse_tabs: ep.reuse_tabs,
            max_tabs: ep.max_tabs,
        });
    }
    selected
}

fn merge_decompose_outputs(outputs: Vec<decompose::DecomposeOutput>) -> decompose::DecomposeOutput {
    let mut tasks: Vec<decompose::TaskSpec> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut counter: HashMap<String, usize> = HashMap::new();
    for output in outputs {
        for mut t in output.tasks {
            if !seen.insert(t.id.clone()) {
                let c = counter.entry(t.id.clone()).or_insert(0);
                *c += 1;
                t.id = format!("{}__{}", t.id, c);
                let _ = seen.insert(t.id.clone());
            }
            tasks.push(t);
        }
    }
    decompose::DecomposeOutput { tasks }
}

fn merge_edge_plans(plans: Vec<planner::EdgePlan>) -> planner::EdgePlan {
    if plans.is_empty() {
        return planner::EdgePlan { edges: Vec::new() };
    }
    let threshold = (plans.len() + 1) / 2;
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut edge_map: HashMap<String, planner::EdgeSpec> = HashMap::new();
    for plan in &plans {
        for edge in &plan.edges {
            let key = format!("{}->{}", edge.from, edge.to);
            *counts.entry(key.clone()).or_insert(0) += 1;
            edge_map.entry(key).or_insert_with(|| planner::EdgeSpec {
                from: edge.from.clone(),
                to: edge.to.clone(),
            });
        }
    }
    let mut edges: Vec<planner::EdgeSpec> = counts
        .into_iter()
        .filter(|(_, c)| *c >= threshold)
        .filter_map(|(k, _)| edge_map.get(&k).cloned())
        .collect();
    if edges.is_empty() {
        edges = plans[0].edges.clone();
    }
    planner::EdgePlan { edges }
}

fn ensure_unique_node_ids(nodes: &mut Vec<dag::TaskNode>) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for n in nodes.iter_mut() {
        let count = counts.entry(n.id.clone()).or_insert(0);
        if *count > 0 {
            let new_id = format!("{}__{}", n.id, *count);
            n.id = new_id;
        }
        *count += 1;
    }
}

async fn decompose_node_burst(
    spec: decompose::TaskSpec,
    bridge: &WsBridge,
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<HashMap<String, usize>>,
    tabs: &tokio::sync::Mutex<tab_management::TabSlots>,
    workspace_root: &Path,
    workspace_listing: &str,
    log_dir: &Path,
    retries: u32,
    delay_secs: u64,
) -> Result<decompose::DecomposeOutput> {
    let burst = role_burst(config, "decompose").min(config.max_concurrency.max(1));
    let endpoints = select_endpoints_for_role(config, role_rr, "decompose", burst).await;
    eprintln!("[capability] decompose_node_burst node={} burst={} endpoints={}", spec.id, burst, endpoints.len());
    let mut futures = Vec::new();
    for ep in endpoints {
        let id = ep.id.clone();
        let url = ep.url.clone();
        let reuse_tabs = ep.reuse_tabs;
        let max_tabs = ep.max_tabs;
        let spec = spec.clone();
        let fut = async move {
            decompose::decompose_node(
                spec,
                bridge,
                id.as_str(),
                url.as_str(),
                "",
                tabs,
                reuse_tabs,
                max_tabs,
                workspace_root,
                workspace_listing,
                log_dir,
                retries,
                delay_secs,
                config.tab_cooldown_ms,
            )
            .await
        };
        futures.push(fut);
    }
    let results = join_all(futures).await;
    eprintln!("[capability] decompose_node_burst node={} results={}", spec.id, results.len());
    let mut outputs = Vec::new();
    for res in results {
        if let Ok(o) = res {
            outputs.push(o);
        }
    }
    if outputs.is_empty() {
        anyhow::bail!("decompose_node burst produced no outputs");
    }
    Ok(merge_decompose_outputs(outputs))
}

async fn plan_edges_burst(
    nodes: &[dag::TaskNode],
    bridge: &WsBridge,
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<HashMap<String, usize>>,
    tabs: &tokio::sync::Mutex<tab_management::TabSlots>,
    workspace_root: &Path,
    workspace_listing: &str,
    constraint_note: Option<&str>,
    planner_signals: Option<&str>,
    log_dir: &Path,
    retries: u32,
    delay_secs: u64,
) -> Result<planner::EdgePlan> {
    let burst = role_burst(config, "planner").min(config.max_concurrency.max(1));
    let endpoints = select_endpoints_for_role(config, role_rr, "planner", burst).await;
    eprintln!("[capability] plan_edges_burst start nodes={} burst={} endpoints={}", nodes.len(), burst, endpoints.len());
    let mut futures = Vec::new();
    for ep in endpoints {
        let id = ep.id.clone();
        let url = ep.url.clone();
        let reuse_tabs = ep.reuse_tabs;
        let max_tabs = ep.max_tabs;
        let fut = async move {
            planner::plan_edges(
                nodes,
                bridge,
                id.as_str(),
                url.as_str(),
                "",
                tabs,
                reuse_tabs,
                max_tabs,
                workspace_root,
                workspace_listing,
                constraint_note,
                planner_signals,
                log_dir,
                retries,
                delay_secs,
                config.tab_cooldown_ms,
            )
            .await
        };
        futures.push(fut);
    }
    let results = join_all(futures).await;
    eprintln!("[capability] plan_edges_burst done results={}", results.len());
    let mut plans = Vec::new();
    for res in results {
        if let Ok(p) = res {
            plans.push(p);
        }
    }
    if plans.is_empty() {
        anyhow::bail!("planner burst produced no outputs");
    }
    Ok(merge_edge_plans(plans))
}

async fn expand_nodes(
    nodes: &mut Vec<dag::TaskNode>,
    bridge: &WsBridge,
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<HashMap<String, usize>>,
    tabs: &tokio::sync::Mutex<tab_management::TabSlots>,
    workspace_root: &Path,
    workspace_listing: &str,
    log_dir: &Path,
    retries: u32,
    delay_secs: u64,
    max_nodes: usize,
    max_iters: u32,
    max_depth: usize,
) -> Result<()> {
    let mut iter = 0u32;
    let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut depth_map: HashMap<String, usize> = HashMap::new();
    {
        let mut expandable: Vec<&dag::TaskNode> = nodes.iter().filter(|n| is_expandable(n)).collect();
        expandable.sort_by(|a, b| a.id.cmp(&b.id));
        for node in expandable {
            let depth = node.id.matches("__").count() + 1;
            depth_map.insert(node.id.clone(), depth);
            queue.push_back(node.id.clone());
        }
    }
    while iter < max_iters && nodes.len() < max_nodes {
        if queue.is_empty() {
            break;
        }
        let burst = role_burst(config, "decompose").max(1);
        let nodes_per_batch = (config.max_concurrency / burst).max(1);
        let current_depth = queue
            .front()
            .and_then(|id| depth_map.get(id).cloned())
            .unwrap_or(1);
        let mut batch_ids = Vec::new();
        while let Some(id) = queue.front() {
            let depth = depth_map.get(id).cloned().unwrap_or(1);
            if depth != current_depth || batch_ids.len() >= nodes_per_batch {
                break;
            }
            batch_ids.push(queue.pop_front().unwrap());
        }
        let mut futures = Vec::new();
        for id in batch_ids {
            let node = match nodes.iter().find(|n| n.id == id) {
                Some(n) if is_expandable(n) => n.clone(),
                _ => continue,
            };
            let spec = decompose::TaskSpec {
                id: node.id.clone(),
                description: node.description.clone(),
                deps: node.deps.clone(),
                required_capabilities: node.required_capabilities.clone(),
                node_type: node.node_type,
            };
            let parent_id = node.id.clone();
            let fut = decompose_node_burst(
                spec,
                bridge,
                config,
                role_rr,
                tabs,
                workspace_root,
                workspace_listing,
                log_dir,
                retries,
                delay_secs,
            );
            futures.push(async move { (parent_id, fut.await) });
        }
        let results = join_all(futures).await;
        for (parent_id, res) in results {
            if nodes.len() >= max_nodes {
                break;
            }
            let output = match res {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(r#"[capability] {{"event":"decompose_node_error","node":"{}","error":"{}"}}"#, parent_id, e);
                    if nodes.iter().any(|n| n.id == parent_id) {
                        queue.push_back(parent_id.clone());
                    }
                    continue;
                }
            };
            nodes.retain(|n| n.id != parent_id);
            let mut idx = 0usize;
            for mut child in output.tasks {
                idx += 1;
                if child.id == parent_id || nodes.iter().any(|n| n.id == child.id) {
                    child.id = format!("{}__{}", parent_id, idx);
                }
                let depth = current_depth + 1;
                if depth > max_depth {
                    continue;
                }
                let child_id = child.id.clone();
                nodes.push(dag::TaskNode {
                    id: child_id.clone(),
                    description: child.description,
                    status: dag::Status::Pending,
                    deps: child.deps,
                    required_capabilities: child.required_capabilities,
                    node_type: child.node_type,
                    result: None,
                    error: None,
                });
                depth_map.insert(child_id.clone(), depth);
                queue.push_back(child_id);
                if nodes.len() >= max_nodes {
                    break;
                }
            }
        }
        ensure_unique_node_ids(nodes);
        iter += 1;
    }
    Ok(())
}

async fn build_graph_from_edges(
    nodes: &[dag::TaskNode],
    bridge: &WsBridge,
    config: &CapabilityConfig,
    role_rr: &tokio::sync::Mutex<HashMap<String, usize>>,
    tabs: &tokio::sync::Mutex<tab_management::TabSlots>,
    workspace_root: &Path,
    workspace_listing: &str,
    constraint_note: Option<&str>,
    planner_signals: Option<&str>,
    log_dir: &Path,
    retries: u32,
    delay_secs: u64,
) -> Result<dag::TaskGraph> {
    let plan = plan_edges_burst(
        nodes,
        bridge,
        config,
        role_rr,
        tabs,
        workspace_root,
        workspace_listing,
        constraint_note,
        planner_signals,
        log_dir,
        retries,
        delay_secs,
    ).await?;

    let mut graph = dag::TaskGraph { nodes: nodes.to_vec() };
    let ids: std::collections::HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
    for n in &mut graph.nodes {
        n.deps.clear();
    }
    for edge in plan.edges {
        if !ids.contains(&edge.from) || !ids.contains(&edge.to) {
            eprintln!(
                r#"[capability] {{"event":"edge_rejected","from":"{}","to":"{}"}}"#,
                edge.from,
                edge.to
            );
            continue;
        }
        if let Some(node) = graph.get_node_mut(&edge.to) {
            node.deps.push(edge.from);
        }
    }
    if let Err(e) = graph.validate() {
        let note = format!("previous edge set invalid: {}", e);
        eprintln!(r#"[capability] {{"event":"edge_validate_failed","error":"{}"}}"#, e);
        let plan = planner::plan_edges(
            nodes,
            bridge,
            &config.llm_endpoints[0].id,
            &config.llm_endpoints[0].url,
            "",
            tabs,
            config.llm_endpoints[0].reuse_tabs,
            config.llm_endpoints[0].max_tabs,
            workspace_root,
            workspace_listing,
            Some(&note),
            planner_signals,
            log_dir,
            retries,
            delay_secs,
            config.tab_cooldown_ms,
        ).await?;
        let mut graph_retry = dag::TaskGraph { nodes: nodes.to_vec() };
        let ids: std::collections::HashSet<String> = graph_retry.nodes.iter().map(|n| n.id.clone()).collect();
        for n in &mut graph_retry.nodes {
            n.deps.clear();
        }
        for edge in plan.edges {
            if !ids.contains(&edge.from) || !ids.contains(&edge.to) {
                eprintln!(
                    r#"[capability] {{"event":"edge_rejected","from":"{}","to":"{}"}}"#,
                    edge.from,
                    edge.to
                );
                continue;
            }
            if let Some(node) = graph_retry.get_node_mut(&edge.to) {
                node.deps.push(edge.from);
            }
        }
        if let Err(e2) = enforce_linking_constraints(&graph_retry) {
            eprintln!(r#"[capability] {{"event":"edge_constraint_failed","error":"{}"}}"#, e2);
            return Err(anyhow::anyhow!(e2));
        }
        graph_retry.validate().map_err(|e| anyhow::anyhow!(e))?;
        return Ok(graph_retry);
    }
    if let Err(e) = enforce_linking_constraints(&graph) {
        let note = format!("linking constraints failed: {}", e);
        eprintln!(r#"[capability] {{"event":"edge_constraint_failed","error":"{}"}}"#, e);
        let plan = planner::plan_edges(
            nodes,
            bridge,
            &config.llm_endpoints[0].id,
            &config.llm_endpoints[0].url,
            "",
            tabs,
            config.llm_endpoints[0].reuse_tabs,
            config.llm_endpoints[0].max_tabs,
            workspace_root,
            workspace_listing,
            Some(&note),
            planner_signals,
            log_dir,
            retries,
            delay_secs,
            config.tab_cooldown_ms,
        ).await?;
        let mut graph_retry = dag::TaskGraph { nodes: nodes.to_vec() };
        let ids: std::collections::HashSet<String> = graph_retry.nodes.iter().map(|n| n.id.clone()).collect();
        for n in &mut graph_retry.nodes {
            n.deps.clear();
        }
        for edge in plan.edges {
            if !ids.contains(&edge.from) || !ids.contains(&edge.to) {
                eprintln!(
                    r#"[capability] {{"event":"edge_rejected","from":"{}","to":"{}"}}"#,
                    edge.from,
                    edge.to
                );
                continue;
            }
            if let Some(node) = graph_retry.get_node_mut(&edge.to) {
                node.deps.push(edge.from);
            }
        }
        graph_retry.validate().map_err(|e| anyhow::anyhow!(e))?;
        if let Err(e3) = enforce_linking_constraints(&graph_retry) {
            return Err(anyhow::anyhow!(e3));
        }
        return Ok(graph_retry);
    }
    Ok(graph)
}


#[async_trait::async_trait]
impl Pipeline for CapabilityPipeline {
    fn name(&self) -> &str {
        "capability"
    }

    async fn run_tick(&self, ctx: &PipelineContext, _ir: &mut SystemState, _layout: &mut FileTopology) -> Result<PipelineOutcome> {
        match self.run_capability_loop(ctx).await {
            Ok(()) => Ok(PipelineOutcome { reward: 1.0, summary: "capability completed".into(), advanced: true }),
            Err(e) => Ok(PipelineOutcome { reward: -1.0, summary: format!("capability error: {e}"), advanced: false }),
        }
    }
}
