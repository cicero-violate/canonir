use crate::ir::{IntentStatePersist, KernelStatePersist, PipelineStage, SystemState};
use crate::layout::FileTopology;
use crate::objectives;
use crate::pipelines_core_4::capability::telemetry::TelemetryFrame;
use crate::pipelines_core_4::capability::CapabilityPipeline;
use crate::pipelines_core_4::{Pipeline, PipelineContext};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;
const LOG_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/capability";
pub struct AgentLoopConfig {
    pub max_ticks: u64,
    pub backoff_ms: u64,
    pub stagnation_window: u64,
    pub retry_threshold: f64,
    pub deadlock_threshold: f64,
    pub state_dir: PathBuf,
    pub resume: bool,
}
impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            max_ticks: 0,
            backoff_ms: 200,
            stagnation_window: 3,
            retry_threshold: 0.4,
            deadlock_threshold: 0.2,
            state_dir: PathBuf::from("/workspace/ai_sandbox/canon/kernel/state"),
            resume: false,
        }
    }
}
pub async fn run_agent_loop(
    pipeline: &CapabilityPipeline,
    base_ctx: &PipelineContext,
    ir: &mut SystemState,
    layout: &mut FileTopology,
    config: AgentLoopConfig,
) -> Result<()> {
    let mut stagnation = 0u64;
    let mut last_reward: Option<f64> = None;
    let mut tick = 0u64;
    let state_dir = config.state_dir.clone();
    let kernel_state_path = state_dir.join("kernel_state.json");
    let agent_state_path = state_dir.join("agent_state.json");
    let mut last_event_id = 0u64;
    let mut invariant_hash = String::new();
    let mut graph_version = 0u64;
    if config.resume {
        if let Some(saved) = KernelStatePersist::load(&kernel_state_path) {
            if saved.tick > 0 {
                tick = saved.tick;
            }
            last_event_id = saved.last_event_id;
            invariant_hash = saved.invariant_hash;
            graph_version = saved.graph_version;
        }
    }
    let mut last_success_tick = tick;
    let mut last_reports_mtime = None;
    loop {
        tick += 1;
        if objectives::maybe_regenerate_reports_if_stale() {
            eprintln!("[agent-loop] reports regenerated");
        }
        if maybe_refresh_objective_goal(&mut last_reports_mtime) {
            eprintln!("[agent-loop] objective updated from reports");
        }
        let ctx = PipelineContext {
            tick,
            ..base_ctx.clone()
        };
        let outcome = match pipeline
            .capability_pipeline_pipeline_run_tick(&ctx, ir, layout)
            .await
        {
            Ok(outcome) => {
                eprintln!("[agent-loop] tick {} done — {}", tick, outcome.summary);
                eprintln!(
                    "[agent-loop] reward={:.4} advanced={}", outcome.reward, outcome
                    .advanced
                );
                eprintln!(
                    "[logs] agent_loop tick={} stage={:?} advanced={} reward={:.4}",
                    tick,
                    outcome.stage,
                    outcome.advanced,
                    outcome.reward
                );
                if outcome.advanced {
                    last_success_tick = tick;
                }
                Some(outcome)
            }
            Err(e) => {
                eprintln!("[agent-loop] tick {} error — {}", tick, e);
                eprintln!("[logs] agent_loop tick={} error={}", tick, e);
                None
            }
        };
        if let Some(metrics) = read_metrics() {
            if let Some(prev) = last_reward {
                if metrics.reward <= prev {
                    stagnation += 1;
                } else {
                    stagnation = 0;
                }
            }
            last_reward = Some(metrics.reward);
            if stagnation >= config.stagnation_window {
                write_recovery_signal("stagnation");
                stagnation = 0;
            }
            if metrics.runtime.queue.retry_rate > config.retry_threshold {
                write_recovery_signal("retry_rate");
            }
            if metrics.runtime.queue.deadlock_rate > config.deadlock_threshold {
                write_recovery_signal("deadlock_rate");
            }
        }
        let phase = outcome
            .as_ref()
            .map(|o| format!("{:?}", o.stage))
            .unwrap_or_else(|| format!("{:?}", PipelineStage::Observe));
        let kernel_state = KernelStatePersist {
            tick,
            phase,
            last_event_id,
            invariant_hash: invariant_hash.clone(),
            graph_version,
        };
        kernel_state.save(&kernel_state_path);
        write_agent_state(&agent_state_path, tick, stagnation, last_success_tick);
        if let Some(outcome) = &outcome {
            if outcome.summary == "capability completed" {
                // Exit only if goal is actually satisfied.
                let snap_path = Path::new("/workspace/ai_sandbox/canon/agent_logs/state_snapshot.json");
                if let Some(snapshot) = crate::state_snapshot::snapshot_store_load(&snap_path) {
                    if crate::graph_runtime::goal_reached(&snapshot.graph, &snapshot.goal) {
                        break;
                    }
                }
            }
        }
        if config.max_ticks > 0 && tick >= config.max_ticks {
            break;
        }
        tokio::time::sleep(Duration::from_millis(config.backoff_ms)).await;
    }
    Ok(())
}
fn read_metrics() -> Option<TelemetryFrame> {
    let path = Path::new(LOG_ROOT).join("metrics.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn maybe_refresh_objective_goal(last_mtime: &mut Option<std::time::SystemTime>) -> bool {
    let latest = match objectives::reports_last_modified() {
        Some(time) => time,
        None => return false,
    };
    let should_refresh = match last_mtime {
        Some(prev) => &latest > prev,
        None => true,
    };
    if !should_refresh {
        return false;
    }
    let selection = objectives::load_goal_from_reports(objectives::ObjectiveWeights::default());
    let Some(selection) = selection else {
        *last_mtime = Some(latest);
        return false;
    };
    let goal_text = objectives::goal_raw_with_artifact("", &selection.artifact);
    let path = Path::new("/workspace/ai_sandbox/canon/kernel/state/intent_state.json");
    let intent = IntentStatePersist {
        goal: goal_text,
        intent_radius: 0,
        execution_budget: 0,
    };
    intent.save(path);
    objectives::maybe_write_baseline(&selection);
    *last_mtime = Some(latest);
    true
}
fn write_recovery_signal(reason: &str) {
    let payload = serde_json::json!(
        { "reason" : reason, "timestamp" : std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(| d | d.as_secs()).unwrap_or(0), }
    );
    let path = Path::new(LOG_ROOT).join("recovery_signal.json");
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, pretty);
    }
}

fn write_agent_state(path: &Path, _tick: u64, stagnation: u64, last_success_tick: u64) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let payload = serde_json::json!({
        "agent_id": "canon-agent",
        "credits": 0,
        "stagnation_counter": stagnation,
        "last_success_tick": last_success_tick
    });
    let tmp = path.with_extension("tmp");
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        if std::fs::write(&tmp, pretty).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}
