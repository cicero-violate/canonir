use canon_types::{EventDelta, InvariantViolation, RustcEvent, RustcState};
use serde_json::Value;

pub fn invariant_violation_delta(message: impl Into<String>) -> EventDelta {
    EventDelta {
        id: 0,
        tick: 0,
        event: RustcEvent::InvariantViolation(InvariantViolation {
            message: message.into(),
            recorded: true,
        }),
    }
}

pub fn invariant_violation_state() -> RustcState {
    RustcState::default()
}

pub fn decision_trace_payload(
    reason: impl Into<String>,
    context: Value,
) -> Value {
    serde_json::json!({
        "reason": reason.into(),
        "context": context,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannedActionClass {
    PassiveDiscovery,
    Verification,
    Mutation,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetaInvariantVerifierOutcome {
    Passed,
    CompilerFailure,
    FailedNoCompilerSignal,
}

impl MetaInvariantVerifierOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::CompilerFailure => "compiler_failure",
            Self::FailedNoCompilerSignal => "failed_no_compiler_signal",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetaInvariantPolicyUpdate {
    pub verifier_outcome: MetaInvariantVerifierOutcome,
    pub retry_policy: &'static str,
    pub reward_bias: &'static str,
    pub actionable_failure: bool,
}

impl MetaInvariantPolicyUpdate {
    pub fn as_summary(self) -> String {
        format!(
            "verifier_outcome={} retry_policy={} reward_bias={} actionable_failure={}",
            self.verifier_outcome.as_str(),
            self.retry_policy,
            self.reward_bias,
            self.actionable_failure
        )
    }
}

impl PlannedActionClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PassiveDiscovery => "passive_discovery",
            Self::Verification => "verification",
            Self::Mutation => "mutation",
            Self::Unknown => "unknown",
        }
    }
}

pub fn meta_invariant_classify_planned_action_class(
    action_kind: &str,
    action_payload: &Value,
) -> PlannedActionClass {
    match action_kind {
        "read_file" | "list_dir" | "search_files" => PlannedActionClass::PassiveDiscovery,
        "run_command" => action_payload
            .get("cmd")
            .and_then(|v| v.as_str())
            .map(|cmd| {
                if cmd.contains("cargo check") || cmd.contains("cargo build") || cmd.contains("cargo test") {
                    PlannedActionClass::Verification
                } else {
                    PlannedActionClass::Mutation
                }
            })
            .unwrap_or(PlannedActionClass::Unknown),
        "write_file"
        | "patch_file"
        | "apply_patch"
        | "edit.rename_symbol"
        | "edit.move_symbol"
        | "edit.add_import"
        | "edit.define_symbol_stub"
        | "edit.create_module_file" => PlannedActionClass::Mutation,
        _ => PlannedActionClass::Unknown,
    }
}

pub fn classify_planned_action_class(
    action_kind: &str,
    action_payload: &Value,
) -> PlannedActionClass {
    meta_invariant_classify_planned_action_class(action_kind, action_payload)
}

pub fn meta_invariant_is_localized_repair_action(action_kind: &str) -> bool {
    matches!(
        action_kind,
        "edit.add_import"
            | "edit.define_symbol_stub"
            | "edit.rename_symbol"
            | "apply_patch"
    )
}

pub fn is_localized_repair_action(action_kind: &str) -> bool {
    meta_invariant_is_localized_repair_action(action_kind)
}

pub fn meta_invariant_all_failures_typed(
    failure_class: Option<&str>,
    failure_scope: Option<&str>,
) -> bool {
    matches!(failure_class, Some(value) if !value.trim().is_empty())
        && matches!(failure_scope, Some("localized" | "workspace" | "tooling" | "none"))
}

pub fn meta_invariant_any_action_cites_failure(
    action_payload: &Value,
    active_failure_class: Option<&str>,
) -> bool {
    match active_failure_class {
        Some(expected) if !expected.trim().is_empty() => action_payload
            .get("failure_class")
            .and_then(|v| v.as_str())
            .map(|value| value == expected)
            .unwrap_or(false),
        _ => true,
    }
}

pub fn meta_invariant_is_mutating_action(
    action_kind: &str,
    action_payload: &Value,
) -> bool {
    matches!(
        meta_invariant_classify_planned_action_class(action_kind, action_payload),
        PlannedActionClass::Mutation
    )
}

pub fn meta_invariant_expected_verifier(
    action_kind: &str,
    action_payload: &Value,
) -> Option<&'static str> {
    if !meta_invariant_is_mutating_action(action_kind, action_payload) {
        return None;
    }
    match action_kind {
        "edit.rename_symbol"
        | "edit.move_symbol"
        | "edit.add_import"
        | "edit.define_symbol_stub"
        | "edit.create_module_file" => Some("graph_proof"),
        "apply_patch" | "patch_file" | "write_file" | "run_command" => Some("cargo_check"),
        _ => Some("cargo_check"),
    }
}

pub fn meta_invariant_action_must_declare_verifier(
    action_kind: &str,
    action_payload: &Value,
) -> bool {
    let Some(expected) = meta_invariant_expected_verifier(action_kind, action_payload) else {
        return true;
    };
    action_payload
        .get("verifier")
        .and_then(|v| v.as_str())
        .map(|value| !value.trim().is_empty() && value == expected)
        .unwrap_or(false)
}

