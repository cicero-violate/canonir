//! AgentPipeline — generic LLM-driven coding agent loop.
//!
//! The LLM picks its own phase each tick: observe / plan / act / verify.
//! Loop terminates when the exit-check command returns exit code 0,
//! or when max_ticks is reached.

pub mod act;
pub mod config;
pub mod observe;
pub mod plan;
pub mod score;

use super::{Pipeline, PipelineContext, PipelineOutcome};
use crate::ir::SystemState;
use crate::layout::FileTopology;
use crate::ws_server::WsBridge;
use anyhow::Result;
use config::{AgentConfig, Phase};
use plan::{plan_via_llm, plan_via_llm_retry, PlanRequest};
use std::collections::VecDeque;

const RETRY_LIMIT: usize = 3;

/// Appended to prompts when the agent has stagnated in observe/plan too long.
const STAGNATION_PRESSURE: &str =
    "\n\n\u{26a0}\u{fe0f}  STAGNATION DETECTED: You have spent many ticks observing or planning \
     without acting. You MUST choose phase `act` this tick and emit at least one `ApplyPatch` or \
     `Bash` delta. Do NOT choose observe or plan.";

pub struct AgentPipeline {
    pub bridge: WsBridge,
    config: AgentConfig,
    tab_id: tokio::sync::Mutex<Option<u32>>,
    bootstrap_sent: tokio::sync::Mutex<bool>,
    /// Ring buffer of recent rationales for RATIONALE_HISTORY.
    rationale_history: tokio::sync::Mutex<VecDeque<String>>,
    /// Count of consecutive ticks where the LLM chose observe or plan.
    stagnation_counter: tokio::sync::Mutex<usize>,
    /// Count of ticks consumed by plan phase (free ticks, for observability).
    plan_tick_credits: tokio::sync::Mutex<usize>,
}

impl AgentPipeline {
    pub fn new(bridge: WsBridge) -> Self {
        let config = AgentConfig::load().expect("failed to load agent_config.toml");
        Self {
            bridge,
            config,
            tab_id: tokio::sync::Mutex::new(None),
            bootstrap_sent: tokio::sync::Mutex::new(false),
            rationale_history: tokio::sync::Mutex::new(VecDeque::new()),
            stagnation_counter: tokio::sync::Mutex::new(0),
            plan_tick_credits: tokio::sync::Mutex::new(0),
        }
    }

    async fn push_rationale(&self, rationale: &str) {
        let mut history = self.rationale_history.lock().await;
        if history.len() >= self.config.rationale_history_len {
            history.pop_front();
        }
        history.push_back(rationale.to_string());
    }

    async fn rationale_history_str(&self) -> String {
        let history = self.rationale_history.lock().await;
        history
            .iter()
            .enumerate()
            .map(|(i, r)| format!("[T-{}] {}", history.len() - i, r))
            .collect::<Vec<_>>()
            .join("\n---\n")
    }
}

#[async_trait::async_trait]
impl Pipeline for AgentPipeline {
    fn name(&self) -> &str {
        "agent"
    }

