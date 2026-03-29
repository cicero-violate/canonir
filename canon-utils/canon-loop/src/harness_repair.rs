use crate::{compiler_hints::classify_failure_metadata, context::LoopContext};
use canon_invariant::{
    meta_invariant_harness_self_repair_missing_capabilities,
    meta_invariant_harness_self_repair_ready, HarnessCapabilityState, HarnessPrimitiveCapability,
};
use canon_semantic_state::FailureScopeKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HarnessRepairTarget {
    pub crate_name: Option<String>,
    pub failing_test: Option<String>,
}

impl HarnessRepairTarget {
    pub fn new(crate_name: Option<String>, failing_test: Option<String>) -> Self {
        Self { crate_name, failing_test }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HarnessRepairPhase {
    Observe,
    Decide,
    Repair,
    Verify,
    Update,
    Stop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HarnessRepairAction {
    ObserveWorkspace,
    CollectDiagnostics,
    ReplanSingleAction,
    ApplyLocalizedRepair,
    ApplyWorkspaceRepair,
    RunCargoCheck,
    RunCargoTest,
    UpdateState,
    StopReady,
    StopBlocked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HarnessRepairDecision {
    pub phase: HarnessRepairPhase,
    pub action: HarnessRepairAction,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HarnessRepairDirective {
    pub decision: HarnessRepairDecision,
    pub verifier_command: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HarnessRepairState {
    pub capabilities: HarnessCapabilityState,
    pub drift_detected: bool,
    pub actionable_failure: bool,
    pub failure_class_no_actionable: bool,
    pub failure_scope_localized: bool,
    pub failure_scope_workspace: bool,
    pub failure_scope_tooling: bool,
    pub verifier_ready: bool,
    pub cargo_check_passed: bool,
    pub stronger_verification_requested: bool,
    pub last_action_was_mutation: bool,
    pub single_action_batch_required: bool,
    pub needs_replan: bool,
    pub progress_stalled: bool,
}

impl HarnessRepairState {
    pub fn from_loop_context(ctx: &LoopContext) -> Self {
        let semantic = ctx
            .last_observed
            .as_ref()
            .map(|observed| &observed.semantic_summary);
        let failure_class = semantic.and_then(|summary| summary.failure_class.as_deref());
        let failure_scope = semantic.and_then(|summary| summary.failure_scope.as_deref());
        let verify_failed = ctx
            .last_verifier_outcome
            .as_deref()
            .map(|value| value != "passed")
            .unwrap_or(false);
        let (_, fallback_scope) = classify_failure_metadata(
            ctx.last_acted
                .as_ref()
                .map(|acted| acted.stderr.as_str())
                .unwrap_or(""),
        );

        Self {
            capabilities: HarnessCapabilityState {
                read_search: true,
                structured_edit: true,
                apply_patch: true,
                run_verifier: true,
                observe_diagnostics: true,
            },
            drift_detected: false,
            actionable_failure: semantic
                .map(|summary| {
                    summary.validation_blocked_by_preconditions
                        || summary.compiler_repair_required
                        || !summary.planning_preconditions.is_empty()
                        || !summary.repair_intents.is_empty()
                        || !summary.module_gaps.is_empty()
                        || summary.has_actionable_compiler_hints()
                })
                .unwrap_or(false)
                || verify_failed,
            failure_class_no_actionable: failure_class == Some("no_actionable_failure"),
            failure_scope_localized: matches!(failure_scope, Some("localized")),
            failure_scope_workspace: matches!(failure_scope, Some("workspace"))
                || (failure_scope.is_none() && fallback_scope == FailureScopeKind::Workspace),
            failure_scope_tooling: matches!(failure_scope, Some("tooling"))
                || (failure_scope.is_none() && fallback_scope == FailureScopeKind::Tooling),
            verifier_ready: ctx.pending_act.is_none() && ctx.pending_plan.is_none(),
            cargo_check_passed: ctx.last_verifier_outcome.as_deref() == Some("passed"),
            stronger_verification_requested: ctx.error_count == 0 && ctx.warning_count == 0,
            last_action_was_mutation: matches!(
                ctx.last_action_kind.as_str(),
                "apply_patch" | "write_file" | "edit_file" | "run_command"
            ),
            single_action_batch_required: true,
            needs_replan: ctx.consecutive_invalid_plan_batches > 0 || ctx.pending_plan.is_none(),
            progress_stalled: ctx.objective_trend_state.current_no_progress_streak > 0,
        }
    }
}

pub fn evaluate_harness_repair_loop(state: &HarnessRepairState) -> HarnessRepairDecision {
    let missing = meta_invariant_harness_self_repair_missing_capabilities(state.capabilities);
    if !missing.is_empty() {
        return HarnessRepairDecision {
            phase: HarnessRepairPhase::Stop,
            action: HarnessRepairAction::StopBlocked,
            reason: format!("missing harness primitives: {}", format_missing_capabilities(&missing)),
        };
    }

    if state.drift_detected {
        return HarnessRepairDecision {
            phase: HarnessRepairPhase::Observe,
            action: HarnessRepairAction::ObserveWorkspace,
            reason: "state drift detected; refresh workspace facts before planning or repair".into(),
        };
    }

    if state.failure_class_no_actionable && !state.actionable_failure {
        return HarnessRepairDecision {
            phase: HarnessRepairPhase::Observe,
            action: HarnessRepairAction::CollectDiagnostics,
            reason: "no actionable failure is scoped; collect fresh diagnostics instead of repairing".into(),
        };
    }

    if state.needs_replan || state.progress_stalled {
        return HarnessRepairDecision {
            phase: HarnessRepairPhase::Decide,
            action: HarnessRepairAction::ReplanSingleAction,
            reason: "derive exactly one legal repair action before executing more work".into(),
        };
    }

    if state.actionable_failure {
        let action = if state.failure_scope_localized {
            HarnessRepairAction::ApplyLocalizedRepair
        } else {
            HarnessRepairAction::ApplyWorkspaceRepair
        };
        let reason = if state.failure_scope_localized {
            "localized failure scope requires semantic or file repair".to_string()
        } else if state.failure_scope_workspace || state.failure_scope_tooling {
            "workspace or tooling failure scope requires environment or command repair".to_string()
        } else {
            "actionable failure without narrow scope defaults to workspace-level repair".to_string()
        };
        return HarnessRepairDecision {
            phase: HarnessRepairPhase::Repair,
            action,
            reason,
        };
    }

    if state.verifier_ready && !state.cargo_check_passed {
        return HarnessRepairDecision {
            phase: HarnessRepairPhase::Verify,
            action: HarnessRepairAction::RunCargoCheck,
            reason: "mutations must be followed by cargo check before stronger verification".into(),
        };
    }

    if state.cargo_check_passed && state.stronger_verification_requested {
        return HarnessRepairDecision {
            phase: HarnessRepairPhase::Verify,
            action: HarnessRepairAction::RunCargoTest,
            reason: "cargo check passed; run stronger verifier to validate the harness repair".into(),
        };
    }

    if meta_invariant_harness_self_repair_ready(state.capabilities) {
        return HarnessRepairDecision {
            phase: HarnessRepairPhase::Update,
            action: HarnessRepairAction::UpdateState,
            reason: "verification is complete; update loop state before the next iteration".into(),
        };
    }

    HarnessRepairDecision {
        phase: HarnessRepairPhase::Stop,
        action: HarnessRepairAction::StopReady,
        reason: "harness repair loop reached a terminal state".into(),
    }
}

pub fn build_harness_repair_directive(
    state: &HarnessRepairState,
    target: &HarnessRepairTarget,
) -> HarnessRepairDirective {
    let decision = evaluate_harness_repair_loop(state);
    let verifier_command = match decision.action {
        HarnessRepairAction::RunCargoCheck => Some(cargo_check_command(target)),
        HarnessRepairAction::RunCargoTest => Some(cargo_test_command(target)),
        _ => None,
    };
    HarnessRepairDirective {
        decision,
        verifier_command,
    }
}

pub fn format_missing_capabilities(missing: &[HarnessPrimitiveCapability]) -> String {
    missing
        .iter()
        .map(|capability| match capability {
            HarnessPrimitiveCapability::ReadSearch => "read_search",
            HarnessPrimitiveCapability::StructuredEdit => "structured_edit",
            HarnessPrimitiveCapability::ApplyPatch => "apply_patch",
            HarnessPrimitiveCapability::RunVerifier => "run_verifier",
            HarnessPrimitiveCapability::ObserveDiagnostics => "observe_diagnostics",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyntheticHarnessRepairMetrics {
    pub total_states: usize,
    pub observe: usize,
    pub decide: usize,
    pub repair: usize,
    pub verify: usize,
    pub update: usize,
    pub stop: usize,
}

fn synthetic_ready_capabilities() -> HarnessCapabilityState {
    HarnessCapabilityState {
        read_search: true,
        structured_edit: true,
        apply_patch: true,
        run_verifier: true,
        observe_diagnostics: true,
    }
}

fn synthetic_bool_at(bits: u16, shift: u8) -> bool {
    bits & (1 << shift) != 0
}

fn synthetic_state_from_bits(bits: u16) -> HarnessRepairState {
    HarnessRepairState {
        capabilities: synthetic_ready_capabilities(),
        drift_detected: synthetic_bool_at(bits, 0),
        actionable_failure: synthetic_bool_at(bits, 1),
        failure_class_no_actionable: synthetic_bool_at(bits, 2),
        failure_scope_localized: synthetic_bool_at(bits, 3),
        failure_scope_workspace: synthetic_bool_at(bits, 4),
        failure_scope_tooling: synthetic_bool_at(bits, 5),
        verifier_ready: synthetic_bool_at(bits, 6),
        cargo_check_passed: synthetic_bool_at(bits, 7),
        stronger_verification_requested: synthetic_bool_at(bits, 8),
        last_action_was_mutation: synthetic_bool_at(bits, 9),
        single_action_batch_required: true,
        needs_replan: synthetic_bool_at(bits, 10),
        progress_stalled: synthetic_bool_at(bits, 11),
    }
}

pub fn synthetic_harness_repair_states() -> Vec<HarnessRepairState> {
    let mut states = Vec::new();
    for bits in 0u16..(1u16 << 12) {
        states.push(synthetic_state_from_bits(bits));
    }
    states
}

pub fn synthetic_harness_repair_metrics() -> SyntheticHarnessRepairMetrics {
    let mut metrics = SyntheticHarnessRepairMetrics::default();

    for state in synthetic_harness_repair_states() {
        let decision = evaluate_harness_repair_loop(&state);
        metrics.total_states += 1;
        match decision.phase {
            HarnessRepairPhase::Observe => metrics.observe += 1,
            HarnessRepairPhase::Decide => metrics.decide += 1,
            HarnessRepairPhase::Repair => metrics.repair += 1,
            HarnessRepairPhase::Verify => metrics.verify += 1,
            HarnessRepairPhase::Update => metrics.update += 1,
            HarnessRepairPhase::Stop => metrics.stop += 1,
        }
    }

    metrics
}

fn cargo_check_command(target: &HarnessRepairTarget) -> String {
    target
        .crate_name
        .as_deref()
        .map(|crate_name| format!("cargo check -p {crate_name}"))
        .unwrap_or_else(|| "cargo check".to_string())
}

fn cargo_test_command(target: &HarnessRepairTarget) -> String {
    match (target.crate_name.as_deref(), target.failing_test.as_deref()) {
        (Some(crate_name), Some(test_name)) => format!("cargo test -p {crate_name} {test_name} -- --nocapture"),
        (Some(crate_name), None) => format!("cargo test -p {crate_name}"),
        (None, Some(test_name)) => format!("cargo test {test_name} -- --nocapture"),
        (None, None) => "cargo test".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_harness_repair_directive, evaluate_harness_repair_loop, HarnessRepairAction,
        HarnessRepairPhase, HarnessRepairState, HarnessRepairTarget,
    };
    use canon_invariant::{
        meta_invariant_harness_self_repair_missing_capabilities,
        meta_invariant_harness_self_repair_ready, HarnessCapabilityState,
    };

    fn ready_caps() -> HarnessCapabilityState {
        HarnessCapabilityState {
            read_search: true,
            structured_edit: true,
            apply_patch: true,
            run_verifier: true,
            observe_diagnostics: true,
        }
    }

    #[test]
    fn harness_loop_observes_when_failure_is_not_actionable() {
        let decision = evaluate_harness_repair_loop(&HarnessRepairState {
            capabilities: ready_caps(),
            failure_class_no_actionable: true,
            ..HarnessRepairState::default()
        });
        assert_eq!(decision.phase, HarnessRepairPhase::Observe);
        assert_eq!(decision.action, HarnessRepairAction::CollectDiagnostics);
    }

    #[test]
    fn harness_loop_prefers_localized_repair() {
        let decision = evaluate_harness_repair_loop(&HarnessRepairState {
            capabilities: ready_caps(),
            actionable_failure: true,
            failure_scope_localized: true,
            ..HarnessRepairState::default()
        });
        assert_eq!(decision.phase, HarnessRepairPhase::Repair);
        assert_eq!(decision.action, HarnessRepairAction::ApplyLocalizedRepair);
    }

    #[test]
    fn harness_directive_builds_targeted_verifier_command() {
        let directive = build_harness_repair_directive(
            &HarnessRepairState {
                capabilities: ready_caps(),
                verifier_ready: true,
                cargo_check_passed: true,
                stronger_verification_requested: true,
                ..HarnessRepairState::default()
            },
            &HarnessRepairTarget::new(
                Some("canon-route".into()),
                Some("policy::tests::foo".into()),
            ),
        );
        assert_eq!(directive.decision.action, HarnessRepairAction::RunCargoTest);
        assert_eq!(
            directive.verifier_command.as_deref(),
            Some("cargo test -p canon-route policy::tests::foo -- --nocapture")
        );
    }

    fn bool_at(bits: u16, shift: u8) -> bool {
        bits & (1 << shift) != 0
    }

    fn state_from_bits(bits: u16) -> HarnessRepairState {
        HarnessRepairState {
            capabilities: ready_caps(),
            drift_detected: bool_at(bits, 0),
            actionable_failure: bool_at(bits, 1),
            failure_class_no_actionable: bool_at(bits, 2),
            failure_scope_localized: bool_at(bits, 3),
            failure_scope_workspace: bool_at(bits, 4),
            failure_scope_tooling: bool_at(bits, 5),
            verifier_ready: bool_at(bits, 6),
            cargo_check_passed: bool_at(bits, 7),
            stronger_verification_requested: bool_at(bits, 8),
            last_action_was_mutation: bool_at(bits, 9),
            single_action_batch_required: true,
            needs_replan: bool_at(bits, 10),
            progress_stalled: bool_at(bits, 11),
        }
    }

    fn assert_decision_matches_precedence(state: &HarnessRepairState) {
        let decision = evaluate_harness_repair_loop(state);
        let missing = meta_invariant_harness_self_repair_missing_capabilities(state.capabilities);

        if !missing.is_empty() {
            assert_eq!(decision.phase, HarnessRepairPhase::Stop);
            assert_eq!(decision.action, HarnessRepairAction::StopBlocked);
            return;
        }

        if state.drift_detected {
            assert_eq!(decision.phase, HarnessRepairPhase::Observe);
            assert_eq!(decision.action, HarnessRepairAction::ObserveWorkspace);
            return;
        }

        if state.failure_class_no_actionable && !state.actionable_failure {
            assert_eq!(decision.phase, HarnessRepairPhase::Observe);
            assert_eq!(decision.action, HarnessRepairAction::CollectDiagnostics);
            return;
        }

        if state.needs_replan || state.progress_stalled {
            assert_eq!(decision.phase, HarnessRepairPhase::Decide);
            assert_eq!(decision.action, HarnessRepairAction::ReplanSingleAction);
            return;
        }

        if state.actionable_failure {
            assert_eq!(decision.phase, HarnessRepairPhase::Repair);
            if state.failure_scope_localized {
                assert_eq!(decision.action, HarnessRepairAction::ApplyLocalizedRepair);
            } else {
                assert_eq!(decision.action, HarnessRepairAction::ApplyWorkspaceRepair);
            }
            return;
        }

        if state.verifier_ready && !state.cargo_check_passed {
            assert_eq!(decision.phase, HarnessRepairPhase::Verify);
            assert_eq!(decision.action, HarnessRepairAction::RunCargoCheck);
            return;
        }

        if state.cargo_check_passed && state.stronger_verification_requested {
            assert_eq!(decision.phase, HarnessRepairPhase::Verify);
            assert_eq!(decision.action, HarnessRepairAction::RunCargoTest);
            return;
        }

        if meta_invariant_harness_self_repair_ready(state.capabilities) {
            assert_eq!(decision.phase, HarnessRepairPhase::Update);
            assert_eq!(decision.action, HarnessRepairAction::UpdateState);
            return;
        }

        assert_eq!(decision.phase, HarnessRepairPhase::Stop);
        assert_eq!(decision.action, HarnessRepairAction::StopReady);
    }

    #[test]
    fn harness_loop_full_state_space_is_exhaustively_mapped() {
        let mut total = 0usize;
        let mut observe = 0usize;
        let mut decide = 0usize;
        let mut repair = 0usize;
        let mut verify = 0usize;
        let mut update = 0usize;
        let mut stop = 0usize;

        for bits in 0u16..(1u16 << 12) {
            let state = state_from_bits(bits);
            let decision = evaluate_harness_repair_loop(&state);
            assert_decision_matches_precedence(&state);

            match decision.phase {
                HarnessRepairPhase::Observe => observe += 1,
                HarnessRepairPhase::Decide => decide += 1,
                HarnessRepairPhase::Repair => repair += 1,
                HarnessRepairPhase::Verify => verify += 1,
                HarnessRepairPhase::Update => update += 1,
                HarnessRepairPhase::Stop => stop += 1,
            }

            total += 1;
        }

        assert_eq!(total, 1 << 12);
        assert_eq!(total, observe + decide + repair + verify + update + stop);
        assert!(observe > 0);
        assert!(decide > 0);
        assert!(repair > 0);
        assert!(verify > 0);
        assert!(update > 0);
        let _ = stop;
    }

    #[test]
    fn harness_directive_command_mapping_is_exhaustive_for_full_state_space() {
        let target = HarnessRepairTarget::new(
            Some("canon-route".into()),
            Some("policy::tests::foo".into()),
        );

        for bits in 0u16..(1u16 << 12) {
            let state = state_from_bits(bits);
            let directive = build_harness_repair_directive(&state, &target);

            match directive.decision.action {
                HarnessRepairAction::RunCargoCheck => {
                    assert_eq!(
                        directive.verifier_command.as_deref(),
                        Some("cargo check -p canon-route")
                    );
                }
                HarnessRepairAction::RunCargoTest => {
                    assert_eq!(
                        directive.verifier_command.as_deref(),
                        Some("cargo test -p canon-route policy::tests::foo -- --nocapture")
                    );
                }
                _ => {
                    assert!(directive.verifier_command.is_none());
                }
            }
        }
    }
}
