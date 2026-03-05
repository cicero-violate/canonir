use std::path::Path;
use std::time::Duration;

use anyhow::Result;

use crate::ir::SystemState;
use crate::layout::FileTopology;
use crate::pipelines::capability::telemetry::TelemetrySnapshot;
use crate::pipelines::capability::CapabilityPipeline;
use crate::pipelines::{Pipeline, PipelineContext};

const LOG_ROOT: &str = "/workspace/ai_sandbox/canon/agent_logs/capability";

pub struct AgentLoopConfig {
    pub max_ticks: u64,
    pub backoff_ms: u64,
    pub stagnation_window: u64,
    pub retry_threshold: f64,
    pub deadlock_threshold: f64,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self { max_ticks: 0, backoff_ms: 200, stagnation_window: 3, retry_threshold: 0.4, deadlock_threshold: 0.2 }
    }
}

pub async fn run_agent_loop(pipeline: &CapabilityPipeline, base_ctx: &PipelineContext, ir: &mut SystemState, layout: &mut FileTopology, config: AgentLoopConfig) -> Result<()> {
    let mut stagnation = 0u64;
    let mut tick = 0u64;
    loop {
        tick += 1;
        let ctx = PipelineContext { tick, ..base_ctx.clone() };
        let outcome = match pipeline.run_tick(&ctx, ir, layout).await {
            Ok(outcome) => {
                eprintln!("[agent-loop] tick {} done — {}", tick, outcome.summary);
                eprintln!("[agent-loop] reward={:.4} advanced={}", outcome.reward, outcome.advanced);
                Some(outcome)
            }
            Err(e) => {
                eprintln!("[agent-loop] tick {} error — {}", tick, e);
                None
            }
        };

        if let Some(metrics) = read_metrics() {
            if metrics.runtime.completion_velocity == 0.0 {
                stagnation += 1;
            } else {
                stagnation = 0;
            }
            if stagnation >= config.stagnation_window {
                write_recovery_signal("stagnation");
                stagnation = 0;
            }
            if metrics.runtime.retry_rate > config.retry_threshold {
                write_recovery_signal("retry_rate");
            }
            if metrics.runtime.deadlock_rate > config.deadlock_threshold {
                write_recovery_signal("deadlock_rate");
            }
        }

        if config.max_ticks > 0 && tick >= config.max_ticks {
            break;
        }

        tokio::time::sleep(Duration::from_millis(config.backoff_ms)).await;
    }
    Ok(())
}

fn read_metrics() -> Option<TelemetrySnapshot> {
    let path = Path::new(LOG_ROOT).join("metrics.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_recovery_signal(reason: &str) {
    let payload = serde_json::json!({
        "reason": reason,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    });
    let path = Path::new(LOG_ROOT).join("recovery_signal.json");
    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, pretty);
    }
}
