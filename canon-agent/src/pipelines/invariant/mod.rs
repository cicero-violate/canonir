//! Deterministic invariant pipeline: Observe → Plan → Act → Verify.
//!
//! Requirements enforced:
//! - Fixed phase machine, one phase per tick.
//! - LLM response schema: { phase, deltas, rationale }.
//! - Structured deltas only (no free-form shell execution).
//! - Explicit logging per tick, replayable from logs.

pub mod act;
pub mod config;
pub mod observe;
pub mod plan;

use super::{Pipeline, PipelineContext, PipelineOutcome};
use crate::ir::SystemState;
use crate::layout::FileTopology;
use crate::ws_server::WsBridge;
use act::{apply_mutations, apply_read_only, DeltaOutcome};
use config::AgentConfig;
use plan::{request_plan, AgentResponse};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

const LOG_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Observe,
    Plan,
    Act,
    Verify,
}

impl Phase {
    fn next(&self) -> Self {
        match self {
            Phase::Observe => Phase::Plan,
            Phase::Plan => Phase::Act,
            Phase::Act => Phase::Verify,
            Phase::Verify => Phase::Observe,
        }
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Observe => write!(f, "observe"),
            Phase::Plan => write!(f, "plan"),
            Phase::Act => write!(f, "act"),
            Phase::Verify => write!(f, "verify"),
        }
    }
}

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

#[derive(Debug, Clone, Serialize)]
struct DeltasAppliedLog {
    phase: Phase,
    deltas: Vec<Delta>,
    results: Vec<DeltaOutcome>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct StateSnapshot<'a> {
    current_phase: &'a Phase,
    tick: u64,
    rationale_history: Vec<String>,
}

struct PipelineState {
    current_phase: Phase,
    tick: u64,
    rationale_history: VecDeque<String>,
    system_prompt_sent: bool,
}

pub struct AgentPipeline {
    bridge: WsBridge,
    config: AgentConfig,
    state: tokio::sync::Mutex<PipelineState>,
    tab_id: tokio::sync::Mutex<Option<u32>>,
}

impl AgentPipeline {
    pub fn new(bridge: WsBridge) -> Self {
        let config = AgentConfig::load().expect("failed to load invariant agent config");
        Self {
            bridge,
            config,
            state: tokio::sync::Mutex::new(PipelineState {
                current_phase: Phase::Observe,
                tick: 0,
                rationale_history: VecDeque::new(),
                system_prompt_sent: false,
            }),
            tab_id: tokio::sync::Mutex::new(None),
        }
    }

    fn log_dir(kind: &str) -> PathBuf {
        Path::new(LOG_ROOT).join(kind)
    }

    fn log_path(kind: &str, tick: u64, ext: &str) -> PathBuf {
        Self::log_dir(kind).join(format!("{:03}.{}", tick, ext))
    }

    fn ensure_log_dirs() {
        for kind in [
            "system_prompt",
            "input_prompt",
            "llm_response",
            "deltas_applied",
            "act_output",
            "verify_output",
            "exit_check_output",
            "state_snapshot",
        ] {
            let _ = std::fs::create_dir_all(Self::log_dir(kind));
        }
    }

    fn read_prev_text(kind: &str, tick: u64, ext: &str) -> String {
        if tick == 0 {
            return String::new();
        }
        let path = Self::log_path(kind, tick, ext);
        std::fs::read_to_string(path).unwrap_or_default()
    }