pub fn meta_invariant_has_actionable_failure(
    validation_blocked_by_preconditions: bool,
    compiler_repair_required: bool,
    planning_preconditions_len: usize,
    compiler_hints_len: usize,
    module_gaps_len: usize,
) -> bool {
    validation_blocked_by_preconditions
        || compiler_repair_required
        || planning_preconditions_len > 0
        || compiler_hints_len > 0
        || module_gaps_len > 0
}

pub fn semantic_summary_has_actionable_failure(
    validation_blocked_by_preconditions: bool,
    compiler_repair_required: bool,
    planning_preconditions_len: usize,
    compiler_hints_len: usize,
    module_gaps_len: usize,
) -> bool {
    meta_invariant_has_actionable_failure(
        validation_blocked_by_preconditions,
        compiler_repair_required,
        planning_preconditions_len,
        compiler_hints_len,
        module_gaps_len,
    )
}

pub fn meta_invariant_failure_scope_is_sufficient(
    compiler_repair_required: bool,
    compiler_hints_len: usize,
    failure_scope: Option<&str>,
) -> bool {
    if !compiler_repair_required || compiler_hints_len == 0 {
        return true;
    }
    matches!(failure_scope, Some("localized") | Some("workspace") | Some("tooling"))
}

pub fn failure_scope_is_sufficient(
    compiler_repair_required: bool,
    compiler_hints_len: usize,
    failure_scope: Option<&str>,
) -> bool {
    meta_invariant_failure_scope_is_sufficient(
        compiler_repair_required,
        compiler_hints_len,
        failure_scope,
    )
}

pub fn meta_invariant_high_invalid_plan_requires_simple_batch(
    invalid_plan_rate: f32,
    planning_attempts: u32,
) -> bool {
    invalid_plan_rate > 0.5 && planning_attempts >= 3
}

pub fn high_invalid_plan_pressure_requires_single_action(
    invalid_plan_rate: f32,
    planning_attempts: u32,
) -> bool {
    meta_invariant_high_invalid_plan_requires_simple_batch(invalid_plan_rate, planning_attempts)
}

pub fn meta_invariant_no_progress_forces_change(
    no_progress_streak: u32,
    action_class: PlannedActionClass,
) -> bool {
    no_progress_streak >= 2
        && matches!(action_class, PlannedActionClass::PassiveDiscovery | PlannedActionClass::Verification)
}

pub fn stalled_loop_forbids_action_class(
    no_progress_streak: u32,
    action_class: PlannedActionClass,
) -> bool {
    meta_invariant_no_progress_forces_change(no_progress_streak, action_class)
}

fn looks_like_compiler_failure(text: &str) -> bool {
    text.contains("error[E")
        || text.contains("could not compile")
        || text.contains("allow(dead_code) incompatible with previous forbid")
        || text.contains("file not found for module `")
        || text.contains("cargo_check_failed")
}

pub fn meta_invariant_classify_verifier_outcome(
    passed: bool,
    compiler_clean: bool,
    diagnostics: &[String],
) -> MetaInvariantVerifierOutcome {
    if passed && compiler_clean {
        MetaInvariantVerifierOutcome::Passed
    } else if diagnostics.iter().any(|d| looks_like_compiler_failure(d)) {
        MetaInvariantVerifierOutcome::CompilerFailure
    } else {
        MetaInvariantVerifierOutcome::FailedNoCompilerSignal
    }
}

pub fn meta_invariant_all_results_update_policy(
    passed: bool,
    compiler_clean: bool,
    diagnostics: &[String],
) -> MetaInvariantPolicyUpdate {
    let verifier_outcome =
        meta_invariant_classify_verifier_outcome(passed, compiler_clean, diagnostics);
    match verifier_outcome {
        MetaInvariantVerifierOutcome::Passed => MetaInvariantPolicyUpdate {
            verifier_outcome,
            retry_policy: "none",
            reward_bias: "positive",
            actionable_failure: false,
        },
        MetaInvariantVerifierOutcome::CompilerFailure
        | MetaInvariantVerifierOutcome::FailedNoCompilerSignal => MetaInvariantPolicyUpdate {
            verifier_outcome,
            retry_policy: "corrective_retry",
            reward_bias: "negative",
            actionable_failure: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        meta_invariant_all_results_update_policy, meta_invariant_classify_verifier_outcome,
        MetaInvariantVerifierOutcome,
    };

    #[test]
    fn meta_invariant_all_results_update_policy_passed_is_positive() {
        let update = meta_invariant_all_results_update_policy(true, true, &[]);
        assert_eq!(update.verifier_outcome, MetaInvariantVerifierOutcome::Passed);
        assert_eq!(update.retry_policy, "none");
        assert_eq!(update.reward_bias, "positive");
        assert!(!update.actionable_failure);
    }

    #[test]
    fn meta_invariant_all_results_update_policy_compiler_failure_is_corrective() {
        let diagnostics = vec![
            "cargo_check_failed".to_string(),
            "error[E0432]: unresolved import".to_string(),
        ];
        let outcome = meta_invariant_classify_verifier_outcome(false, false, &diagnostics);
        assert_eq!(outcome, MetaInvariantVerifierOutcome::CompilerFailure);
        let update = meta_invariant_all_results_update_policy(false, false, &diagnostics);
        assert_eq!(update.retry_policy, "corrective_retry");
        assert_eq!(update.reward_bias, "negative");
        assert!(update.actionable_failure);
    }
}
