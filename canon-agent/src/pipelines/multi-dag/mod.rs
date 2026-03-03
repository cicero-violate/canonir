//! DAG-controlled multi-dag pipeline.
//!
//! Replaces the linear phase loop with:
//! Goal → D_g → Tasks → P → TaskGraph → X → V → TaskGraph → repeat.

pub mod act;
pub mod config;
pub mod goal;
pub mod decompose;
pub mod dag;
pub mod planner;
pub mod execute;
pub mod verify;
pub mod scheduler;
pub mod llm;

use super::{Pipeline, PipelineContext, PipelineOutcome};
use crate::ir::SystemState;
use crate::layout::FileTopology;
use crate::ws_server::WsBridge;
use anyhow::Result;
use config::AgentConfig;
use dag::TaskGraph;
use decompose::decompose_goal;
use execute::execute_ready;
use goal::GoalSpec;
use llm::DagTabSlots;
use llm::preflight_tabs;
use planner::plan_dag;
use scheduler::update_ready_states;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use verify::verify_graph;

const LOG_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/dag";
const MAX_ITERS: u64 = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    ReadFile { path: String },
    ListDir { path: String },
    ReadCommand { command: String, args: Vec<String> },
    WriteFile { path: String, content: String },
    ReplaceText { path: String, find: String, replace: String },
    DeleteFile { path: String },
}

pub struct AgentPipeline {
    bridge: WsBridge,
    config: AgentConfig,
    tabs: tokio::sync::Mutex<DagTabSlots>,
    state: tokio::sync::Mutex<PipelineState>,
}

struct PipelineState {
    started: bool,
    completed: bool,
    preflight_done: bool,
}

impl AgentPipeline {
    pub fn new(bridge: WsBridge) -> Self {
        let config = AgentConfig::load().expect("failed to load multi-dag agent config");
        Self {
            bridge,
            config,
            tabs: tokio::sync::Mutex::new(DagTabSlots::new()),
            state: tokio::sync::Mutex::new(PipelineState { started: false, completed: false, preflight_done: false }),
        }
    }

    fn ensure_log_dir() {
        let _ = std::fs::create_dir_all(LOG_ROOT);
    }

    fn log_path(name: &str) -> PathBuf {
        Path::new(LOG_ROOT).join(name)
    }

    fn iter_log_path(iter: u64, name: &str) -> PathBuf {
        Path::new(LOG_ROOT).join(format!("iter_{:03}_{}", iter, name))
    }

    fn build_system_prompt(&self, role: &str) -> Result<String> {
        let card = self.config.card_by_role(role)?;
        let tools = if card.tool_capabilities.is_empty() { "(none)".to_string() } else { card.tool_capabilities.join(", ") };
        let schema = role_schema(role);
        Ok(format!(
            "{}\n\n# Role\n{}\n\n# Goal\n{}\n\n# Tool Capabilities\n{}\n\n# Tool Protocol\n{}\n\n# Role Output Schema\n{}\n\n# Delta Schema\n{}\n",
            card.role_markdown.trim_end(),
            role,
            card.goal_markdown.trim_end(),
            tools,
            tool_protocol(),
            schema,
            delta_schema(),
        ))
    }

