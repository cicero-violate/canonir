use canon_decision::{RouteKind, RouteSelection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// LLM self-assessment signals emitted with every planner/router response.
/// All values are 0.0–1.0 unless otherwise noted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmSignals {
    pub goal_alignment_score: f32,
    pub confidence: f32,
    pub task_completion_likelihood: f32,
    pub error_likelihood: f32,
    pub plan_validity: f32,
    pub state_consistency: f32,
    pub action_effectiveness: f32,
    pub progress_score: f32,
    pub blocking_severity: f32,
    pub ambiguity_level: f32,
    pub context_completeness: f32,
    pub plan_optimality: f32,
    pub redundancy_level: f32,
    pub recovery_difficulty: f32,
    pub tool_reliability: f32,
    pub execution_risk: f32,
    pub verification_coverage: f32,
    pub change_impact: f32,
    pub stability_score: f32,
    pub iteration_efficiency: f32,
    pub novelty_score: f32,
    pub dependency_health: f32,
    pub resource_efficiency: f32,
    pub termination_readiness: f32,
}

impl LlmSignals {
    /// Parse from the raw JSON Value emitted by the LLM.
    pub fn from_value(v: &Value) -> Self {
        let f = |key: &str| v.get(key).and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(0.5);
        Self {
            goal_alignment_score: f("goal_alignment_score"),
            confidence: f("confidence"),
            task_completion_likelihood: f("task_completion_likelihood"),
            error_likelihood: f("error_likelihood"),
            plan_validity: f("plan_validity"),
            state_consistency: f("state_consistency"),
            action_effectiveness: f("action_effectiveness"),
            progress_score: f("progress_score"),
            blocking_severity: f("blocking_severity"),
            ambiguity_level: f("ambiguity_level"),
            context_completeness: f("context_completeness"),
            plan_optimality: f("plan_optimality"),
            redundancy_level: f("redundancy_level"),
            recovery_difficulty: f("recovery_difficulty"),
            tool_reliability: f("tool_reliability"),
            execution_risk: f("execution_risk"),
            verification_coverage: f("verification_coverage"),
            change_impact: f("change_impact"),
            stability_score: f("stability_score"),
            iteration_efficiency: f("iteration_efficiency"),
            novelty_score: f("novelty_score"),
            dependency_health: f("dependency_health"),
            resource_efficiency: f("resource_efficiency"),
            termination_readiness: f("termination_readiness"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardConfig {
    pub max_cycles: u32,
    pub max_repeat_lane: u32,
    pub minimum_confidence: Option<f32>,
    pub fallback_lane: RouteKind,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self { max_cycles: 64, max_repeat_lane: 3, minimum_confidence: Some(0.20), fallback_lane: RouteKind::Observe }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeSignals {
    pub context_ready: bool,
    pub has_queued_plan: bool,
    pub workspace_dirty: bool,
    pub performed_recently: bool,
    pub repair_stalled: bool,
    pub finish_ready: bool,
    #[serde(default)]
    pub last_action_kind: String,
    #[serde(default)]
    pub llm_signals: Option<LlmSignals>,
    #[serde(default)]
    pub goodness: Option<f32>,
    #[serde(default)]
    pub delta_g: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateState {
    pub cycle_count: u32,
    pub previous_lane: Option<RouteKind>,
    pub repeat_count: u32,
}

impl Default for GateState {
    fn default() -> Self {
        Self { cycle_count: 0, previous_lane: None, repeat_count: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub lane: RouteKind,
    pub changed: bool,
    pub note: String,
    pub should_stop: bool,
}

#[derive(Debug, Clone)]
pub struct Gatekeeper {
    cfg: GuardConfig,
    state: GateState,
}

impl Gatekeeper {
    pub fn new(cfg: GuardConfig) -> Self {
        Self { cfg, state: GateState::default() }
    }

    pub fn state(&self) -> &GateState {
        &self.state
    }

    pub fn review(&mut self, pick: &RouteSelection, signals: &RuntimeSignals) -> GateResult {
        self.state.cycle_count = self.state.cycle_count.saturating_add(1);

        if self.state.cycle_count > self.cfg.max_cycles {
            if signals.has_queued_plan {
                // Mid-batch guard: let queued work drain before reconsidering cycle cap.
                self.state.cycle_count = 0;
            } else {
                return GateResult { lane: RouteKind::Conclude, changed: true, note: "cycle cap reached; forcing conclude".to_string(), should_stop: true };
            }
        }

        let mut lane = pick.route;
        let mut changed = false;
        let mut notes: Vec<&str> = Vec::new();

        // ── Signal-driven control system (from decision range table) ────────
        // Rules are evaluated highest-priority first. Each rule that fires
        // overwrites `lane` and appends a note. Later rules can further refine.
        // Action mapping: scan/repair/block→observe  shape/replan/explore→plan
        //                 validate/checkpoint→verify  execute→act
        if let Some(s) = &signals.llm_signals {

            // ── CRITICAL: universal blockers (fire regardless of current lane) ──

            // goal_alignment_score critical < 0.3 → replan+block → plan
            if s.goal_alignment_score < 0.3 {
                lane = RouteKind::Plan;
                changed = true;
                notes.push("sig:goal_align<0.3[critical] → plan");
            }
            // state_consistency critical (< 0.4) → scan+repair → observe
            if s.state_consistency < 0.4 {
                lane = RouteKind::Observe;
                changed = true;
                notes.push("sig:state_consistency<0.4[critical] → observe");
            }
            // plan_validity critical (< 0.5) → shape+replan → plan
            if s.plan_validity < 0.5 && !signals.has_queued_plan {
                lane = RouteKind::Plan;
                changed = true;
                notes.push("sig:plan_validity<0.5[critical] → plan");
            }
            // blocking_severity critical > 0.8 → block+scan+repair → observe
            if s.blocking_severity > 0.8 {
                lane = RouteKind::Observe;
                changed = true;
                notes.push("sig:blocking_severity>0.8[critical] → observe");
            }
            // context_completeness critical < 0.3 → scan+block → observe
            if s.context_completeness < 0.3 {
                lane = RouteKind::Observe;
                changed = true;
                notes.push("sig:context_completeness<0.3[critical] → observe");
            }
            // execution_risk critical > 0.8 → block+shape → plan (redesign)
            if s.execution_risk > 0.8 {
                lane = RouteKind::Plan;
                changed = true;
                notes.push("sig:execution_risk>0.8[critical] → plan");
            }
            // recovery_difficulty critical > 0.8 → checkpoint+block → verify
            if s.recovery_difficulty > 0.8 {
                lane = RouteKind::Verify;
                changed = true;
                notes.push("sig:recovery_difficulty>0.8[critical] → verify");
            }

            // ── WARNING: act-gate rules (applied when about to execute) ────────

            // goal_alignment_score warning 0.3–0.5 → replan+shape → plan
            if s.goal_alignment_score < 0.5 && s.goal_alignment_score >= 0.3 && !signals.has_queued_plan {
                lane = RouteKind::Plan;
                changed = true;
                notes.push("sig:goal_align 0.3–0.5[warning] → plan");
            }
            // confidence critical < 0.4 → block+scan → observe
            if s.confidence < 0.4 && lane == RouteKind::Act {
                lane = RouteKind::Observe;
                changed = true;
                notes.push("sig:confidence<0.4[critical] → observe");
            }
            // confidence warning 0.4–0.6 → validate+scan → verify
            if s.confidence >= 0.4 && s.confidence < 0.6 && lane == RouteKind::Act {
                lane = RouteKind::Verify;
                changed = true;
                notes.push("sig:confidence 0.4–0.6[warning] → verify");
            }
            // execution_risk warning 0.6–0.8 → validate+checkpoint → verify
            if s.execution_risk >= 0.6 && s.execution_risk <= 0.8 && lane == RouteKind::Act {
                lane = RouteKind::Verify;
                changed = true;
                notes.push("sig:execution_risk 0.6–0.8[warning] → verify");
            }
            // error_likelihood critical > 0.7 → block+validate → verify
            if s.error_likelihood > 0.7 && lane == RouteKind::Act {
                lane = RouteKind::Verify;
                changed = true;
                notes.push("sig:error_likelihood>0.7[critical] → verify");
            }
            // error_likelihood warning 0.5–0.7 → validate+scan → verify
            if s.error_likelihood >= 0.5 && s.error_likelihood <= 0.7 && lane == RouteKind::Act {
                lane = RouteKind::Verify;
                changed = true;
                notes.push("sig:error_likelihood 0.5–0.7[warning] → verify");
            }
            // blocking_severity warning 0.6–0.8 → scan+replan → observe
            if s.blocking_severity >= 0.6 && s.blocking_severity <= 0.8 && lane == RouteKind::Act {
                lane = RouteKind::Observe;
                changed = true;
                notes.push("sig:blocking_severity 0.6–0.8[warning] → observe");
            }
            // context_completeness warning 0.3–0.5 → scan+validate → observe
            if s.context_completeness >= 0.3 && s.context_completeness < 0.5 && lane == RouteKind::Act {
                lane = RouteKind::Observe;
                changed = true;
                notes.push("sig:context_completeness 0.3–0.5[warning] → observe");
            }
            // novelty_score critical < 0.1 → explore+replan → plan
            if s.novelty_score < 0.1 && !signals.has_queued_plan {
                lane = RouteKind::Plan;
                changed = true;
                notes.push("sig:novelty<0.1[critical] → plan");
            }
            // novelty_score warning 0.1–0.3 → explore+execute → plan nudge
            if s.novelty_score >= 0.1 && s.novelty_score < 0.3 && lane == RouteKind::Act {
                lane = RouteKind::Plan;
                changed = true;
                notes.push("sig:novelty 0.1–0.3[warning] → plan");
            }
            // verification_coverage critical < 0.3 → validate+block → verify
            if s.verification_coverage < 0.3 && lane == RouteKind::Act {
                lane = RouteKind::Verify;
                changed = true;
                notes.push("sig:verify_coverage<0.3[critical] → verify");
            }
            // change_impact critical > 0.8 → checkpoint+validate → verify
            if s.change_impact > 0.8 && lane == RouteKind::Act {
                lane = RouteKind::Verify;
                changed = true;
                notes.push("sig:change_impact>0.8[critical] → verify");
            }
            // recovery_difficulty warning 0.6–0.8 → checkpoint+validate → verify
            if s.recovery_difficulty >= 0.6 && s.recovery_difficulty <= 0.8 && lane == RouteKind::Act {
                lane = RouteKind::Verify;
                changed = true;
                notes.push("sig:recovery_difficulty 0.6–0.8[warning] → verify");
            }

            // ── STAGNATION: progress_score via stagnant_ticks ──────────────────
            // (stagnant_ticks is tracked in reward stage, not in LlmSignals directly)
            // progress_score stagnant ≥ 3 → replan+explore → plan
            if s.progress_score < 0.2 && s.novelty_score < 0.3 {
                lane = RouteKind::Plan;
                changed = true;
                notes.push("sig:stagnation(progress+novelty)[critical] → plan");
            }

            // ── TERMINATION readiness ──────────────────────────────────────────
            // > 0.95 + low coverage → validate+block → verify (premature convergence guard)
            if s.termination_readiness > 0.95 && s.verification_coverage < 0.5 {
                lane = RouteKind::Verify;
                changed = true;
                notes.push("sig:term_ready>0.95+coverage_low → verify");
            }
            // > 0.9 + high coverage + finish_ready → conclude
            if s.termination_readiness > 0.9 && s.verification_coverage >= 0.7 && signals.finish_ready {
                lane = RouteKind::Conclude;
                changed = true;
                notes.push("sig:term_ready>0.9+coverage_ok → conclude");
            }
        }
        // ΔG gate: sustained negative goodness delta forces replanning.
        if let Some(delta_g) = signals.delta_g {
            if delta_g < 0.0 && !signals.has_queued_plan {
                lane = RouteKind::Plan;
                changed = true;
                notes.push("delta_g<0 → plan");
            }
        }
        // ── FINISH: hard conclude gate ─────────────────────────────────────────
        // Once finish_ready=true and no queued plan remains, force conclude.
        // This eliminates the extra LLM round-trip after verify sets finish_ready.
        if signals.finish_ready && !signals.has_queued_plan {
            lane = RouteKind::Conclude;
            changed = true;
            notes.push("finish_ready=true → conclude");
        }
        // ─────────────────────────────────────────────────────────────────────

        if let Some(minimum) = self.cfg.minimum_confidence {
            if let Some(confidence) = pick.confidence {
                if confidence < minimum {
                    lane = self.cfg.fallback_lane;
                    changed = true;
                    notes.push("low confidence");
                }
            }
        }

        if self.state.repeat_count > self.cfg.max_repeat_lane && !signals.has_queued_plan {
            if signals.performed_recently {
                lane = RouteKind::Verify;
                notes.push("repeat limit reached under unverified state; forcing verify");
            } else {
                lane = self.cfg.fallback_lane;
                notes.push("repeat limit reached");
            }
            changed = true;
        }

        if signals.performed_recently && !signals.has_queued_plan && lane != RouteKind::Verify {
            lane = RouteKind::Verify;
            changed = true;
            notes.push("acted_unverified=true requires verify");
        }

        if signals.repair_stalled && !signals.has_queued_plan && !signals.performed_recently && lane != RouteKind::Plan {
            lane = RouteKind::Plan;
            changed = true;
            notes.push("repair loop stalled requires plan for replan");
        }

        if signals.has_queued_plan && lane != RouteKind::Act {
            lane = RouteKind::Act;
            changed = true;
            notes.push("queued plan requires act");
        }

        if lane == RouteKind::Act && !signals.has_queued_plan {
            lane = RouteKind::Plan;
            changed = true;
            notes.push("act blocked: no queued plan; select plan to produce one");
        } else if lane == RouteKind::Act && !(signals.context_ready || signals.has_queued_plan) {
            lane = RouteKind::Observe;
            changed = true;
            notes.push("act requires context_ready or queued plan");
        }

        if lane == RouteKind::Verify && !(signals.performed_recently || signals.workspace_dirty || signals.last_action_kind == "done") {
            lane = RouteKind::Plan;
            changed = true;
            notes.push("verify requires performed_recently or workspace_dirty");
        }

        if lane == RouteKind::Conclude && !signals.finish_ready {
            lane = RouteKind::Verify;
            changed = true;
            notes.push("conclude requires finish_ready");
        }

        // Track repeats on the final gated lane (not the raw pick) for repeat_limit logic.
        if let Some(previous) = self.state.previous_lane {
            if previous == lane {
                self.state.repeat_count = self.state.repeat_count.saturating_add(1);
            } else {
                self.state.repeat_count = 1;
            }
        } else {
            self.state.repeat_count = 1;
        }
        self.state.previous_lane = Some(lane);

        let note = if notes.is_empty() { "accepted".to_string() } else { notes.join("; ") };

        GateResult { lane, changed, note, should_stop: lane == RouteKind::Conclude }
    }
}
