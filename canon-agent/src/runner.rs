//! Top-level agent runner — drives the full Tier 7 loop.
//!
//! Loop per tick:
//!   1. CapabilityNodeDispatcher::topological_call_order() → ordered node list
//!   2. For each node: dispatcher.dispatch() → AgentCallInput
//!                     call_llm()             → AgentCallOutput
//!                     dispatcher.record_output()
//!   3. run_refactor_pipeline(ir, layout, proposal, stage_outputs) → RefactorResult
//!   4. record_refactor_reward(ledger, node_id, result)
//!   5. Every meta_tick_interval ticks: evolve_capability_graph(graph, ledger) → Φ(G)
//!   6. Every policy_update_interval ticks: ledger.update_policy()
//!
//! No LLM calls inside this file — delegated entirely to llm_provider::call_llm().
//! Async — requires a tokio runtime. WsBridge is passed in from main.
use super::call::AgentCallOutput;
use super::capability::AgentGraph;
use super::dispatcher::AgentScheduler;
use super::llm_provider::{call_llm, LlmProviderError};
use super::meta::evolve_capability_graph;
use super::pipelines::refactor::{record_refactor_reward, run_refactor_pipeline, RefactorError, RefactorKind, RefactorProposal, RefactorTarget};
use super::reward::NodeRewardLedger;
use super::ws_server::WsBridge;
use crate::io::{load_capability_graph, save_capability_graph};
use crate::ir::SystemState;
use crate::layout::FileTopology;
use std::fs;
use std::path::{Path, PathBuf};
use crate::executor::{execute_deltas, ExecutorError};
/// Configuration for one agent run session.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// How many pipeline ticks to run before stopping (0 = run forever).
    pub max_ticks: u64,
    /// Fire meta-tick every N ticks (0 = never).
    pub meta_tick_interval: u64,
    /// Update policy every N ticks (0 = never).
    pub policy_update_interval: u64,
    /// EMA alpha for reward ledger.
    pub ledger_alpha: f64,
    /// Base trust threshold for dispatcher.
    pub base_trust_threshold: f64,
    /// Path to save the capability graph after each meta-tick.
    pub graph_out: PathBuf,
    /// Path to save the ledger after each tick.
    pub ledger_out: PathBuf,
    /// Path to save the mutated IR after each successful pipeline run.
    pub ir_out: PathBuf,
    /// Workspace root — deltas are applied here and cargo check is run here.
    pub workspace: PathBuf,
}
impl RunnerConfig {
    pub fn new(graph_out: PathBuf, ledger_out: PathBuf, ir_out: PathBuf) -> Self {
        Self {
            max_ticks: 0,
            meta_tick_interval: 10,
            policy_update_interval: 5,
            ledger_alpha: 0.1,
            base_trust_threshold: 0.5,
            graph_out,
            ledger_out,
            ir_out,
            workspace: PathBuf::from("."),
        }
    }
}
/// Errors from the agent runner.
#[derive(Debug)]
pub enum RunnerError {
    /// Dispatcher could not produce a call order.
    Dispatch(crate::call::AgentCallError),
    /// LLM provider failed for a node.
    Llm { node_id: String, error: LlmProviderError },
    /// No stage outputs were collected (empty graph).
    NoOutputs,
    /// I/O error persisting graph or ledger.
    Io(String),
}
impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunnerError::Dispatch(e) => write!(f, "dispatch error: {e}"),
            RunnerError::Llm { node_id, error } => {
                write!(f, "llm error on node {node_id}: {error}")
            }
            RunnerError::NoOutputs => {
                write!(f, "no stage outputs collected — graph may be empty")
            }
            RunnerError::Io(e) => write!(f, "i/o error: {e}"),
        }
    }
}
impl std::error::Error for RunnerError {}
/// Statistics for one completed tick.
#[derive(Debug)]
pub struct TickStats {
    pub tick_number: u64,
    pub nodes_called: usize,
    pub llm_errors: usize,
    pub pipeline_reward: Option<f64>,
    pub pipeline_error: Option<String>,
    pub meta_tick_fired: bool,
    pub policy_updated: bool,
}
/// Runs the Tier 7 agent loop.
///
/// `ir` and `layout` are the starting state — they are updated in-place
/// after each successful pipeline run.
/// `graph` is the CapabilityGraph — mutated by meta-ticks.
/// `proposal_seed` is used to construct the RefactorProposal for each tick;
/// the runner increments the id each tick.
pub async fn run_agent(
    ir: &mut SystemState,
    layout: &mut FileTopology,
    graph: &mut AgentGraph,
    proposal_seed: RefactorProposal,
    config: &RunnerConfig,
    bridge: &WsBridge,
) -> Result<Vec<TickStats>, RunnerError> {
    let mut ledger = NodeRewardLedger::new(
        config.ledger_alpha,
        config.base_trust_threshold,
    );
    let mut stats: Vec<TickStats> = Vec::new();
    let mut tick_number: u64 = 0;
    eprintln!("[runner] waiting for extension to connect...");
    bridge.wait_for_connection().await;
    eprintln!("[runner] extension connected — starting tick loop");
    loop {
        tick_number += 1;
        if config.max_ticks > 0 && tick_number > config.max_ticks {
            break;
        }
        eprintln!(
            "[runner] tick {tick_number} — dispatching {} nodes", graph.nodes.len()
        );
        let mut tick_stats = TickStats {
            tick_number,
            nodes_called: 0,
            llm_errors: 0,
            pipeline_reward: None,
            pipeline_error: None,
            meta_tick_fired: false,
            policy_updated: false,
        };
        let mut dispatcher = AgentScheduler::new(graph, ir)
            .with_trust_threshold(config.base_trust_threshold);
        let order = dispatcher.topological_call_order().map_err(RunnerError::Dispatch)?;
        let mut stage_outputs: Vec<AgentCallOutput> = Vec::new();
        for node_id in &order {
            let input = match dispatcher.dispatch(node_id) {
                Ok(inp) => inp,
                Err(e) => {
                    eprintln!("[runner] dispatch skip {node_id}: {e}");
                    continue;
                }
            };
            eprintln!("[runner]   → calling node {node_id} (stage={:?})", input.stage);
            const GPT_URL: &str =
		"https://chatgpt.com/gg/699d3878fbd0819a9d73741b03e8128e";
		// "https://chatgpt.com/?temporary-chat=true";

            match call_llm(bridge, &input, Some(GPT_URL)).await {
                Ok(output) => {
                    eprintln!("[runner]   ✓ node {node_id} responded");
                    tick_stats.nodes_called += 1;
                    stage_outputs.push(output.clone());
                    dispatcher.record_output(output);
                }
                Err(e) => {
                    eprintln!("[runner]   ✗ node {node_id} llm error: {e}");
                    tick_stats.llm_errors += 1;
                }
            }
        }
        if stage_outputs.is_empty() {
            eprintln!("[runner] tick {tick_number} — no outputs, skipping pipeline");
            stats.push(tick_stats);
            continue;
        }
        let mut proposal = proposal_seed.clone();
        proposal.id = format!("{}-tick-{}", proposal_seed.id, tick_number);
        let pipeline_result = run_refactor_pipeline(
            ir,
            layout,
            proposal,
            &stage_outputs,
        );
        let primary_node = order.first().map(|s| s.as_str()).unwrap_or("unknown");
        let _threshold = record_refactor_reward(
            &mut ledger,
            primary_node,
            pipeline_result.as_ref(),
        );
        match &pipeline_result {
            Ok(result) => {
                eprintln!(
                    "[runner] tick {tick_number} — pipeline OK reward={:.4} admission={}",
                    result.reward, result.admission_id
                );
                tick_stats.pipeline_reward = Some(result.reward);
                *ir = result.ir.clone();
                *layout = result.layout.clone();
                match execute_deltas(&result.code_deltas, &config.workspace) {
                    Ok(()) => {
                        eprintln!("[runner] tick {tick_number} — deltas applied, cargo check passed");
                        write_ir_to_disk(ir, &config.ir_out)?;
                    }
                    Err(ExecutorError::StashDropFailed(e)) => {
                        // Non-fatal — changes are on disk, stash cleanup just failed.
                        eprintln!("[runner] tick {tick_number} — stash drop failed (non-fatal): {e}");
                        write_ir_to_disk(ir, &config.ir_out)?;
                    }
                    Err(e) => {
                        // Tree has been restored by executor. Roll back IR promotion too.
                        eprintln!("[runner] tick {tick_number} — executor failed, IR rolled back: {e}");
                        *ir = result.ir.clone(); // already promoted above, re-assign original
                        tick_stats.pipeline_error = Some(e.to_string());
                    }
                }
            }
            Err(super::pipelines::refactor::RefactorError::StageSkipped { stage }) => {
                eprintln!(
                    "[runner] tick {tick_number} — pipeline incomplete: stage {stage} skipped (trust building)"
                );
            }
            Err(e) => {
                eprintln!("[runner] tick {tick_number} — pipeline error: {e}");
                tick_stats.pipeline_error = Some(e.to_string());
            }
        }
        if config.meta_tick_interval > 0 && tick_number % config.meta_tick_interval == 0
        {
            tick_stats.meta_tick_fired = true;
            match evolve_capability_graph(graph, &ledger) {
                Ok(result) => {
                    eprintln!(
                        "[runner] meta-tick fired — applied={} rejected={} H={:.4}→{:.4}",
                        result.applied.len(), result.rejected.len(), result
                        .entropy_before, result.entropy_after,
                    );
                    *graph = result.graph;
                    save_capability_graph(graph, &config.graph_out)
                        .map_err(|e| RunnerError::Io(e.to_string()))?;
                }
                Err(crate::meta::GraphEvolutionError::NothingToDo) => {
                    eprintln!("[runner] meta-tick — nothing to do");
                }
                Err(e) => {
                    eprintln!("[runner] meta-tick error: {e}");
                }
            }
        }
        if config.policy_update_interval > 0
            && tick_number % config.policy_update_interval == 0
        {
            if let Some(current_policy) = ir.policy_parameters.last() {
                match ledger.update_policy(current_policy) {
                    Ok(new_policy) => {
                        eprintln!(
                            "[runner] policy updated — lr={:.4} baseline={:.4}",
                            new_policy.learning_rate, new_policy.reward_baseline
                        );
                        ir.policy_parameters.push(new_policy);
                        tick_stats.policy_updated = true;
                    }
                    Err(e) => {
                        eprintln!("[runner] policy update error: {e:?}");
                    }
                }
            }
        }
        write_ledger_to_disk(&ledger, &config.ledger_out)?;
        stats.push(tick_stats);
    }
    Ok(stats)
}
fn write_ir_to_disk(ir: &SystemState, path: &Path) -> Result<(), RunnerError> {
    let json = serde_json::to_string_pretty(ir)
        .map_err(|e| RunnerError::Io(e.to_string()))?;
    fs::write(path, json).map_err(|e| RunnerError::Io(e.to_string()))
}
fn write_ledger_to_disk(
    ledger: &NodeRewardLedger,
    path: &Path,
) -> Result<(), RunnerError> {
    let json = serde_json::to_string_pretty(ledger)
        .map_err(|e| RunnerError::Io(e.to_string()))?;
    fs::write(path, json).map_err(|e| RunnerError::Io(e.to_string()))
}
