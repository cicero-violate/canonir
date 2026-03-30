/// RepairControlConsumer — state-space controller for the repair loop.
///
/// Purpose
/// =======
/// The repair agent is a state-space control system, not a shell bot.
/// Its job is to map the repair state space from the event stream and use that
/// map to drive constrained repair transitions.
///
/// This consumer:
///   1. Observes every event in the stream continuously
///   2. Extracts semantic signals from each event
///   3. Classifies the current repair phase
///   4. Detects illegal and stagnant regions of the state space
///   5. Emits control signals to block invalid transitions and fake progress
///   6. Records the full signal/phase history for exhaustive mapping
///
/// Semantic Signals (derived from events + invariants)
/// ===================================================
///   missing_target           — real path or entrypoint absent in SemanticStateSummary
///   planned_to_act           — LoopPlanned followed by matching LoopActed
///   noop_spam                — N identical consecutive action_kinds with no progress
///   invariant_violation      — ErrorOccurred from invariant/constraint source
///   invalid_tool_selection   — ToolCall rejected or ToolResult failed repeatedly
///   replan_required          — route rewrite, halt, or objective contradiction
///   compiler_clean           — LoopVerified passed=true, compiler_clean=true
///   stagnant_loop            — stagnant_ticks exceeded threshold with delta_g ≤ 0
///   escape_detected          — action attempt while in Stuck without prior replan
///
/// Repair Phases
/// =============
///   Observing      — nominal: reading the event stream
///   Classifying    — active failure pattern detected, mapping signals
///   Transitioning  — repair action selected and underway
///   Verifying      — repair applied, awaiting verification result
///   Stuck          — multiple failed attempts; region marked illegal
///
/// Transition Table (phase × signal → next phase)
/// ================================================
///   Observing    × missing_target        → Classifying
///   Observing    × invariant_violation   → Classifying
///   Observing    × stagnant_loop         → Classifying
///   Observing    × compiler_clean        → Observing    (stay nominal)
///   Classifying  × planned_to_act        → Transitioning
///   Classifying  × noop_spam             → Stuck
///   Classifying  × invalid_tool          → Classifying  (emit replan signal)
///   Classifying  × replan_required       → Classifying
///   Classifying  × compiler_clean        → Observing    (recovered)
///   Classifying  × invariant_violation   → Classifying
///   Transitioning × invariant_violation  → Classifying  (reject transition)
///   Transitioning × planned_to_act       → Verifying
///   Transitioning × escape_detected      → Stuck
///   Transitioning × compiler_clean       → Observing
///   Transitioning × replan_required      → Classifying
///   Transitioning × noop_spam            → Stuck
///   Verifying    × compiler_clean        → Observing    (success)
///   Verifying    × replan_required       → Classifying  (retry)
///   Verifying    × invariant_violation   → Classifying
///   Stuck        × escape_detected       → Stuck        (emit violation)
///   Stuck        × replan_required       → Classifying  (fresh start)
///   Stuck        × compiler_clean        → Observing    (recovered)
use canon_event::{new_error_occurred, DebugEvent, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RuntimeEvent};
use std::collections::VecDeque;

// ── Semantic signal ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SemanticSignal {
    MissingTarget,
    PlannedToAct,
    NoopSpam,
    InvariantViolation,
    InvalidToolSelection,
    ReplanRequired,
    CompilerClean,
    StagnantLoop,
    EscapeDetected,
    ActStall,
    ControlDesync,
}

impl SemanticSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingTarget => "missing_target",
            Self::PlannedToAct => "planned_to_act",
            Self::NoopSpam => "noop_spam",
            Self::InvariantViolation => "invariant_violation",
            Self::InvalidToolSelection => "invalid_tool_selection",
            Self::ReplanRequired => "replan_required",
            Self::CompilerClean => "compiler_clean",
            Self::StagnantLoop => "stagnant_loop",
            Self::EscapeDetected => "escape_detected",
            Self::ActStall => "act_stall",
            Self::ControlDesync => "control_desync",
        }
    }
}