    async fn run_tick(
        &self,
        ctx: &PipelineContext,
        _ir: &mut SystemState,
        _layout: &mut FileTopology,
    ) -> Result<PipelineOutcome> {
        let cwd = &ctx.cwd[0];
        let log_dir = cwd
            .join("agent_logs")
            .join(format!("tick_{}", ctx.tick));
        std::fs::create_dir_all(&log_dir).ok();

        // -----------------------------------------------------------------------
        // 1. Observe — run exit check to get current state
        // -----------------------------------------------------------------------
        let observe_result = observe::run_exit_check(&self.config.exit_check_command, cwd)?;
        let already_done = observe_result.exit_code == 0;

        std::fs::write(log_dir.join("exit_check_output.txt"), &observe_result.stdout).ok();

        // Load bash output persisted by the previous tick's act phase.
        let prev_log_dir = cwd
            .join("agent_logs")
            .join(format!("tick_{}", ctx.tick.saturating_sub(1)));
       let prev_bash_output = std::fs::read_to_string(prev_log_dir.join("bash_output.txt"))
           .unwrap_or_default();

        // Surface act_error.txt from the previous tick so the LLM sees why its
        // patch was rejected.  This populates {{LAST_ERROR}} in the act template.
        let prev_act_error = std::fs::read_to_string(prev_log_dir.join("act_error.txt"))
            .unwrap_or_default();

       if already_done {
            return Ok(PipelineOutcome {
                reward: 1.0,
                summary: "exit check passed — done".into(),
                advanced: false,
            });
        }

        // -----------------------------------------------------------------------
        // 2. Plan — LLM decides phase + deltas
        // -----------------------------------------------------------------------
        let rationale_history = self.rationale_history_str().await;
        let is_bootstrap = !*self.bootstrap_sent.lock().await;

        // Snapshot stagnation count before the plan call so the pressure string
        // is stable for the entire retry loop of this tick.
        let stagnation_count = *self.stagnation_counter.lock().await;
        let stagnation_pressure = if stagnation_count >= self.config.stagnation_threshold {
            STAGNATION_PRESSURE
        } else {
            ""
        };

        // Track last phase for template selection on retries.
        let mut last_error: Option<String> = None;
        let mut response = None;
        let mut current_phase: Option<Phase> = None;

        for _attempt in 0..RETRY_LIMIT {
            let plan_result = if let Some(err) = &last_error {
                plan_via_llm_retry(&self.bridge, &self.config, err, &log_dir, &self.tab_id).await
            } else {
              let req = PlanRequest {
                  tick: ctx.tick,
                  cwd,
                  bash_output: &prev_bash_output,
                   last_error: &prev_act_error,
                  rationale_history: &rationale_history,
                   exit_check_output: &observe_result.stdout,
                   is_bootstrap,
                   current_phase: current_phase.as_ref(),
                   stagnation_pressure,
               };
                plan_via_llm(&self.bridge, &self.config, &req, &log_dir, &self.tab_id).await
            };

            match plan_result {
                Ok(r) => {
                    response = Some(r);
                    break;
                }
                Err(e) => {
                    last_error = Some(e.to_string());
                    continue;
                }
            }
        }

        // Mark bootstrap sent after first successful plan.
        if is_bootstrap && response.is_some() {
            *self.bootstrap_sent.lock().await = true;
        }

        let response = match response {
            Some(r) => r,
            None => {
                let err = last_error.unwrap_or_else(|| "plan failed after retries".into());
                return Ok(PipelineOutcome {
                    reward: -1.0,
                    summary: format!("plan failed: {}", err),
                    advanced: false,
                });
            }
        };

        current_phase = Some(response.phase.clone());
        self.push_rationale(&response.rationale).await;

        std::fs::write(
            log_dir.join("phase.txt"),
            response.phase.to_string(),
        ).ok();

       // -----------------------------------------------------------------------
       // 3. Act — execute deltas (skip for Plan phase)
       // -----------------------------------------------------------------------
   let mut act_failed = false;
   let mut bash_output = String::new();

       // Act: execute Bash/ApplyPatch deltas.
       // Observe/Verify: execute BashReadOnly deltas only if any were emitted.
       // Plan: no deltas, skip entirely.
       // Observe/Verify execution failures are non-fatal (soft error, reward stays 0).
       let run_deltas = match response.phase {
           Phase::Act => true,
           Phase::Observe | Phase::Verify => !response.deltas.is_empty(),
           Phase::Plan => false,
       };
       if run_deltas {
           match act::act(&response.deltas, &ctx.cwd) {
               Ok(out) => {
                   bash_output = out;
                   if !bash_output.is_empty() {
                       std::fs::write(log_dir.join("bash_output.txt"), &bash_output).ok();
                   }
               }
               Err(e) => {
                   let is_fatal = response.phase == Phase::Act;
                   if is_fatal {
                       act_failed = true;
                   }
                   std::fs::write(log_dir.join("act_error.txt"), e.to_string()).ok();
               }
           }
       }

        // -----------------------------------------------------------------------
        // 3b. Act-failure inline retry — one free correction per tick
        // -----------------------------------------------------------------------
        // When the act phase fails (e.g. apply_patch "Failed to find expected
        // lines"), surface the error back to the LLM immediately as a retry
        // addendum rather than burning a full new tick.  The retry uses the
        // existing plan_via_llm_retry path which already handles JSON errors;
        // we extend it to cover act execution failures as well.
        if act_failed {
            let act_error_str = std::fs::read_to_string(log_dir.join("act_error.txt"))
                .unwrap_or_else(|_| "apply_patch failed (unknown error)".into());

            match plan_via_llm_retry(&self.bridge, &self.config, &act_error_str, &log_dir, &self.tab_id).await {
                Ok(retry_response) => {
                    // Only accept if the retry chose act phase again.
                    if retry_response.phase == Phase::Act {
                        match act::act(&retry_response.deltas, &ctx.cwd) {
                            Ok(out) => {
                                act_failed = false;
                                bash_output = out;
                                if !bash_output.is_empty() {
                                    std::fs::write(log_dir.join("bash_output.txt"), &bash_output).ok();
                                }
                                std::fs::write(log_dir.join("act_retry_ok.txt"), "1").ok();
                            }
                            Err(e) => {
                                // Second failure — keep act_failed=true, persist new error.
                                std::fs::write(log_dir.join("act_error.txt"), e.to_string()).ok();
                            }
                        }
                    }
                    self.push_rationale(&retry_response.rationale).await;
                }
                Err(e) => {
                    eprintln!("[agent] act-retry plan failed: {e}");
                }
            }
        }

        // -----------------------------------------------------------------------
        // 4. Verify — run exit check again if LLM chose verify phase
        // -----------------------------------------------------------------------
        let mut exit_ok = false;

        if response.phase == Phase::Verify {
            let verify_result = observe::run_exit_check(&self.config.exit_check_command, cwd)?;
            exit_ok = verify_result.exit_code == 0;
            std::fs::write(log_dir.join("verify_output.txt"), &verify_result.stdout).ok();
        }

        // -----------------------------------------------------------------------
        // 5. Update stagnation counter
        // -----------------------------------------------------------------------
        {
            let mut sc = self.stagnation_counter.lock().await;
            if response.phase == Phase::Act || response.phase == Phase::Verify {
                *sc = 0;
            } else {
                *sc += 1;
            }
        }

        // Plan ticks are "free" — record credit for observability.
        if response.phase == Phase::Plan {
            let mut credits = self.plan_tick_credits.lock().await;
            *credits += 1;
        }

        // -----------------------------------------------------------------------
        // 6. Score
        // -----------------------------------------------------------------------
        let reward = score::compute_reward(exit_ok, act_failed);
        let advanced = exit_ok && !act_failed;
        let plan_credits = *self.plan_tick_credits.lock().await;

        let summary = format!(
            "tick={} phase={} exit_ok={} act_failed={} reward={:.1} stagnation={} plan_credits={}",
            ctx.tick, response.phase, exit_ok, act_failed, reward, stagnation_count, plan_credits,
        );
        println!("[agent] {}", summary);

        Ok(PipelineOutcome { reward, summary, advanced })
    }
}