    fn last_output_and_error(tick: u64) -> (String, String) {
        if tick == 0 {
            return (String::new(), String::new());
        }
        let act = Self::read_prev_text("act_output", tick, "txt");
        let verify = Self::read_prev_text("verify_output", tick, "txt");
        let mut last_output = act;
        if !verify.trim().is_empty() {
            if !last_output.trim().is_empty() {
                last_output.push('\n');
            }
            last_output.push_str(&verify);
        }

        let mut last_error = String::new();
        let deltas_path = Self::log_path("deltas_applied", tick, "json");
        if let Ok(raw) = std::fs::read_to_string(deltas_path) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
                    last_error = err.to_string();
                }
            }
        }

        (last_output, last_error)
    }

    fn build_prompt(&self, phase: &Phase, cwd: &Path, last_output: &str, last_error: &str) -> String {
        let mode = match phase {
            Phase::Observe | Phase::Verify => "read-only",
            Phase::Plan => "plan-only",
            Phase::Act => "mutate",
        };
        format!(
            "Deterministic Agent Tick\n\
             ------------------------\n\
             Execution mode: {mode}\n\
             Inputs (only these):\n\
             - cwd: {cwd}\n\
             - last_output:\n{last_output}\n\
             - last_error:\n{last_error}\n\
             \n\
             If execution mode is mutate and the goal is not met, emit at least one write delta.\n\
             Respond with one JSON block per the system prompt schema.",
            mode = mode,
            cwd = cwd.display(),
            last_output = indent_block(last_output, 2),
            last_error = indent_block(last_error, 2),
        )
    }

    fn system_prompt(&self) -> String {
        let card = match self.config.primary_card() {
            Ok(c) => c,
            Err(_) => {
                return "System Prompt — Invariant Pipeline\nMissing agent card.".to_string();
            }
        };
        let tools = if card.tool_capabilities.is_empty() {
            "(none)".to_string()
        } else {
            card.tool_capabilities.join(", ")
        };
        format!(
            "{}\n\n# Goal\n{}\n\n# Tool Capabilities\n{}\n\n# Tool Protocol\n{}\n\n# Plan Example\n{}\n\n# Delta Schema\n{}\n",
            card.role_markdown.trim_end(),
            card.goal_markdown.trim_end(),
            tools,
            tool_protocol(),
            self.config.plan_example.trim_end(),
            delta_schema()
        )
    }
}

#[async_trait::async_trait]
impl Pipeline for AgentPipeline {
    fn name(&self) -> &str {
        "invariant"
    }