// ── Repair phase ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RepairPhase {
    Observing,
    Classifying,
    Transitioning,
    Verifying,
    Stuck,
}

impl RepairPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observing => "observing",
            Self::Classifying => "classifying",
            Self::Transitioning => "transitioning",
            Self::Verifying => "verifying",
            Self::Stuck => "stuck",
        }
    }
}

// ── Illegal region ────────────────────────────────────────────────────────────

/// A (phase, signal) region proven unproductive by repeated failed entries.
#[derive(Clone, Debug, PartialEq, Eq)]
struct IllegalRegion {
    phase: RepairPhase,
    signal: SemanticSignal,
    tick_first: u64,
    entry_count: u32,
}

// ── Transition record (full history) ─────────────────────────────────────────

#[derive(Clone, Debug)]
#[allow(dead_code)]
struct TransitionRecord {
    tick: u64,
    from_phase: RepairPhase,
    signal: SemanticSignal,
    to_phase: RepairPhase,
}

// ── Tuning constants ──────────────────────────────────────────────────────────

/// Consecutive identical acted kinds before declaring noop_spam.
const NOOP_SPAM_WINDOW: usize = 4;
/// stagnant_ticks from LoopRewarded before declaring stagnant_loop.
const STAGNANT_TICKS_THRESHOLD: u32 = 4;
/// Times a (phase, signal) pair is entered before marking it illegal.
const ILLEGAL_REGION_THRESHOLD: u32 = 3;
/// Consecutive failed tool selections before emitting invalid_tool signal.
const TOOL_REJECT_THRESHOLD: u32 = 2;

// ── Main consumer ─────────────────────────────────────────────────────────────

pub struct RepairControlConsumer {
    emitter: Option<EventEmitterHandle>,
    phase: RepairPhase,
    tick: u64,

    /// Sliding window of recent acted action_kinds for noop-spam detection.
    recent_acted_kinds: VecDeque<String>,
    /// Action kind from the last LoopPlanned, waiting for a matching LoopActed.
    pending_plan_action_kind: Option<String>,

    /// Consecutive rejected/failed tool calls.
    consecutive_tool_rejections: u32,

    /// Regions of the state space that have been entered repeatedly without progress.
    illegal_regions: Vec<IllegalRegion>,
    /// Full transition history, bounded to avoid unbounded growth.
    transition_history: VecDeque<TransitionRecord>,
}

impl RepairControlConsumer {
    pub fn new() -> Self {
        Self {
            emitter: None,
            phase: RepairPhase::Observing,
            tick: 0,
            recent_acted_kinds: VecDeque::with_capacity(NOOP_SPAM_WINDOW + 1),
            pending_plan_action_kind: None,
            consecutive_tool_rejections: 0,
            illegal_regions: Vec::new(),
            transition_history: VecDeque::with_capacity(256),
        }
    }

    // ── signal extraction ─────────────────────────────────────────────────────

