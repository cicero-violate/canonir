#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandClass {
    Discovery,
    Edit,
    Validation,
    Bootstrap,
    Completion,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionOutcomeClass {
    BootstrapSuccess,
    ValidationFailureCompiler,
    ValidationSuccess,
    SemanticFailure,
    PatchMissingTargetFile,
    PatchApplyFailure,
    EditSuccess,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopRecoveryRule {
    ClearPlannerSuppressionOnInvalidPlan,
    TriggerObserveOnActStall,
    RecoverLoopRewarded,
}

pub struct LoopTransitionEvaluation {
    pub recovery_rules: Vec<LoopRecoveryRule>,
    pub trigger_observe: bool,
    pub force_reward_recovery: bool,
    pub observe_blocked_by_successor: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserveExecutionMode {
    None,
    Forced,
    Triggered,
    SuppressedByInvariant,
    SuppressedByPendingSuccessor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopRuntimeRule {
    ExecuteForcedObserve,
    ExecuteTriggeredObserve,
    SuppressObserveOnInvariant,
    SuppressObserveOnPendingSuccessor,
    BlockStageWhenHalted,
    WarnRouteSelectedWhileHalted,
}

pub struct LoopRuntimeEvaluation {
    pub observe_mode: ObserveExecutionMode,
    pub halt_blocks_stage: bool,
    pub warn_route_selected_while_halted: bool,
    pub rules: Vec<LoopRuntimeRule>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryEventRule {
    None,
    ForceObserve,
    ExecuteRewardRecovery,
    SkipRewardAlreadySatisfied,
    MissingRewardContext,
}

pub struct RecoveryEventEvaluation {
    pub rule: RecoveryEventRule,
    pub force_observe_recovery: bool,
    pub execute_reward_recovery: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryOperation {
    RewardRecovery,
    ObserveForced,
    ObserveTriggered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageExecutionOutcomeClass {
    Emit,
    EmitMany,
    Deferred,
    Noop,
    Error,
}

pub struct RecoveryExecutionEvaluation {
    pub debug_kind: Option<&'static str>,
    pub debug_reason: Option<&'static str>,
    pub error_kind: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapRule {
    None,
    InvalidateQueuedPlanWork,
}

pub struct BootstrapEvaluation {
    pub rule: BootstrapRule,
    pub clear_scheduler: bool,
    pub clear_dep_tracker: bool,
    pub clear_pending_act: bool,
    pub clear_active_batch: bool,
    pub clear_act_batch_tracker: bool,
    pub emit_refresh_required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorObserveRule {
    None,
    ExplicitRecovery,
    GenericErrorObserve,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidPlanReasonClass {
    MixedBatch,
    PatchFormat,
    PathOrCwd,
    MissingContext,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryPolicy {
    None,
    DiscoveryOnly,
    SinglePatchOnly,
    CorrectiveRetry,
}

impl RetryPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DiscoveryOnly => "discovery_only",
            Self::SinglePatchOnly => "single_patch_only",
            Self::CorrectiveRetry => "corrective_retry",
        }
    }
}

pub fn classify_action(action_kind: &str, stdout: &str, stderr: &str) -> CommandClass {
    match action_kind {
        "list_dir" | "read_file" | "search_files" => CommandClass::Discovery,
        "apply_patch" | "write_file" | "patch_file" => CommandClass::Edit,
        "done" => CommandClass::Completion,
        "run_command" if is_bootstrap_command_output(stdout, stderr) => CommandClass::Bootstrap,
        "run_command" => CommandClass::Validation,
        _ => CommandClass::Other,
    }
}

pub fn classify_action_outcome(action_kind: &str, success: bool, stdout: &str, stderr: &str) -> ActionOutcomeClass {
    let text = if !stderr.is_empty() { stderr } else { stdout };
    match action_kind {
        "run_command" if success && is_bootstrap_command_output(stdout, stderr) => ActionOutcomeClass::BootstrapSuccess,
        "run_command" if success && looks_semantically_failed(text) => ActionOutcomeClass::SemanticFailure,
        "run_command" if success => ActionOutcomeClass::ValidationSuccess,
        "run_command" if looks_like_compiler_failure(text) => ActionOutcomeClass::ValidationFailureCompiler,
        "run_command" if looks_semantically_failed(text) => ActionOutcomeClass::SemanticFailure,
        "apply_patch" if success => ActionOutcomeClass::EditSuccess,
        "apply_patch" if text.contains("No such file or directory") || text.contains("Failed to read file to update") => {
            ActionOutcomeClass::PatchMissingTargetFile
        }
        "apply_patch"
            if text.contains("invalid hunk")
                || text.contains("unexpected line in update chunk")
                || text.contains("Failed to find expected lines")
                || text.contains("apply_patch failed") =>
        {
            ActionOutcomeClass::PatchApplyFailure
        }
        _ => ActionOutcomeClass::Other,
    }
}

pub fn recovery_rules_for_error_kind(error_kind: &str) -> Vec<LoopRecoveryRule> {
    match error_kind {
        "act_stall" => vec![LoopRecoveryRule::TriggerObserveOnActStall],
        _ => Vec::new(),
    }
}

pub fn recovery_rules_for_planning_status(status: &str) -> Vec<LoopRecoveryRule> {
    match status {
        "invalid_plan" => vec![LoopRecoveryRule::ClearPlannerSuppressionOnInvalidPlan],
        _ => Vec::new(),
    }
}

pub fn recovery_rules_for_expected_successor(expected_successor: &str) -> Vec<LoopRecoveryRule> {
    match expected_successor {
        "loop_rewarded" => vec![LoopRecoveryRule::RecoverLoopRewarded],
        _ => Vec::new(),
    }
}

pub fn evaluate_loop_transition(
    pending_required_successor: Option<&str>,
    planning_status: Option<&str>,
    error_kind: Option<&str>,
    expected_successor: Option<&str>,
) -> LoopTransitionEvaluation {
    let mut recovery_rules = Vec::new();
    if let Some(status) = planning_status {
        recovery_rules.extend(recovery_rules_for_planning_status(status));
    }
    if let Some(kind) = error_kind {
        recovery_rules.extend(recovery_rules_for_error_kind(kind));
    }
    if let Some(expected) = expected_successor {
        recovery_rules.extend(recovery_rules_for_expected_successor(expected));
    }
    let trigger_observe = recovery_rules.contains(&LoopRecoveryRule::TriggerObserveOnActStall);
    let force_reward_recovery = recovery_rules.contains(&LoopRecoveryRule::RecoverLoopRewarded);
    let observe_blocked_by_successor = pending_required_successor.is_some_and(|expected| expected != "loop_observed");
    LoopTransitionEvaluation {
        recovery_rules,
        trigger_observe,
        force_reward_recovery,
        observe_blocked_by_successor,
    }
}

pub fn evaluate_loop_runtime(
    halted: bool,
    force_observe_recovery: bool,
    trigger_observe: bool,
    suppress_observe_on_invariant: bool,
    pending_required_successor: Option<&str>,
    is_route_selected_event: bool,
) -> LoopRuntimeEvaluation {
    let mut rules = Vec::new();
    let observe_mode = if force_observe_recovery && !halted {
        rules.push(LoopRuntimeRule::ExecuteForcedObserve);
        ObserveExecutionMode::Forced
    } else if trigger_observe && !halted && suppress_observe_on_invariant {
        rules.push(LoopRuntimeRule::SuppressObserveOnInvariant);
        ObserveExecutionMode::SuppressedByInvariant
    } else if trigger_observe && !halted && pending_required_successor.is_some_and(|expected| expected != "loop_observed") {
        rules.push(LoopRuntimeRule::SuppressObserveOnPendingSuccessor);
        ObserveExecutionMode::SuppressedByPendingSuccessor
    } else if trigger_observe && !halted {
        rules.push(LoopRuntimeRule::ExecuteTriggeredObserve);
        ObserveExecutionMode::Triggered
    } else {
        ObserveExecutionMode::None
    };

    let halt_blocks_stage = halted;
    if halt_blocks_stage {
        rules.push(LoopRuntimeRule::BlockStageWhenHalted);
    }
    let warn_route_selected_while_halted = halted && is_route_selected_event;
    if warn_route_selected_while_halted {
        rules.push(LoopRuntimeRule::WarnRouteSelectedWhileHalted);
    }

    LoopRuntimeEvaluation {
        observe_mode,
        halt_blocks_stage,
        warn_route_selected_while_halted,
        rules,
    }
}

pub fn evaluate_recovery_event(
    expected_successor: Option<&str>,
    pending_required_successor: Option<&str>,
    has_last_verified: bool,
) -> RecoveryEventEvaluation {
    match expected_successor {
        Some("loop_observed") => RecoveryEventEvaluation {
            rule: RecoveryEventRule::ForceObserve,
            force_observe_recovery: true,
            execute_reward_recovery: false,
        },
        Some("loop_rewarded") if pending_required_successor != Some("loop_rewarded") => RecoveryEventEvaluation {
            rule: RecoveryEventRule::SkipRewardAlreadySatisfied,
            force_observe_recovery: false,
            execute_reward_recovery: false,
        },
        Some("loop_rewarded") if !has_last_verified => RecoveryEventEvaluation {
            rule: RecoveryEventRule::MissingRewardContext,
            force_observe_recovery: false,
            execute_reward_recovery: false,
        },
        Some("loop_rewarded") => RecoveryEventEvaluation {
            rule: RecoveryEventRule::ExecuteRewardRecovery,
            force_observe_recovery: false,
            execute_reward_recovery: true,
        },
        _ => RecoveryEventEvaluation {
            rule: RecoveryEventRule::None,
            force_observe_recovery: false,
            execute_reward_recovery: false,
        },
    }
}

pub fn evaluate_recovery_execution(
    operation: RecoveryOperation,
    outcome: StageExecutionOutcomeClass,
) -> RecoveryExecutionEvaluation {
    match (operation, outcome) {
        (_, StageExecutionOutcomeClass::Emit | StageExecutionOutcomeClass::EmitMany) => RecoveryExecutionEvaluation {
            debug_kind: None,
            debug_reason: None,
            error_kind: None,
        },
        (RecoveryOperation::RewardRecovery, StageExecutionOutcomeClass::Deferred | StageExecutionOutcomeClass::Noop) => {
            RecoveryExecutionEvaluation {
                debug_kind: Some("reward_recovery_noop"),
                debug_reason: Some("reward recovery produced no events"),
                error_kind: None,
            }
        }
        (RecoveryOperation::RewardRecovery, StageExecutionOutcomeClass::Error) => RecoveryExecutionEvaluation {
            debug_kind: None,
            debug_reason: None,
            error_kind: Some("reward_recovery_execution"),
        },
        (RecoveryOperation::ObserveForced | RecoveryOperation::ObserveTriggered, StageExecutionOutcomeClass::Deferred) => {
            RecoveryExecutionEvaluation {
                debug_kind: Some("observe_deferred"),
                debug_reason: Some("observe returned deferred"),
                error_kind: None,
            }
        }
        (RecoveryOperation::ObserveForced | RecoveryOperation::ObserveTriggered, StageExecutionOutcomeClass::Noop) => {
            RecoveryExecutionEvaluation {
                debug_kind: Some("observe_noop"),
                debug_reason: Some("observe returned noop"),
                error_kind: None,
            }
        }
        (RecoveryOperation::ObserveForced, StageExecutionOutcomeClass::Error) => RecoveryExecutionEvaluation {
            debug_kind: None,
            debug_reason: None,
            error_kind: Some("observe_recovery_execution"),
        },
        (RecoveryOperation::ObserveTriggered, StageExecutionOutcomeClass::Error) => RecoveryExecutionEvaluation {
            debug_kind: None,
            debug_reason: None,
            error_kind: Some("observe_stage_execution"),
        },
    }
}

pub fn evaluate_error_observe(
    error_kind: &str,
    explicit_observe_recovery: bool,
    fatal_invariant_diag: bool,
) -> ErrorObserveRule {
    if explicit_observe_recovery {
        return ErrorObserveRule::ExplicitRecovery;
    }
    if error_kind != "invariant_violation" && error_kind != "invalid_plan_batch" && !fatal_invariant_diag {
        return ErrorObserveRule::GenericErrorObserve;
    }
    ErrorObserveRule::None
}

pub fn evaluate_bootstrap_effects(action_outcome: ActionOutcomeClass) -> BootstrapEvaluation {
    if action_outcome == ActionOutcomeClass::BootstrapSuccess {
        return BootstrapEvaluation {
            rule: BootstrapRule::InvalidateQueuedPlanWork,
            clear_scheduler: true,
            clear_dep_tracker: true,
            clear_pending_act: true,
            clear_active_batch: true,
            clear_act_batch_tracker: true,
            emit_refresh_required: true,
        };
    }
    BootstrapEvaluation {
        rule: BootstrapRule::None,
        clear_scheduler: false,
        clear_dep_tracker: false,
        clear_pending_act: false,
        clear_active_batch: false,
        clear_act_batch_tracker: false,
        emit_refresh_required: false,
    }
}

pub fn is_read_only_action(action_kind: &str) -> bool {
    matches!(action_kind, "list_dir" | "read_file" | "search_files" | "done")
}

pub fn classify_invalid_plan_reason(reason: Option<&str>) -> InvalidPlanReasonClass {
    let Some(reason) = reason else {
        return InvalidPlanReasonClass::Unknown;
    };
    if reason.contains("mixed discovery actions with execution/edit actions") {
        InvalidPlanReasonClass::MixedBatch
    } else if reason.contains("apply_patch payload is invalid")
        || reason.contains("unexpected line in update chunk")
        || reason.contains("invalid hunk")
        || reason.contains("single-patch retry required")
    {
        InvalidPlanReasonClass::PatchFormat
    } else if reason.contains("absolute cwd")
        || reason.contains("path is invalid")
        || reason.contains("absolute paths are not allowed")
        || reason.contains("parent traversal")
    {
        InvalidPlanReasonClass::PathOrCwd
    } else if reason.contains("missing_last_observed") || reason.contains("missing_observed_context") {
        InvalidPlanReasonClass::MissingContext
    } else {
        InvalidPlanReasonClass::Unknown
    }
}

pub fn retry_policy_for_invalid_plan(reason: Option<&str>, consecutive_invalid_plan_batches: u32) -> RetryPolicy {
    if consecutive_invalid_plan_batches == 0 {
        return RetryPolicy::None;
    }
    match classify_invalid_plan_reason(reason) {
        InvalidPlanReasonClass::MixedBatch => RetryPolicy::DiscoveryOnly,
        InvalidPlanReasonClass::PatchFormat => RetryPolicy::SinglePatchOnly,
        InvalidPlanReasonClass::PathOrCwd
        | InvalidPlanReasonClass::MissingContext
        | InvalidPlanReasonClass::Unknown => RetryPolicy::CorrectiveRetry,
    }
}

pub fn retry_policy_for_planning_context(
    reason: Option<&str>,
    consecutive_invalid_plan_batches: u32,
    recent_execution_results: &[SemanticExecutionResultRecord],
    objective_trend_state: &ObjectiveTrendState,
) -> RetryPolicy {
    let base = retry_policy_for_invalid_plan(reason, consecutive_invalid_plan_batches);
    if base != RetryPolicy::None {
        return base;
    }
    if objective_trend_state.repeated_stall_count > 0 && objective_trend_state.current_no_progress_streak > 0 {
        return RetryPolicy::CorrectiveRetry;
    }
    if semantic_no_progress_streak(recent_execution_results) >= 2 {
        RetryPolicy::CorrectiveRetry
    } else if latest_no_semantic_progress(recent_execution_results) {
        RetryPolicy::CorrectiveRetry
    } else {
        RetryPolicy::None
    }
}

pub fn planner_hint_lines(
    reason: Option<&str>,
    consecutive_invalid_plan_batches: u32,
    recent_execution_results: &[SemanticExecutionResultRecord],
    objective_trend_state: &ObjectiveTrendState,
    last_failed_action_kind: Option<&str>,
    last_failed_text: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(reason) = reason {
        out.push(format!("Previous invalid-plan reason: {reason}"));
    }
    match retry_policy_for_planning_context(
        reason,
        consecutive_invalid_plan_batches,
        recent_execution_results,
        objective_trend_state,
    ) {
        RetryPolicy::None => {}
        RetryPolicy::DiscoveryOnly => out.push(
            "Programmatic tip: next batch must be discovery-only; emit only list_dir/read_file.".to_string(),
        ),
        RetryPolicy::SinglePatchOnly => out.push(
            "Programmatic tip: next batch must contain exactly one apply_patch and no run_command.".to_string(),
        ),
        RetryPolicy::CorrectiveRetry => out.push(
            "Programmatic tip: fix the previous invalid payload directly; do not default to discovery unless file contents are missing.".to_string(),
        ),
    }
    if latest_no_semantic_progress(recent_execution_results) {
        out.push(
            "Programmatic tip: the last executed batch produced no semantic progress; change the repair strategy instead of retrying the same edit."
                .to_string(),
        );
    }
    if semantic_no_progress_streak(recent_execution_results) >= 2 {
        out.push(
            "Programmatic tip: repeated no-progress batches indicate a stalled repair loop; change approach or refresh context before retrying."
                .to_string(),
        );
    }
    if objective_trend_state.invalid_plan_rate() > 0.5 && objective_trend_state.planning_attempts >= 3 {
        out.push(
            "Programmatic tip: invalid-plan rate is high; simplify the next batch and avoid mixing multiple repair strategies."
                .to_string(),
        );
    }
    if let Some(result) = recent_execution_results.iter().rev().next() {
        out.push(format!("Recent execution semantics: {}", result.render_line()));
    }
    if let (Some(kind), Some(text)) = (last_failed_action_kind, last_failed_text) {
        let text = text.trim().replace('\n', " ");
        let text = if text.len() > 240 { format!("{}...", &text[..240]) } else { text };
        if !text.is_empty() {
            out.push(format!("Recent failure hint: last failed action was {kind} with output: {text}"));
        }
    }
    out
}

fn is_bootstrap_command_output(stdout: &str, stderr: &str) -> bool {
    let text = if !stdout.is_empty() { stdout } else { stderr };
    text.contains("Creating binary (application) package")
        || text.contains("Creating library package")
        || text.contains("Creating binary (application) `")
        || text.contains("Creating library `")
}

fn looks_like_compiler_failure(text: &str) -> bool {
    text.contains("error[E")
        || text.contains("could not compile")
        || text.contains("For more information about this error")
}

fn looks_semantically_failed(text: &str) -> bool {
    text.contains("test result: FAILED")
        || text.contains("failed")
        || text.contains("panic")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_policy_is_reason_specific() {
        let cases = [
            (
                Some("invalid plan batch before execution: mixed discovery actions with execution/edit actions in one plan batch"),
                1,
                RetryPolicy::DiscoveryOnly,
            ),
            (
                Some("invalid plan batch before execution: apply_patch payload is invalid: invalid hunk at line 12"),
                1,
                RetryPolicy::SinglePatchOnly,
            ),
            (
                Some("invalid plan batch before execution: run_command requires an absolute cwd; got \".\""),
                1,
                RetryPolicy::CorrectiveRetry,
            ),
            (None, 0, RetryPolicy::None),
        ];

        for (reason, count, expected) in cases {
            assert_eq!(retry_policy_for_invalid_plan(reason, count), expected, "reason={reason:?}");
        }
    }

    #[test]
    fn read_only_actions_do_not_count_as_mutating() {
        for action in ["list_dir", "read_file", "search_files", "done"] {
            assert!(is_read_only_action(action), "action={action}");
        }
        for action in ["apply_patch", "run_command", "write_file"] {
            assert!(!is_read_only_action(action), "action={action}");
        }
    }

    #[test]
    fn planner_hints_include_failure_output() {
        let hints = planner_hint_lines(
            Some("invalid hunk at line 12"),
            2,
            &[],
            &ObjectiveTrendState::default(),
            Some("apply_patch"),
            Some("invalid hunk at line 12, unexpected line in update chunk"),
        );
        let text = hints.join("\n");
        assert!(text.contains("exactly one apply_patch"));
        assert!(text.contains("unexpected line in update chunk"));
    }

    #[test]
    fn no_semantic_progress_forces_corrective_retry_context() {
        let results = vec![SemanticExecutionResultRecord::new(
            "no_semantic_progress",
            "action failed",
            Vec::new(),
            false,
        )];
        assert_eq!(
            retry_policy_for_planning_context(None, 0, &results, &ObjectiveTrendState::default()),
            RetryPolicy::CorrectiveRetry
        );
        let hints =
            planner_hint_lines(None, 0, &results, &ObjectiveTrendState::default(), None, None).join("\n");
        assert!(hints.contains("no semantic progress"));
        assert!(hints.contains("Recent execution semantics:"));
    }

    #[test]
    fn run_command_is_classified_by_outcome_shape() {
        assert_eq!(
            classify_action("run_command", "", "    Creating binary (application) package"),
            CommandClass::Bootstrap
        );
        assert_eq!(
            classify_action("run_command", "", "error[E0453]: allow(dead_code) incompatible with previous forbid"),
            CommandClass::Validation
        );
    }

    #[test]
    fn action_outcomes_are_classified_explicitly() {
        assert_eq!(
            classify_action_outcome("run_command", true, "", "Creating binary (application) package"),
            ActionOutcomeClass::BootstrapSuccess
        );
        assert_eq!(
            classify_action_outcome("run_command", false, "", "error[E0453]: allow(dead_code) incompatible with previous forbid"),
            ActionOutcomeClass::ValidationFailureCompiler
        );
        assert_eq!(
            classify_action_outcome("apply_patch", false, "apply_patch failed: invalid hunk at line 12", ""),
            ActionOutcomeClass::PatchApplyFailure
        );
    }

    #[test]
    fn recovery_rules_are_explicitly_classified() {
        assert_eq!(
            recovery_rules_for_planning_status("invalid_plan"),
            vec![LoopRecoveryRule::ClearPlannerSuppressionOnInvalidPlan]
        );
        assert_eq!(
            recovery_rules_for_error_kind("act_stall"),
            vec![LoopRecoveryRule::TriggerObserveOnActStall]
        );
        assert_eq!(
            recovery_rules_for_expected_successor("loop_rewarded"),
            vec![LoopRecoveryRule::RecoverLoopRewarded]
        );
    }

    #[test]
    fn loop_transition_rows_cover_recovery_and_successor_state() {
        let rows = [
            (
                evaluate_loop_transition(Some("loop_acted"), None, Some("act_stall"), None),
                true,
                false,
                true,
            ),
            (
                evaluate_loop_transition(Some("loop_rewarded"), None, None, Some("loop_rewarded")),
                false,
                true,
                true,
            ),
        ];

        for (eval, trigger_observe, force_reward_recovery, blocked) in rows {
            assert_eq!(eval.trigger_observe, trigger_observe);
            assert_eq!(eval.force_reward_recovery, force_reward_recovery);
            assert_eq!(eval.observe_blocked_by_successor, blocked);
        }
    }
}
use canon_semantic_state::{
    latest_no_semantic_progress, semantic_no_progress_streak, ObjectiveTrendState,
    SemanticExecutionResultRecord,
};