    async fn run_tick(&self, ctx: &PipelineContext, _ir: &mut SystemState, _layout: &mut FileTopology) -> anyhow::Result<PipelineOutcome> {
        Self::ensure_log_dirs();

        let mut state = self.state.lock().await;
        state.tick = ctx.tick;
        let phase = state.current_phase.clone();
        let prev_tick = ctx.tick.saturating_sub(1);
        let (last_output, last_error) = Self::last_output_and_error(prev_tick);

        let cwd = &ctx.cwd[0];
        if !state.system_prompt_sent {
            let sys_prompt = self.system_prompt();
            std::fs::write(Self::log_path("system_prompt", 0, "md"), &sys_prompt).ok();
            if let Err(e) = plan::send_system_prompt(&self.bridge, &self.config, &self.tab_id, &sys_prompt).await {
                eprintln!("[invariant] system prompt send failed: {e}");
            } else {
                state.system_prompt_sent = true;
            }
        }

        let prompt = self.build_prompt(&phase, cwd, &last_output, &last_error);
        std::fs::write(Self::log_path("input_prompt", ctx.tick, "md"), &prompt).ok();

        let mut llm_payload: serde_json::Value = serde_json::json!({});
        let mut response: Option<AgentResponse> = None;
        let mut validation_error: Option<String> = None;

        match request_plan(&self.bridge, &self.config, &prompt, &self.tab_id).await {
            Ok((payload, parsed)) => {
                llm_payload = payload;
                response = Some(parsed);
            }
            Err(e) => {
                validation_error = Some(format!("LLM_ERROR: {e}"));
                llm_payload = serde_json::json!({ "error": e.to_string() });
            }
        }

        if let Ok(pretty) = serde_json::to_string_pretty(&llm_payload) {
            std::fs::write(Self::log_path("llm_response", ctx.tick, "json"), pretty).ok();
        }

        let mut act_output = String::new();
        let mut verify_output = String::new();
        let mut exit_check_output = String::new();
        let mut delta_results: Vec<DeltaOutcome> = Vec::new();
        let mut delta_error = validation_error.clone();
        let mut exit_ok = false;

        let deltas = response.as_ref().map(|r| r.deltas.clone()).unwrap_or_default();
        let (allowed_deltas, mut ignored_outcomes) = filter_deltas_for_phase(&phase, &deltas);

        match phase {
            Phase::Observe => {
                let (out, mut results, err) = apply_read_only(&allowed_deltas, &ctx.cwd, self.config.max_output_lines);
                results.append(&mut ignored_outcomes);
                act_output = out;
                delta_results = results;
                if err.is_some() && delta_error.is_none() {
                    delta_error = err;
                }
            }
            Phase::Plan => {
                // No deltas allowed.
                delta_results.append(&mut ignored_outcomes);
            }
            Phase::Act => {
                let (out, mut results, err) = apply_mutations(&allowed_deltas, &ctx.cwd, self.config.max_output_lines);
                results.append(&mut ignored_outcomes);
                act_output = out;
                delta_results = results;
                if err.is_some() && delta_error.is_none() {
                    delta_error = err;
                }
            }
            Phase::Verify => {
                let (out, mut results, err) = apply_read_only(&allowed_deltas, &ctx.cwd, self.config.max_output_lines);
                results.append(&mut ignored_outcomes);
                verify_output = out;
                delta_results = results;
                if err.is_some() && delta_error.is_none() {
                    delta_error = err;
                }

                let verify = observe::run_exit_check(&self.config.exit_check_command, cwd)?;
                exit_ok = verify.exit_code == 0;
                exit_check_output = format!("{}\n[exit code {}]", verify.stdout.trim_end(), verify.exit_code);
            }
        }

        let deltas_log = DeltasAppliedLog { phase: phase.clone(), deltas: deltas.clone(), results: delta_results.clone(), error: delta_error.clone() };
        if let Ok(pretty) = serde_json::to_string_pretty(&deltas_log) {
            std::fs::write(Self::log_path("deltas_applied", ctx.tick, "json"), pretty).ok();
        }

        std::fs::write(Self::log_path("act_output", ctx.tick, "txt"), &act_output).ok();
        std::fs::write(Self::log_path("verify_output", ctx.tick, "txt"), &verify_output).ok();
        std::fs::write(Self::log_path("exit_check_output", ctx.tick, "txt"), &exit_check_output).ok();

        if let Some(_resp) = response {
            // Rationale history is intentionally not retained or sent.
        }

        state.current_phase = phase.next();

        let snapshot = StateSnapshot { current_phase: &state.current_phase, tick: state.tick, rationale_history: state.rationale_history.iter().cloned().collect() };
        if let Ok(pretty) = serde_json::to_string_pretty(&snapshot) {
            std::fs::write(Self::log_path("state_snapshot", ctx.tick, "json"), pretty).ok();
        }

        let summary = if exit_ok {
            format!("phase={} exit_ok=true", phase)
        } else if let Some(err) = delta_error {
            format!("phase={} error={}", phase, err)
        } else {
            format!("phase={} exit_ok=false", phase)
        };

        Ok(PipelineOutcome { reward: 0.0, summary, advanced: exit_ok })
    }
}

fn allowed_delta(phase: &Phase, delta: &Delta) -> bool {
    match phase {
        Phase::Observe => matches!(delta, Delta::ReadFile { .. } | Delta::ListDir { .. } | Delta::ReadCommand { .. }),
        Phase::Plan => false,
        Phase::Act => matches!(delta, Delta::WriteFile { .. } | Delta::ReplaceText { .. } | Delta::DeleteFile { .. }),
        Phase::Verify => matches!(delta, Delta::ReadFile { .. } | Delta::ListDir { .. } | Delta::ReadCommand { .. }),
    }
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

fn indent_block(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    if text.trim().is_empty() {
        return format!("{}(empty)", pad);
    }
    text.lines().map(|l| format!("{}{}", pad, l)).collect::<Vec<_>>().join("\n")
}

fn filter_deltas_for_phase(phase: &Phase, deltas: &[Delta]) -> (Vec<Delta>, Vec<DeltaOutcome>) {
    let mut allowed = Vec::new();
    let mut ignored = Vec::new();

    for delta in deltas {
        if allowed_delta(phase, delta) {
            allowed.push(delta.clone());
        } else {
            ignored.push(DeltaOutcome {
                delta: delta.clone(),
                status: "ignored".into(),
                message: format!("ignored delta not allowed in phase {}", phase),
            });
        }
    }

    (allowed, ignored)
}

fn tool_protocol() -> String {
    "apply_patch -> write_file | replace_text | delete_file (Act phase only)\n\
bash (read-only) -> read_command (Observe/Verify phases only)\n\
Notes: free-form shell is not allowed; use read_command with explicit command+args."
        .to_string()
}
