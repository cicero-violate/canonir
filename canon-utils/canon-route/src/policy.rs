use crate::{context::RouteContext, decision::RouteDecision};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoutePolicyState {}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePolicyRule {
    ForcePlanOnObjectiveContradiction,
    ForcePlanOnRepeatedObserve,
    ForcePlanOnMissingTarget,
    CycleCapToObserve,
}

#[derive(Default)]
pub struct RouteEmitState<'a> {
    pub last_control_kind: Option<&'a str>,
    pub pending_required_successor: Option<&'a str>,
}

pub struct RouteEmitEvaluation {
    pub allowed: bool,
    pub rule: RouteEmitRule,
}

pub struct RouteEmitEffectsEvaluation {
    pub clear_pending_request: bool,
    pub clear_pending_prompt: bool,
    pub set_halted: bool,
    pub rules: Vec<RouteEmitEffectRule>,
}

pub struct RouteRecoveryEvaluation {
    pub expected_successor: Option<String>,
    pub rule: RouteRecoveryRule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteEmitRule {
    Allowed,
    DuplicateEmitBeforeSuccessor,
    IllegalControlReentry,
    IllegalControlEmit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteEmitEffectRule {
    None,
    ClearDeterministicObserveSentinel,
    HaltOnConclude,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteRecoveryRule {
    None,
    EmitExpectedSuccessorRecovery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteFailureRule {
    HeuristicFailureReroute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteCacheRule {
    ReplayCachedRoute,
    InvalidateCachedObserveRoute,
    SuppressDuplicatePrompt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteDispatchRule {
    SuppressHalted,
    SuppressContextNotReady,
    SuppressPendingRequest,
    SuppressDuplicateRouteForCurrentControl,
}

pub struct RouteCacheState {
    pub force_fresh_route_once: bool,
    pub last_prompt_hash: Option<u64>,
    pub prompt_hash: u64,
    pub pending_required_successor: Option<&'static str>,
    pub last_route_prompt_hash: Option<u64>,
    pub route_emitted_for_current_control: bool,
    pub has_cached_route: bool,
    pub cached_route_is_observe: bool,
    pub can_emit_route_selected: bool,
}

pub struct RouteDispatchState<'a> {
    pub pending_request_id: Option<&'a str>,
    pub route_emitted_for_current_control: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeterministicRouteRule {
    BootstrapRefreshObserve,
    DoneVerify,
    SemanticProgressVerify,
    NoSemanticProgressPlan,
    ContinueAct,
    PlannedToAct,
    MissingObservedContextObserve,
    MissingTargetPlan,
    InvalidPlanReplan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyPatchOutcomeClass {
    Success,
    MissingTargetFile,
    PatchApplyFailure,
    OtherFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunCommandOutcomeClass {
    BootstrapSuccess,
    ValidationFailureCompiler,
    BootstrapSelectionMismatch,
    ValidationSuccess,
    SemanticFailure,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyOutcomeClass {
    CompilerFailure,
    Passed,
    FailedNoCompilerSignal,
}

pub struct RouteCacheEvaluation {
    pub rule: RouteCacheRule,
}

pub fn evaluate_route_cache(_state: RouteCacheState) -> RouteCacheEvaluation {
    RouteCacheEvaluation { rule: RouteCacheRule::ReplayCachedRoute }
}

pub struct RouteDispatchEvaluation {
    pub suppression: Option<RouteDispatchSuppression>,
    pub deterministic: Option<DeterministicEvaluation>,
}

pub struct RouteDispatchSuppression {
    pub rule: RouteDispatchRule,
}

pub fn evaluate_route_dispatch(
    _ctx: &RouteContext,
    _state: RoutePolicyState,
    _dispatch: RouteDispatchState<'_>
) -> RouteDispatchEvaluation {
    RouteDispatchEvaluation {
        suppression: None,
        deterministic: None,
    }
}

pub struct RouteFailureEvaluation {
    pub rule: RouteFailureRule,
}

pub fn evaluate_route_failure(_ctx: &RouteContext) -> RouteFailureEvaluation {
    RouteFailureEvaluation { rule: RouteFailureRule::HeuristicFailureReroute }
}

pub struct DeterministicEvaluation {
    pub rule: DeterministicRouteRule,
}

pub struct RouteTransitionEvaluation {
    pub deterministic: Option<DeterministicEvaluation>,
    pub rules: Vec<RoutePolicyRule>,
}

pub fn evaluate_route_transition(
    _ctx: &RouteContext,
    _state: RoutePolicyState,
    _event: Option<&canon_event::RuntimeEvent>,
    _decision: Option<&RouteDecision>
) -> RouteTransitionEvaluation {
    RouteTransitionEvaluation {
        deterministic: None,
        rules: vec![],
    }
}

pub fn latest_apply_patch_outcome(_ctx: &RouteContext) -> Option<ApplyPatchOutcomeClass> { None }
pub fn latest_run_command_outcome(_ctx: &RouteContext) -> Option<RunCommandOutcomeClass> { None }
pub fn latest_verify_outcome(_ctx: &RouteContext) -> Option<VerifyOutcomeClass> { None }

pub fn has_actionable_failure(_ctx: &RouteContext) -> bool { false }

pub fn apply_route_policy(_ctx: &RouteContext, _state: RoutePolicyState, _decision: &mut RouteDecision) -> Vec<RoutePolicyRule> {
    // POLICY LAYER IS PURELY OBSERVATIONAL
    // All routing must originate from canonical decision()
    Vec::new()
}

pub fn evaluate_route_emit(_state: RouteEmitState<'_>) -> RouteEmitEvaluation {
    RouteEmitEvaluation { allowed: true, rule: RouteEmitRule::Allowed }
}

pub fn evaluate_route_emit_effects(_decision: &RouteDecision) -> RouteEmitEffectsEvaluation {
    RouteEmitEffectsEvaluation {
        clear_pending_request: false,
        clear_pending_prompt: false,
        set_halted: false,
        rules: vec![RouteEmitEffectRule::None],
    }
}

pub fn evaluate_route_recovery(_pending_required_successor: Option<&str>) -> RouteRecoveryEvaluation {
    RouteRecoveryEvaluation {
        expected_successor: None,
        rule: RouteRecoveryRule::None,
    }
}

// All previous routing logic has been removed.
// Any attempt to route from policy is a violation of SPEC.