    fn extract_signal(&mut self, event: &RuntimeEvent) -> Option<SemanticSignal> {
        match event {
            RuntimeEvent::LoopObserved(e) => {
                self.tick = e.tick;

                // missing_target: real path absent, entrypoint missing, or module gaps.
                let s = &e.semantic_summary;
                if !s.path_exists || s.entrypoint_kind.is_none() || !s.module_gaps.is_empty() {
                    return Some(SemanticSignal::MissingTarget);
                }
                None
            }

            RuntimeEvent::LoopPlanned(e) => {
                self.tick = e.tick;
                self.pending_plan_action_kind = Some(e.action_kind.clone());
                None
            }

            RuntimeEvent::LoopActed(e) => {
                self.tick = e.tick;

                // Noop-spam: sliding window of consecutive identical kinds.
                self.recent_acted_kinds.push_back(e.action_kind.clone());
                if self.recent_acted_kinds.len() > NOOP_SPAM_WINDOW {
                    self.recent_acted_kinds.pop_front();
                }
                let all_same = self.recent_acted_kinds.len() >= NOOP_SPAM_WINDOW && self.recent_acted_kinds.iter().all(|k| k == &e.action_kind);
                if all_same {
                    self.pending_plan_action_kind = None;
                    return Some(SemanticSignal::NoopSpam);
                }

                // planned_to_act: plan was pending and action kind matches.
                let matched = self.pending_plan_action_kind.as_deref().map(|k| k == e.action_kind.as_str()).unwrap_or(false);
                self.pending_plan_action_kind = None;
                if matched {
                    return Some(SemanticSignal::PlannedToAct);
                }
                None
            }

            RuntimeEvent::LoopVerified(e) => {
                self.tick = e.tick;
                if e.compiler_clean && e.passed {
                    Some(SemanticSignal::CompilerClean)
                } else if !e.passed {
                    Some(SemanticSignal::ReplanRequired)
                } else {
                    None
                }
            }

            RuntimeEvent::LoopRewarded(e) => {
                self.tick = e.tick;
                if e.halt {
                    return Some(SemanticSignal::ReplanRequired);
                }
                if e.stagnant_ticks >= STAGNANT_TICKS_THRESHOLD && e.delta_g <= 0.0 {
                    return Some(SemanticSignal::StagnantLoop);
                }
                None
            }

            RuntimeEvent::RouteSelected(e) => {
                if e.gate_changed || e.gate_should_stop {
                    Some(SemanticSignal::ReplanRequired)
                } else {
                    None
                }
            }

            RuntimeEvent::ToolCall(e) => {
                if !e.accepted {
                    self.consecutive_tool_rejections += 1;
                    if self.consecutive_tool_rejections >= TOOL_REJECT_THRESHOLD {
                        return Some(SemanticSignal::InvalidToolSelection);
                    }
                } else {
                    self.consecutive_tool_rejections = 0;
                }
                None
            }

            RuntimeEvent::ToolResult(e) => {
                if !e.success {
                    self.consecutive_tool_rejections += 1;
                    if self.consecutive_tool_rejections >= TOOL_REJECT_THRESHOLD {
                        return Some(SemanticSignal::InvalidToolSelection);
                    }
                } else {
                    self.consecutive_tool_rejections = 0;
                }
                None
            }

            RuntimeEvent::ErrorOccurred(e) => {
                let src = e.source.as_str();
                let kind = e.kind.as_str();
                let msg = e.message.to_ascii_lowercase();
                if kind == "act_stall" || msg.contains("scheduler is empty") {
                    return Some(SemanticSignal::ActStall);
                }
                if kind == "control_desync" || msg.contains("missing required successor") || msg.contains("expected=loop_acted; got=planning_completed") {
                    return Some(SemanticSignal::ControlDesync);
                }
                if src.contains("invariant") || src.contains("constraint") {
                    return Some(SemanticSignal::InvariantViolation);
                }
                if src.contains("missing_target") || src.contains("no_target") {
                    return Some(SemanticSignal::MissingTarget);
                }
                if src.contains("replan") || src.contains("route_rewrite") {
                    return Some(SemanticSignal::ReplanRequired);
                }
                None
            }

            _ => None,
        }
    }

    // ── phase transition ──────────────────────────────────────────────────────