    async fn run_dag_loop(&self, ctx: &PipelineContext) -> Result<()> {
        Self::ensure_log_dir();

        let decompose_card = self.config.card_by_role("decompose")?;
        let planner_card = self.config.card_by_role("planner")?;
        let executor_card = self.config.card_by_role("executor")?;
        let verifier_card = self.config.card_by_role("verifier")?;

        let decompose_prompt = self.build_system_prompt("decompose")?;
        let planner_prompt = self.build_system_prompt("planner")?;
        let executor_prompt = self.build_system_prompt("executor")?;
        let verifier_prompt = self.build_system_prompt("verifier")?;

        {
            let mut state = self.state.lock().await;
            if !state.preflight_done {
                eprintln!("[dag] preflight start");
                preflight_tabs(
                    &self.bridge,
                    &[
                        ("decompose", &decompose_card.agent_url),
                        ("planner", &planner_card.agent_url),
                        ("executor", &executor_card.agent_url),
                        ("verifier", &verifier_card.agent_url),
                    ],
                    &self.tabs,
                )
                .await?;
                eprintln!("[dag] preflight ok");
                state.preflight_done = true;
            }
        }

        let goal_file = "/workspace/ai_sandbox/canon/canon-agent-prompts/AGENT_GOAL.md";
        let goal = GoalSpec::from_file(goal_file).unwrap_or_else(|_| GoalSpec::new(decompose_card.goal_markdown.clone()));
        let goal_path = Self::log_path("goal_spec.json");
        if let Ok(pretty) = serde_json::to_string_pretty(&goal) {
            let _ = std::fs::write(goal_path, pretty);
        }

        let log_dir = Path::new(LOG_ROOT);

        eprintln!("[dag] decompose start");
        let decomp = decompose_goal(
            &goal,
            &self.bridge,
            &decompose_card.agent_url,
            &decompose_prompt,
            &self.tabs,
            log_dir,
        )
        .await?;
        eprintln!("[dag] decompose ok (tasks={})", decomp.tasks.len());

        eprintln!("[dag] planner start");
        let mut graph: TaskGraph = plan_dag(
            &decomp.tasks,
            &self.bridge,
            &planner_card.agent_url,
            &planner_prompt,
            &self.tabs,
            log_dir,
        )
        .await?;
        eprintln!("[dag] planner ok (nodes={})", graph.nodes.len());

        for iter in 1..=MAX_ITERS {
            update_ready_states(&mut graph);

            if graph.all_completed() {
                eprintln!("[dag] all completed at iter={}", iter);
                return Ok(());
            }

            if graph.has_failed() && graph.ready_nodes().is_empty() {
                anyhow::bail!("dag blocked: failed nodes present with no ready nodes");
            }

            eprintln!("[dag] iter {} ready={}", iter, graph.ready_nodes().len());
            match execute_ready(
                &graph,
                &self.bridge,
                &executor_card.agent_url,
                &executor_prompt,
                &self.tabs,
                log_dir,
                iter,
                &ctx.cwd,
                self.config.max_output_lines,
            )
            .await
            {
                Ok(exec) => {
                    graph = exec.updated_graph;
                    eprintln!("[dag] iter {} execute ok", iter);
                }
                Err(e) => {
                    eprintln!("[dag] execute error: {e}");
                }
            }

            match verify_graph(
                &graph,
                &self.bridge,
                &verifier_card.agent_url,
                &verifier_prompt,
                &self.tabs,
                log_dir,
                iter,
            )
            .await
            {
                Ok(verified) => {
                    graph = verified.updated_graph;
                    eprintln!("[dag] iter {} verify ok", iter);
                }
                Err(e) => {
                    eprintln!("[dag] verify error: {e}");
                }
            }
        }

        anyhow::bail!("iteration limit exceeded");
    }
}

#[async_trait::async_trait]
impl Pipeline for AgentPipeline {
    fn name(&self) -> &str {
        "multi-dag"
    }

    async fn run_tick(&self, ctx: &PipelineContext, _ir: &mut SystemState, _layout: &mut FileTopology) -> Result<PipelineOutcome> {
        let mut state = self.state.lock().await;
        if state.completed {
            return Ok(PipelineOutcome { reward: 0.0, summary: "dag already completed".into(), advanced: true });
        }
        if !state.started {
            state.started = true;
        }
        drop(state);

        let outcome = match self.run_dag_loop(ctx).await {
            Ok(()) => {
                let mut state = self.state.lock().await;
                state.completed = true;
                Ok(PipelineOutcome { reward: 1.0, summary: "dag completed".into(), advanced: true })
            }
            Err(e) => {
                Ok(PipelineOutcome { reward: -1.0, summary: format!("dag error: {e}"), advanced: false })
            }
        };

        outcome
    }
}

fn tool_protocol() -> String {
    "apply_patch -> write_file | replace_text | delete_file (executor only)\n\
read-only -> read_file | list_dir | read_command (all agents as needed)\n\
Notes: free-form shell is not allowed; use read_command with explicit command+args."
        .to_string()
}

fn delta_schema() -> String {
    r#"[
  { "type": "read_file", "path": "relative/or/absolute" },
  { "type": "list_dir", "path": "relative/or/absolute" },
  { "type": "read_command", "command": "rg", "args": ["pattern", "path"] },
  { "type": "write_file", "path": "relative/or/absolute", "content": "full file content" },
  { "type": "replace_text", "path": "relative/or/absolute", "find": "old", "replace": "new" },
  { "type": "delete_file", "path": "relative/or/absolute" }
]"#
        .to_string()
}

fn role_schema(role: &str) -> String {
    match role {
        "decompose" => {
            r#"Return exactly one fenced ```json block with:
{
  "tasks": [
    { "id": "t1", "description": "string", "deps": [] }
  ]
}
No extra keys, no prose outside the JSON fence."#
                .to_string()
        }
        "planner" => {
            r#"Return exactly one fenced ```json block with:
{
  "nodes": [
    { "id": "t1", "description": "string", "status": "pending", "deps": [] }
  ]
}
No extra keys, no prose outside the JSON fence."#
                .to_string()
        }
        "executor" => {
            r#"Return exactly one fenced ```json block with:
{
  "results": [
    { "id": "t1", "deltas": [ { "type": "write_file", "path": "x", "content": "..." } ], "rationale": "string" }
  ]
}
No extra keys, no prose outside the JSON fence."#
                .to_string()
        }
        "verifier" => {
            r#"Return exactly one fenced ```json block with:
{
  "updates": [
    { "id": "t1", "status": "completed", "error": null }
  ]
}
No extra keys, no prose outside the JSON fence."#
                .to_string()
        }
        _ => "Return exactly one fenced ```json block.".to_string(),
    }
}
