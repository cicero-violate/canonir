//! Capability-driven DAG pipeline.

pub mod capability;
pub mod dag;
pub mod config;
pub mod graph_algo;
pub mod graph_runtime;
pub mod executor_dispatch;
pub mod endpoint_scheduler;
pub mod endpoint_worker;
pub mod response_router;
pub mod scheduler;
pub mod planner_session;
pub mod telemetry;
pub mod llm;
pub mod decompose;
pub mod engine;
pub mod act;
pub mod tab_management;

use super::{Pipeline, PipelineContext, PipelineOutcome};
use crate::ir::SystemState;
use crate::layout::FileTopology;
use crate::ws_server::WsBridge;
use anyhow::Result;
use config::{CapabilityConfig, GoalSpec};
use graph_algo::{emit_planned_graph, run_graph_algorithms};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) const LOG_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/capability";

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
    tabs: tab_management::TabsHandle,
    role_rr: tokio::sync::Mutex<HashMap<String, usize>>,
}

impl CapabilityPipeline {
    pub fn new(bridge: WsBridge) -> Self {
        let config = CapabilityConfig::load().expect("failed to load capability config");
        Self {
            bridge,
            config,
            tabs: Arc::new(tokio::sync::Mutex::new(tab_management::TabSlots::new())),
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
        endpoint_worker::init_workers(&self.bridge, &self.config, &self.tabs).await;

        let goal = GoalSpec::from_file(&self.config.goal_file)?;
        if let Ok(pretty) = serde_json::to_string_pretty(&goal) {
            let _ = std::fs::write(Self::log_path("goal_spec.json"), pretty);
        }

        let endpoint = self
            .config
            .llm_endpoints
            .iter()
            .find(|e| e.role.as_deref() != Some("planner"))
            .unwrap_or(&self.config.llm_endpoints[0]);
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

        ensure_unique_node_ids(&mut nodes);
        let mut graph = dag::TaskGraph { nodes, id_index: HashMap::new() };
        let planner_endpoint = self.config.planner_endpoint()?;
        let mut planner_session = planner_session::PlannerSession::new(planner_endpoint, goal.raw.clone());
        emit_planned_graph(&graph, Path::new(LOG_ROOT), 0);
        run_graph_algorithms(&graph, Path::new(LOG_ROOT), 0);
        scheduler::run_planner_execution_loop(
            &mut planner_session,
            &mut graph,
            &self.bridge,
            &self.config,
            &self.role_rr,
            &self.tabs,
            &ctx.cwd,
            &workspace_listing,
            endpoint,
            &policy,
            self.config.context_radius,
            self.config.max_concurrency,
            self.config.max_iterations,
            self.config.tab_cooldown_ms,
            retry_count,
            retry_delay,
            max_output_lines,
        )
        .await
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