    /// Apply a signal to the current phase. Returns (note, new_phase_str) if the
    /// phase changed, None if it stayed the same.
    fn advance(&mut self, signal: SemanticSignal) -> Option<(&'static str, &'static str)> {
        use RepairPhase::*;
        use SemanticSignal::*;

        self.record_entry_into_region(self.phase, signal);

        // Illegal-region guard: if this (phase, signal) has been entered too many
        // times without progress, escalate to Stuck regardless of the normal table.
        if self.is_illegal_region(self.phase, signal) && self.phase != Stuck {
            self.record_transition(signal, Stuck);
            self.phase = Stuck;
            return Some(("illegal_region_escalation", Stuck.as_str()));
        }

        let (next, note): (RepairPhase, &'static str) = match (self.phase, signal) {
            // ── Observing ─────────────────────────────────────────────────────
            (Observing, MissingTarget) => (Classifying, "missing_target_enter_classifying"),
            (Observing, InvariantViolation) => (Classifying, "invariant_violation_enter_classifying"),
            (Observing, ActStall) => (Classifying, "act_stall_enter_classifying"),
            (Observing, ControlDesync) => (Classifying, "control_desync_enter_classifying"),
            (Observing, StagnantLoop) => (Classifying, "stagnant_loop_enter_classifying"),
            (Observing, CompilerClean) => (Observing, "compiler_clean_stay_observing"),
            (Observing, _) => return None,

            // ── Classifying ───────────────────────────────────────────────────
            (Classifying, PlannedToAct) => (Transitioning, "plan_accepted_begin_transition"),
            (Classifying, NoopSpam) => (Stuck, "noop_spam_enter_stuck"),
            (Classifying, InvalidToolSelection) => (Classifying, "invalid_tool_replan"),
            (Classifying, ReplanRequired) => (Classifying, "replan_stay_classifying"),
            (Classifying, ActStall) => (Classifying, "act_stall_force_replan"),
            (Classifying, ControlDesync) => (Classifying, "control_desync_force_replan"),
            (Classifying, CompilerClean) => (Observing, "compiler_clean_recovered"),
            (Classifying, InvariantViolation) => (Classifying, "invariant_violation_classifying"),
            (Classifying, _) => return None,

            // ── Transitioning ─────────────────────────────────────────────────
            (Transitioning, InvariantViolation) => (Classifying, "invariant_violation_rejects_transition"),
            (Transitioning, PlannedToAct) => (Verifying, "transition_acted_now_verifying"),
            (Transitioning, EscapeDetected) => (Stuck, "escape_detected_in_transition"),
            (Transitioning, ActStall) => (Classifying, "act_stall_recover_to_classifying"),
            (Transitioning, ControlDesync) => (Classifying, "control_desync_recover_to_classifying"),
            (Transitioning, CompilerClean) => (Observing, "compiler_clean_in_transition"),
            (Transitioning, ReplanRequired) => (Classifying, "replan_during_transition"),
            (Transitioning, NoopSpam) => (Stuck, "noop_spam_in_transition"),
            (Transitioning, _) => return None,

            // ── Verifying ─────────────────────────────────────────────────────
            (Verifying, CompilerClean) => (Observing, "verify_passed_success"),
            (Verifying, ReplanRequired) => (Classifying, "verify_failed_retry"),
            (Verifying, InvariantViolation) => (Classifying, "invariant_violation_in_verifying"),
            (Verifying, ActStall) => (Classifying, "act_stall_from_verifying"),
            (Verifying, ControlDesync) => (Classifying, "control_desync_from_verifying"),
            (Verifying, _) => return None,

            // ── Stuck ─────────────────────────────────────────────────────────
            (Stuck, EscapeDetected) => (Stuck, "escape_blocked_still_stuck"),
            (Stuck, ReplanRequired) => (Classifying, "stuck_fresh_replan"),
            (Stuck, CompilerClean) => (Observing, "stuck_then_clean_recovered"),
            (Stuck, ActStall) => (Stuck, "act_stall_stuck"),
            (Stuck, ControlDesync) => (Stuck, "control_desync_stuck"),
            (Stuck, _) => return None,
        };

        if next != self.phase {
            self.record_transition(signal, next);
            self.phase = next;
            Some((note, next.as_str()))
        } else {
            // Phase unchanged but note is still meaningful (e.g. replan in Classifying).
            Some((note, next.as_str()))
        }
    }

    // ── illegal region tracking ───────────────────────────────────────────────

    fn is_illegal_region(&self, phase: RepairPhase, signal: SemanticSignal) -> bool {
        self.illegal_regions.iter().any(|r| r.phase == phase && r.signal == signal && r.entry_count >= ILLEGAL_REGION_THRESHOLD)
    }

