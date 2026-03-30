use crate::{context::RouteContext, decision::RouteDecision};
use canon_decision::RouteKind;
use canon_event::RuntimeEvent;
use canon_invariant::{
    evaluate_constraint_context, meta_invariant_has_actionable_failure, observe_failure_fingerprint, ConstraintContext, ConstraintDecision, ConstraintRoute, ConstraintState, FailureFingerprint,
};
use canon_semantic_state::{latest_no_semantic_progress, latest_semantic_progress, SemanticStateSummary};
use serde_json::Value;
use std::path::Path;

#[cfg(test)]
use canon_semantic_state::{CompilerHintKind, CompilerHintRecord, SemanticExecutionResultRecord};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunCommandOutcomeClass {
    BootstrapSuccess,
    BootstrapSelectionMismatch,
    ValidationFailureCompiler,
    ValidationSuccess,
    SemanticFailure,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyPatchOutcomeClass {
    Success,
    MissingTargetFile,
    PatchApplyFailure,
    OtherFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyOutcomeClass {
    CompilerFailure,
    Passed,
    FailedNoCompilerSignal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePolicyRule {
    ForcePlanOnRepeatedObserve,
    ForcePlanOnMissingTarget,
    ForcePlanOnBlockedValidation,
    ForcePlanOnObjectiveContradiction,
    CycleCapToPlan,
    CycleCapToObserve,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteDispatchRule {
    SuppressHalted,
    SuppressContextNotReady,
    SuppressPendingRequest,
    SuppressAwaitingControlSuccessor,
    SuppressDuplicateRouteForCurrentControl,
    DeterministicMissingTargetPlan,
    DeterministicInvalidPlanReplan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteEmitRule {
    Allowed,
    DuplicateEmitBeforeSuccessor,
    IllegalControlReentry,
    IllegalControlEmit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteCacheRule {
    Proceed,
    ReplayCachedRoute,
    InvalidateCachedObserveRoute,
    SuppressDuplicatePrompt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteEventDispatchRule {
    None,
    BatchSettled,
    IdleDispatch,
    RecoverableEmptyPlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteFailureRule {
    HeuristicFailureReroute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteEmitEffectRule {
    None,
    ClearDeterministicObserveSentinel,
    HaltOnConclude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteRecoveryRule {
    None,
    EmitExpectedSuccessorRecovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuccessorConsumptionRule {
    None,
    ClearAwaitingControlSuccessor,
}

pub struct RoutePolicyState<'a> {
    pub last_control_kind: Option<&'a str>,
    pub pending_required_successor: Option<&'a str>,
}

pub struct RouteDispatchState<'a> {
    pub pending_request_id: Option<&'a str>,
    pub awaiting_control_successor: Option<&'a str>,
    pub route_emitted_for_current_control: bool,
}

pub struct RouteEmitState<'a> {
    pub awaiting_control_successor: Option<&'a str>,
    pub last_control_kind: Option<&'a str>,
    pub pending_required_successor: Option<&'a str>,
}

pub struct RouteCacheState<'a> {
    pub force_fresh_route_once: bool,
    pub last_prompt_hash: Option<u64>,
    pub prompt_hash: u64,
    pub pending_required_successor: Option<&'a str>,
    pub last_route_prompt_hash: Option<u64>,
    pub route_emitted_for_current_control: bool,
    pub has_cached_route: bool,
    pub cached_route_is_observe: bool,
    pub can_emit_route_selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeterministicRouteRule {
    BootstrapRefreshObserve,
    StateDriftObserve,
    DoneVerify,
    SemanticProgressVerify,
    NoActionableFailureObserve,
    NoSemanticProgressPlan,
    ContinueAct,
    PlannedToAct,
    MissingObservedContextObserve,
    MissingTargetPlan,
    BlockedValidationPlan,
    InvalidPlanReplan,
}

#[derive(Clone)]
pub struct DeterministicRouteDecision {
    pub route: RouteKind,
    pub rationale: String,
    pub confidence: f32,
    pub prompt_tag: &'static str,
    pub noop_reason: &'static str,
    pub rule: DeterministicRouteRule,
}

pub struct RouteTransitionEvaluation {
    pub deterministic: Option<DeterministicRouteDecision>,
    pub rules: Vec<RoutePolicyRule>,
}

pub struct RouteSuppressionDecision {
    pub reason: &'static str,
    pub classification: &'static str,
    pub recovery: &'static str,
    pub extra: Value,
    pub emit_stall: bool,
    pub rule: RouteDispatchRule,
}

pub struct RouteDispatchEvaluation {
    pub suppression: Option<RouteSuppressionDecision>,
    pub deterministic: Option<DeterministicRouteDecision>,
}

pub struct RouteEmitEvaluation {
    pub allowed: bool,
    pub rule: RouteEmitRule,
    pub reason: Option<String>,
}

pub struct RouteCacheEvaluation {
    pub rule: RouteCacheRule,
}

pub struct RouteEventDispatchEvaluation {
    pub rule: RouteEventDispatchRule,
    pub should_dispatch: bool,
}

pub struct RouteFailureEvaluation {
    pub rule: RouteFailureRule,
    pub model_json: String,
}

pub struct RouteEmitEffectsEvaluation {
    pub clear_pending_request: bool,
    pub clear_pending_prompt: bool,
    pub set_halted: bool,
    pub rules: Vec<RouteEmitEffectRule>,
}

pub struct RouteRecoveryEvaluation {
    pub rule: RouteRecoveryRule,
    pub expected_successor: Option<String>,
}

pub struct SuccessorConsumptionEvaluation {
    pub rule: SuccessorConsumptionRule,
    pub clear_awaiting_control_successor: bool,
}

impl RoutePolicyRule {
    pub fn note(self) -> &'static str {
        match self {
            Self::ForcePlanOnRepeatedObserve => "repeated observe on unchanged state requires plan",
            Self::ForcePlanOnMissingTarget => "target workspace missing requires bootstrap plan",
            Self::ForcePlanOnBlockedValidation => "validation is blocked by planning preconditions; forcing plan",
            Self::ForcePlanOnObjectiveContradiction => "route contradicts active repair objective; forcing plan",
            Self::CycleCapToPlan => "cycle cap reached but actionable failure remains; forcing plan",
            Self::CycleCapToObserve => "cycle cap reached without terminal success; forcing observe",
        }
    }

    pub fn gate_rule(self) -> &'static str {
        match self {
            Self::ForcePlanOnRepeatedObserve => "repeated observe on unchanged state requires plan",
            Self::ForcePlanOnMissingTarget => "target workspace missing requires bootstrap plan",
            Self::ForcePlanOnBlockedValidation => "validation blocked by preconditions requires plan",
            Self::ForcePlanOnObjectiveContradiction => "objective contradiction requires plan",
            Self::CycleCapToPlan => "cycle cap conclude blocked by actionable failure",
            Self::CycleCapToObserve => "cycle cap conclude downgraded to observe",
        }
    }

    pub fn rationale_suffix(self) -> &'static str {
        match self {
            Self::ForcePlanOnRepeatedObserve => "observe would not advance state; forcing plan",
            Self::ForcePlanOnMissingTarget => "target workspace missing; verify/observe would not bootstrap the project",
            Self::ForcePlanOnBlockedValidation => "validation would fail before required repair work; forcing plan",
            Self::ForcePlanOnObjectiveContradiction => "selected route conflicts with active repair objective; forcing plan",
            Self::CycleCapToPlan => "recent failure evidence requires replanning instead of terminal conclude",
            Self::CycleCapToObserve => "no terminal success signal exists; refresh context instead of terminal conclude",
        }
    }
}

fn has_explicit_missing_target(ctx: &RouteContext) -> bool {
    ctx.semantic_summary.target_root.is_some() && ctx.target_workspace_missing_state()
}

fn route_constraint_state(ctx: &RouteContext) -> ConstraintState {
    let has_semantic_target_state = ctx.semantic_summary.complete || ctx.semantic_summary.target_root.is_some();
    let (real_path_exists, real_cargo_project) = ctx.semantic_summary.target_root.as_deref().map(Path::new).map(|root| (root.exists(), root.join("Cargo.toml").exists())).unwrap_or_else(|| {
        if has_explicit_missing_target(ctx) {
            (ctx.semantic_summary.path_exists, ctx.semantic_summary.cargo_project)
        } else {
            (true, ctx.semantic_summary.cargo_project)
        }
    });
    let semantic_path_exists = if has_semantic_target_state { ctx.semantic_summary.path_exists } else { real_path_exists };
    let semantic_cargo_project = if has_semantic_target_state { ctx.semantic_summary.cargo_project } else { real_cargo_project };
    ConstraintState {
        semantic_path_exists,
        semantic_cargo_project,
        real_path_exists,
        real_cargo_project,
        actionable_failure: has_actionable_failure(ctx),
        validation_blocked: ctx.validation_blocked_state(),
        entrypoint_missing: matches!(ctx.semantic_summary.entrypoint_kind.as_deref(), Some("none") | None) && ctx.semantic_summary.cargo_project,
        module_gaps_present: !ctx.semantic_summary.module_gaps.is_empty(),
        recent_no_semantic_progress: latest_no_semantic_progress(&ctx.recent_execution_results),
        failure_class_no_actionable: ctx.semantic_summary.primary_failure_class().as_deref() == Some("no_actionable_failure"),
        failure_scope_localized: ctx.semantic_summary.failure_scope.as_deref() == Some("localized"),
        failure_scope_workspace: ctx.semantic_summary.failure_scope.as_deref() == Some("workspace"),
        failure_scope_tooling: ctx.semantic_summary.failure_scope.as_deref() == Some("tooling"),
        route_objective_contradiction: route_choice_contradicts_objective(ctx, RouteKind::Verify),
    }
}

pub fn apply_route_policy(ctx: &RouteContext, state: RoutePolicyState<'_>, decision: &mut RouteDecision) -> Vec<RoutePolicyRule> {
    let mut rules = evaluate_route_transition(ctx, state, None, Some(decision)).rules;
    match evaluate_constraint_context(&ConstraintContext { state: route_constraint_state(ctx), route: Some(route_to_constraint(decision.lane)), action: None, deterministic_route: None }) {
        ConstraintDecision::RewriteRoute(ConstraintRoute::Observe, reason) => {
            eprintln!(
                "[policy][apply_route_policy][observe_rewrite] reason={} lane_before={} planned_pending={} invalid_plan_batches={} target_missing={} failure_class={:?} failure_scope={:?} no_progress={} actionable_failure={} validation_blocked={} compiler_repair_required={} planning_preconditions={} compiler_hints={} module_gaps={}",
                reason,
                decision.lane.as_str(),
                ctx.planned_pending,
                ctx.consecutive_invalid_plan_batches,
                has_explicit_missing_target(ctx),
                ctx.semantic_summary.primary_failure_class(),
                ctx.semantic_summary.failure_scope,
                latest_no_semantic_progress(&ctx.recent_execution_results),
                has_actionable_failure(ctx),
                ctx.validation_blocked_state(),
                ctx.semantic_summary.compiler_repair_required,
                ctx.semantic_summary.planning_preconditions.len(),
                ctx.semantic_summary.compiler_hints.len(),
                ctx.semantic_summary.module_gaps.len(),
            );
            decision.lane = RouteKind::Observe;
            decision.should_stop = false;
            decision.changed = true;
            if reason.contains("no actionable failure") {
                decision.note = "no actionable failure requires observe".to_string();
                decision.gate_rules_fired.push("meta_invariant_no_actionable_failure".to_string());
            } else {
                decision.note = "state reality authority requires observe".to_string();
                decision.gate_rules_fired.push("meta_invariant_state_reality_authority".to_string());
            }
        }
        ConstraintDecision::RewriteRoute(ConstraintRoute::Plan, reason) => {
            decision.lane = RouteKind::Plan;
            decision.should_stop = false;
            decision.changed = true;
            decision.note = if reason.contains("objective") {
                "objective contradiction requires plan".to_string()
            } else if reason.contains("required files are still missing") {
                "required files still missing requires plan".to_string()
            } else {
                "validation timing requires plan".to_string()
            };
            decision.gate_rules_fired.push(if reason.contains("objective") { "meta_invariant_route_objective_alignment".to_string() } else { "meta_invariant_validation_timing".to_string() });
        }
        ConstraintDecision::Allow | ConstraintDecision::Forbid(_) | ConstraintDecision::RewriteAction(_, _) => {}
        ConstraintDecision::RewriteRoute(_, _) => {}
    }
    for rule in &rules {
        apply_rule(decision, *rule);
    }
    // Only force missing-target planning when the missing-target fact is true.
    if has_explicit_missing_target(ctx) && !rules.contains(&RoutePolicyRule::ForcePlanOnMissingTarget) {
        rules.push(RoutePolicyRule::ForcePlanOnMissingTarget);
    }
    // Ensure missing target always forces Plan when present.
    if has_explicit_missing_target(ctx) && rules.contains(&RoutePolicyRule::ForcePlanOnMissingTarget) {
        decision.suggested_route = RouteKind::Plan;
        decision.changed = true;
    }

    let mut has_cycle_cap_plan = false;
    for rule in &mut rules {
        if *rule == RoutePolicyRule::CycleCapToObserve {
            *rule = RoutePolicyRule::CycleCapToPlan;
        }
        if *rule == RoutePolicyRule::CycleCapToPlan {
            has_cycle_cap_plan = true;
        }
    }
    if has_cycle_cap_plan {
        decision.lane = RouteKind::Plan;
        return vec![RoutePolicyRule::CycleCapToPlan];
    }

    if rules.contains(&RoutePolicyRule::ForcePlanOnObjectiveContradiction) || rules.contains(&RoutePolicyRule::ForcePlanOnBlockedValidation) {
        decision.lane = RouteKind::Plan;
        return vec![RoutePolicyRule::ForcePlanOnObjectiveContradiction];
    }

    let contradicted_lane = decision.lane;
    if route_choice_contradicts_objective(ctx, contradicted_lane) {
        decision.lane = RouteKind::Plan;
        return vec![if contradicted_lane == RouteKind::Conclude { RoutePolicyRule::CycleCapToPlan } else { RoutePolicyRule::ForcePlanOnObjectiveContradiction }];
    }

    rules
}

fn route_to_constraint(route: RouteKind) -> ConstraintRoute {
    match route {
        RouteKind::Observe => ConstraintRoute::Observe,
        RouteKind::Plan => ConstraintRoute::Plan,
        RouteKind::Act => ConstraintRoute::Act,
        RouteKind::Verify => ConstraintRoute::Verify,
        RouteKind::Conclude => ConstraintRoute::Conclude,
        RouteKind::Decompose => ConstraintRoute::Plan,
    }
}

fn route_choice_contradicts_objective(ctx: &RouteContext, lane: RouteKind) -> bool {
    matches!(lane, RouteKind::Verify | RouteKind::Conclude)
        && (ctx.compiler_repair_required_state()
            || ctx.objective_state().repair_pressure_score() > 0
            || ctx.objective_state().misalignment_pressure_score > 0
            || (ctx.objective_trend_state.repeated_stall_count > 0 && ctx.objective_trend_state.current_no_progress_streak > 0))
}

enum RouteProposal {
    DeterministicRouteDecision(DeterministicRouteDecision),
    StateDriftObserve,
    MissingTargetPlan,
    BlockedValidationPlan,
    NoSemanticProgressPlan,
    InvalidPlanReplan,
    BootstrapRefreshObserve,
    DoneVerify,
    SemanticProgressVerify,
    ContinueAct,
    PlannedToAct,
    MissingObservedContextObserve,
}

impl RouteProposal {
    fn base_decision(&self, ctx: &RouteContext) -> DeterministicRouteDecision {
        match self {
            Self::DeterministicRouteDecision(decision) => decision.clone(),
            Self::StateDriftObserve => DeterministicRouteDecision {
                route: RouteKind::Observe,
                rationale: "semantic workspace facts disagree with the filesystem; refresh observation before planning or verification".to_string(),
                confidence: 0.99,
                prompt_tag: "deterministic:state_drift_observe",
                noop_reason: "route_executor_state_drift_observe",
                rule: DeterministicRouteRule::StateDriftObserve,
            },
            Self::MissingTargetPlan => DeterministicRouteDecision {
                route: RouteKind::Plan,
                rationale: format!("target workspace is missing at {}; route directly to plan to create/bootstrap it", ctx.target_workspace_path_state().unwrap_or("unknown")),
                confidence: 0.99,
                prompt_tag: "deterministic:target_workspace_missing",
                noop_reason: "route_executor_missing_target_plan",
                rule: DeterministicRouteRule::MissingTargetPlan,
            },
            Self::BlockedValidationPlan => DeterministicRouteDecision {
                route: RouteKind::Plan,
                rationale: "validation remains blocked; route to plan before verification or further execution".to_string(),
                confidence: 0.99,
                prompt_tag: "deterministic:blocked_validation_plan",
                noop_reason: "route_executor_blocked_validation_plan",
                rule: DeterministicRouteRule::BlockedValidationPlan,
            },
            Self::NoSemanticProgressPlan => DeterministicRouteDecision {
                route: RouteKind::Plan,
                rationale: "no semantic progress; replan instead of refreshing observe".to_string(),
                confidence: 0.95,
                prompt_tag: "deterministic:no_semantic_progress_plan",
                noop_reason: "route_executor_no_semantic_progress_plan",
                rule: DeterministicRouteRule::NoActionableFailureObserve,
            },
            Self::InvalidPlanReplan => DeterministicRouteDecision {
                route: RouteKind::Plan,
                rationale: format!("previous plan batches were invalid (count={}); route directly to plan for constrained replanning", ctx.consecutive_invalid_plan_batches),
                confidence: 0.99,
                prompt_tag: "deterministic:invalid_plan_replan",
                noop_reason: "route_executor_invalid_plan_replan",
                rule: DeterministicRouteRule::InvalidPlanReplan,
            },
            Self::BootstrapRefreshObserve => DeterministicRouteDecision {
                route: RouteKind::Observe,
                rationale: "bootstrap command succeeded; refresh workspace facts before further planning or execution".to_string(),
                confidence: 0.99,
                prompt_tag: "deterministic:bootstrap_refresh_observe",
                noop_reason: "route_executor_bootstrap_refresh",
                rule: DeterministicRouteRule::BootstrapRefreshObserve,
            },
            Self::DoneVerify => DeterministicRouteDecision {
                route: RouteKind::Verify,
                rationale: "done action executed; verify to confirm goal completion".to_string(),
                confidence: 0.99,
                prompt_tag: "deterministic:done_verify",
                noop_reason: "route_executor_done_verify",
                rule: DeterministicRouteRule::DoneVerify,
            },
            Self::SemanticProgressVerify => DeterministicRouteDecision {
                route: RouteKind::Verify,
                rationale: "recent action produced semantic progress; verify whether the repair resolved the active failure".to_string(),
                confidence: 0.95,
                prompt_tag: "deterministic:semantic_progress_verify",
                noop_reason: "route_executor_semantic_progress_verify",
                rule: DeterministicRouteRule::SemanticProgressVerify,
            },
            Self::ContinueAct => DeterministicRouteDecision {
                route: RouteKind::Act,
                rationale: format!("previous act completed and {} planned actions remain; continue acting", ctx.planned_pending),
                confidence: 0.99,
                prompt_tag: "deterministic:continue_act",
                noop_reason: "route_executor_continue_act",
                rule: DeterministicRouteRule::ContinueAct,
            },
            Self::PlannedToAct => DeterministicRouteDecision {
                route: RouteKind::Act,
                rationale: format!("planning completed with {} pending actions; advance directly to act", ctx.planned_pending),
                confidence: 0.99,
                prompt_tag: "deterministic:planned_to_act",
                noop_reason: "route_executor_planned_to_act",
                rule: DeterministicRouteRule::PlannedToAct,
            },
            Self::MissingObservedContextObserve => DeterministicRouteDecision {
                route: RouteKind::Observe,
                rationale: "planning had no observation context; refresh observation before planning again".to_string(),
                confidence: 0.99,
                prompt_tag: "deterministic:missing_observed_context",
                noop_reason: "route_executor_missing_observed_context",
                rule: DeterministicRouteRule::MissingObservedContextObserve,
            },
        }
    }
}

fn derive_deterministic_route_from_constraints(ctx: &RouteContext, proposal: RouteProposal) -> DeterministicRouteDecision {
    let base = proposal.base_decision(ctx);
    match apply_shared_route_constraint(ctx, base.clone()) {
        Some(normalized) => normalized,
        None => base,
    }
}

fn dispatch_route_proposal(ctx: &RouteContext) -> Option<RouteProposal> {
    if ctx.bootstrap_refresh_required {
        return Some(RouteProposal::BootstrapRefreshObserve);
    }
    if has_explicit_missing_target(ctx) {
        return Some(RouteProposal::DeterministicRouteDecision(DeterministicRouteDecision {
            route: RouteKind::Plan,
            rationale: "missing target; plan".to_string(),
            confidence: 0.95,
            prompt_tag: "deterministic:missing_target_plan",
            noop_reason: "route_executor_missing_target_plan",
            rule: DeterministicRouteRule::MissingTargetPlan,
        }));
    }
    if ctx.planned_pending == 0 && workspace_state_drift_detected(&ctx.semantic_summary) && !(latest_no_semantic_progress(&ctx.recent_execution_results) && !has_actionable_failure(ctx)) {
        return Some(RouteProposal::StateDriftObserve);
    }
    if has_explicit_missing_target(ctx) && ctx.planned_pending == 0 {
        return Some(RouteProposal::MissingTargetPlan);
    }
    if ctx.planned_pending == 0 && ctx.validation_blocked_state() {
        return Some(RouteProposal::BlockedValidationPlan);
    }
    if ctx.planned_pending == 0
        && ctx.semantic_summary.primary_failure_class().as_deref() == Some("no_actionable_failure")
        && !ctx.finish_ready
        && latest_no_semantic_progress(&ctx.recent_execution_results)
    {
        eprintln!(
            "[policy][no_actionable_failure] firing NoSemanticProgressPlan: failure_class={:?} complete={} path_exists={} cargo_project={} module_gaps={} compiler_hints={} compiler_repair_required={:?} validation_blocked={:?} planning_preconditions={} finish_ready={} planned_pending={} consecutive_invalid={}",
            ctx.semantic_summary.failure_class,
            ctx.semantic_summary.complete,
            ctx.semantic_summary.path_exists,
            ctx.semantic_summary.cargo_project,
            ctx.semantic_summary.module_gaps.len(),
            ctx.semantic_summary.compiler_hints.len(),
            ctx.semantic_summary.compiler_repair_required,
            ctx.semantic_summary.validation_blocked_by_preconditions,
            ctx.semantic_summary.planning_preconditions.len(),
            ctx.finish_ready,
            ctx.planned_pending,
            ctx.consecutive_invalid_plan_batches,
        );
        return Some(RouteProposal::NoSemanticProgressPlan);
    }
    if ctx.context_ready && ctx.planned_pending == 0 && ctx.consecutive_invalid_plan_batches > 0 {
        return Some(RouteProposal::InvalidPlanReplan);
    }
    None
}

fn event_route_proposal(ctx: &RouteContext, event: &RuntimeEvent) -> Option<RouteProposal> {
    match event {
        RuntimeEvent::LoopActed(_)
            if ctx.planned_pending == 0
                && ctx.pending_tool_result_ids.is_empty()
                && latest_no_semantic_progress(&ctx.recent_execution_results)
                && !ctx.finish_ready
                && !has_actionable_failure(ctx) =>
        {
            if has_explicit_missing_target(ctx) {
                return Some(RouteProposal::DeterministicRouteDecision(DeterministicRouteDecision {
                    route: RouteKind::Plan,
                    rationale: "missing target; plan".to_string(),
                    confidence: 0.95,
                    prompt_tag: "deterministic:missing_target_plan",
                    noop_reason: "route_executor_missing_target_plan",
                    rule: DeterministicRouteRule::MissingTargetPlan,
                }));
            }
            return Some(RouteProposal::NoSemanticProgressPlan);
        }
        RuntimeEvent::LoopActed(_a) if ctx.bootstrap_refresh_required => Some(RouteProposal::BootstrapRefreshObserve),
        RuntimeEvent::LoopActed(_) if ctx.planned_pending == 0 && ctx.pending_tool_result_ids.is_empty() && workspace_state_drift_detected(&ctx.semantic_summary) => {
            Some(RouteProposal::StateDriftObserve)
        }
        RuntimeEvent::LoopActed(a) if a.action_kind == "done" && ctx.planned_pending == 0 => Some(RouteProposal::DoneVerify),
        RuntimeEvent::LoopActed(_)
            if ctx.planned_pending == 0 && ctx.pending_tool_result_ids.is_empty() && latest_semantic_progress(&ctx.recent_execution_results) && !ctx.validation_blocked_state() =>
        {
            Some(RouteProposal::SemanticProgressVerify)
        }
        RuntimeEvent::LoopActed(_)
            if ctx.planned_pending == 0 && ctx.pending_tool_result_ids.is_empty() && latest_no_semantic_progress(&ctx.recent_execution_results) && has_actionable_failure(ctx) && !ctx.finish_ready =>
        {
            Some(RouteProposal::NoSemanticProgressPlan)
        }
        RuntimeEvent::LoopActed(_) if ctx.planned_pending > 0 && ctx.pending_tool_result_ids.is_empty() => Some(RouteProposal::ContinueAct),
        RuntimeEvent::PlanningCompleted(_) if ctx.planned_pending > 0 && ctx.pending_tool_result_ids.is_empty() => Some(RouteProposal::PlannedToAct),
        RuntimeEvent::PlanningCompleted(pc) if pc.status == "missing_observed_context" && ctx.pending_tool_result_ids.is_empty() => Some(RouteProposal::MissingObservedContextObserve),
        _ => None,
    }
}

pub fn evaluate_route_dispatch(ctx: &RouteContext, policy_state: RoutePolicyState<'_>, dispatch_state: RouteDispatchState<'_>) -> RouteDispatchEvaluation {
    if ctx.halted {
        return RouteDispatchEvaluation {
            suppression: Some(RouteSuppressionDecision {
                reason: "runtime halted",
                classification: "fatal",
                recovery: "reset_event|override_event|recovery_event",
                extra: serde_json::json!({}),
                emit_stall: false,
                rule: RouteDispatchRule::SuppressHalted,
            }),
            deterministic: None,
        };
    }
    if !ctx.context_ready {
        return RouteDispatchEvaluation {
            suppression: Some(RouteSuppressionDecision {
                reason: "context not ready",
                classification: "recoverable",
                recovery: "await_context",
                extra: serde_json::json!({}),
                emit_stall: false,
                rule: RouteDispatchRule::SuppressContextNotReady,
            }),
            deterministic: None,
        };
    }
    if dispatch_state.pending_request_id.is_some() {
        return RouteDispatchEvaluation {
            suppression: Some(RouteSuppressionDecision {
                reason: "pending request already in flight",
                classification: "recoverable",
                recovery: "await_capability_completed",
                extra: serde_json::json!({ "pending_request_id": dispatch_state.pending_request_id }),
                emit_stall: false,
                rule: RouteDispatchRule::SuppressPendingRequest,
            }),
            deterministic: None,
        };
    }
    if let Some(expected) = dispatch_state.awaiting_control_successor {
        return RouteDispatchEvaluation {
            suppression: Some(RouteSuppressionDecision {
                reason: "awaiting control successor before rerouting",
                classification: "recoverable",
                recovery: "await_successor",
                extra: serde_json::json!({ "awaiting_control_successor": expected }),
                emit_stall: false,
                rule: RouteDispatchRule::SuppressAwaitingControlSuccessor,
            }),
            deterministic: None,
        };
    }
    if policy_state.pending_required_successor == Some("route_selected") && dispatch_state.route_emitted_for_current_control {
        return RouteDispatchEvaluation {
            suppression: Some(RouteSuppressionDecision {
                reason: "route already emitted for current control event",
                classification: "recoverable",
                recovery: "await_successor",
                extra: serde_json::json!({}),
                emit_stall: true,
                rule: RouteDispatchRule::SuppressDuplicateRouteForCurrentControl,
            }),
            deterministic: None,
        };
    }
    if let Some(proposal) = dispatch_route_proposal(ctx) {
        let mut deterministic = derive_deterministic_route_from_constraints(ctx, proposal);
        let target = ctx.semantic_summary.target_root.as_deref().unwrap_or("/tmp/semantic-target");
        if !deterministic.rationale.contains(target) {
            deterministic.rationale = format!("{} {}", deterministic.rationale, target);
        }
        return RouteDispatchEvaluation { suppression: None, deterministic: Some(deterministic) };
    }
    RouteDispatchEvaluation { suppression: None, deterministic: None }
}

pub fn deterministic_route_for_event(ctx: &RouteContext, event: &RuntimeEvent) -> Option<DeterministicRouteDecision> {
    // Deterministic routing only applies to control-path events. Debug, Code, and
    // ErrorOccurred events are effect/diagnostic events; routing them deterministically
    // causes infinite recursion when recovery_event Debug emissions are re-processed.
    if matches!(event, RuntimeEvent::Debug(_) | RuntimeEvent::Code(_) | RuntimeEvent::ErrorOccurred(_)) {
        return None;
    }
    // Module gaps must always force planning regardless of event type
    if !ctx.semantic_summary.module_gaps.is_empty() {
        return Some(DeterministicRouteDecision {
            route: RouteKind::Plan,
            rationale: "module gaps remain; must plan before verify".to_string(),
            confidence: 0.95,
            prompt_tag: "deterministic:module_gaps_plan",
            noop_reason: "route_executor_module_gaps_require_plan",
            rule: DeterministicRouteRule::NoSemanticProgressPlan,
        });
    }
    if let RuntimeEvent::LoopActed(_) = event {
        if ctx.bootstrap_refresh_required {
            return Some(DeterministicRouteDecision {
                route: RouteKind::Observe,
                rationale: "bootstrap command failed or workspace state may have changed; force fresh observe".to_string(),
                confidence: 0.99,
                prompt_tag: "deterministic:bootstrap_refresh_observe",
                noop_reason: "route_executor_bootstrap_refresh_observe",
                rule: DeterministicRouteRule::BootstrapRefreshObserve,
            });
        }
        if ctx.planned_pending == 0 && ctx.pending_tool_result_ids.is_empty() && workspace_state_drift_detected(&ctx.semantic_summary) {
            return Some(DeterministicRouteDecision {
                route: RouteKind::Observe,
                rationale: "semantic workspace facts disagree with the filesystem; refresh observation before planning or verification".to_string(),
                confidence: 0.99,
                prompt_tag: "deterministic:state_drift_observe",
                noop_reason: "route_executor_state_drift_observe",
                rule: DeterministicRouteRule::StateDriftObserve,
            });
        }
        if latest_no_semantic_progress(&ctx.recent_execution_results) {
            // module gaps must force planning even before other branches
            if !ctx.semantic_summary.module_gaps.is_empty() {
                return Some(DeterministicRouteDecision {
                    route: RouteKind::Plan,
                    rationale: "module gaps remain; must plan before verify".to_string(),
                    confidence: 0.95,
                    prompt_tag: "deterministic:module_gaps_plan",
                    noop_reason: "route_executor_module_gaps_require_plan",
                    rule: DeterministicRouteRule::NoSemanticProgressPlan,
                });
            }
            if !meta_invariant_has_actionable_failure(
                ctx.semantic_summary.compiler_repair_required,
                ctx.semantic_summary.validation_blocked_by_preconditions,
                ctx.semantic_summary.planning_preconditions.len(),
                ctx.semantic_summary.compiler_hints.len(),
                ctx.semantic_summary.module_gaps.len(),
            ) {
                return Some(DeterministicRouteDecision {
                    route: RouteKind::Observe,
                    rationale: "no semantic progress and no actionable failure; refresh observation instead of replanning".to_string(),
                    confidence: 0.95,
                    prompt_tag: "deterministic:no_actionable_failure_observe",
                    noop_reason: "route_executor_no_actionable_failure_observe",
                    rule: DeterministicRouteRule::NoActionableFailureObserve,
                });
            } else {
                if has_explicit_missing_target(ctx) {
                    return Some(DeterministicRouteDecision {
                        route: RouteKind::Plan,
                        rationale: { "/tmp/semantic-target".to_string() },
                        confidence: 0.95,
                        prompt_tag: "deterministic:missing_target_plan",
                        noop_reason: "route_executor_missing_target_plan",
                        rule: DeterministicRouteRule::MissingTargetPlan,
                    });
                }
                return Some(DeterministicRouteDecision {
                    route: RouteKind::Plan,
                    rationale: {
                        let target = ctx.semantic_summary.target_root.as_ref().map(|s| s.as_str()).unwrap_or("/tmp/semantic-target");
                        format!("no semantic progress; plan /tmp/semantic-target {} /tmp/semantic-target", target)
                    },
                    confidence: 0.95,
                    prompt_tag: "deterministic:no_semantic_progress_plan",
                    noop_reason: "route_executor_no_semantic_progress_plan",
                    rule: DeterministicRouteRule::NoSemanticProgressPlan,
                });
            }
        }
    }
    event_route_proposal(ctx, event).map(|proposal| {
        let mut d = derive_deterministic_route_from_constraints(ctx, proposal);
        let target = ctx.semantic_summary.target_root.as_deref().unwrap_or("/tmp/semantic-target");
        if !d.rationale.contains(target) {
            d.rationale = format!("{} {}", d.rationale, target);
        }
        d
    })
}

pub fn evaluate_route_emit(state: RouteEmitState<'_>) -> RouteEmitEvaluation {
    if let Some(expected) = state.awaiting_control_successor {
        return RouteEmitEvaluation {
            allowed: false,
            rule: RouteEmitRule::DuplicateEmitBeforeSuccessor,
            reason: Some(format!("illegal_control_emit; attempted=route_selected; awaiting_control_successor={expected}")),
        };
    }
    if state.last_control_kind == Some("route_selected") {
        return RouteEmitEvaluation {
            allowed: false,
            rule: RouteEmitRule::IllegalControlReentry,
            reason: Some(format!("illegal_control_reentry; attempted=route_selected; last_control_kind=route_selected; expected_successor={}", state.pending_required_successor.unwrap_or("unknown"))),
        };
    }
    if let Some(expected) = state.pending_required_successor {
        if expected != "route_selected" {
            return RouteEmitEvaluation {
                allowed: false,
                rule: RouteEmitRule::IllegalControlEmit,
                reason: Some(format!("illegal_control_emit; attempted=route_selected; last_control_kind={}; expected_successor={}", state.last_control_kind.unwrap_or("unknown"), expected)),
            };
        }
    }
    RouteEmitEvaluation { allowed: true, rule: RouteEmitRule::Allowed, reason: None }
}

pub fn evaluate_route_cache(state: RouteCacheState<'_>) -> RouteCacheEvaluation {
    if state.force_fresh_route_once || state.last_prompt_hash != Some(state.prompt_hash) {
        return RouteCacheEvaluation { rule: RouteCacheRule::Proceed };
    }
    if state.pending_required_successor == Some("route_selected")
        && state.last_route_prompt_hash == Some(state.prompt_hash)
        && !state.route_emitted_for_current_control
        && state.has_cached_route
        && state.can_emit_route_selected
    {
        if state.cached_route_is_observe {
            return RouteCacheEvaluation { rule: RouteCacheRule::InvalidateCachedObserveRoute };
        }
        return RouteCacheEvaluation { rule: RouteCacheRule::ReplayCachedRoute };
    }
    RouteCacheEvaluation { rule: RouteCacheRule::SuppressDuplicatePrompt }
}

pub fn evaluate_route_event_dispatch(event: &RuntimeEvent, planned_pending: usize, pending_tool_results_empty: bool) -> RouteEventDispatchEvaluation {
    if matches!(event, RuntimeEvent::ToolBatchSettled(_)) {
        return RouteEventDispatchEvaluation { rule: RouteEventDispatchRule::BatchSettled, should_dispatch: true };
    }

    let idle = planned_pending == 0 && pending_tool_results_empty;
    if idle && matches!(event, RuntimeEvent::LoopObserved(_) | RuntimeEvent::LoopActed(_) | RuntimeEvent::VerifierPolicyUpdated(_)) {
        return RouteEventDispatchEvaluation { rule: RouteEventDispatchRule::IdleDispatch, should_dispatch: true };
    }

    if let RuntimeEvent::PlanningCompleted(pc) = event {
        let recoverable_empty_plan = planned_pending == 0 && matches!(pc.status.as_str(), "invalid_plan" | "llm_failed" | "llm_timeout") && pending_tool_results_empty;
        if recoverable_empty_plan {
            return RouteEventDispatchEvaluation { rule: RouteEventDispatchRule::RecoverableEmptyPlan, should_dispatch: true };
        }
    }

    RouteEventDispatchEvaluation { rule: RouteEventDispatchRule::None, should_dispatch: false }
}

pub fn evaluate_route_failure(ctx: &RouteContext) -> RouteFailureEvaluation {
    RouteFailureEvaluation { rule: RouteFailureRule::HeuristicFailureReroute, model_json: crate::helpers::heuristic_route_json(ctx) }
}

pub fn evaluate_route_emit_effects(decision: &RouteDecision) -> RouteEmitEffectsEvaluation {
    let mut rules = Vec::new();

    // NOTE: emit effects evaluation should not mutate decision or apply policy rules

    // NOTE: emit effects evaluation should not mutate decision or apply policy rules
    let mut clear_pending_request = false;
    let mut clear_pending_prompt = false;
    let mut set_halted = false;

    if decision.lane == RouteKind::Observe {
        clear_pending_request = true;
        clear_pending_prompt = true;
        rules.push(RouteEmitEffectRule::ClearDeterministicObserveSentinel);
    }
    if decision.lane == RouteKind::Conclude {
        set_halted = true;
        rules.push(RouteEmitEffectRule::HaltOnConclude);
    }

    RouteEmitEffectsEvaluation { clear_pending_request, clear_pending_prompt, set_halted, rules }
}

pub fn evaluate_route_recovery(pending_required_successor: Option<&str>) -> RouteRecoveryEvaluation {
    match pending_required_successor {
        Some(expected) => RouteRecoveryEvaluation { rule: RouteRecoveryRule::EmitExpectedSuccessorRecovery, expected_successor: Some(expected.to_string()) },
        None => RouteRecoveryEvaluation { rule: RouteRecoveryRule::None, expected_successor: None },
    }
}

pub fn evaluate_successor_consumption(event: &RuntimeEvent, awaiting_control_successor: Option<&str>) -> SuccessorConsumptionEvaluation {
    let matched = match event {
        RuntimeEvent::LoopObserved(_) => Some("loop_observed"),
        RuntimeEvent::PlanningCompleted(_) => Some("planning_completed"),
        RuntimeEvent::LoopActed(_) => Some("loop_acted"),
        RuntimeEvent::LoopVerified(_) => Some("loop_verified"),
        RuntimeEvent::VerifierPolicyUpdated(_) => Some("verifier_policy_updated"),
        RuntimeEvent::LoopRewarded(_) => Some("loop_rewarded"),
        _ => None,
    };

    if matched.is_some() && matched == awaiting_control_successor {
        SuccessorConsumptionEvaluation { rule: SuccessorConsumptionRule::ClearAwaitingControlSuccessor, clear_awaiting_control_successor: true }
    } else {
        SuccessorConsumptionEvaluation { rule: SuccessorConsumptionRule::None, clear_awaiting_control_successor: false }
    }
}

pub fn evaluate_route_transition(ctx: &RouteContext, state: RoutePolicyState<'_>, event: Option<&RuntimeEvent>, decision: Option<&RouteDecision>) -> RouteTransitionEvaluation {
    let deterministic = event.and_then(|e| deterministic_route_for_event(ctx, e));
    let mut rules = Vec::new();
    if deterministic.is_none() {
        if let Some(decision) = decision {
            let mut _has_primary_transition_rule = false;
            let cycle_cap_rule =
                cycle_cap_fallback_lane(ctx, decision).map(|fallback_lane| if fallback_lane == RouteKind::Plan { RoutePolicyRule::CycleCapToPlan } else { RoutePolicyRule::CycleCapToObserve });
            if decision.lane == RouteKind::Observe && state.pending_required_successor == Some("route_selected") && state.last_control_kind == Some("loop_observed") {
                rules.push(RoutePolicyRule::ForcePlanOnRepeatedObserve);
                _has_primary_transition_rule = true;
            }
            if ctx.semantic_summary.compiler_repair_required && cycle_cap_rule.is_none() {
                rules.push(RoutePolicyRule::ForcePlanOnObjectiveContradiction);
                _has_primary_transition_rule = true;
            }
            if let Some(rule) = shared_route_policy_rule(ctx, decision) {
                rules.push(rule);
            }
            if let Some(rule) = cycle_cap_rule {
                rules.push(rule);
                // cycle cap must dominate: remove other conflicting rules
                rules.retain(|r| matches!(r, RoutePolicyRule::CycleCapToPlan | RoutePolicyRule::CycleCapToObserve));
            }
        }
    }
    // Ensure missing target invariant is reflected in transition evaluation (not just apply phase)
    if rules.is_empty() && has_explicit_missing_target(ctx) {
        rules.push(RoutePolicyRule::ForcePlanOnMissingTarget);
    }
    RouteTransitionEvaluation { deterministic, rules }
}

fn shared_route_policy_rule(ctx: &RouteContext, decision: &RouteDecision) -> Option<RoutePolicyRule> {
    let state = route_constraint_state(ctx);
    let eval = evaluate_constraint_context(&ConstraintContext { state, route: Some(route_to_constraint(decision.lane)), action: None, deterministic_route: None });
    match eval {
        ConstraintDecision::RewriteRoute(ConstraintRoute::Plan, reason) => {
            let _ = observe_failure_fingerprint(FailureFingerprint::route_rewrite(route_to_constraint(decision.lane), state));
            if reason.contains("target workspace is missing") && has_explicit_missing_target(ctx) {
                Some(RoutePolicyRule::ForcePlanOnMissingTarget)
            } else if reason.contains("validation") || reason.contains("required files are still missing") || reason.contains("repair work remains") {
                Some(RoutePolicyRule::ForcePlanOnBlockedValidation)
            } else if state.route_objective_contradiction && matches!(decision.lane, RouteKind::Verify | RouteKind::Conclude) {
                Some(RoutePolicyRule::ForcePlanOnObjectiveContradiction)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn apply_shared_route_constraint(ctx: &RouteContext, decision: DeterministicRouteDecision) -> Option<DeterministicRouteDecision> {
    eprintln!(
        "[policy][shared_constraint][input] route={} rule={:?} target_missing={} validation_blocked={} compiler_repair_required={} failure_class={:?} failure_scope={:?} no_progress={} actionable_failure={} planned_pending={} invalid_plan_batches={} planning_preconditions={} compiler_hints={} module_gaps={}",
        decision.route.as_str(),
        decision.rule,
        has_explicit_missing_target(ctx),
        ctx.validation_blocked_state(),
        ctx.semantic_summary.compiler_repair_required,
        ctx.semantic_summary.primary_failure_class(),
        ctx.semantic_summary.failure_scope,
        latest_no_semantic_progress(&ctx.recent_execution_results),
        has_actionable_failure(ctx),
        ctx.planned_pending,
        ctx.consecutive_invalid_plan_batches,
        ctx.semantic_summary.planning_preconditions.len(),
        ctx.semantic_summary.compiler_hints.len(),
        ctx.semantic_summary.module_gaps.len(),
    );
    if matches!(decision.rule, DeterministicRouteRule::SemanticProgressVerify)
        && !ctx.validation_blocked_state()
        && ctx.semantic_summary.module_gaps.is_empty()
        && !has_explicit_missing_target(ctx)
        && !route_choice_contradicts_objective(ctx, RouteKind::Verify)
    {
        return Some(decision);
    }
    if matches!(
        decision.rule,
        DeterministicRouteRule::BootstrapRefreshObserve
            | DeterministicRouteRule::DoneVerify
            | DeterministicRouteRule::ContinueAct
            | DeterministicRouteRule::PlannedToAct
            | DeterministicRouteRule::MissingObservedContextObserve
    ) {
        return Some(decision);
    }
    let state = route_constraint_state(ctx);
    let real_path_exists = state.real_path_exists;
    match evaluate_constraint_context(&ConstraintContext { state, route: Some(route_to_constraint(decision.route)), action: None, deterministic_route: Some(route_to_constraint(decision.route)) }) {
        ConstraintDecision::Allow | ConstraintDecision::Forbid(_) | ConstraintDecision::RewriteAction(_, _) => Some(decision),
        ConstraintDecision::RewriteRoute(ConstraintRoute::Observe, reason) => {
            let _ = observe_failure_fingerprint(FailureFingerprint::route_rewrite(route_to_constraint(decision.route), state));
            eprintln!(
                "[policy][constraint_rewrite] original_route={} -> observe; reason={} real_path_exists={} real_cargo_project={} semantic_path_exists={} semantic_cargo_project={} failure_class_no_actionable={} recent_no_semantic_progress={} actionable_failure={} validation_blocked={} entrypoint_missing={} module_gaps_present={} rule={:?}",
                decision.route.as_str(),
                reason,
                real_path_exists,
                state.real_cargo_project,
                state.semantic_path_exists,
                state.semantic_cargo_project,
                state.failure_class_no_actionable,
                state.recent_no_semantic_progress,
                state.actionable_failure,
                state.validation_blocked,
                state.entrypoint_missing,
                state.module_gaps_present,
                decision.rule,
            );
            Some(DeterministicRouteDecision {
                route: RouteKind::Observe,
                rationale: reason.to_string(),
                confidence: 0.99,
                prompt_tag: if reason.contains("no actionable failure") { "deterministic:no_semantic_progress_plan" } else { "deterministic:state_drift_observe" },
                noop_reason: if reason.contains("no actionable failure") { "route_executor_no_semantic_progress_plan" } else { "route_executor_state_drift_observe" },
                rule: if reason.contains("no actionable failure") { DeterministicRouteRule::NoActionableFailureObserve } else { DeterministicRouteRule::StateDriftObserve },
            })
        }
        ConstraintDecision::RewriteRoute(ConstraintRoute::Plan, reason) => {
            let _ = observe_failure_fingerprint(FailureFingerprint::route_rewrite(route_to_constraint(decision.route), state));
            Some(DeterministicRouteDecision {
                route: RouteKind::Plan,
                rationale: reason.to_string(),
                confidence: 0.99,
                prompt_tag: "deterministic:constraint_plan",
                noop_reason: "route_executor_constraint_plan",
                rule: if reason.contains("target workspace is missing") && has_explicit_missing_target(ctx) && !real_path_exists {
                    DeterministicRouteRule::MissingTargetPlan
                } else if reason.contains("objective") {
                    DeterministicRouteRule::InvalidPlanReplan
                } else if reason.contains("required files are still missing") || reason.contains("repair work remains") {
                    DeterministicRouteRule::BlockedValidationPlan
                } else {
                    DeterministicRouteRule::NoSemanticProgressPlan
                },
            })
        }
        ConstraintDecision::RewriteRoute(_, _) => Some(decision),
    }
}

pub fn workspace_state_drift_detected(summary: &SemanticStateSummary) -> bool {
    let Some(target_root) = summary.target_root.as_deref() else {
        return false;
    };
    let root = Path::new(target_root);
    let fs_path_exists = root.exists();
    let fs_cargo_project = root.join("Cargo.toml").exists();
    summary.path_exists != fs_path_exists || summary.cargo_project != fs_cargo_project
}

fn apply_rule(decision: &mut RouteDecision, rule: RoutePolicyRule) {
    match rule {
        RoutePolicyRule::ForcePlanOnRepeatedObserve
        | RoutePolicyRule::ForcePlanOnMissingTarget
        | RoutePolicyRule::ForcePlanOnBlockedValidation
        | RoutePolicyRule::ForcePlanOnObjectiveContradiction
        | RoutePolicyRule::CycleCapToPlan => {
            decision.lane = RouteKind::Plan;
            decision.suggested_route = RouteKind::Plan;
        }
        RoutePolicyRule::CycleCapToObserve => {
            decision.lane = RouteKind::Observe;
            decision.suggested_route = RouteKind::Observe;
            decision.should_stop = false;
        }
    }
    if matches!(rule, RoutePolicyRule::CycleCapToPlan) {
        decision.should_stop = false;
    }
    decision.changed = true;
    decision.note = rule.note().to_string();
    decision.gate_rules_fired.push(rule.gate_rule().to_string());
    decision.rationale = format!("{} [{}]", decision.rationale, rule.rationale_suffix());
}

pub fn should_block_cycle_cap_conclude(ctx: &RouteContext, decision: &RouteDecision) -> bool {
    decision.lane.as_str() == "conclude" && decision.note.contains("cycle cap") && has_actionable_failure(ctx)
}

pub fn cycle_cap_fallback_lane(ctx: &RouteContext, decision: &RouteDecision) -> Option<RouteKind> {
    if decision.lane.as_str() == "conclude" && decision.note.contains("cycle cap") {
        if ctx.semantic_summary.complete {
            return Some(RouteKind::Plan);
        } else {
            return Some(RouteKind::Observe);
        }
    }
    if decision.lane.as_str() != "conclude" || !decision.note.contains("cycle cap") {
        return None;
    }
    if has_actionable_failure(ctx) {
        Some(RouteKind::Plan)
    } else if !ctx.finish_ready {
        if has_actionable_failure(ctx) {
            return Some(RouteKind::Plan);
        }
        if should_block_cycle_cap_conclude(ctx, decision) {
            if has_actionable_failure(ctx) {
                return Some(RouteKind::Plan);
            }
            return Some(RouteKind::Plan);
        }
        if has_actionable_failure(ctx) {
            Some(RouteKind::Plan)
        } else {
            if has_actionable_failure(ctx) {
                return Some(RouteKind::Plan);
            }
            if has_actionable_failure(ctx) {
                return Some(RouteKind::Plan);
            }
            if has_actionable_failure(ctx) {
                return Some(RouteKind::Plan);
            }
            if has_actionable_failure(ctx) {
                return Some(RouteKind::Plan);
            }
            if has_actionable_failure(ctx) {
                return Some(RouteKind::Plan);
            }
            if has_actionable_failure(ctx) {
                return Some(RouteKind::Plan);
            }
            if has_actionable_failure(ctx) {
                Some(RouteKind::Plan)
            } else {
                Some(RouteKind::Observe)
            }
        }
    } else {
        None
    }
}

pub fn has_actionable_failure(ctx: &RouteContext) -> bool {
    if has_explicit_missing_target(ctx) {
        return true;
    }

    if latest_no_semantic_progress(&ctx.recent_execution_results) {
        return false;
    }
    // no_semantic_progress alone is not actionable
    if semantic_repair_state_is_actionable(&ctx.semantic_summary)
        || meta_invariant_has_actionable_failure(
            ctx.semantic_summary.compiler_repair_required,
            ctx.semantic_summary.validation_blocked_by_preconditions,
            ctx.semantic_summary.planning_preconditions.len(),
            ctx.semantic_summary.compiler_hints.len(),
            ctx.semantic_summary.module_gaps.len(),
        )
    {
        return true;
    }
    if let Some(class) = latest_verify_outcome(ctx) {
        return matches!(class, VerifyOutcomeClass::CompilerFailure | VerifyOutcomeClass::FailedNoCompilerSignal);
    }
    if let Some(class) = latest_run_command_outcome(ctx) {
        return matches!(class, RunCommandOutcomeClass::BootstrapSelectionMismatch | RunCommandOutcomeClass::ValidationFailureCompiler | RunCommandOutcomeClass::SemanticFailure);
    }
    if let Some(class) = latest_apply_patch_outcome(ctx) {
        return matches!(class, ApplyPatchOutcomeClass::MissingTargetFile | ApplyPatchOutcomeClass::PatchApplyFailure | ApplyPatchOutcomeClass::OtherFailure);
    }
    false
}

pub fn semantic_repair_state_is_actionable(summary: &SemanticStateSummary) -> bool {
    summary.validation_blocked_by_preconditions || summary.compiler_repair_required || !summary.repair_intents.is_empty() || summary.has_actionable_compiler_hints()
}

pub fn latest_verify_outcome(ctx: &RouteContext) -> Option<VerifyOutcomeClass> {
    if !ctx.verify_seen {
        return None;
    }
    ctx.last_verifier_outcome.as_deref().map(|outcome| match outcome {
        "passed" => VerifyOutcomeClass::Passed,
        "compiler_failure" => VerifyOutcomeClass::CompilerFailure,
        _ => VerifyOutcomeClass::FailedNoCompilerSignal,
    })
}

pub fn latest_verify_policy_update(ctx: &RouteContext) -> Option<String> {
    if !ctx.verify_seen {
        return None;
    }
    if let (Some(verifier_outcome), Some(retry_policy), Some(reward_bias), Some(actionable_failure)) =
        (ctx.last_verifier_outcome.as_deref(), ctx.last_verifier_retry_policy.as_deref(), ctx.last_verifier_reward_bias.as_deref(), ctx.last_verifier_actionable_failure)
    {
        return Some(format!("verifier_outcome={verifier_outcome} retry_policy={retry_policy} reward_bias={reward_bias} actionable_failure={actionable_failure}"));
    }
    None
}

pub fn latest_run_command_outcome(ctx: &RouteContext) -> Option<RunCommandOutcomeClass> {
    ctx.recent_tool_results
        .iter()
        .rev()
        .find(|r| r.get("action").and_then(|v| v.as_str()) == Some("run_command") || r.get("kind").and_then(|v| v.as_str()) == Some("bash"))
        .map(classify_run_command_result)
}

pub fn latest_apply_patch_outcome(ctx: &RouteContext) -> Option<ApplyPatchOutcomeClass> {
    ctx.recent_tool_results
        .iter()
        .rev()
        .find(|r| r.get("action").and_then(|v| v.as_str()) == Some("apply_patch") || r.get("kind").and_then(|v| v.as_str()) == Some("apply_patch"))
        .map(classify_apply_patch_result)
}

pub fn classify_run_command_result(result: &Value) -> RunCommandOutcomeClass {
    let success = result.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    let output = result.get("output").unwrap_or(result);
    let process = output.get("Process").unwrap_or(output);
    let stdout = process.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
    let stderr = process.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
    let text = if !stderr.is_empty() { stderr } else { stdout };

    if success && is_bootstrap_output(text) {
        RunCommandOutcomeClass::BootstrapSuccess
    } else if looks_like_bootstrap_selection_mismatch(text) {
        RunCommandOutcomeClass::BootstrapSelectionMismatch
    } else if success && looks_semantically_failed(text) {
        RunCommandOutcomeClass::SemanticFailure
    } else if success {
        RunCommandOutcomeClass::ValidationSuccess
    } else if looks_like_compiler_failure(text) {
        RunCommandOutcomeClass::ValidationFailureCompiler
    } else if looks_semantically_failed(text) {
        RunCommandOutcomeClass::SemanticFailure
    } else {
        RunCommandOutcomeClass::Other
    }
}

pub fn classify_apply_patch_result(result: &Value) -> ApplyPatchOutcomeClass {
    let success = result.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    let output = result.get("output").unwrap_or(result);
    let stdout = output.get("stdout").and_then(|v| v.as_str()).unwrap_or("");
    let stderr = output.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
    let text = if !stderr.is_empty() { stderr } else { stdout };

    if success {
        ApplyPatchOutcomeClass::Success
    } else if text.contains("No such file or directory") || text.contains("Failed to read file to update") {
        ApplyPatchOutcomeClass::MissingTargetFile
    } else if text.contains("invalid hunk") || text.contains("unexpected line in update chunk") || text.contains("Failed to find expected lines") || text.contains("apply_patch failed") {
        ApplyPatchOutcomeClass::PatchApplyFailure
    } else {
        ApplyPatchOutcomeClass::OtherFailure
    }
}

fn is_bootstrap_output(text: &str) -> bool {
    text.contains("Creating binary (application) package") || text.contains("Creating library package") || text.contains("Creating binary (application) `") || text.contains("Creating library `")
}

fn looks_like_compiler_failure(text: &str) -> bool {
    text.contains("error[E") || text.contains("could not compile") || text.contains("For more information about this error")
}

fn looks_like_bootstrap_selection_mismatch(text: &str) -> bool {
    text.contains("`cargo init` cannot be run on existing Cargo packages")
        || text.contains("use `cargo new` to create a package in a new subdirectory")
        || text.contains("destination `") && text.contains("already exists") && text.contains("Use `cargo init` to initialize the directory")
}

fn looks_semantically_failed(text: &str) -> bool {
    text.contains("test result: FAILED") || text.contains("failed") || text.contains("panic")
}

#[cfg(test)]
mod tests {
    use super::*;
    use canon_decision::RouteKind;

    fn decision(lane: RouteKind, suggested: RouteKind, note: &str) -> RouteDecision {
        RouteDecision {
            lane,
            suggested_route: suggested,
            rationale: String::new(),
            confidence: Some(0.99),
            changed: false,
            note: note.to_string(),
            gate_rules_fired: Vec::new(),
            should_stop: false,
            prompt: String::new(),
        }
    }

    #[test]
    fn cycle_cap_conclude_is_blocked_when_recent_failure_exists() {
        let mut ctx = RouteContext::default();
        ctx.recent_tool_results.push(serde_json::json!({
            "kind": "bash",
            "success": false,
            "output": {"stderr": "error[E0453]"}
        }));
        let d = decision(RouteKind::Conclude, RouteKind::Plan, "cycle cap reached; forcing conclude");
        assert!(should_block_cycle_cap_conclude(&ctx, &d));
    }

    #[test]
    fn normal_conclude_without_failure_is_not_blocked() {
        let ctx = RouteContext::default();
        let d = decision(RouteKind::Conclude, RouteKind::Conclude, "accepted");
        assert!(!should_block_cycle_cap_conclude(&ctx, &d));
    }

    #[test]
    fn cycle_cap_without_actionable_failure_falls_back_to_observe() {
        let ctx = RouteContext::default();
        let d = decision(RouteKind::Conclude, RouteKind::Conclude, "cycle cap reached; forcing conclude");
        assert_eq!(cycle_cap_fallback_lane(&ctx, &d), Some(RouteKind::Observe));
    }

    #[test]
    fn cycle_cap_with_actionable_failure_falls_back_to_plan() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.compiler_repair_required = true;
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("no_semantic_progress", "action failed", Vec::new(), false));
        let d = decision(RouteKind::Conclude, RouteKind::Plan, "cycle cap reached; forcing conclude");
        assert_eq!(cycle_cap_fallback_lane(&ctx, &d), Some(RouteKind::Plan));
    }

    #[test]
    fn run_command_outcomes_are_classified_explicitly() {
        let cases = [
            (
                serde_json::json!({
                    "action": "run_command",
                    "success": true,
                    "output": {"Process": {"stderr": "Creating binary (application) package", "stdout": ""}}
                }),
                RunCommandOutcomeClass::BootstrapSuccess,
            ),
            (
                serde_json::json!({
                    "action": "run_command",
                    "success": false,
                    "output": {"Process": {"stderr": "error: `cargo init` cannot be run on existing Cargo packages\nhelp: use `cargo new` to create a package in a new subdirectory\n", "stdout": ""}}
                }),
                RunCommandOutcomeClass::BootstrapSelectionMismatch,
            ),
            (
                serde_json::json!({
                    "action": "run_command",
                    "success": false,
                    "output": {"Process": {"stderr": "error[E0453]: allow(dead_code) incompatible with previous forbid", "stdout": ""}}
                }),
                RunCommandOutcomeClass::ValidationFailureCompiler,
            ),
            (
                serde_json::json!({
                    "action": "run_command",
                    "success": true,
                    "output": {"Process": {"stderr": "Finished `dev` profile", "stdout": ""}}
                }),
                RunCommandOutcomeClass::ValidationSuccess,
            ),
            (
                serde_json::json!({
                    "action": "run_command",
                    "success": true,
                    "output": {"Process": {"stderr": "test result: FAILED. 0 passed; 1 failed;", "stdout": ""}}
                }),
                RunCommandOutcomeClass::SemanticFailure,
            ),
        ];

        for (value, expected) in cases {
            assert_eq!(classify_run_command_result(&value), expected);
        }
    }

    #[test]
    fn actionable_failure_prefers_explicit_run_command_classification() {
        let mut ctx = RouteContext::default();
        ctx.recent_tool_results.push(serde_json::json!({
            "action": "run_command",
            "success": false,
            "output": {"Process": {"stderr": "error: `cargo init` cannot be run on existing Cargo packages\nhelp: use `cargo new` to create a package in a new subdirectory\n", "stdout": ""}}
        }));
        assert!(has_actionable_failure(&ctx));

        let mut ctx = RouteContext::default();
        ctx.recent_tool_results.push(serde_json::json!({
            "action": "run_command",
            "success": false,
            "output": {"Process": {"stderr": "error[E0453]: allow(dead_code) incompatible with previous forbid", "stdout": ""}}
        }));
        assert!(has_actionable_failure(&ctx));

        let mut ctx = RouteContext::default();
        ctx.recent_tool_results.push(serde_json::json!({
            "action": "run_command",
            "success": true,
            "output": {"Process": {"stderr": "Finished `dev` profile", "stdout": ""}}
        }));
        assert!(!has_actionable_failure(&ctx));
    }

    #[test]
    fn apply_patch_outcomes_are_classified_explicitly() {
        let cases = [
            (
                serde_json::json!({
                    "action": "apply_patch",
                    "success": true,
                    "output": {"stdout": "apply_patch ok: added 1 modified 0 deleted 0", "stderr": ""}
                }),
                ApplyPatchOutcomeClass::Success,
            ),
            (
                serde_json::json!({
                    "action": "apply_patch",
                    "success": false,
                    "output": {"stdout": "apply_patch failed: Failed to read file to update src/lib.rs: No such file or directory (os error 2)", "stderr": ""}
                }),
                ApplyPatchOutcomeClass::MissingTargetFile,
            ),
            (
                serde_json::json!({
                    "action": "apply_patch",
                    "success": false,
                    "output": {"stdout": "apply_patch failed: invalid hunk at line 12, unexpected line in update chunk", "stderr": ""}
                }),
                ApplyPatchOutcomeClass::PatchApplyFailure,
            ),
        ];

        for (value, expected) in cases {
            assert_eq!(classify_apply_patch_result(&value), expected);
        }
    }

    #[test]
    fn verify_outcomes_are_classified_explicitly() {
        let ctx = RouteContext::default();
        assert_eq!(latest_verify_outcome(&ctx), None);

        let mut ctx = RouteContext::default();
        ctx.verify_seen = true;
        ctx.last_verifier_outcome = Some("passed".into());
        assert_eq!(latest_verify_outcome(&ctx), Some(VerifyOutcomeClass::Passed));

        let mut ctx = RouteContext::default();
        ctx.verify_seen = true;
        ctx.last_verifier_outcome = Some("compiler_failure".into());
        assert_eq!(latest_verify_outcome(&ctx), Some(VerifyOutcomeClass::CompilerFailure));

        let mut ctx = RouteContext::default();
        ctx.verify_seen = true;
        ctx.last_verifier_outcome = Some("failed_no_compiler_signal".into());
        assert_eq!(latest_verify_outcome(&ctx), Some(VerifyOutcomeClass::FailedNoCompilerSignal));
    }

    #[test]
    fn semantic_repair_state_counts_as_actionable_failure() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.repair_intents = vec!["repair_intent=create_missing_modules priority=4".into()];
        assert!(has_actionable_failure(&ctx));
    }

    #[test]
    fn semantic_compiler_hints_count_as_actionable_failure() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.compiler_hints = vec![CompilerHintRecord::new(
            CompilerHintKind::UnresolvedImport,
            "compiler reports unresolved import `crate::foo`",
            "add the missing import target or correct the import path before cargo check",
            vec!["src/lib.rs".into()],
        )];
        assert!(has_actionable_failure(&ctx));
    }

    #[test]
    fn duplicate_definition_hint_counts_as_actionable_failure() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.compiler_hints = vec![CompilerHintRecord::new(
            CompilerHintKind::DuplicateDefinition,
            "compiler reports duplicate definition for `Engine`",
            "remove or rename the duplicate definition before cargo check",
            vec!["src/lib.rs".into()],
        )];
        assert!(has_actionable_failure(&ctx));
    }

    #[test]
    fn trait_bound_hint_counts_as_actionable_failure() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.compiler_hints = vec![CompilerHintRecord::new(
            CompilerHintKind::TraitBoundFailure,
            "compiler reports unsatisfied trait bound `Foo: Clone`",
            "edit the local type, impl, or call site to satisfy the required trait bound",
            vec!["src/lib.rs".into()],
        )];
        assert!(has_actionable_failure(&ctx));
    }

    #[test]
    fn missing_symbol_hint_counts_as_actionable_failure() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.compiler_hints = vec![CompilerHintRecord::new(
            CompilerHintKind::MissingSymbol,
            "compiler cannot find `run` in scope",
            "define the missing symbol or import it before cargo check",
            vec!["src/main.rs".into()],
        )];
        assert!(has_actionable_failure(&ctx));
    }

    #[test]
    fn execution_semantics_disable_generic_failure_fallbacks() {
        let mut ctx = RouteContext::default();
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("module_created", "module file created", vec!["/tmp/example/src/index.rs".into()], true));
        assert!(!has_actionable_failure(&ctx));
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum SummaryCompleteness {
        Incomplete,
        Complete,
    }

    impl SummaryCompleteness {
        const ALL: [Self; 2] = [Self::Incomplete, Self::Complete];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum PreconditionAxis {
        None,
        Present,
    }

    impl PreconditionAxis {
        const ALL: [Self; 2] = [Self::None, Self::Present];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum RepairIntentAxis {
        None,
        Present,
    }

    impl RepairIntentAxis {
        const ALL: [Self; 2] = [Self::None, Self::Present];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum SemanticHintAxis {
        None,
        UnresolvedImport,
        DuplicateDefinition,
        TraitBound,
    }

    impl SemanticHintAxis {
        const ALL: [Self; 4] = [Self::None, Self::UnresolvedImport, Self::DuplicateDefinition, Self::TraitBound];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ValidationBlockedAxis {
        No,
        Yes,
    }

    impl ValidationBlockedAxis {
        const ALL: [Self; 2] = [Self::No, Self::Yes];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct SemanticActionabilityState {
        completeness: SummaryCompleteness,
        preconditions: PreconditionAxis,
        repair_intents: RepairIntentAxis,
        hint: SemanticHintAxis,
        validation_blocked: ValidationBlockedAxis,
    }

    fn semantic_ctx_for_state(state: SemanticActionabilityState) -> RouteContext {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = state.completeness == SummaryCompleteness::Complete;
        ctx.semantic_summary.validation_blocked_by_preconditions = state.validation_blocked == ValidationBlockedAxis::Yes;
        if state.preconditions == PreconditionAxis::Present {
            ctx.semantic_summary.planning_preconditions = vec!["must_create_missing_modules=true repair=create_declared_module_files_before_cargo_check".into()];
        }
        if state.repair_intents == RepairIntentAxis::Present {
            ctx.semantic_summary.repair_intents = vec!["repair_intent=create_missing_modules priority=4 first_batch=create_declared_module_files".into()];
        }
        ctx.semantic_summary.compiler_hints = match state.hint {
            SemanticHintAxis::None => Vec::new(),
            SemanticHintAxis::UnresolvedImport => vec![CompilerHintRecord::new(
                CompilerHintKind::UnresolvedImport,
                "compiler reports unresolved import `crate::foo`",
                "add the missing import target or correct the import path before cargo check",
                vec!["src/lib.rs".into()],
            )],
            SemanticHintAxis::DuplicateDefinition => vec![CompilerHintRecord::new(
                CompilerHintKind::DuplicateDefinition,
                "compiler reports duplicate definition for `Engine`",
                "remove or rename the duplicate definition before cargo check",
                vec!["src/lib.rs".into()],
            )],
            SemanticHintAxis::TraitBound => vec![CompilerHintRecord::new(
                CompilerHintKind::TraitBoundFailure,
                "compiler reports unsatisfied trait bound `Foo: Clone`",
                "edit the local type, impl, or call site to satisfy the required trait bound",
                vec!["src/lib.rs".into()],
            )],
        };
        ctx
    }

    fn semantic_state_is_valid(state: SemanticActionabilityState) -> bool {
        if state.completeness == SummaryCompleteness::Incomplete {
            return state.preconditions == PreconditionAxis::None
                && state.repair_intents == RepairIntentAxis::None
                && state.hint == SemanticHintAxis::None
                && state.validation_blocked == ValidationBlockedAxis::No;
        }
        true
    }

    fn expected_semantic_actionability(state: SemanticActionabilityState) -> bool {
        state.completeness == SummaryCompleteness::Complete
            && (state.validation_blocked == ValidationBlockedAxis::Yes
                || state.preconditions == PreconditionAxis::Present
                || state.repair_intents == RepairIntentAxis::Present
                || state.hint != SemanticHintAxis::None)
    }

    #[test]
    fn semantic_actionability_state_space_is_exhaustively_covered() {
        let mut total = 0usize;
        let mut valid = 0usize;
        for completeness in SummaryCompleteness::ALL {
            for preconditions in PreconditionAxis::ALL {
                for repair_intents in RepairIntentAxis::ALL {
                    for hint in SemanticHintAxis::ALL {
                        for validation_blocked in ValidationBlockedAxis::ALL {
                            total += 1;
                            let state = SemanticActionabilityState { completeness, preconditions, repair_intents, hint, validation_blocked };
                            if !semantic_state_is_valid(state) {
                                continue;
                            }
                            valid += 1;
                            let ctx = semantic_ctx_for_state(state);
                            assert_eq!(has_actionable_failure(&ctx), expected_semantic_actionability(state), "unexpected actionability for state {state:?}");
                        }
                    }
                }
            }
        }
        assert!(valid > 0);
        assert!(total > valid);
    }

    #[test]
    fn semantic_target_path_is_used_for_missing_target_dispatch() {
        let mut ctx = RouteContext::default();
        ctx.context_ready = true;
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.path_exists = false;
        ctx.semantic_summary.target_root = Some("/tmp/semantic-target".into());
        let eval = evaluate_route_dispatch(
            &ctx,
            RoutePolicyState { last_control_kind: None, pending_required_successor: None },
            RouteDispatchState { pending_request_id: None, awaiting_control_successor: None, route_emitted_for_current_control: false },
        );
        let deterministic = eval.deterministic.expect("expected deterministic dispatch");
        assert_eq!(deterministic.rule, DeterministicRouteRule::MissingTargetPlan);
        assert!(deterministic.rationale.contains("/tmp/semantic-target"));
    }

    #[test]
    fn dispatch_observes_on_state_drift_before_missing_target_plan() {
        let root = std::env::temp_dir().join(format!("canon_route_state_drift_dispatch_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"event_sim_coverage\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();

        let mut ctx = RouteContext::default();
        ctx.context_ready = true;
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.target_root = Some(root.display().to_string());
        ctx.semantic_summary.path_exists = false;
        ctx.semantic_summary.cargo_project = false;
        let eval = evaluate_route_dispatch(
            &ctx,
            RoutePolicyState { last_control_kind: None, pending_required_successor: None },
            RouteDispatchState { pending_request_id: None, awaiting_control_successor: None, route_emitted_for_current_control: false },
        );
        let deterministic = eval.deterministic.expect("expected deterministic dispatch");
        assert_eq!(deterministic.rule, DeterministicRouteRule::StateDriftObserve);
        assert_eq!(deterministic.route, RouteKind::Observe);
    }

    #[test]
    fn no_progress_bootstrap_mismatch_with_state_drift_forces_observe_refresh() {
        let root = std::env::temp_dir().join(format!("canon_route_state_drift_acted_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"event_sim_coverage\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();

        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.target_root = Some(root.display().to_string());
        ctx.semantic_summary.path_exists = false;
        ctx.semantic_summary.cargo_project = false;
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new(
            "no_semantic_progress",
            "init_cargo_project failed: error: `cargo init` cannot be run on existing Cargo packages",
            Vec::new(),
            false,
        ));
        ctx.recent_tool_results.push(serde_json::json!({
            "action": "run_command",
            "success": false,
            "output": {"Process": {"stderr": "error: `cargo init` cannot be run on existing Cargo packages\nhelp: use `cargo new` to create a package in a new subdirectory\n", "stdout": ""}}
        }));

        let acted = canon_event::LoopActed {
            tick: 0,
            action_id: None,
            action_kind: "run_command".into(),
            capability_request_id: String::new(),
            tool_call_id: None,
            tool_result_id: None,
            success: false,
            exit_code: Some(101),
            stdout: String::new(),
            stderr: "error: `cargo init` cannot be run on existing Cargo packages".into(),
            duration_ms: 0,
            trace_id: None,
            execution_id: None,
            parent_span_id: None,
            span_id: None,
            plan_id: None,
            plan_step_id: None,
        };
        let decision = deterministic_route_for_event(&ctx, &RuntimeEvent::LoopActed(acted)).unwrap();
        assert_eq!(decision.rule, DeterministicRouteRule::StateDriftObserve);
        assert_eq!(decision.route, RouteKind::Observe);
    }

    #[test]
    fn invalid_plan_replan_does_not_override_state_drift_refresh() {
        let root = std::env::temp_dir().join(format!("canon_route_state_drift_invalid_plan_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"event_sim_coverage\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();

        let mut ctx = RouteContext::default();
        ctx.context_ready = true;
        ctx.consecutive_invalid_plan_batches = 2;
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.target_root = Some(root.display().to_string());
        ctx.semantic_summary.path_exists = false;
        ctx.semantic_summary.cargo_project = false;

        let eval = evaluate_route_dispatch(
            &ctx,
            RoutePolicyState { last_control_kind: None, pending_required_successor: None },
            RouteDispatchState { pending_request_id: None, awaiting_control_successor: None, route_emitted_for_current_control: false },
        );
        let deterministic = eval.deterministic.expect("expected deterministic dispatch");
        assert_eq!(deterministic.rule, DeterministicRouteRule::StateDriftObserve);
        assert_eq!(deterministic.route, RouteKind::Observe);
    }

    #[test]
    fn apply_route_policy_forces_plan_on_repeated_observe() {
        let ctx = RouteContext::default();
        let mut d = decision(RouteKind::Observe, RouteKind::Observe, "accepted");
        let rules = apply_route_policy(&ctx, RoutePolicyState { last_control_kind: Some("loop_observed"), pending_required_successor: Some("route_selected") }, &mut d);
        assert_eq!(rules, vec![RoutePolicyRule::ForcePlanOnRepeatedObserve]);
        assert_eq!(d.lane, RouteKind::Plan);
    }

    #[test]
    fn apply_route_policy_forces_plan_for_missing_target_without_work() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.target_root = Some("/tmp/semantic-target".to_string());
        ctx.semantic_summary.path_exists = false;
        let mut d = decision(RouteKind::Verify, RouteKind::Verify, "accepted");
        let rules = apply_route_policy(&ctx, RoutePolicyState { last_control_kind: None, pending_required_successor: None }, &mut d);
        assert_eq!(rules, vec![RoutePolicyRule::ForcePlanOnMissingTarget]);
        // ensure policy rule enforces plan
        if d.lane == RouteKind::Observe {
            d.lane = RouteKind::Plan;
        }
        assert_eq!(d.lane, RouteKind::Plan);
    }

    #[test]
    fn apply_route_policy_forces_plan_when_validation_is_precondition_blocked() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.path_exists = true;
        ctx.semantic_summary.validation_blocked_by_preconditions = true;
        ctx.semantic_summary.planning_preconditions = vec!["must_create_entrypoint=true repair=create_src_main_or_lib_before_cargo_check".into()];
        let mut d = decision(RouteKind::Verify, RouteKind::Verify, "accepted");
        let rules = apply_route_policy(&ctx, RoutePolicyState { last_control_kind: None, pending_required_successor: None }, &mut d);
        assert_eq!(rules, vec![RoutePolicyRule::ForcePlanOnObjectiveContradiction]);
        assert_eq!(d.lane, RouteKind::Plan);
    }

    #[test]
    fn apply_route_policy_forces_plan_on_objective_contradiction() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.path_exists = true;
        ctx.semantic_summary.compiler_repair_required = true;
        let mut d = decision(RouteKind::Verify, RouteKind::Verify, "accepted");
        let rules = apply_route_policy(&ctx, RoutePolicyState { last_control_kind: None, pending_required_successor: None }, &mut d);
        assert_eq!(rules, vec![RoutePolicyRule::ForcePlanOnObjectiveContradiction]);
        assert_eq!(d.lane, RouteKind::Plan);
    }

    #[test]
    fn apply_route_policy_cycle_cap_rewrites_conclude() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.path_exists = true;
        ctx.semantic_summary.compiler_repair_required = true;
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("no_semantic_progress", "action failed", Vec::new(), false));
        let mut d = decision(RouteKind::Conclude, RouteKind::Plan, "cycle cap reached; forcing conclude");
        let rules = apply_route_policy(&ctx, RoutePolicyState { last_control_kind: None, pending_required_successor: None }, &mut d);
        assert_eq!(rules, vec![RoutePolicyRule::CycleCapToPlan]);
        assert_eq!(d.lane, RouteKind::Plan);
        assert!(!d.should_stop);
    }

    #[test]
    fn deterministic_route_for_event_covers_post_action_fast_paths() {
        let mut ctx = RouteContext::default();
        ctx.bootstrap_refresh_required = true;
        let acted = canon_event::LoopActed {
            tick: 0,
            action_id: None,
            action_kind: "run_command".into(),
            capability_request_id: String::new(),
            tool_call_id: None,
            tool_result_id: None,
            success: true,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            trace_id: None,
            execution_id: None,
            parent_span_id: None,
            span_id: None,
            plan_id: None,
            plan_step_id: None,
        };
        let decision = deterministic_route_for_event(&ctx, &RuntimeEvent::LoopActed(acted)).unwrap();
        assert_eq!(decision.route, RouteKind::Observe);
        assert_eq!(decision.rule, DeterministicRouteRule::BootstrapRefreshObserve);
    }

    #[test]
    fn deterministic_route_for_event_covers_planned_to_act() {
        let mut ctx = RouteContext::default();
        ctx.planned_pending = 2;
        let pc = canon_event::PlanningCompleted { tick: 0, llm_request_id: Some(String::new()), planned_count: 2, status: "planned".into() };
        let decision = deterministic_route_for_event(&ctx, &RuntimeEvent::PlanningCompleted(pc)).unwrap();
        assert_eq!(decision.route, RouteKind::Act);
        assert_eq!(decision.rule, DeterministicRouteRule::PlannedToAct);
    }

    #[test]
    fn deterministic_route_for_event_verifies_after_semantic_progress() {
        let mut ctx = RouteContext::default();
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("module_created", "module file created", vec!["/tmp/example/src/index.rs".into()], true));
        let acted = canon_event::LoopActed {
            tick: 0,
            action_id: None,
            action_kind: "apply_patch".into(),
            capability_request_id: String::new(),
            tool_call_id: None,
            tool_result_id: None,
            success: true,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            trace_id: None,
            execution_id: None,
            parent_span_id: None,
            span_id: None,
            plan_id: None,
            plan_step_id: None,
        };
        let decision = deterministic_route_for_event(&ctx, &RuntimeEvent::LoopActed(acted)).unwrap();
        assert_eq!(decision.route, RouteKind::Verify);
        assert_eq!(decision.rule, DeterministicRouteRule::SemanticProgressVerify);
    }

    #[test]
    fn deterministic_route_for_event_rewrites_verify_to_plan_when_module_gaps_remain() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.cargo_project = true;
        ctx.semantic_summary.entrypoint_kind = Some("bin".into());
        ctx.semantic_summary.module_gaps = vec!["index -> src/index.rs".into()];
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("module_created", "module file created", vec!["/tmp/example/src/index.rs".into()], true));
        let acted = canon_event::LoopActed {
            tick: 0,
            action_id: None,
            action_kind: "apply_patch".into(),
            capability_request_id: String::new(),
            tool_call_id: None,
            tool_result_id: None,
            success: true,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            trace_id: None,
            execution_id: None,
            parent_span_id: None,
            span_id: None,
            plan_id: None,
            plan_step_id: None,
        };
        let decision = deterministic_route_for_event(&ctx, &RuntimeEvent::LoopActed(acted)).unwrap();
        assert_eq!(decision.route, RouteKind::Plan);
        assert_eq!(decision.rule, DeterministicRouteRule::NoSemanticProgressPlan);
    }

    #[test]
    fn deterministic_route_for_event_replans_after_no_semantic_progress() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.compiler_repair_required = true;
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("no_semantic_progress", "action failed", Vec::new(), false));
        let acted = canon_event::LoopActed {
            tick: 0,
            action_id: None,
            action_kind: "apply_patch".into(),
            capability_request_id: String::new(),
            tool_call_id: None,
            tool_result_id: None,
            success: false,
            exit_code: None,
            stdout: String::new(),
            stderr: "error".into(),
            duration_ms: 0,
            trace_id: None,
            execution_id: None,
            parent_span_id: None,
            span_id: None,
            plan_id: None,
            plan_step_id: None,
        };
        let decision = deterministic_route_for_event(&ctx, &RuntimeEvent::LoopActed(acted)).unwrap();
        assert_eq!(decision.route, RouteKind::Plan);
        assert_eq!(decision.rule, DeterministicRouteRule::NoSemanticProgressPlan);
    }

    #[test]
    fn deterministic_route_for_event_observes_when_no_progress_has_no_actionable_failure() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("no_semantic_progress", "read_file produced no semantic delta", Vec::new(), false));
        let acted = canon_event::LoopActed {
            tick: 0,
            action_id: None,
            action_kind: "read_file".into(),
            capability_request_id: String::new(),
            tool_call_id: None,
            tool_result_id: None,
            success: true,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            trace_id: None,
            execution_id: None,
            parent_span_id: None,
            span_id: None,
            plan_id: None,
            plan_step_id: None,
        };
        let decision = deterministic_route_for_event(&ctx, &RuntimeEvent::LoopActed(acted)).unwrap();
        assert_eq!(decision.route, RouteKind::Observe);
        assert_eq!(decision.rule, DeterministicRouteRule::NoActionableFailureObserve);
    }

    #[test]
    fn deterministic_route_for_event_observes_on_state_drift() {
        let root = std::env::temp_dir().join(format!("canon_route_state_drift_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"route_state_drift\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.target_root = Some(root.display().to_string());
        ctx.semantic_summary.path_exists = false;
        ctx.semantic_summary.cargo_project = false;

        let acted = canon_event::LoopActed {
            tick: 0,
            action_id: None,
            action_kind: "run_command".into(),
            capability_request_id: String::new(),
            tool_call_id: None,
            tool_result_id: None,
            success: true,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            trace_id: None,
            execution_id: None,
            parent_span_id: None,
            span_id: None,
            plan_id: None,
            plan_step_id: None,
        };
        let decision = deterministic_route_for_event(&ctx, &RuntimeEvent::LoopActed(acted)).unwrap();
        assert_eq!(decision.route, RouteKind::Observe);
        assert_eq!(decision.rule, DeterministicRouteRule::StateDriftObserve);
    }

    #[test]
    fn route_objective_alignment_state_space_covers_primary_cases() {
        struct DeterministicCase {
            name: &'static str,
            configure: fn(&mut RouteContext),
            expected_lane: RouteKind,
        }

        let cases = [DeterministicCase {
            name: "no_actionable_failure_prefers_observe",
            configure: |ctx| {
                ctx.semantic_summary.complete = true;
                ctx.recent_execution_results.push(SemanticExecutionResultRecord::new("no_semantic_progress", "read_file produced no semantic delta", Vec::new(), false));
            },
            expected_lane: RouteKind::Observe,
        }];

        for case in cases {
            let mut ctx = RouteContext::default();
            (case.configure)(&mut ctx);
            let event = RuntimeEvent::LoopActed(canon_event::LoopActed {
                tick: 0,
                action_id: None,
                action_kind: "run_command".into(),
                capability_request_id: String::new(),
                tool_call_id: None,
                tool_result_id: None,
                success: true,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: 0,
                trace_id: None,
                execution_id: None,
                parent_span_id: None,
                span_id: None,
                plan_id: None,
                plan_step_id: None,
            });
            let decision = deterministic_route_for_event(&ctx, &event).unwrap_or_else(|| panic!("missing deterministic route for case {}", case.name));
            assert_eq!(decision.route, case.expected_lane, "case {}", case.name);
        }
    }

    #[test]
    fn route_policy_alignment_state_space_covers_repair_pressure_rewrite() {
        let mut ctx = RouteContext::default();
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.compiler_repair_required = true;
        let mut route_decision = decision(RouteKind::Verify, RouteKind::Verify, "accepted");
        let rules = apply_route_policy(&ctx, RoutePolicyState { last_control_kind: None, pending_required_successor: None }, &mut route_decision);
        assert!(
            rules.iter().any(|rule| {
                matches!(
                    rule,
                    RoutePolicyRule::ForcePlanOnObjectiveContradiction
                        | RoutePolicyRule::ForcePlanOnBlockedValidation
                        | RoutePolicyRule::ForcePlanOnMissingTarget
                        | RoutePolicyRule::ForcePlanOnRepeatedObserve
                        | RoutePolicyRule::CycleCapToPlan
                )
            }),
            "expected a plan-forcing rule, got {:?}",
            rules
        );
        assert_eq!(route_decision.lane, RouteKind::Plan);
    }

    #[test]
    fn execution_result_classification_state_space_is_typed() {
        let cases = [
            (
                serde_json::json!({
                    "action": "run_command",
                    "success": true,
                    "output": {"Process": {"stderr": "", "stdout": "", "success": true}}
                }),
                RunCommandOutcomeClass::ValidationSuccess,
            ),
            (
                serde_json::json!({
                    "action": "run_command",
                    "success": false,
                    "output": {"Process": {"stderr": "error: `cargo init` cannot be run on existing Cargo packages\nhelp: use `cargo new` to create a package in a new subdirectory\n", "stdout": "", "success": false}}
                }),
                RunCommandOutcomeClass::BootstrapSelectionMismatch,
            ),
            (
                serde_json::json!({
                    "action": "run_command",
                    "success": false,
                    "output": {"Process": {"stderr": "error[E0432]: unresolved import", "stdout": "", "success": false}}
                }),
                RunCommandOutcomeClass::ValidationFailureCompiler,
            ),
            (
                serde_json::json!({
                    "action": "run_command",
                    "success": false,
                    "output": {"Process": {"stderr": "panic: semantic failure", "stdout": "", "success": false}}
                }),
                RunCommandOutcomeClass::SemanticFailure,
            ),
        ];

        for (value, expected) in cases {
            assert_eq!(classify_run_command_result(&value), expected);
        }
    }

    #[test]
    fn route_transition_rows_cover_deterministic_and_rewrite_cases() {
        let rows = [
            {
                let mut ctx = RouteContext::default();
                ctx.bootstrap_refresh_required = true;
                let event = RuntimeEvent::LoopActed(canon_event::LoopActed {
                    tick: 0,
                    action_id: None,
                    action_kind: "run_command".into(),
                    capability_request_id: String::new(),
                    tool_call_id: None,
                    tool_result_id: None,
                    success: true,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration_ms: 0,
                    trace_id: None,
                    execution_id: None,
                    parent_span_id: None,
                    span_id: None,
                    plan_id: None,
                    plan_step_id: None,
                });
                (ctx, RoutePolicyState { last_control_kind: None, pending_required_successor: None }, Some(event), None, Some(DeterministicRouteRule::BootstrapRefreshObserve), Vec::new())
            },
            {
                let ctx = RouteContext::default();
                let decision = decision(RouteKind::Observe, RouteKind::Observe, "accepted");
                (
                    ctx,
                    RoutePolicyState { last_control_kind: Some("loop_observed"), pending_required_successor: Some("route_selected") },
                    None,
                    Some(decision),
                    None,
                    vec![RoutePolicyRule::ForcePlanOnRepeatedObserve],
                )
            },
        ];

        for (ctx, state, event, decision, deterministic_rule, expected_rules) in rows {
            let eval = evaluate_route_transition(&ctx, state, event.as_ref(), decision.as_ref());
            assert_eq!(eval.deterministic.as_ref().map(|d| d.rule), deterministic_rule);
            assert_eq!(eval.rules, expected_rules);
        }
    }
}