    fn record_entry_into_region(&mut self, phase: RepairPhase, signal: SemanticSignal) {
        if let Some(r) = self.illegal_regions.iter_mut().find(|r| r.phase == phase && r.signal == signal) {
            r.entry_count += 1;
        } else {
            self.illegal_regions.push(IllegalRegion { phase, signal, tick_first: self.tick, entry_count: 1 });
        }
    }

    fn record_transition(&mut self, signal: SemanticSignal, to: RepairPhase) {
        // Note: record_entry_into_region is called separately in advance() before
        // the phase change, so we only append the history entry here.
        self.transition_history.push_back(TransitionRecord { tick: self.tick, from_phase: self.phase, signal, to_phase: to });
        if self.transition_history.len() > 256 {
            self.transition_history.pop_front();
        }
    }

    // ── escape detection ──────────────────────────────────────────────────────

    fn check_escape(&self, event: &RuntimeEvent) -> bool {
        self.phase == RepairPhase::Stuck && matches!(event, RuntimeEvent::LoopPlanned(_) | RuntimeEvent::LoopActed(_))
    }

    // ── emit helpers ──────────────────────────────────────────────────────────

    fn emit_transition_debug(&self, trigger_id: &EventId, signal: SemanticSignal, note: &str, to_phase: &str) -> EventOutcome {
        let Some(emitter) = self.emitter.as_ref() else {
            return EventOutcome::NoOp("repair_control_no_emitter");
        };
        emitter.emit_child(
            RuntimeEvent::Debug(DebugEvent {
                source: "repair_control_consumer".to_string(),
                kind: "repair_transition".to_string(),
                payload: serde_json::json!({
                    "signal": signal.as_str(),
                    "to_phase": to_phase,
                    "note": note,
                    "tick": self.tick,
                    "illegal_regions": self.illegal_regions.len(),
                }),
            }),
            vec![trigger_id.clone()],
            file!(),
            line!(),
        );
        EventOutcome::NoOp("repair_control_debug_emitted")
    }

    fn emit_violation(&self, _trigger_id: &EventId, signal: SemanticSignal, note: &'static str) -> EventOutcome {
        EventOutcome::error(
            RuntimeEvent::ErrorOccurred(new_error_occurred(
                "repair_control_consumer",
                "repair_control_consumer",
                note,
                "warning",
                serde_json::json!({
                    "signal": signal.as_str(),
                    "phase": self.phase.as_str(),
                    "tick": self.tick,
                    "illegal_regions": self.illegal_regions.len(),
                    "transition_count": self.transition_history.len(),
                }),
                None,
            )),
            file!(),
            line!(),
        )
    }
}

impl Default for RepairControlConsumer {
    fn default() -> Self {
        Self::new()
    }
}

impl EventConsumer for RepairControlConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool {
        true
    }

    fn consumer_name(&self) -> &'static str {
        "repair_control_consumer"
    }

    fn set_emitter(&mut self, emitter: EventEmitterHandle) {
        self.emitter = Some(emitter);
    }

    fn on_event(&mut self, event: &RuntimeEvent, trigger_id: EventId) -> EventOutcome {
        // Escape detection: action or plan attempt while Stuck = escape.
        if self.check_escape(event) {
            let _ = self.advance(SemanticSignal::EscapeDetected);
            return self.emit_violation(&trigger_id, SemanticSignal::EscapeDetected, "repair_escape_detected_while_stuck");
        }

        let Some(signal) = self.extract_signal(event) else {
            return EventOutcome::NoOp("repair_control_no_signal");
        };

        let Some((note, to_phase)) = self.advance(signal) else {
            return EventOutcome::NoOp("repair_control_no_transition");
        };

        match signal {
            SemanticSignal::NoopSpam if self.phase == RepairPhase::Stuck => self.emit_violation(&trigger_id, signal, "repair_noop_spam_stuck"),
            SemanticSignal::InvariantViolation if self.phase == RepairPhase::Stuck => self.emit_violation(&trigger_id, signal, "repair_invariant_violation_stuck"),
            _ => self.emit_transition_debug(&trigger_id, signal, note, to_phase),
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use canon_semantic_state::SemanticStateSummary;

    fn tid() -> EventId {
        EventId::new("test-trigger")
    }

    fn observed_nominal(tick: u64) -> RuntimeEvent {
        RuntimeEvent::LoopObserved(canon_event::LoopObserved {
            tick,
            error_count: 0,
            warning_count: 0,
            compiler_errors: vec![],
            goal_text: None,
            semantic_summary: SemanticStateSummary { path_exists: true, entrypoint_kind: Some("bin".to_string()), module_gaps: vec![], ..SemanticStateSummary::default() },
            observe_diagnostics: vec![],
        })
    }

    fn observed_missing(tick: u64) -> RuntimeEvent {
        RuntimeEvent::LoopObserved(canon_event::LoopObserved {
            tick,
            error_count: 1,
            warning_count: 0,
            compiler_errors: vec![],
            goal_text: None,
            semantic_summary: SemanticStateSummary { path_exists: false, entrypoint_kind: None, module_gaps: vec![], ..SemanticStateSummary::default() },
            observe_diagnostics: vec![],
        })
    }

    fn planned(tick: u64, action_kind: &str) -> RuntimeEvent {
        RuntimeEvent::LoopPlanned(canon_event::LoopPlanned {
            tick,
            action_kind: action_kind.to_string(),
            action_payload: serde_json::Value::Null,
            reason: String::new(),
            llm_request_id: None,
            signals: None,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
            depends_on: vec![],
        })
    }

    fn acted(tick: u64, action_kind: &str) -> RuntimeEvent {
        RuntimeEvent::LoopActed(canon_event::LoopActed {
            tick,
            action_kind: action_kind.to_string(),
            capability_request_id: String::new(),
            tool_call_id: None,
            tool_result_id: None,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
            duration_ms: 0,
            success: true,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
            plan_id: None,
            plan_step_id: None,
            action_id: None,
        })
    }

    fn verified_clean(tick: u64) -> RuntimeEvent {
        RuntimeEvent::LoopVerified(canon_event::LoopVerified {
            tick,
            passed: true,
            compiler_clean: true,
            tlog_clean: true,
            error_count: 0,
            diagnostics: vec![],
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
        })
    }

    fn verified_failed(tick: u64) -> RuntimeEvent {
        RuntimeEvent::LoopVerified(canon_event::LoopVerified {
            tick,
            passed: false,
            compiler_clean: false,
            tlog_clean: false,
            error_count: 2,
            diagnostics: vec![],
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
        })
    }

    fn rewarded_stagnant(tick: u64) -> RuntimeEvent {
        RuntimeEvent::LoopRewarded(canon_event::LoopRewarded {
            tick,
            errors_before: 1,
            errors_after: 1,
            stagnant_ticks: STAGNANT_TICKS_THRESHOLD,
            halt: false,
            goodness: 0.0,
            reward: 0.0,
            delta_g: -0.1,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
        })
    }

    fn rewarded_halt(tick: u64) -> RuntimeEvent {
        RuntimeEvent::LoopRewarded(canon_event::LoopRewarded {
            tick,
            errors_before: 1,
            errors_after: 1,
            stagnant_ticks: 0,
            halt: true,
            goodness: 0.0,
            reward: 0.0,
            delta_g: 0.0,
            trace_id: None,
            execution_id: None,
            span_id: None,
            parent_span_id: None,
        })
    }

    fn error_invariant(tick_hint: u64) -> RuntimeEvent {
        RuntimeEvent::ErrorOccurred(new_error_occurred("invariant_check", "invariant_engine", "constraint violated", "error", serde_json::json!({"tick": tick_hint}), None))
    }

    // ── signal extraction ──────────────────────────────────────────────────────

    #[test]
    fn test_missing_target_from_path_absent() {
        let mut c = RepairControlConsumer::new();
        let sig = c.extract_signal(&observed_missing(1));
        assert_eq!(sig, Some(SemanticSignal::MissingTarget));
    }

    #[test]
    fn test_no_signal_from_nominal_observation() {
        let mut c = RepairControlConsumer::new();
        let sig = c.extract_signal(&observed_nominal(1));
        assert_eq!(sig, None);
    }

    #[test]
    fn test_planned_to_act_signal() {
        let mut c = RepairControlConsumer::new();
        c.extract_signal(&planned(1, "apply_patch"));
        let sig = c.extract_signal(&acted(2, "apply_patch"));
        assert_eq!(sig, Some(SemanticSignal::PlannedToAct));
    }

    #[test]
    fn test_noop_spam_signal() {
        let mut c = RepairControlConsumer::new();
        for i in 0..NOOP_SPAM_WINDOW {
            let sig = c.extract_signal(&acted(i as u64, "read_file"));
            if i < NOOP_SPAM_WINDOW - 1 {
                assert_ne!(sig, Some(SemanticSignal::NoopSpam));
            } else {
                assert_eq!(sig, Some(SemanticSignal::NoopSpam));
            }
        }
    }

    #[test]
    fn test_compiler_clean_signal() {
        let mut c = RepairControlConsumer::new();
        let sig = c.extract_signal(&verified_clean(1));
        assert_eq!(sig, Some(SemanticSignal::CompilerClean));
    }

    #[test]
    fn test_replan_from_halt() {
        let mut c = RepairControlConsumer::new();
        let sig = c.extract_signal(&rewarded_halt(1));
        assert_eq!(sig, Some(SemanticSignal::ReplanRequired));
    }

    #[test]
    fn test_stagnant_loop_signal() {
        let mut c = RepairControlConsumer::new();
        let sig = c.extract_signal(&rewarded_stagnant(1));
        assert_eq!(sig, Some(SemanticSignal::StagnantLoop));
    }

    #[test]
    fn test_invariant_violation_from_error_source() {
        let mut c = RepairControlConsumer::new();
        let sig = c.extract_signal(&error_invariant(1));
        assert_eq!(sig, Some(SemanticSignal::InvariantViolation));
    }

    // ── phase transitions ──────────────────────────────────────────────────────

    #[test]
    fn test_observing_to_classifying_on_missing_target() {
        let mut c = RepairControlConsumer::new();
        c.advance(SemanticSignal::MissingTarget);
        assert_eq!(c.phase, RepairPhase::Classifying);
    }

    #[test]
    fn test_classifying_to_transitioning_on_plan() {
        let mut c = RepairControlConsumer::new();
        c.phase = RepairPhase::Classifying;
        c.advance(SemanticSignal::PlannedToAct);
        assert_eq!(c.phase, RepairPhase::Transitioning);
    }

    #[test]
    fn test_transitioning_to_verifying_on_acted() {
        let mut c = RepairControlConsumer::new();
        c.phase = RepairPhase::Transitioning;
        c.advance(SemanticSignal::PlannedToAct);
        assert_eq!(c.phase, RepairPhase::Verifying);
    }

    #[test]
    fn test_verifying_to_observing_on_clean() {
        let mut c = RepairControlConsumer::new();
        c.phase = RepairPhase::Verifying;
        c.advance(SemanticSignal::CompilerClean);
        assert_eq!(c.phase, RepairPhase::Observing);
    }

    #[test]
    fn test_verifying_to_classifying_on_replan() {
        let mut c = RepairControlConsumer::new();
        c.phase = RepairPhase::Verifying;
        c.advance(SemanticSignal::ReplanRequired);
        assert_eq!(c.phase, RepairPhase::Classifying);
    }

    #[test]
    fn test_noop_spam_to_stuck() {
        let mut c = RepairControlConsumer::new();
        c.phase = RepairPhase::Classifying;
        c.advance(SemanticSignal::NoopSpam);
        assert_eq!(c.phase, RepairPhase::Stuck);
    }

    #[test]
    fn test_invariant_violation_rejects_transition() {
        let mut c = RepairControlConsumer::new();
        c.phase = RepairPhase::Transitioning;
        c.advance(SemanticSignal::InvariantViolation);
        assert_eq!(c.phase, RepairPhase::Classifying);
    }

    #[test]
    fn test_escape_detected_while_stuck() {
        let mut c = RepairControlConsumer::new();
        c.phase = RepairPhase::Stuck;
        assert!(c.check_escape(&planned(10, "apply_patch")));
        assert!(c.check_escape(&acted(11, "apply_patch")));
        assert!(!c.check_escape(&verified_clean(12)));
    }

    #[test]
    fn test_stuck_absorbs_most_signals() {
        let stuck_signals = [
            SemanticSignal::MissingTarget,
            SemanticSignal::PlannedToAct,
            SemanticSignal::NoopSpam,
            SemanticSignal::InvariantViolation,
            SemanticSignal::InvalidToolSelection,
            SemanticSignal::StagnantLoop,
        ];
        for signal in stuck_signals {
            let mut c = RepairControlConsumer::new();
            c.phase = RepairPhase::Stuck;
            c.advance(signal);
            assert_eq!(c.phase, RepairPhase::Stuck, "signal {signal:?} must not escape Stuck");
        }
    }

    #[test]
    fn test_stuck_recovers_on_compiler_clean() {
        let mut c = RepairControlConsumer::new();
        c.phase = RepairPhase::Stuck;
        c.advance(SemanticSignal::CompilerClean);
        assert_eq!(c.phase, RepairPhase::Observing);
    }

    #[test]
    fn test_illegal_region_escalation() {
        let mut c = RepairControlConsumer::new();
        // Drive (Classifying × InvariantViolation) past the threshold.
        for _ in 0..ILLEGAL_REGION_THRESHOLD {
            c.phase = RepairPhase::Classifying;
            c.advance(SemanticSignal::InvariantViolation);
        }
        // Next entry must escalate to Stuck.
        c.phase = RepairPhase::Classifying;
        c.advance(SemanticSignal::InvariantViolation);
        assert_eq!(c.phase, RepairPhase::Stuck);
    }

    #[test]
    fn test_transition_history_bounded() {
        let mut c = RepairControlConsumer::new();
        for _ in 0..300 {
            c.phase = RepairPhase::Observing;
            c.advance(SemanticSignal::MissingTarget);
            c.phase = RepairPhase::Classifying;
            c.advance(SemanticSignal::CompilerClean);
        }
        assert!(c.transition_history.len() <= 256, "history must be bounded: got {}", c.transition_history.len());
    }

    #[test]
    fn test_full_repair_cycle() {
        let mut c = RepairControlConsumer::new();

        // 1. nominal → no signal
        c.on_event(&observed_nominal(1), tid());
        assert_eq!(c.phase, RepairPhase::Observing);

        // 2. missing target → Classifying
        c.on_event(&observed_missing(2), tid());
        assert_eq!(c.phase, RepairPhase::Classifying);

        // 3. plan + act → Transitioning
        c.on_event(&planned(3, "apply_patch"), tid());
        c.on_event(&acted(4, "apply_patch"), tid());
        assert_eq!(c.phase, RepairPhase::Transitioning);

        // 4. second plan+act in Transitioning → Verifying
        c.on_event(&planned(5, "apply_patch"), tid());
        c.on_event(&acted(6, "apply_patch"), tid());
        assert_eq!(c.phase, RepairPhase::Verifying);

        // 5. verification passes → Observing
        c.on_event(&verified_clean(7), tid());
        assert_eq!(c.phase, RepairPhase::Observing);
    }

    #[test]
    fn test_verify_failed_returns_to_classifying() {
        let mut c = RepairControlConsumer::new();
        c.phase = RepairPhase::Verifying;
        c.on_event(&verified_failed(1), tid());
        assert_eq!(c.phase, RepairPhase::Classifying);
    }

    #[test]
    fn test_stagnant_loop_from_observing_enters_classifying() {
        let mut c = RepairControlConsumer::new();
        c.on_event(&rewarded_stagnant(5), tid());
        assert_eq!(c.phase, RepairPhase::Classifying);
    }
}
