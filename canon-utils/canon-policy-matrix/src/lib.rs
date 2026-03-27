use canon_decision::RouteKind;
use canon_event::{LoopActed, PlanningCompleted, RuntimeEvent};
use canon_loop::planning_preconditions::{
    goal_route_objective_drift, route_choice_contradicts_primary_objective,
    validate_objective_route_plan_alignment, validate_preconditions, validate_trend_intent_alignment,
    PlanningPrecondition,
};
use canon_loop::policy::{
    classify_invalid_plan_reason, evaluate_loop_runtime, evaluate_loop_transition, evaluate_recovery_event,
    evaluate_recovery_execution, evaluate_bootstrap_effects, retry_policy_for_invalid_plan,
    retry_policy_for_planning_context,
    ActionOutcomeClass, BootstrapRule, InvalidPlanReasonClass, LoopRecoveryRule, LoopRuntimeRule,
    ObserveExecutionMode, RecoveryEventRule, RecoveryOperation, RetryPolicy, StageExecutionOutcomeClass,
};
use canon_loop::stage::reward::{evaluate_reward_semantics, RewardSemantics};
use canon_route::{
    context::RouteContext,
    decision::RouteDecision,
    policy::{
        evaluate_route_cache, evaluate_route_dispatch, evaluate_route_emit, evaluate_route_emit_effects,
        evaluate_route_failure, evaluate_route_recovery, evaluate_route_transition,
        evaluate_successor_consumption, latest_apply_patch_outcome, latest_run_command_outcome,
        latest_verify_outcome, ApplyPatchOutcomeClass, DeterministicRouteRule, RouteCacheRule,
        RouteCacheState, RouteDispatchRule, RouteDispatchState, RouteEmitEffectRule, RouteEmitRule,
        RouteEmitState, RouteFailureRule, RoutePolicyRule, RoutePolicyState, RouteRecoveryRule,
        RunCommandOutcomeClass, SuccessorConsumptionRule, VerifyOutcomeClass,
    },
};
use canon_semantic_state::{
    CompilerHintKind, CompilerHintRecord, SemanticExecutionResultRecord, SemanticStateSummary,
};

#[derive(Clone, Debug)]
pub enum TransitionRow {
    Route(RouteTransitionRow),
    Loop(LoopTransitionRow),
    RunCommandOutcome(RunCommandOutcomeRow),
    ApplyPatchOutcome(ApplyPatchOutcomeRow),
    VerifyOutcome(VerifyOutcomeRow),
    InvalidPlanRetry(InvalidPlanRetryRow),
    RouteDispatch(RouteDispatchRow),
    RouteEmit(RouteEmitRow),
    RouteCache(RouteCacheRow),
    LoopRuntime(LoopRuntimeRow),
    RecoveryEvent(RecoveryEventRow),
    RecoveryExecution(RecoveryExecutionRow),
    BootstrapEffect(BootstrapEffectRow),
    PlannerRecovery(PlannerRecoveryRow),
    RewardSemantics(RewardSemanticsRow),
    RouteFailure(RouteFailureRow),
    RouteEmitEffect(RouteEmitEffectRow),
    RouteRecovery(RouteRecoveryRow),
    SuccessorConsumption(SuccessorConsumptionRow),
    PlannerJudgment(PlannerJudgmentRow),
    PlannerObjectiveAlignment(PlannerObjectiveAlignmentRow),
    RouteObjectiveAlignment(RouteObjectiveAlignmentRow),
    GoalRouteObjectiveDrift(GoalRouteObjectiveDriftRow),
    RouteSemanticActionability(RouteSemanticActionabilityRow),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RouteScenarioFamily {
    BootstrapRefreshObserve,
    DoneVerify,
    SemanticProgressVerify,
    NoSemanticProgressPlan,
    ContinueAct,
    PlannedToAct,
    MissingObservedContextObserve,
    ForcePlanOnRepeatedObserve,
    ForcePlanOnMissingTarget,
    CycleCapToPlan,
    CycleCapToObserve,
    NoRewriteAccepted,
    DispatchSuppressHalted,
    DispatchSuppressContextNotReady,
    DispatchSuppressPendingRequest,
    DispatchSuppressAwaitingSuccessor,
    DispatchSuppressDuplicateCurrentControl,
    DispatchMissingTargetPlan,
    DispatchInvalidPlanReplan,
    EmitDuplicateBeforeSuccessor,
    EmitIllegalControlReentry,
    EmitWrongExpectedSuccessor,
    CacheReplay,
    CacheInvalidateObserve,
    CacheSuppressDuplicatePrompt,
    FailureHeuristicReroute,
    EmitEffectObserveClearsDeterministicSentinel,
    EmitEffectConcludeHalts,
    RecoveryEmitExpectedSuccessor,
    SuccessorConsumesAwaiting,
}

impl RouteScenarioFamily {
    pub const ALL: [Self; 30] = [
        Self::BootstrapRefreshObserve,
        Self::DoneVerify,
        Self::SemanticProgressVerify,
        Self::NoSemanticProgressPlan,
        Self::ContinueAct,
        Self::PlannedToAct,
        Self::MissingObservedContextObserve,
        Self::ForcePlanOnRepeatedObserve,
        Self::ForcePlanOnMissingTarget,
        Self::CycleCapToPlan,
        Self::CycleCapToObserve,
        Self::NoRewriteAccepted,
        Self::DispatchSuppressHalted,
        Self::DispatchSuppressContextNotReady,
        Self::DispatchSuppressPendingRequest,
        Self::DispatchSuppressAwaitingSuccessor,
        Self::DispatchSuppressDuplicateCurrentControl,
        Self::DispatchMissingTargetPlan,
        Self::DispatchInvalidPlanReplan,
        Self::EmitDuplicateBeforeSuccessor,
        Self::EmitIllegalControlReentry,
        Self::EmitWrongExpectedSuccessor,
        Self::CacheReplay,
        Self::CacheInvalidateObserve,
        Self::CacheSuppressDuplicatePrompt,
        Self::FailureHeuristicReroute,
        Self::EmitEffectObserveClearsDeterministicSentinel,
        Self::EmitEffectConcludeHalts,
        Self::RecoveryEmitExpectedSuccessor,
        Self::SuccessorConsumesAwaiting,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoopScenarioFamily {
    InvalidPlanClearsSuppression,
    InvalidPlanNoRecoveryForOtherStatus,
    ActStallTriggersObserve,
    NonActStallDoesNotTriggerObserve,
    RewardRecoveryForExpectedSuccessor,
    NonRewardSuccessorDoesNotRecover,
    ObserveBlockedByPendingSuccessor,
    ObserveNotBlockedWithoutSuccessor,
    RuntimeTriggeredObserve,
    RuntimeForcedObserve,
    RuntimeSuppressObserveOnInvariant,
    RuntimeSuppressObserveOnPendingSuccessor,
    RuntimeBlockWhenHalted,
    RecoveryEventForceObserve,
    RecoveryEventRewardExecute,
    RecoveryEventRewardSkipSatisfied,
    RecoveryEventRewardMissingContext,
    RewardRecoveryNoop,
    RewardRecoveryExecutionError,
    ObserveForcedDeferred,
    ObserveForcedNoop,
    ObserveTriggeredDeferred,
    ObserveTriggeredNoop,
    BootstrapInvalidatesQueuedWork,
    RewardSemanticProgress,
    RewardNoSemanticProgress,
}

impl LoopScenarioFamily {
    pub const ALL: [Self; 26] = [
        Self::InvalidPlanClearsSuppression,
        Self::InvalidPlanNoRecoveryForOtherStatus,
        Self::ActStallTriggersObserve,
        Self::NonActStallDoesNotTriggerObserve,
        Self::RewardRecoveryForExpectedSuccessor,
        Self::NonRewardSuccessorDoesNotRecover,
        Self::ObserveBlockedByPendingSuccessor,
        Self::ObserveNotBlockedWithoutSuccessor,
        Self::RuntimeTriggeredObserve,
        Self::RuntimeForcedObserve,
        Self::RuntimeSuppressObserveOnInvariant,
        Self::RuntimeSuppressObserveOnPendingSuccessor,
        Self::RuntimeBlockWhenHalted,
        Self::RecoveryEventForceObserve,
        Self::RecoveryEventRewardExecute,
        Self::RecoveryEventRewardSkipSatisfied,
        Self::RecoveryEventRewardMissingContext,
        Self::RewardRecoveryNoop,
        Self::RewardRecoveryExecutionError,
        Self::ObserveForcedDeferred,
        Self::ObserveForcedNoop,
        Self::ObserveTriggeredDeferred,
        Self::ObserveTriggeredNoop,
        Self::BootstrapInvalidatesQueuedWork,
        Self::RewardSemanticProgress,
        Self::RewardNoSemanticProgress,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RunCommandOutcomeFamily {
    BootstrapSuccess,
    ValidationFailureCompiler,
    ValidationSuccess,
    SemanticFailure,
    Other,
}

impl RunCommandOutcomeFamily {
    pub const ALL: [Self; 5] = [
        Self::BootstrapSuccess,
        Self::ValidationFailureCompiler,
        Self::ValidationSuccess,
        Self::SemanticFailure,
        Self::Other,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ApplyPatchOutcomeFamily {
    Success,
    MissingTargetFile,
    PatchApplyFailure,
    OtherFailure,
}

impl ApplyPatchOutcomeFamily {
    pub const ALL: [Self; 4] = [
        Self::Success,
        Self::MissingTargetFile,
        Self::PatchApplyFailure,
        Self::OtherFailure,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VerifyOutcomeFamily {
    CompilerFailure,
    Passed,
    FailedNoCompilerSignal,
}

impl VerifyOutcomeFamily {
    pub const ALL: [Self; 3] = [Self::CompilerFailure, Self::Passed, Self::FailedNoCompilerSignal];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvalidPlanRetryFamily {
    MixedBatchDiscoveryOnly,
    PatchFormatSinglePatchOnly,
    PathOrCwdCorrectiveRetry,
    MissingContextCorrectiveRetry,
    UnknownCorrectiveRetry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JudgmentScenarioFamily {
    PlannerBootstrapWorkspace,
    PlannerInitCargoProject,
    PlannerCreateEntrypoint,
    PlannerCreateMissingModules,
    PlannerFixDeadCodeConflict,
    PlannerFixUnresolvedImport,
    PlannerDefineMissingSymbol,
    PlannerResolveDuplicateDefinition,
    PlannerFixTraitBoundFailure,
    PlannerObjectiveAligned,
    PlannerObjectiveContradiction,
    PlannerTrendIntentMismatch,
    PlannerRetryNoSemanticProgress,
    PlannerRetryTrendStalled,
    RouteSemanticPreconditionActionable,
    RouteSemanticRepairIntentActionable,
    RouteSemanticValidationBlockedActionable,
    RouteSemanticUnresolvedImportActionable,
    RouteSemanticDuplicateDefinitionActionable,
    RouteSemanticTraitBoundActionable,
    RouteTrendStallActionable,
    RouteObjectiveContradiction,
    GoalRouteObjectiveDrift,
}

impl JudgmentScenarioFamily {
    pub const ALL: [Self; 23] = [
        Self::PlannerBootstrapWorkspace,
        Self::PlannerInitCargoProject,
        Self::PlannerCreateEntrypoint,
        Self::PlannerCreateMissingModules,
        Self::PlannerFixDeadCodeConflict,
        Self::PlannerFixUnresolvedImport,
        Self::PlannerDefineMissingSymbol,
        Self::PlannerResolveDuplicateDefinition,
        Self::PlannerFixTraitBoundFailure,
        Self::PlannerObjectiveAligned,
        Self::PlannerObjectiveContradiction,
        Self::PlannerTrendIntentMismatch,
        Self::PlannerRetryNoSemanticProgress,
        Self::PlannerRetryTrendStalled,
        Self::RouteSemanticPreconditionActionable,
        Self::RouteSemanticRepairIntentActionable,
        Self::RouteSemanticValidationBlockedActionable,
        Self::RouteSemanticUnresolvedImportActionable,
        Self::RouteSemanticDuplicateDefinitionActionable,
        Self::RouteSemanticTraitBoundActionable,
        Self::RouteTrendStallActionable,
        Self::RouteObjectiveContradiction,
        Self::GoalRouteObjectiveDrift,
    ];
}

#[derive(Clone, Debug)]
pub struct PlannerJudgmentRow {
    pub name: &'static str,
    pub family: JudgmentScenarioFamily,
    pub actions: Vec<canon_event::LoopPlanned>,
    pub preconditions: Vec<PlanningPrecondition>,
    pub summary: SemanticStateSummary,
    pub expected_ok: bool,
}

#[derive(Clone, Debug)]
pub struct PlannerObjectiveAlignmentRow {
    pub name: &'static str,
    pub family: JudgmentScenarioFamily,
    pub actions: Vec<canon_event::LoopPlanned>,
    pub summary: SemanticStateSummary,
    pub primary_objective: &'static str,
    pub route_choice: &'static str,
    pub recent_execution_results: Vec<SemanticExecutionResultRecord>,
    pub objective_trend_state: canon_semantic_state::ObjectiveTrendState,
    pub expected_ok: bool,
}

#[derive(Clone, Debug)]
pub struct GoalRouteObjectiveDriftRow {
    pub name: &'static str,
    pub family: JudgmentScenarioFamily,
    pub goal_objective: &'static str,
    pub route_objective: &'static str,
    pub expected_drift: bool,
}

#[derive(Clone, Debug)]
pub struct RouteObjectiveAlignmentRow {
    pub name: &'static str,
    pub family: JudgmentScenarioFamily,
    pub summary: SemanticStateSummary,
    pub primary_objective: &'static str,
    pub route_choice: &'static str,
    pub expected_ok: bool,
}

#[derive(Clone, Debug)]
pub struct RouteSemanticActionabilityRow {
    pub name: &'static str,
    pub family: JudgmentScenarioFamily,
    pub summary: SemanticStateSummary,
    pub objective_trend_state: canon_semantic_state::ObjectiveTrendState,
    pub expected_actionable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannerPathState {
    Missing,
    Present,
}

impl PlannerPathState {
    pub const ALL: [Self; 2] = [Self::Missing, Self::Present];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannerCargoState {
    Missing,
    Present,
}

impl PlannerCargoState {
    pub const ALL: [Self; 2] = [Self::Missing, Self::Present];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannerEntrypointState {
    Missing,
    Main,
}

impl PlannerEntrypointState {
    pub const ALL: [Self; 2] = [Self::Missing, Self::Main];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannerModuleGapState {
    None,
    Present,
}

impl PlannerModuleGapState {
    pub const ALL: [Self; 2] = [Self::None, Self::Present];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannerHintState {
    None,
    DeadCode,
    UnresolvedImport,
    MissingSymbol,
    DuplicateDefinition,
    TraitBound,
}

impl PlannerHintState {
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::DeadCode,
        Self::UnresolvedImport,
        Self::MissingSymbol,
        Self::DuplicateDefinition,
        Self::TraitBound,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannerActionCase {
    BootstrapWorkspace,
    InitCargoProject,
    CreateEntrypoint,
    CreateModuleFile,
    FixDeadCodeConflict,
    FixUnresolvedImport,
    DefineMissingSymbol,
    ResolveDuplicateDefinition,
    FixTraitBoundFailure,
    WrongEdit,
    ValidateCargoCheck,
}

impl PlannerActionCase {
    pub const ALL: [Self; 11] = [
        Self::BootstrapWorkspace,
        Self::InitCargoProject,
        Self::CreateEntrypoint,
        Self::CreateModuleFile,
        Self::FixDeadCodeConflict,
        Self::FixUnresolvedImport,
        Self::DefineMissingSymbol,
        Self::ResolveDuplicateDefinition,
        Self::FixTraitBoundFailure,
        Self::WrongEdit,
        Self::ValidateCargoCheck,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlannerJudgmentState {
    pub path: PlannerPathState,
    pub cargo: PlannerCargoState,
    pub entrypoint: PlannerEntrypointState,
    pub module_gap: PlannerModuleGapState,
    pub hint: PlannerHintState,
    pub action: PlannerActionCase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteSummaryCompleteness {
    Incomplete,
    Complete,
}

impl RouteSummaryCompleteness {
    pub const ALL: [Self; 2] = [Self::Incomplete, Self::Complete];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutePreconditionState {
    None,
    Present,
}

impl RoutePreconditionState {
    pub const ALL: [Self; 2] = [Self::None, Self::Present];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteRepairIntentState {
    None,
    Present,
}

impl RouteRepairIntentState {
    pub const ALL: [Self; 2] = [Self::None, Self::Present];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteHintState {
    None,
    UnresolvedImport,
    DuplicateDefinition,
    TraitBound,
}

impl RouteHintState {
    pub const ALL: [Self; 4] = [
        Self::None,
        Self::UnresolvedImport,
        Self::DuplicateDefinition,
        Self::TraitBound,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteValidationBlockedState {
    No,
    Yes,
}

impl RouteValidationBlockedState {
    pub const ALL: [Self; 2] = [Self::No, Self::Yes];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteSemanticState {
    pub completeness: RouteSummaryCompleteness,
    pub preconditions: RoutePreconditionState,
    pub repair_intents: RouteRepairIntentState,
    pub hint: RouteHintState,
    pub validation_blocked: RouteValidationBlockedState,
}

impl InvalidPlanRetryFamily {
    pub const ALL: [Self; 5] = [
        Self::MixedBatchDiscoveryOnly,
        Self::PatchFormatSinglePatchOnly,
        Self::PathOrCwdCorrectiveRetry,
        Self::MissingContextCorrectiveRetry,
        Self::UnknownCorrectiveRetry,
    ];
}

#[derive(Clone, Debug)]
pub struct RouteTransitionRow {
    pub name: &'static str,
    pub family: RouteScenarioFamily,
    pub context: RouteRowContext,
    pub state: RouteRowState,
    pub event: Option<RouteRowEvent>,
    pub decision: Option<RouteRowDecision>,
    pub expected_deterministic: Option<DeterministicRouteRule>,
    pub expected_rules: Vec<RoutePolicyRule>,
}

#[derive(Clone, Debug, Default)]
pub struct RouteRowContext {
    pub halted: bool,
    pub context_ready: bool,
    pub consecutive_invalid_plan_batches: u32,
    pub planned_pending: usize,
    pub pending_tool_results_empty: bool,
    pub bootstrap_refresh_required: bool,
    pub target_workspace_missing: bool,
    pub finish_ready: bool,
    pub verify_outcome: Option<VerifyOutcomeClass>,
    pub run_command_outcome: Option<RunCommandOutcomeClass>,
    pub apply_patch_outcome: Option<ApplyPatchOutcomeClass>,
    pub semantic_progress: bool,
    pub no_semantic_progress: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RouteRowState {
    pub last_control_kind: Option<&'static str>,
    pub pending_required_successor: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub enum RouteRowEvent {
    LoopActed { action_kind: &'static str },
    PlanningCompleted { status: &'static str, planned_count: usize },
}

#[derive(Clone, Debug)]
pub struct RouteRowDecision {
    pub lane: RouteKind,
    pub suggested_route: RouteKind,
    pub note: &'static str,
}

#[derive(Clone, Debug)]
pub struct LoopTransitionRow {
    pub name: &'static str,
    pub family: LoopScenarioFamily,
    pub pending_required_successor: Option<&'static str>,
    pub planning_status: Option<&'static str>,
    pub error_kind: Option<&'static str>,
    pub expected_successor: Option<&'static str>,
    pub expected_rules: Vec<LoopRecoveryRule>,
    pub expected_trigger_observe: bool,
    pub expected_force_reward_recovery: bool,
    pub expected_observe_blocked: bool,
}

#[derive(Clone, Debug)]
pub struct RouteDispatchRow {
    pub name: &'static str,
    pub family: RouteScenarioFamily,
    pub context: RouteRowContext,
    pub state: RouteRowState,
    pub dispatch: RouteDispatchRowState,
    pub expected_rule: Option<RouteDispatchRule>,
    pub expected_deterministic: Option<DeterministicRouteRule>,
}

#[derive(Clone, Debug, Default)]
pub struct RouteDispatchRowState {
    pub pending_request_id: Option<&'static str>,
    pub awaiting_control_successor: Option<&'static str>,
    pub route_emitted_for_current_control: bool,
}

#[derive(Clone, Debug)]
pub struct RouteEmitRow {
    pub name: &'static str,
    pub family: RouteScenarioFamily,
    pub awaiting_control_successor: Option<&'static str>,
    pub last_control_kind: Option<&'static str>,
    pub pending_required_successor: Option<&'static str>,
    pub expected_rule: RouteEmitRule,
}

#[derive(Clone, Debug)]
pub struct RouteCacheRow {
    pub name: &'static str,
    pub family: RouteScenarioFamily,
    pub state: RouteCacheRowState,
    pub expected_rule: RouteCacheRule,
}

#[derive(Clone, Debug)]
pub struct RouteCacheRowState {
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

#[derive(Clone, Debug)]
pub struct LoopRuntimeRow {
    pub name: &'static str,
    pub family: LoopScenarioFamily,
    pub halted: bool,
    pub force_observe_recovery: bool,
    pub trigger_observe: bool,
    pub suppress_observe_on_invariant: bool,
    pub pending_required_successor: Option<&'static str>,
    pub is_route_selected_event: bool,
    pub expected_mode: ObserveExecutionMode,
    pub expected_halt_blocks_stage: bool,
    pub expected_warn_route_selected_while_halted: bool,
    pub expected_rules: Vec<LoopRuntimeRule>,
}

#[derive(Clone, Debug)]
pub struct RecoveryEventRow {
    pub name: &'static str,
    pub family: LoopScenarioFamily,
    pub expected_successor: Option<&'static str>,
    pub pending_required_successor: Option<&'static str>,
    pub has_last_verified: bool,
    pub expected_rule: RecoveryEventRule,
    pub expected_force_observe_recovery: bool,
    pub expected_execute_reward_recovery: bool,
}

#[derive(Clone, Debug)]
pub struct RecoveryExecutionRow {
    pub name: &'static str,
    pub family: LoopScenarioFamily,
    pub operation: RecoveryOperation,
    pub outcome: StageExecutionOutcomeClass,
    pub expected_debug_kind: Option<&'static str>,
    pub expected_error_kind: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct BootstrapEffectRow {
    pub name: &'static str,
    pub family: LoopScenarioFamily,
    pub action_outcome: ActionOutcomeClass,
    pub expected_rule: BootstrapRule,
    pub expected_emit_refresh_required: bool,
}

#[derive(Clone, Debug)]
pub struct PlannerRecoveryRow {
    pub name: &'static str,
    pub family: JudgmentScenarioFamily,
    pub reason: Option<&'static str>,
    pub consecutive_invalid_plan_batches: u32,
    pub recent_execution_results: Vec<SemanticExecutionResultRecord>,
    pub objective_trend_state: canon_semantic_state::ObjectiveTrendState,
    pub expected_retry: RetryPolicy,
}

#[derive(Clone, Debug)]
pub struct RewardSemanticsRow {
    pub name: &'static str,
    pub family: LoopScenarioFamily,
    pub compiler_clean: bool,
    pub last_action_kind: &'static str,
    pub recent_execution_results: Vec<SemanticExecutionResultRecord>,
    pub expected: RewardSemantics,
}

#[derive(Clone, Debug)]
pub struct RouteFailureRow {
    pub name: &'static str,
    pub family: RouteScenarioFamily,
    pub expected_rule: RouteFailureRule,
}

#[derive(Clone, Debug)]
pub struct RouteEmitEffectRow {
    pub name: &'static str,
    pub family: RouteScenarioFamily,
    pub decision: RouteRowDecision,
    pub expected_rules: Vec<RouteEmitEffectRule>,
    pub expected_clear_pending_request: bool,
    pub expected_clear_pending_prompt: bool,
    pub expected_set_halted: bool,
}

#[derive(Clone, Debug)]
pub struct RouteRecoveryRow {
    pub name: &'static str,
    pub family: RouteScenarioFamily,
    pub pending_required_successor: Option<&'static str>,
    pub expected_rule: RouteRecoveryRule,
}

#[derive(Clone, Debug)]
pub struct SuccessorConsumptionRow {
    pub name: &'static str,
    pub family: RouteScenarioFamily,
    pub event: RouteRowEvent,
    pub awaiting_control_successor: Option<&'static str>,
    pub expected_rule: SuccessorConsumptionRule,
}

#[derive(Clone, Debug)]
pub struct RunCommandOutcomeRow {
    pub name: &'static str,
    pub family: RunCommandOutcomeFamily,
    pub input: RunCommandOutcomeClass,
    pub expected: RunCommandOutcomeClass,
}

#[derive(Clone, Debug)]
pub struct ApplyPatchOutcomeRow {
    pub name: &'static str,
    pub family: ApplyPatchOutcomeFamily,
    pub input: ApplyPatchOutcomeClass,
    pub expected: ApplyPatchOutcomeClass,
}

#[derive(Clone, Debug)]
pub struct VerifyOutcomeRow {
    pub name: &'static str,
    pub family: VerifyOutcomeFamily,
    pub input: VerifyOutcomeClass,
    pub expected: VerifyOutcomeClass,
}

#[derive(Clone, Debug)]
pub struct InvalidPlanRetryRow {
    pub name: &'static str,
    pub family: InvalidPlanRetryFamily,
    pub reason: Option<&'static str>,
    pub count: u32,
    pub expected_reason_class: InvalidPlanReasonClass,
    pub expected_retry: RetryPolicy,
}

#[derive(Clone, Debug, Default)]
pub struct CoverageReport {
    pub route_covered: Vec<RouteScenarioFamily>,
    pub route_missing: Vec<RouteScenarioFamily>,
    pub loop_covered: Vec<LoopScenarioFamily>,
    pub loop_missing: Vec<LoopScenarioFamily>,
    pub run_command_covered: Vec<RunCommandOutcomeFamily>,
    pub run_command_missing: Vec<RunCommandOutcomeFamily>,
    pub apply_patch_covered: Vec<ApplyPatchOutcomeFamily>,
    pub apply_patch_missing: Vec<ApplyPatchOutcomeFamily>,
    pub verify_covered: Vec<VerifyOutcomeFamily>,
    pub verify_missing: Vec<VerifyOutcomeFamily>,
    pub invalid_plan_retry_covered: Vec<InvalidPlanRetryFamily>,
    pub invalid_plan_retry_missing: Vec<InvalidPlanRetryFamily>,
    pub judgment_covered: Vec<JudgmentScenarioFamily>,
    pub judgment_missing: Vec<JudgmentScenarioFamily>,
    pub planner_generated_total: usize,
    pub planner_generated_valid: usize,
    pub route_generated_total: usize,
    pub route_generated_valid: usize,
}

pub fn assert_transition_rows(rows: &[TransitionRow]) {
    for row in rows {
        match row {
            TransitionRow::Route(row) => assert_route_row(row),
            TransitionRow::Loop(row) => assert_loop_row(row),
            TransitionRow::RunCommandOutcome(row) => assert_run_command_outcome_row(row),
            TransitionRow::ApplyPatchOutcome(row) => assert_apply_patch_outcome_row(row),
            TransitionRow::VerifyOutcome(row) => assert_verify_outcome_row(row),
            TransitionRow::InvalidPlanRetry(row) => assert_invalid_plan_retry_row(row),
            TransitionRow::RouteDispatch(row) => assert_route_dispatch_row(row),
            TransitionRow::RouteEmit(row) => assert_route_emit_row(row),
            TransitionRow::RouteCache(row) => assert_route_cache_row(row),
            TransitionRow::LoopRuntime(row) => assert_loop_runtime_row(row),
            TransitionRow::RecoveryEvent(row) => assert_recovery_event_row(row),
            TransitionRow::RecoveryExecution(row) => assert_recovery_execution_row(row),
            TransitionRow::BootstrapEffect(row) => assert_bootstrap_effect_row(row),
            TransitionRow::PlannerRecovery(row) => assert_planner_recovery_row(row),
            TransitionRow::RewardSemantics(row) => assert_reward_semantics_row(row),
            TransitionRow::RouteFailure(row) => assert_route_failure_row(row),
            TransitionRow::RouteEmitEffect(row) => assert_route_emit_effect_row(row),
            TransitionRow::RouteRecovery(row) => assert_route_recovery_row(row),
            TransitionRow::SuccessorConsumption(row) => assert_successor_consumption_row(row),
            TransitionRow::PlannerJudgment(row) => assert_planner_judgment_row(row),
            TransitionRow::PlannerObjectiveAlignment(row) => assert_planner_objective_alignment_row(row),
            TransitionRow::RouteObjectiveAlignment(row) => assert_route_objective_alignment_row(row),
            TransitionRow::GoalRouteObjectiveDrift(row) => assert_goal_route_objective_drift_row(row),
            TransitionRow::RouteSemanticActionability(row) => {
                assert_route_semantic_actionability_row(row)
            }
        }
    }
}

pub fn coverage_report(rows: &[TransitionRow]) -> CoverageReport {
    let mut report = CoverageReport::default();

    for row in rows {
        match row {
            TransitionRow::Route(row) => push_unique(&mut report.route_covered, row.family),
            TransitionRow::RouteDispatch(row) => push_unique(&mut report.route_covered, row.family),
            TransitionRow::RouteEmit(row) => push_unique(&mut report.route_covered, row.family),
            TransitionRow::RouteCache(row) => push_unique(&mut report.route_covered, row.family),
            TransitionRow::RouteFailure(row) => push_unique(&mut report.route_covered, row.family),
            TransitionRow::RouteEmitEffect(row) => push_unique(&mut report.route_covered, row.family),
            TransitionRow::Loop(row) => push_unique(&mut report.loop_covered, row.family),
            TransitionRow::LoopRuntime(row) => push_unique(&mut report.loop_covered, row.family),
            TransitionRow::RecoveryEvent(row) => push_unique(&mut report.loop_covered, row.family),
            TransitionRow::RecoveryExecution(row) => push_unique(&mut report.loop_covered, row.family),
            TransitionRow::BootstrapEffect(row) => push_unique(&mut report.loop_covered, row.family),
            TransitionRow::PlannerRecovery(row) => push_unique(&mut report.judgment_covered, row.family),
            TransitionRow::RewardSemantics(row) => push_unique(&mut report.loop_covered, row.family),
            TransitionRow::RunCommandOutcome(row) => push_unique(&mut report.run_command_covered, row.family),
            TransitionRow::ApplyPatchOutcome(row) => push_unique(&mut report.apply_patch_covered, row.family),
            TransitionRow::VerifyOutcome(row) => push_unique(&mut report.verify_covered, row.family),
            TransitionRow::InvalidPlanRetry(row) => push_unique(&mut report.invalid_plan_retry_covered, row.family),
            TransitionRow::RouteRecovery(row) => push_unique(&mut report.route_covered, row.family),
            TransitionRow::SuccessorConsumption(row) => push_unique(&mut report.route_covered, row.family),
            TransitionRow::PlannerJudgment(row) => push_unique(&mut report.judgment_covered, row.family),
            TransitionRow::PlannerObjectiveAlignment(row) => {
                push_unique(&mut report.judgment_covered, row.family)
            }
            TransitionRow::RouteObjectiveAlignment(row) => {
                push_unique(&mut report.judgment_covered, row.family)
            }
            TransitionRow::GoalRouteObjectiveDrift(row) => {
                push_unique(&mut report.judgment_covered, row.family)
            }
            TransitionRow::RouteSemanticActionability(row) => {
                push_unique(&mut report.judgment_covered, row.family)
            }
        }
    }

    report.route_missing = missing_families(&RouteScenarioFamily::ALL, &report.route_covered);
    report.loop_missing = missing_families(&LoopScenarioFamily::ALL, &report.loop_covered);
    report.run_command_missing = missing_families(&RunCommandOutcomeFamily::ALL, &report.run_command_covered);
    report.apply_patch_missing = missing_families(&ApplyPatchOutcomeFamily::ALL, &report.apply_patch_covered);
    report.verify_missing = missing_families(&VerifyOutcomeFamily::ALL, &report.verify_covered);
    report.invalid_plan_retry_missing =
        missing_families(&InvalidPlanRetryFamily::ALL, &report.invalid_plan_retry_covered);
    report.judgment_missing = missing_families(&JudgmentScenarioFamily::ALL, &report.judgment_covered);
    report.planner_generated_total = planner_judgment_state_count(false);
    report.planner_generated_valid = planner_judgment_state_count(true);
    report.route_generated_total = route_semantic_state_count(false);
    report.route_generated_valid = route_semantic_state_count(true);

    report
}

pub fn baseline_transition_rows() -> Vec<TransitionRow> {
    let mut rows = Vec::new();
    rows.extend(route_transition_rows().into_iter().map(TransitionRow::Route));
    rows.extend(route_dispatch_rows().into_iter().map(TransitionRow::RouteDispatch));
    rows.extend(route_emit_rows().into_iter().map(TransitionRow::RouteEmit));
    rows.extend(route_cache_rows().into_iter().map(TransitionRow::RouteCache));
    rows.extend(route_failure_rows().into_iter().map(TransitionRow::RouteFailure));
    rows.extend(route_emit_effect_rows().into_iter().map(TransitionRow::RouteEmitEffect));
    rows.extend(loop_transition_rows().into_iter().map(TransitionRow::Loop));
    rows.extend(loop_runtime_rows().into_iter().map(TransitionRow::LoopRuntime));
    rows.extend(recovery_event_rows().into_iter().map(TransitionRow::RecoveryEvent));
    rows.extend(recovery_execution_rows().into_iter().map(TransitionRow::RecoveryExecution));
    rows.extend(bootstrap_effect_rows().into_iter().map(TransitionRow::BootstrapEffect));
    rows.extend(planner_recovery_rows().into_iter().map(TransitionRow::PlannerRecovery));
    rows.extend(reward_semantics_rows().into_iter().map(TransitionRow::RewardSemantics));
    rows.extend(run_command_outcome_rows().into_iter().map(TransitionRow::RunCommandOutcome));
    rows.extend(apply_patch_outcome_rows().into_iter().map(TransitionRow::ApplyPatchOutcome));
    rows.extend(verify_outcome_rows().into_iter().map(TransitionRow::VerifyOutcome));
    rows.extend(invalid_plan_retry_rows().into_iter().map(TransitionRow::InvalidPlanRetry));
    rows.extend(route_recovery_rows().into_iter().map(TransitionRow::RouteRecovery));
    rows.extend(successor_consumption_rows().into_iter().map(TransitionRow::SuccessorConsumption));
    rows.extend(planner_judgment_rows().into_iter().map(TransitionRow::PlannerJudgment));
    rows.extend(
        planner_objective_alignment_rows()
            .into_iter()
            .map(TransitionRow::PlannerObjectiveAlignment),
    );
    rows.extend(
        route_objective_alignment_rows()
            .into_iter()
            .map(TransitionRow::RouteObjectiveAlignment),
    );
    rows.extend(
        goal_route_objective_drift_rows()
            .into_iter()
            .map(TransitionRow::GoalRouteObjectiveDrift),
    );
    rows.extend(
        route_semantic_actionability_rows()
            .into_iter()
            .map(TransitionRow::RouteSemanticActionability),
    );
    rows.extend(
        route_trend_actionability_rows()
            .into_iter()
            .map(TransitionRow::RouteSemanticActionability),
    );
    rows
}

pub fn route_transition_rows() -> Vec<RouteTransitionRow> {
    vec![
        RouteTransitionRow {
            name: "bootstrap_refresh_observe",
            family: RouteScenarioFamily::BootstrapRefreshObserve,
            context: RouteRowContext {
                bootstrap_refresh_required: true,
                pending_tool_results_empty: true,
                ..RouteRowContext::default()
            },
            state: RouteRowState::default(),
            event: Some(RouteRowEvent::LoopActed { action_kind: "run_command" }),
            decision: None,
            expected_deterministic: Some(DeterministicRouteRule::BootstrapRefreshObserve),
            expected_rules: vec![],
        },
        RouteTransitionRow {
            name: "done_verify",
            family: RouteScenarioFamily::DoneVerify,
            context: RouteRowContext {
                pending_tool_results_empty: true,
                ..RouteRowContext::default()
            },
            state: RouteRowState::default(),
            event: Some(RouteRowEvent::LoopActed { action_kind: "done" }),
            decision: None,
            expected_deterministic: Some(DeterministicRouteRule::DoneVerify),
            expected_rules: vec![],
        },
        RouteTransitionRow {
            name: "semantic_progress_verify",
            family: RouteScenarioFamily::SemanticProgressVerify,
            context: RouteRowContext {
                pending_tool_results_empty: true,
                semantic_progress: true,
                ..RouteRowContext::default()
            },
            state: RouteRowState::default(),
            event: Some(RouteRowEvent::LoopActed { action_kind: "apply_patch" }),
            decision: None,
            expected_deterministic: Some(DeterministicRouteRule::SemanticProgressVerify),
            expected_rules: vec![],
        },
        RouteTransitionRow {
            name: "no_semantic_progress_plan",
            family: RouteScenarioFamily::NoSemanticProgressPlan,
            context: RouteRowContext {
                pending_tool_results_empty: true,
                no_semantic_progress: true,
                ..RouteRowContext::default()
            },
            state: RouteRowState::default(),
            event: Some(RouteRowEvent::LoopActed { action_kind: "apply_patch" }),
            decision: None,
            expected_deterministic: Some(DeterministicRouteRule::NoSemanticProgressPlan),
            expected_rules: vec![],
        },
        RouteTransitionRow {
            name: "continue_act",
            family: RouteScenarioFamily::ContinueAct,
            context: RouteRowContext {
                planned_pending: 2,
                pending_tool_results_empty: true,
                ..RouteRowContext::default()
            },
            state: RouteRowState::default(),
            event: Some(RouteRowEvent::LoopActed { action_kind: "apply_patch" }),
            decision: None,
            expected_deterministic: Some(DeterministicRouteRule::ContinueAct),
            expected_rules: vec![],
        },
        RouteTransitionRow {
            name: "planned_to_act",
            family: RouteScenarioFamily::PlannedToAct,
            context: RouteRowContext {
                planned_pending: 3,
                pending_tool_results_empty: true,
                ..RouteRowContext::default()
            },
            state: RouteRowState::default(),
            event: Some(RouteRowEvent::PlanningCompleted {
                status: "planned",
                planned_count: 3,
            }),
            decision: None,
            expected_deterministic: Some(DeterministicRouteRule::PlannedToAct),
            expected_rules: vec![],
        },
        RouteTransitionRow {
            name: "missing_observed_context_observe",
            family: RouteScenarioFamily::MissingObservedContextObserve,
            context: RouteRowContext {
                pending_tool_results_empty: true,
                ..RouteRowContext::default()
            },
            state: RouteRowState::default(),
            event: Some(RouteRowEvent::PlanningCompleted {
                status: "missing_observed_context",
                planned_count: 0,
            }),
            decision: None,
            expected_deterministic: Some(DeterministicRouteRule::MissingObservedContextObserve),
            expected_rules: vec![],
        },
        RouteTransitionRow {
            name: "repeat_observe_forces_plan",
            family: RouteScenarioFamily::ForcePlanOnRepeatedObserve,
            context: RouteRowContext::default(),
            state: RouteRowState {
                last_control_kind: Some("loop_observed"),
                pending_required_successor: Some("route_selected"),
            },
            event: None,
            decision: Some(RouteRowDecision {
                lane: RouteKind::Observe,
                suggested_route: RouteKind::Observe,
                note: "accepted",
            }),
            expected_deterministic: None,
            expected_rules: vec![RoutePolicyRule::ForcePlanOnRepeatedObserve],
        },
        RouteTransitionRow {
            name: "missing_target_forces_plan",
            family: RouteScenarioFamily::ForcePlanOnMissingTarget,
            context: RouteRowContext {
                target_workspace_missing: true,
                ..RouteRowContext::default()
            },
            state: RouteRowState::default(),
            event: None,
            decision: Some(RouteRowDecision {
                lane: RouteKind::Verify,
                suggested_route: RouteKind::Verify,
                note: "accepted",
            }),
            expected_deterministic: None,
            expected_rules: vec![RoutePolicyRule::ForcePlanOnMissingTarget],
        },
        RouteTransitionRow {
            name: "cycle_cap_with_verify_failure_to_plan",
            family: RouteScenarioFamily::CycleCapToPlan,
            context: RouteRowContext {
                verify_outcome: Some(VerifyOutcomeClass::CompilerFailure),
                ..RouteRowContext::default()
            },
            state: RouteRowState::default(),
            event: None,
            decision: Some(RouteRowDecision {
                lane: RouteKind::Conclude,
                suggested_route: RouteKind::Plan,
                note: "cycle cap reached; forcing conclude",
            }),
            expected_deterministic: None,
            expected_rules: vec![RoutePolicyRule::CycleCapToPlan],
        },
        RouteTransitionRow {
            name: "cycle_cap_without_failure_to_observe",
            family: RouteScenarioFamily::CycleCapToObserve,
            context: RouteRowContext::default(),
            state: RouteRowState::default(),
            event: None,
            decision: Some(RouteRowDecision {
                lane: RouteKind::Conclude,
                suggested_route: RouteKind::Conclude,
                note: "cycle cap reached; forcing conclude",
            }),
            expected_deterministic: None,
            expected_rules: vec![RoutePolicyRule::CycleCapToObserve],
        },
        RouteTransitionRow {
            name: "accepted_plan_without_rewrite",
            family: RouteScenarioFamily::NoRewriteAccepted,
            context: RouteRowContext {
                finish_ready: true,
                ..RouteRowContext::default()
            },
            state: RouteRowState::default(),
            event: None,
            decision: Some(RouteRowDecision {
                lane: RouteKind::Conclude,
                suggested_route: RouteKind::Conclude,
                note: "accepted",
            }),
            expected_deterministic: None,
            expected_rules: vec![],
        },
    ]
}

pub fn route_dispatch_rows() -> Vec<RouteDispatchRow> {
    vec![
        RouteDispatchRow {
            name: "dispatch_suppress_halted",
            family: RouteScenarioFamily::DispatchSuppressHalted,
            context: RouteRowContext { halted: true, context_ready: true, ..RouteRowContext::default() },
            state: RouteRowState::default(),
            dispatch: RouteDispatchRowState::default(),
            expected_rule: Some(RouteDispatchRule::SuppressHalted),
            expected_deterministic: None,
        },
        RouteDispatchRow {
            name: "dispatch_suppress_context_not_ready",
            family: RouteScenarioFamily::DispatchSuppressContextNotReady,
            context: RouteRowContext { halted: false, context_ready: false, ..RouteRowContext::default() },
            state: RouteRowState::default(),
            dispatch: RouteDispatchRowState::default(),
            expected_rule: Some(RouteDispatchRule::SuppressContextNotReady),
            expected_deterministic: None,
        },
        RouteDispatchRow {
            name: "dispatch_suppress_pending_request",
            family: RouteScenarioFamily::DispatchSuppressPendingRequest,
            context: RouteRowContext { context_ready: true, pending_tool_results_empty: true, ..RouteRowContext::default() },
            state: RouteRowState::default(),
            dispatch: RouteDispatchRowState { pending_request_id: Some("req-1"), ..RouteDispatchRowState::default() },
            expected_rule: Some(RouteDispatchRule::SuppressPendingRequest),
            expected_deterministic: None,
        },
        RouteDispatchRow {
            name: "dispatch_suppress_awaiting_successor",
            family: RouteScenarioFamily::DispatchSuppressAwaitingSuccessor,
            context: RouteRowContext { context_ready: true, ..RouteRowContext::default() },
            state: RouteRowState::default(),
            dispatch: RouteDispatchRowState { awaiting_control_successor: Some("loop_acted"), ..RouteDispatchRowState::default() },
            expected_rule: Some(RouteDispatchRule::SuppressAwaitingControlSuccessor),
            expected_deterministic: None,
        },
        RouteDispatchRow {
            name: "dispatch_suppress_duplicate_current_control",
            family: RouteScenarioFamily::DispatchSuppressDuplicateCurrentControl,
            context: RouteRowContext { context_ready: true, ..RouteRowContext::default() },
            state: RouteRowState { last_control_kind: Some("loop_acted"), pending_required_successor: Some("route_selected") },
            dispatch: RouteDispatchRowState { route_emitted_for_current_control: true, ..RouteDispatchRowState::default() },
            expected_rule: Some(RouteDispatchRule::SuppressDuplicateRouteForCurrentControl),
            expected_deterministic: None,
        },
        RouteDispatchRow {
            name: "dispatch_missing_target_plan",
            family: RouteScenarioFamily::DispatchMissingTargetPlan,
            context: RouteRowContext { context_ready: true, target_workspace_missing: true, pending_tool_results_empty: true, ..RouteRowContext::default() },
            state: RouteRowState::default(),
            dispatch: RouteDispatchRowState::default(),
            expected_rule: None,
            expected_deterministic: Some(DeterministicRouteRule::MissingTargetPlan),
        },
        RouteDispatchRow {
            name: "dispatch_invalid_plan_replan",
            family: RouteScenarioFamily::DispatchInvalidPlanReplan,
            context: RouteRowContext { context_ready: true, consecutive_invalid_plan_batches: 2, pending_tool_results_empty: true, ..RouteRowContext::default() },
            state: RouteRowState::default(),
            dispatch: RouteDispatchRowState::default(),
            expected_rule: None,
            expected_deterministic: Some(DeterministicRouteRule::InvalidPlanReplan),
        },
    ]
}

pub fn route_emit_rows() -> Vec<RouteEmitRow> {
    vec![
        RouteEmitRow {
            name: "emit_duplicate_before_successor",
            family: RouteScenarioFamily::EmitDuplicateBeforeSuccessor,
            awaiting_control_successor: Some("loop_observed"),
            last_control_kind: None,
            pending_required_successor: None,
            expected_rule: RouteEmitRule::DuplicateEmitBeforeSuccessor,
        },
        RouteEmitRow {
            name: "emit_illegal_control_reentry",
            family: RouteScenarioFamily::EmitIllegalControlReentry,
            awaiting_control_successor: None,
            last_control_kind: Some("route_selected"),
            pending_required_successor: Some("loop_observed"),
            expected_rule: RouteEmitRule::IllegalControlReentry,
        },
        RouteEmitRow {
            name: "emit_wrong_expected_successor",
            family: RouteScenarioFamily::EmitWrongExpectedSuccessor,
            awaiting_control_successor: None,
            last_control_kind: Some("loop_verified"),
            pending_required_successor: Some("loop_rewarded"),
            expected_rule: RouteEmitRule::IllegalControlEmit,
        },
    ]
}

pub fn route_cache_rows() -> Vec<RouteCacheRow> {
    vec![
        RouteCacheRow {
            name: "cache_replay",
            family: RouteScenarioFamily::CacheReplay,
            state: RouteCacheRowState {
                force_fresh_route_once: false,
                last_prompt_hash: Some(7),
                prompt_hash: 7,
                pending_required_successor: Some("route_selected"),
                last_route_prompt_hash: Some(7),
                route_emitted_for_current_control: false,
                has_cached_route: true,
                cached_route_is_observe: false,
                can_emit_route_selected: true,
            },
            expected_rule: RouteCacheRule::ReplayCachedRoute,
        },
        RouteCacheRow {
            name: "cache_invalidate_observe",
            family: RouteScenarioFamily::CacheInvalidateObserve,
            state: RouteCacheRowState {
                force_fresh_route_once: false,
                last_prompt_hash: Some(7),
                prompt_hash: 7,
                pending_required_successor: Some("route_selected"),
                last_route_prompt_hash: Some(7),
                route_emitted_for_current_control: false,
                has_cached_route: true,
                cached_route_is_observe: true,
                can_emit_route_selected: true,
            },
            expected_rule: RouteCacheRule::InvalidateCachedObserveRoute,
        },
        RouteCacheRow {
            name: "cache_suppress_duplicate_prompt",
            family: RouteScenarioFamily::CacheSuppressDuplicatePrompt,
            state: RouteCacheRowState {
                force_fresh_route_once: false,
                last_prompt_hash: Some(7),
                prompt_hash: 7,
                pending_required_successor: None,
                last_route_prompt_hash: None,
                route_emitted_for_current_control: false,
                has_cached_route: false,
                cached_route_is_observe: false,
                can_emit_route_selected: false,
            },
            expected_rule: RouteCacheRule::SuppressDuplicatePrompt,
        },
    ]
}

pub fn route_failure_rows() -> Vec<RouteFailureRow> {
    vec![RouteFailureRow {
        name: "failure_heuristic_reroute",
        family: RouteScenarioFamily::FailureHeuristicReroute,
        expected_rule: RouteFailureRule::HeuristicFailureReroute,
    }]
}

pub fn route_emit_effect_rows() -> Vec<RouteEmitEffectRow> {
    vec![
        RouteEmitEffectRow {
            name: "emit_effect_observe_clears_deterministic_sentinel",
            family: RouteScenarioFamily::EmitEffectObserveClearsDeterministicSentinel,
            decision: RouteRowDecision {
                lane: RouteKind::Observe,
                suggested_route: RouteKind::Observe,
                note: "accepted",
            },
            expected_rules: vec![RouteEmitEffectRule::ClearDeterministicObserveSentinel],
            expected_clear_pending_request: true,
            expected_clear_pending_prompt: true,
            expected_set_halted: false,
        },
        RouteEmitEffectRow {
            name: "emit_effect_conclude_halts",
            family: RouteScenarioFamily::EmitEffectConcludeHalts,
            decision: RouteRowDecision {
                lane: RouteKind::Conclude,
                suggested_route: RouteKind::Conclude,
                note: "accepted",
            },
            expected_rules: vec![RouteEmitEffectRule::HaltOnConclude],
            expected_clear_pending_request: false,
            expected_clear_pending_prompt: false,
            expected_set_halted: true,
        },
    ]
}

pub fn route_recovery_rows() -> Vec<RouteRecoveryRow> {
    vec![RouteRecoveryRow {
        name: "route_recovery_emit_expected_successor",
        family: RouteScenarioFamily::RecoveryEmitExpectedSuccessor,
        pending_required_successor: Some("loop_rewarded"),
        expected_rule: RouteRecoveryRule::EmitExpectedSuccessorRecovery,
    }]
}

pub fn successor_consumption_rows() -> Vec<SuccessorConsumptionRow> {
    vec![SuccessorConsumptionRow {
        name: "successor_consumes_awaiting",
        family: RouteScenarioFamily::SuccessorConsumesAwaiting,
        event: RouteRowEvent::LoopActed { action_kind: "apply_patch" },
        awaiting_control_successor: Some("loop_acted"),
        expected_rule: SuccessorConsumptionRule::ClearAwaitingControlSuccessor,
    }]
}

pub fn planner_judgment_rows() -> Vec<PlannerJudgmentRow> {
    let mut rows = Vec::new();
    for path in PlannerPathState::ALL {
        for cargo in PlannerCargoState::ALL {
            for entrypoint in PlannerEntrypointState::ALL {
                for module_gap in PlannerModuleGapState::ALL {
                    for hint in PlannerHintState::ALL {
                        for action in PlannerActionCase::ALL {
                            let state = PlannerJudgmentState {
                                path,
                                cargo,
                                entrypoint,
                                module_gap,
                                hint,
                                action,
                            };
                            if !valid_planner_judgment_state(state) {
                                continue;
                            }
                            let preconditions = planner_preconditions_for_state(state);
                            let Some(primary) = preconditions.first() else {
                                continue;
                            };
                            let family = planner_family_for_precondition(primary);
                            let actions = planner_actions_for_state(state);
                            rows.push(PlannerJudgmentRow {
                                name: Box::leak(
                                    format!("planner_judgment_{state:?}").into_boxed_str(),
                                ),
                                family,
                                actions,
                                preconditions: preconditions.clone(),
                                summary: planner_summary_for_state(state),
                                expected_ok: planner_action_matches_primary_intent(state),
                            });
                        }
                    }
                }
            }
        }
    }
    rows
}

pub fn route_semantic_actionability_rows() -> Vec<RouteSemanticActionabilityRow> {
    let mut rows = Vec::new();
    for completeness in RouteSummaryCompleteness::ALL {
        for preconditions in RoutePreconditionState::ALL {
            for repair_intents in RouteRepairIntentState::ALL {
                for hint in RouteHintState::ALL {
                    for validation_blocked in RouteValidationBlockedState::ALL {
                        let state = RouteSemanticState {
                            completeness,
                            preconditions,
                            repair_intents,
                            hint,
                            validation_blocked,
                        };
                        if !valid_route_semantic_state(state) {
                            continue;
                        }
                        let Some(family) = route_family_for_state(state) else {
                            continue;
                        };
                        rows.push(RouteSemanticActionabilityRow {
                            name: Box::leak(
                                format!("route_semantic_actionability_{state:?}")
                                    .into_boxed_str(),
                            ),
                            family,
                            summary: route_summary_for_state(state),
                            objective_trend_state: canon_semantic_state::ObjectiveTrendState::default(),
                            expected_actionable: route_state_is_actionable(state),
                        });
                    }
                }
            }
        }
    }
    rows
}

pub fn route_trend_actionability_rows() -> Vec<RouteSemanticActionabilityRow> {
    vec![RouteSemanticActionabilityRow {
        name: "route_trend_stall_actionability",
        family: JudgmentScenarioFamily::RouteTrendStallActionable,
        summary: SemanticStateSummary {
            complete: true,
            path_exists: true,
            cargo_project: true,
            ..SemanticStateSummary::default()
        },
        objective_trend_state: canon_semantic_state::ObjectiveTrendState {
            repeated_stall_count: 1,
            current_no_progress_streak: 1,
            ..canon_semantic_state::ObjectiveTrendState::default()
        },
        expected_actionable: true,
    }]
}

pub fn planner_objective_alignment_rows() -> Vec<PlannerObjectiveAlignmentRow> {
    vec![
        PlannerObjectiveAlignmentRow {
            name: "planner_objective_aligned_repair_edit",
            family: JudgmentScenarioFamily::PlannerObjectiveAligned,
            actions: vec![planned_update_file("src/lib.rs", "+use crate::foo;\n")],
            summary: SemanticStateSummary {
                complete: true,
                path_exists: true,
                cargo_project: true,
                target_root: Some("/tmp/example".into()),
                planning_preconditions: vec!["must_fix_unresolved_import=true".into()],
                compiler_repair_required: true,
                ..SemanticStateSummary::default()
            },
            primary_objective: "reduce compiler repair pressure",
            route_choice: "plan",
            recent_execution_results: Vec::new(),
            objective_trend_state: canon_semantic_state::ObjectiveTrendState::default(),
            expected_ok: true,
        },
        PlannerObjectiveAlignmentRow {
            name: "planner_objective_contradiction_validate_only",
            family: JudgmentScenarioFamily::PlannerObjectiveContradiction,
            actions: vec![planned_run_command("cargo check", "/tmp/example")],
            summary: SemanticStateSummary {
                complete: true,
                path_exists: true,
                cargo_project: true,
                target_root: Some("/tmp/example".into()),
                planning_preconditions: vec!["must_fix_unresolved_import=true".into()],
                compiler_repair_required: true,
                ..SemanticStateSummary::default()
            },
            primary_objective: "reduce compiler repair pressure",
            route_choice: "plan",
            recent_execution_results: Vec::new(),
            objective_trend_state: canon_semantic_state::ObjectiveTrendState::default(),
            expected_ok: false,
        },
        PlannerObjectiveAlignmentRow {
            name: "planner_trend_intent_mismatch_repeats_stalled_import_fix",
            family: JudgmentScenarioFamily::PlannerTrendIntentMismatch,
            actions: vec![planned_update_file("src/lib.rs", "+use crate::foo;\n")],
            summary: SemanticStateSummary {
                complete: true,
                path_exists: true,
                cargo_project: true,
                target_root: Some("/tmp/example".into()),
                compiler_repair_required: true,
                ..SemanticStateSummary::default()
            },
            primary_objective: "break the stalled repair loop with a different strategy",
            route_choice: "plan",
            recent_execution_results: vec![
                SemanticExecutionResultRecord::new(
                    "no_semantic_progress",
                    "fix_unresolved_import failed: unresolved import persists",
                    vec!["src/lib.rs".into()],
                    false,
                )
                .with_attempted_kind("fix_unresolved_import"),
            ],
            objective_trend_state: canon_semantic_state::ObjectiveTrendState {
                repeated_stall_count: 1,
                current_no_progress_streak: 1,
                ..canon_semantic_state::ObjectiveTrendState::default()
            },
            expected_ok: false,
        },
    ]
}

pub fn goal_route_objective_drift_rows() -> Vec<GoalRouteObjectiveDriftRow> {
    vec![GoalRouteObjectiveDriftRow {
        name: "goal_route_objective_drift_repair_vs_sustain",
        family: JudgmentScenarioFamily::GoalRouteObjectiveDrift,
        goal_objective: "reduce compiler repair pressure",
        route_objective: "sustain semantic progress while reducing repair pressure",
        expected_drift: true,
    }]
}

pub fn route_objective_alignment_rows() -> Vec<RouteObjectiveAlignmentRow> {
    vec![RouteObjectiveAlignmentRow {
        name: "route_objective_contradiction_verify_under_repair_pressure",
        family: JudgmentScenarioFamily::RouteObjectiveContradiction,
        summary: SemanticStateSummary {
            complete: true,
            path_exists: true,
            cargo_project: true,
            compiler_repair_required: true,
            planning_preconditions: vec!["must_fix_unresolved_import=true".into()],
            ..SemanticStateSummary::default()
        },
        primary_objective: "reduce compiler repair pressure",
        route_choice: "verify",
        expected_ok: false,
    }]
}

fn valid_planner_judgment_state(state: PlannerJudgmentState) -> bool {
    if state.path == PlannerPathState::Missing {
        return state.cargo == PlannerCargoState::Missing
            && state.entrypoint == PlannerEntrypointState::Missing
            && state.module_gap == PlannerModuleGapState::None
            && state.hint == PlannerHintState::None;
    }
    if state.cargo == PlannerCargoState::Missing {
        return state.entrypoint == PlannerEntrypointState::Missing
            && state.module_gap == PlannerModuleGapState::None
            && state.hint == PlannerHintState::None;
    }
    if state.entrypoint == PlannerEntrypointState::Missing {
        return state.module_gap == PlannerModuleGapState::None && state.hint == PlannerHintState::None;
    }
    true
}

fn planner_preconditions_for_state(state: PlannerJudgmentState) -> Vec<PlanningPrecondition> {
    let mut out = Vec::new();
    if state.path == PlannerPathState::Missing {
        out.push(PlanningPrecondition::MustBootstrapWorkspace);
        return out;
    }
    if state.cargo == PlannerCargoState::Missing {
        out.push(PlanningPrecondition::MustInitCargoProject);
        return out;
    }
    if state.entrypoint == PlannerEntrypointState::Missing {
        out.push(PlanningPrecondition::MustCreateEntrypoint);
        return out;
    }
    if state.module_gap == PlannerModuleGapState::Present {
        out.push(PlanningPrecondition::MustCreateMissingModules);
    }
    match state.hint {
        PlannerHintState::None => {}
        PlannerHintState::DeadCode => out.push(PlanningPrecondition::MustFixDeadCodeForbidConflict),
        PlannerHintState::UnresolvedImport => out.push(PlanningPrecondition::MustFixUnresolvedImport),
        PlannerHintState::MissingSymbol => out.push(PlanningPrecondition::MustDefineMissingSymbol),
        PlannerHintState::DuplicateDefinition => {
            out.push(PlanningPrecondition::MustResolveDuplicateDefinition)
        }
        PlannerHintState::TraitBound => out.push(PlanningPrecondition::MustFixTraitBoundFailure),
    }
    out
}

fn planner_summary_for_state(state: PlannerJudgmentState) -> SemanticStateSummary {
    let mut summary = SemanticStateSummary {
        complete: true,
        target_root: Some("/tmp/example".into()),
        path_exists: state.path == PlannerPathState::Present,
        cargo_project: state.cargo == PlannerCargoState::Present,
        entrypoint_kind: Some(
            match state.entrypoint {
                PlannerEntrypointState::Missing => "none",
                PlannerEntrypointState::Main => "bin",
            }
            .to_string(),
        ),
        source_files: vec!["src/main.rs".into(), "src/lib.rs".into()],
        ..SemanticStateSummary::default()
    };
    if state.module_gap == PlannerModuleGapState::Present {
        summary.module_gaps = vec!["index -> src/index.rs".into()];
    }
    summary.compiler_hints = match state.hint {
        PlannerHintState::None => Vec::new(),
        PlannerHintState::DeadCode => vec![CompilerHintRecord::new(
            CompilerHintKind::DeadCodeForbidConflict,
            "dead_code conflict",
            "remove allow(dead_code)",
            vec!["src/lib.rs".into()],
        )],
        PlannerHintState::UnresolvedImport => vec![CompilerHintRecord::new(
            CompilerHintKind::UnresolvedImport,
            "unresolved import",
            "fix import",
            vec!["src/lib.rs".into()],
        )],
        PlannerHintState::MissingSymbol => vec![CompilerHintRecord::new(
            CompilerHintKind::MissingSymbol,
            "missing symbol",
            "define or import symbol",
            vec!["src/main.rs".into()],
        )],
        PlannerHintState::DuplicateDefinition => vec![CompilerHintRecord::new(
            CompilerHintKind::DuplicateDefinition,
            "duplicate definition",
            "remove duplicate",
            vec!["src/lib.rs".into()],
        )],
        PlannerHintState::TraitBound => vec![CompilerHintRecord::new(
            CompilerHintKind::TraitBoundFailure,
            "trait bound failure",
            "fix trait bound",
            vec!["src/lib.rs".into()],
        )],
    };
    summary
}

fn planner_actions_for_state(state: PlannerJudgmentState) -> Vec<canon_event::LoopPlanned> {
    match state.action {
        PlannerActionCase::BootstrapWorkspace => {
            vec![planned_run_command("cargo new example", "/tmp")]
        }
        PlannerActionCase::InitCargoProject => {
            vec![planned_run_command("cargo init", "/tmp/example")]
        }
        PlannerActionCase::CreateEntrypoint => vec![planned_add_file("src/main.rs", "+fn main() {}\n")],
        PlannerActionCase::CreateModuleFile => vec![planned_add_file("src/index.rs", "+pub struct Index;\n")],
        PlannerActionCase::FixDeadCodeConflict => {
            vec![planned_update_file("src/lib.rs", "-#![allow(dead_code)]\n+#![allow(dead_code)]\n")]
        }
        PlannerActionCase::FixUnresolvedImport => {
            vec![planned_update_file("src/lib.rs", "+use crate::foo;\n")]
        }
        PlannerActionCase::DefineMissingSymbol => {
            vec![planned_update_file("src/main.rs", "+fn run() {}\n")]
        }
        PlannerActionCase::ResolveDuplicateDefinition => vec![planned_update_file(
            "src/lib.rs",
            "-pub struct Engine;\n+pub struct EngineV2;\n",
        )],
        PlannerActionCase::FixTraitBoundFailure => vec![planned_update_file(
            "src/lib.rs",
            "+impl Clone for Foo { fn clone(&self) -> Self { Self } }\n",
        )],
        PlannerActionCase::WrongEdit => vec![planned_add_file("src/other.rs", "+pub struct Other;\n")],
        PlannerActionCase::ValidateCargoCheck => {
            vec![planned_run_command("cargo check", "/tmp/example")]
        }
    }
}

fn planner_family_for_precondition(precondition: &PlanningPrecondition) -> JudgmentScenarioFamily {
    match precondition {
        PlanningPrecondition::MustBootstrapWorkspace => JudgmentScenarioFamily::PlannerBootstrapWorkspace,
        PlanningPrecondition::MustInitCargoProject => JudgmentScenarioFamily::PlannerInitCargoProject,
        PlanningPrecondition::MustCreateEntrypoint => JudgmentScenarioFamily::PlannerCreateEntrypoint,
        PlanningPrecondition::MustCreateMissingModules => {
            JudgmentScenarioFamily::PlannerCreateMissingModules
        }
        PlanningPrecondition::MustFixDeadCodeForbidConflict => {
            JudgmentScenarioFamily::PlannerFixDeadCodeConflict
        }
        PlanningPrecondition::MustFixUnresolvedImport => {
            JudgmentScenarioFamily::PlannerFixUnresolvedImport
        }
        PlanningPrecondition::MustDefineMissingSymbol => {
            JudgmentScenarioFamily::PlannerDefineMissingSymbol
        }
        PlanningPrecondition::MustResolveDuplicateDefinition => {
            JudgmentScenarioFamily::PlannerResolveDuplicateDefinition
        }
        PlanningPrecondition::MustFixTraitBoundFailure => {
            JudgmentScenarioFamily::PlannerFixTraitBoundFailure
        }
    }
}

fn planner_action_matches_primary_intent(state: PlannerJudgmentState) -> bool {
    match planner_preconditions_for_state(state).first() {
        Some(PlanningPrecondition::MustBootstrapWorkspace) => {
            matches!(
                state.action,
                PlannerActionCase::BootstrapWorkspace | PlannerActionCase::InitCargoProject
            )
        }
        Some(PlanningPrecondition::MustInitCargoProject) => {
            state.action == PlannerActionCase::InitCargoProject
        }
        Some(PlanningPrecondition::MustCreateEntrypoint) => {
            state.action == PlannerActionCase::CreateEntrypoint
        }
        Some(PlanningPrecondition::MustCreateMissingModules) => {
            state.action == PlannerActionCase::CreateModuleFile
        }
        Some(PlanningPrecondition::MustFixDeadCodeForbidConflict) => {
            state.action == PlannerActionCase::FixDeadCodeConflict
        }
        Some(PlanningPrecondition::MustFixUnresolvedImport) => {
            state.action == PlannerActionCase::FixUnresolvedImport
        }
        Some(PlanningPrecondition::MustDefineMissingSymbol) => {
            state.action == PlannerActionCase::DefineMissingSymbol
        }
        Some(PlanningPrecondition::MustResolveDuplicateDefinition) => {
            state.action == PlannerActionCase::ResolveDuplicateDefinition
        }
        Some(PlanningPrecondition::MustFixTraitBoundFailure) => {
            state.action == PlannerActionCase::FixTraitBoundFailure
        }
        None => true,
    }
}

fn valid_route_semantic_state(state: RouteSemanticState) -> bool {
    if state.completeness == RouteSummaryCompleteness::Incomplete {
        return state.preconditions == RoutePreconditionState::None
            && state.repair_intents == RouteRepairIntentState::None
            && state.hint == RouteHintState::None
            && state.validation_blocked == RouteValidationBlockedState::No;
    }
    true
}

fn route_summary_for_state(state: RouteSemanticState) -> SemanticStateSummary {
    let mut summary = SemanticStateSummary {
        complete: state.completeness == RouteSummaryCompleteness::Complete,
        validation_blocked_by_preconditions: state.validation_blocked == RouteValidationBlockedState::Yes,
        ..SemanticStateSummary::default()
    };
    if state.preconditions == RoutePreconditionState::Present {
        summary.planning_preconditions =
            vec!["must_create_missing_modules=true repair=create_declared_module_files_before_cargo_check".into()];
    }
    if state.repair_intents == RouteRepairIntentState::Present {
        summary.repair_intents =
            vec!["repair_intent=create_missing_modules priority=4 first_batch=create_declared_module_files".into()];
    }
    summary.compiler_hints = match state.hint {
        RouteHintState::None => Vec::new(),
        RouteHintState::UnresolvedImport => vec![CompilerHintRecord::new(
            CompilerHintKind::UnresolvedImport,
            "compiler reports unresolved import `crate::foo`",
            "fix import",
            vec!["src/lib.rs".into()],
        )],
        RouteHintState::DuplicateDefinition => vec![CompilerHintRecord::new(
            CompilerHintKind::DuplicateDefinition,
            "compiler reports duplicate definition for `Engine`",
            "remove duplicate",
            vec!["src/lib.rs".into()],
        )],
        RouteHintState::TraitBound => vec![CompilerHintRecord::new(
            CompilerHintKind::TraitBoundFailure,
            "compiler reports unsatisfied trait bound `Foo: Clone`",
            "fix trait bound",
            vec!["src/lib.rs".into()],
        )],
    };
    summary
}

fn route_state_is_actionable(state: RouteSemanticState) -> bool {
    state.completeness == RouteSummaryCompleteness::Complete
        && (state.validation_blocked == RouteValidationBlockedState::Yes
            || state.preconditions == RoutePreconditionState::Present
            || state.repair_intents == RouteRepairIntentState::Present
            || state.hint != RouteHintState::None)
}

fn route_family_for_state(state: RouteSemanticState) -> Option<JudgmentScenarioFamily> {
    if state.completeness != RouteSummaryCompleteness::Complete {
        return None;
    }
    if state.validation_blocked == RouteValidationBlockedState::Yes {
        return Some(JudgmentScenarioFamily::RouteSemanticValidationBlockedActionable);
    }
    if state.repair_intents == RouteRepairIntentState::Present {
        return Some(JudgmentScenarioFamily::RouteSemanticRepairIntentActionable);
    }
    if state.preconditions == RoutePreconditionState::Present {
        return Some(JudgmentScenarioFamily::RouteSemanticPreconditionActionable);
    }
    match state.hint {
        RouteHintState::UnresolvedImport => {
            Some(JudgmentScenarioFamily::RouteSemanticUnresolvedImportActionable)
        }
        RouteHintState::DuplicateDefinition => {
            Some(JudgmentScenarioFamily::RouteSemanticDuplicateDefinitionActionable)
        }
        RouteHintState::TraitBound => Some(JudgmentScenarioFamily::RouteSemanticTraitBoundActionable),
        RouteHintState::None => None,
    }
}

fn planner_judgment_state_count(valid_only: bool) -> usize {
    let mut count = 0usize;
    for path in PlannerPathState::ALL {
        for cargo in PlannerCargoState::ALL {
            for entrypoint in PlannerEntrypointState::ALL {
                for module_gap in PlannerModuleGapState::ALL {
                    for hint in PlannerHintState::ALL {
                        for action in PlannerActionCase::ALL {
                            let state = PlannerJudgmentState {
                                path,
                                cargo,
                                entrypoint,
                                module_gap,
                                hint,
                                action,
                            };
                            if valid_only && !valid_planner_judgment_state(state) {
                                continue;
                            }
                            count += 1;
                        }
                    }
                }
            }
        }
    }
    count
}

fn route_semantic_state_count(valid_only: bool) -> usize {
    let mut count = 0usize;
    for completeness in RouteSummaryCompleteness::ALL {
        for preconditions in RoutePreconditionState::ALL {
            for repair_intents in RouteRepairIntentState::ALL {
                for hint in RouteHintState::ALL {
                    for validation_blocked in RouteValidationBlockedState::ALL {
                        let state = RouteSemanticState {
                            completeness,
                            preconditions,
                            repair_intents,
                            hint,
                            validation_blocked,
                        };
                        if valid_only && !valid_route_semantic_state(state) {
                            continue;
                        }
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

pub fn loop_transition_rows() -> Vec<LoopTransitionRow> {
    vec![
        LoopTransitionRow {
            name: "invalid_plan_clears_suppression",
            family: LoopScenarioFamily::InvalidPlanClearsSuppression,
            pending_required_successor: None,
            planning_status: Some("invalid_plan"),
            error_kind: None,
            expected_successor: None,
            expected_rules: vec![LoopRecoveryRule::ClearPlannerSuppressionOnInvalidPlan],
            expected_trigger_observe: false,
            expected_force_reward_recovery: false,
            expected_observe_blocked: false,
        },
        LoopTransitionRow {
            name: "planned_status_has_no_invalid_plan_recovery",
            family: LoopScenarioFamily::InvalidPlanNoRecoveryForOtherStatus,
            pending_required_successor: None,
            planning_status: Some("planned"),
            error_kind: None,
            expected_successor: None,
            expected_rules: vec![],
            expected_trigger_observe: false,
            expected_force_reward_recovery: false,
            expected_observe_blocked: false,
        },
        LoopTransitionRow {
            name: "act_stall_triggers_observe",
            family: LoopScenarioFamily::ActStallTriggersObserve,
            pending_required_successor: Some("loop_acted"),
            planning_status: None,
            error_kind: Some("act_stall"),
            expected_successor: None,
            expected_rules: vec![LoopRecoveryRule::TriggerObserveOnActStall],
            expected_trigger_observe: true,
            expected_force_reward_recovery: false,
            expected_observe_blocked: true,
        },
        LoopTransitionRow {
            name: "non_act_stall_does_not_trigger_observe",
            family: LoopScenarioFamily::NonActStallDoesNotTriggerObserve,
            pending_required_successor: Some("loop_acted"),
            planning_status: None,
            error_kind: Some("invariant_violation"),
            expected_successor: None,
            expected_rules: vec![],
            expected_trigger_observe: false,
            expected_force_reward_recovery: false,
            expected_observe_blocked: true,
        },
        LoopTransitionRow {
            name: "reward_recovery_for_expected_successor",
            family: LoopScenarioFamily::RewardRecoveryForExpectedSuccessor,
            pending_required_successor: Some("loop_rewarded"),
            planning_status: None,
            error_kind: None,
            expected_successor: Some("loop_rewarded"),
            expected_rules: vec![LoopRecoveryRule::RecoverLoopRewarded],
            expected_trigger_observe: false,
            expected_force_reward_recovery: true,
            expected_observe_blocked: true,
        },
        LoopTransitionRow {
            name: "non_reward_successor_does_not_recover",
            family: LoopScenarioFamily::NonRewardSuccessorDoesNotRecover,
            pending_required_successor: Some("route_selected"),
            planning_status: None,
            error_kind: None,
            expected_successor: Some("route_selected"),
            expected_rules: vec![],
            expected_trigger_observe: false,
            expected_force_reward_recovery: false,
            expected_observe_blocked: true,
        },
        LoopTransitionRow {
            name: "observe_blocked_by_pending_successor",
            family: LoopScenarioFamily::ObserveBlockedByPendingSuccessor,
            pending_required_successor: Some("loop_acted"),
            planning_status: None,
            error_kind: None,
            expected_successor: None,
            expected_rules: vec![],
            expected_trigger_observe: false,
            expected_force_reward_recovery: false,
            expected_observe_blocked: true,
        },
        LoopTransitionRow {
            name: "observe_not_blocked_without_successor",
            family: LoopScenarioFamily::ObserveNotBlockedWithoutSuccessor,
            pending_required_successor: None,
            planning_status: None,
            error_kind: None,
            expected_successor: None,
            expected_rules: vec![],
            expected_trigger_observe: false,
            expected_force_reward_recovery: false,
            expected_observe_blocked: false,
        },
    ]
}

pub fn loop_runtime_rows() -> Vec<LoopRuntimeRow> {
    vec![
        LoopRuntimeRow {
            name: "runtime_triggered_observe",
            family: LoopScenarioFamily::RuntimeTriggeredObserve,
            halted: false,
            force_observe_recovery: false,
            trigger_observe: true,
            suppress_observe_on_invariant: false,
            pending_required_successor: None,
            is_route_selected_event: false,
            expected_mode: ObserveExecutionMode::Triggered,
            expected_halt_blocks_stage: false,
            expected_warn_route_selected_while_halted: false,
            expected_rules: vec![LoopRuntimeRule::ExecuteTriggeredObserve],
        },
        LoopRuntimeRow {
            name: "runtime_forced_observe",
            family: LoopScenarioFamily::RuntimeForcedObserve,
            halted: false,
            force_observe_recovery: true,
            trigger_observe: false,
            suppress_observe_on_invariant: false,
            pending_required_successor: None,
            is_route_selected_event: false,
            expected_mode: ObserveExecutionMode::Forced,
            expected_halt_blocks_stage: false,
            expected_warn_route_selected_while_halted: false,
            expected_rules: vec![LoopRuntimeRule::ExecuteForcedObserve],
        },
        LoopRuntimeRow {
            name: "runtime_suppress_observe_on_invariant",
            family: LoopScenarioFamily::RuntimeSuppressObserveOnInvariant,
            halted: false,
            force_observe_recovery: false,
            trigger_observe: true,
            suppress_observe_on_invariant: true,
            pending_required_successor: None,
            is_route_selected_event: false,
            expected_mode: ObserveExecutionMode::SuppressedByInvariant,
            expected_halt_blocks_stage: false,
            expected_warn_route_selected_while_halted: false,
            expected_rules: vec![LoopRuntimeRule::SuppressObserveOnInvariant],
        },
        LoopRuntimeRow {
            name: "runtime_suppress_observe_on_pending_successor",
            family: LoopScenarioFamily::RuntimeSuppressObserveOnPendingSuccessor,
            halted: false,
            force_observe_recovery: false,
            trigger_observe: true,
            suppress_observe_on_invariant: false,
            pending_required_successor: Some("loop_acted"),
            is_route_selected_event: false,
            expected_mode: ObserveExecutionMode::SuppressedByPendingSuccessor,
            expected_halt_blocks_stage: false,
            expected_warn_route_selected_while_halted: false,
            expected_rules: vec![LoopRuntimeRule::SuppressObserveOnPendingSuccessor],
        },
        LoopRuntimeRow {
            name: "runtime_block_when_halted",
            family: LoopScenarioFamily::RuntimeBlockWhenHalted,
            halted: true,
            force_observe_recovery: false,
            trigger_observe: false,
            suppress_observe_on_invariant: false,
            pending_required_successor: None,
            is_route_selected_event: true,
            expected_mode: ObserveExecutionMode::None,
            expected_halt_blocks_stage: true,
            expected_warn_route_selected_while_halted: true,
            expected_rules: vec![LoopRuntimeRule::BlockStageWhenHalted, LoopRuntimeRule::WarnRouteSelectedWhileHalted],
        },
    ]
}

pub fn recovery_event_rows() -> Vec<RecoveryEventRow> {
    vec![
        RecoveryEventRow {
            name: "recovery_event_force_observe",
            family: LoopScenarioFamily::RecoveryEventForceObserve,
            expected_successor: Some("loop_observed"),
            pending_required_successor: Some("route_selected"),
            has_last_verified: false,
            expected_rule: RecoveryEventRule::ForceObserve,
            expected_force_observe_recovery: true,
            expected_execute_reward_recovery: false,
        },
        RecoveryEventRow {
            name: "recovery_event_reward_execute",
            family: LoopScenarioFamily::RecoveryEventRewardExecute,
            expected_successor: Some("loop_rewarded"),
            pending_required_successor: Some("loop_rewarded"),
            has_last_verified: true,
            expected_rule: RecoveryEventRule::ExecuteRewardRecovery,
            expected_force_observe_recovery: false,
            expected_execute_reward_recovery: true,
        },
        RecoveryEventRow {
            name: "recovery_event_reward_skip_satisfied",
            family: LoopScenarioFamily::RecoveryEventRewardSkipSatisfied,
            expected_successor: Some("loop_rewarded"),
            pending_required_successor: Some("route_selected"),
            has_last_verified: true,
            expected_rule: RecoveryEventRule::SkipRewardAlreadySatisfied,
            expected_force_observe_recovery: false,
            expected_execute_reward_recovery: false,
        },
        RecoveryEventRow {
            name: "recovery_event_reward_missing_context",
            family: LoopScenarioFamily::RecoveryEventRewardMissingContext,
            expected_successor: Some("loop_rewarded"),
            pending_required_successor: Some("loop_rewarded"),
            has_last_verified: false,
            expected_rule: RecoveryEventRule::MissingRewardContext,
            expected_force_observe_recovery: false,
            expected_execute_reward_recovery: false,
        },
    ]
}

pub fn recovery_execution_rows() -> Vec<RecoveryExecutionRow> {
    vec![
        RecoveryExecutionRow {
            name: "reward_recovery_noop",
            family: LoopScenarioFamily::RewardRecoveryNoop,
            operation: RecoveryOperation::RewardRecovery,
            outcome: StageExecutionOutcomeClass::Noop,
            expected_debug_kind: Some("reward_recovery_noop"),
            expected_error_kind: None,
        },
        RecoveryExecutionRow {
            name: "reward_recovery_execution_error",
            family: LoopScenarioFamily::RewardRecoveryExecutionError,
            operation: RecoveryOperation::RewardRecovery,
            outcome: StageExecutionOutcomeClass::Error,
            expected_debug_kind: None,
            expected_error_kind: Some("reward_recovery_execution"),
        },
        RecoveryExecutionRow {
            name: "observe_forced_deferred",
            family: LoopScenarioFamily::ObserveForcedDeferred,
            operation: RecoveryOperation::ObserveForced,
            outcome: StageExecutionOutcomeClass::Deferred,
            expected_debug_kind: Some("observe_deferred"),
            expected_error_kind: None,
        },
        RecoveryExecutionRow {
            name: "observe_forced_noop",
            family: LoopScenarioFamily::ObserveForcedNoop,
            operation: RecoveryOperation::ObserveForced,
            outcome: StageExecutionOutcomeClass::Noop,
            expected_debug_kind: Some("observe_noop"),
            expected_error_kind: None,
        },
        RecoveryExecutionRow {
            name: "observe_triggered_deferred",
            family: LoopScenarioFamily::ObserveTriggeredDeferred,
            operation: RecoveryOperation::ObserveTriggered,
            outcome: StageExecutionOutcomeClass::Deferred,
            expected_debug_kind: Some("observe_deferred"),
            expected_error_kind: None,
        },
        RecoveryExecutionRow {
            name: "observe_triggered_noop",
            family: LoopScenarioFamily::ObserveTriggeredNoop,
            operation: RecoveryOperation::ObserveTriggered,
            outcome: StageExecutionOutcomeClass::Noop,
            expected_debug_kind: Some("observe_noop"),
            expected_error_kind: None,
        },
    ]
}

pub fn bootstrap_effect_rows() -> Vec<BootstrapEffectRow> {
    vec![BootstrapEffectRow {
        name: "bootstrap_invalidates_queued_work",
        family: LoopScenarioFamily::BootstrapInvalidatesQueuedWork,
        action_outcome: ActionOutcomeClass::BootstrapSuccess,
        expected_rule: BootstrapRule::InvalidateQueuedPlanWork,
        expected_emit_refresh_required: true,
    }]
}

pub fn planner_recovery_rows() -> Vec<PlannerRecoveryRow> {
    vec![
        PlannerRecoveryRow {
            name: "planner_retry_no_semantic_progress",
            family: JudgmentScenarioFamily::PlannerRetryNoSemanticProgress,
            reason: None,
            consecutive_invalid_plan_batches: 0,
            recent_execution_results: vec![SemanticExecutionResultRecord::new(
                "no_semantic_progress",
                "action failed",
                Vec::new(),
                false,
            )],
            objective_trend_state: canon_semantic_state::ObjectiveTrendState::default(),
            expected_retry: RetryPolicy::CorrectiveRetry,
        },
        PlannerRecoveryRow {
            name: "planner_retry_trend_stalled",
            family: JudgmentScenarioFamily::PlannerRetryTrendStalled,
            reason: None,
            consecutive_invalid_plan_batches: 0,
            recent_execution_results: Vec::new(),
            objective_trend_state: canon_semantic_state::ObjectiveTrendState {
                repeated_stall_count: 1,
                current_no_progress_streak: 1,
                ..canon_semantic_state::ObjectiveTrendState::default()
            },
            expected_retry: RetryPolicy::CorrectiveRetry,
        },
    ]
}

pub fn reward_semantics_rows() -> Vec<RewardSemanticsRow> {
    vec![
        RewardSemanticsRow {
            name: "reward_semantic_progress",
            family: LoopScenarioFamily::RewardSemanticProgress,
            compiler_clean: false,
            last_action_kind: "apply_patch",
            recent_execution_results: vec![SemanticExecutionResultRecord::new(
                "module_created",
                "module file created",
                vec!["/tmp/example/src/index.rs".into()],
                true,
            )],
            expected: RewardSemantics {
                reward: -0.6,
                resets_stagnation: true,
            },
        },
        RewardSemanticsRow {
            name: "reward_no_semantic_progress",
            family: LoopScenarioFamily::RewardNoSemanticProgress,
            compiler_clean: false,
            last_action_kind: "apply_patch",
            recent_execution_results: vec![SemanticExecutionResultRecord::new(
                "no_semantic_progress",
                "action failed",
                Vec::new(),
                false,
            )],
            expected: RewardSemantics {
                reward: -1.6,
                resets_stagnation: false,
            },
        },
    ]
}

pub fn run_command_outcome_rows() -> Vec<RunCommandOutcomeRow> {
    vec![
        RunCommandOutcomeRow {
            name: "run_command_bootstrap_success",
            family: RunCommandOutcomeFamily::BootstrapSuccess,
            input: RunCommandOutcomeClass::BootstrapSuccess,
            expected: RunCommandOutcomeClass::BootstrapSuccess,
        },
        RunCommandOutcomeRow {
            name: "run_command_validation_failure_compiler",
            family: RunCommandOutcomeFamily::ValidationFailureCompiler,
            input: RunCommandOutcomeClass::ValidationFailureCompiler,
            expected: RunCommandOutcomeClass::ValidationFailureCompiler,
        },
        RunCommandOutcomeRow {
            name: "run_command_validation_success",
            family: RunCommandOutcomeFamily::ValidationSuccess,
            input: RunCommandOutcomeClass::ValidationSuccess,
            expected: RunCommandOutcomeClass::ValidationSuccess,
        },
        RunCommandOutcomeRow {
            name: "run_command_semantic_failure",
            family: RunCommandOutcomeFamily::SemanticFailure,
            input: RunCommandOutcomeClass::SemanticFailure,
            expected: RunCommandOutcomeClass::SemanticFailure,
        },
        RunCommandOutcomeRow {
            name: "run_command_other",
            family: RunCommandOutcomeFamily::Other,
            input: RunCommandOutcomeClass::Other,
            expected: RunCommandOutcomeClass::Other,
        },
    ]
}

pub fn apply_patch_outcome_rows() -> Vec<ApplyPatchOutcomeRow> {
    vec![
        ApplyPatchOutcomeRow {
            name: "apply_patch_success",
            family: ApplyPatchOutcomeFamily::Success,
            input: ApplyPatchOutcomeClass::Success,
            expected: ApplyPatchOutcomeClass::Success,
        },
        ApplyPatchOutcomeRow {
            name: "apply_patch_missing_target_file",
            family: ApplyPatchOutcomeFamily::MissingTargetFile,
            input: ApplyPatchOutcomeClass::MissingTargetFile,
            expected: ApplyPatchOutcomeClass::MissingTargetFile,
        },
        ApplyPatchOutcomeRow {
            name: "apply_patch_patch_apply_failure",
            family: ApplyPatchOutcomeFamily::PatchApplyFailure,
            input: ApplyPatchOutcomeClass::PatchApplyFailure,
            expected: ApplyPatchOutcomeClass::PatchApplyFailure,
        },
        ApplyPatchOutcomeRow {
            name: "apply_patch_other_failure",
            family: ApplyPatchOutcomeFamily::OtherFailure,
            input: ApplyPatchOutcomeClass::OtherFailure,
            expected: ApplyPatchOutcomeClass::OtherFailure,
        },
    ]
}

pub fn verify_outcome_rows() -> Vec<VerifyOutcomeRow> {
    vec![
        VerifyOutcomeRow {
            name: "verify_compiler_failure",
            family: VerifyOutcomeFamily::CompilerFailure,
            input: VerifyOutcomeClass::CompilerFailure,
            expected: VerifyOutcomeClass::CompilerFailure,
        },
        VerifyOutcomeRow {
            name: "verify_passed",
            family: VerifyOutcomeFamily::Passed,
            input: VerifyOutcomeClass::Passed,
            expected: VerifyOutcomeClass::Passed,
        },
        VerifyOutcomeRow {
            name: "verify_failed_no_compiler_signal",
            family: VerifyOutcomeFamily::FailedNoCompilerSignal,
            input: VerifyOutcomeClass::FailedNoCompilerSignal,
            expected: VerifyOutcomeClass::FailedNoCompilerSignal,
        },
    ]
}

pub fn invalid_plan_retry_rows() -> Vec<InvalidPlanRetryRow> {
    vec![
        InvalidPlanRetryRow {
            name: "invalid_plan_mixed_batch_discovery_only",
            family: InvalidPlanRetryFamily::MixedBatchDiscoveryOnly,
            reason: Some("invalid plan batch before execution: mixed discovery actions with execution/edit actions in one plan batch"),
            count: 1,
            expected_reason_class: InvalidPlanReasonClass::MixedBatch,
            expected_retry: RetryPolicy::DiscoveryOnly,
        },
        InvalidPlanRetryRow {
            name: "invalid_plan_patch_format_single_patch_only",
            family: InvalidPlanRetryFamily::PatchFormatSinglePatchOnly,
            reason: Some("invalid plan batch before execution: apply_patch payload is invalid: invalid hunk at line 12"),
            count: 1,
            expected_reason_class: InvalidPlanReasonClass::PatchFormat,
            expected_retry: RetryPolicy::SinglePatchOnly,
        },
        InvalidPlanRetryRow {
            name: "invalid_plan_path_or_cwd_corrective_retry",
            family: InvalidPlanRetryFamily::PathOrCwdCorrectiveRetry,
            reason: Some("invalid plan batch before execution: run_command requires an absolute cwd; got '.'"),
            count: 1,
            expected_reason_class: InvalidPlanReasonClass::PathOrCwd,
            expected_retry: RetryPolicy::CorrectiveRetry,
        },
        InvalidPlanRetryRow {
            name: "invalid_plan_missing_context_corrective_retry",
            family: InvalidPlanRetryFamily::MissingContextCorrectiveRetry,
            reason: Some("missing_observed_context"),
            count: 1,
            expected_reason_class: InvalidPlanReasonClass::MissingContext,
            expected_retry: RetryPolicy::CorrectiveRetry,
        },
        InvalidPlanRetryRow {
            name: "invalid_plan_unknown_corrective_retry",
            family: InvalidPlanRetryFamily::UnknownCorrectiveRetry,
            reason: Some("some other planner issue"),
            count: 1,
            expected_reason_class: InvalidPlanReasonClass::Unknown,
            expected_retry: RetryPolicy::CorrectiveRetry,
        },
    ]
}

pub fn coverage_report_markdown(rows: &[TransitionRow]) -> String {
    let report = coverage_report(rows);
    let mut out = String::new();
    out.push_str("# Policy Matrix Coverage\n\n");
    out.push_str("## Judgment generation\n");
    out.push_str(&format!(
        "- planner states: valid={} total={}\n",
        report.planner_generated_valid, report.planner_generated_total
    ));
    out.push_str(&format!(
        "- route states: valid={} total={}\n\n",
        report.route_generated_valid, report.route_generated_total
    ));
    out.push_str(&coverage_section("Route transitions", &report.route_covered, &report.route_missing, |v| format!("{:?}", v)));
    out.push_str(&coverage_section("Loop transitions", &report.loop_covered, &report.loop_missing, |v| format!("{:?}", v)));
    out.push_str(&coverage_section("Run command outcomes", &report.run_command_covered, &report.run_command_missing, |v| format!("{:?}", v)));
    out.push_str(&coverage_section("Apply patch outcomes", &report.apply_patch_covered, &report.apply_patch_missing, |v| format!("{:?}", v)));
    out.push_str(&coverage_section("Verify outcomes", &report.verify_covered, &report.verify_missing, |v| format!("{:?}", v)));
    out.push_str(&coverage_section(
        "Invalid-plan retry mappings",
        &report.invalid_plan_retry_covered,
        &report.invalid_plan_retry_missing,
        |v| format!("{:?}", v),
    ));
    out.push_str(&coverage_section(
        "Judgment coverage",
        &report.judgment_covered,
        &report.judgment_missing,
        |v| format!("{:?}", v),
    ));
    out
}

fn assert_route_row(row: &RouteTransitionRow) {
    let mut ctx = RouteContext::default();
    ctx.halted = row.context.halted;
    ctx.context_ready = row.context.context_ready;
    ctx.consecutive_invalid_plan_batches = row.context.consecutive_invalid_plan_batches;
    ctx.planned_pending = row.context.planned_pending;
    ctx.bootstrap_refresh_required = row.context.bootstrap_refresh_required;
    if row.context.target_workspace_missing {
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.path_exists = false;
        ctx.semantic_summary.target_root = Some("/tmp/matrix-target".to_string());
    }
    ctx.finish_ready = row.context.finish_ready;
    if row.context.pending_tool_results_empty {
        ctx.pending_tool_result_ids.clear();
    } else {
        ctx.pending_tool_result_ids.insert("pending".to_string());
    }
    apply_route_outcome_context(&mut ctx, &row.context);

    let event = row.event.as_ref().map(to_runtime_event);
    let decision = row.decision.as_ref().map(to_route_decision);
    let eval = evaluate_route_transition(
        &ctx,
        RoutePolicyState {
            last_control_kind: row.state.last_control_kind,
            pending_required_successor: row.state.pending_required_successor,
        },
        event.as_ref(),
        decision.as_ref(),
    );

    assert_eq!(
        eval.deterministic.as_ref().map(|d| d.rule),
        row.expected_deterministic,
        "route row {} deterministic mismatch",
        row.name
    );
    assert_eq!(eval.rules, row.expected_rules, "route row {} rule mismatch", row.name);
}

fn assert_route_dispatch_row(row: &RouteDispatchRow) {
    let mut ctx = RouteContext::default();
    ctx.halted = row.context.halted;
    ctx.context_ready = row.context.context_ready;
    ctx.consecutive_invalid_plan_batches = row.context.consecutive_invalid_plan_batches;
    ctx.planned_pending = row.context.planned_pending;
    if row.context.target_workspace_missing {
        ctx.semantic_summary.complete = true;
        ctx.semantic_summary.path_exists = false;
        ctx.semantic_summary.target_root = Some("/tmp/matrix-target".to_string());
    }
    apply_route_outcome_context(&mut ctx, &row.context);
    let eval = evaluate_route_dispatch(
        &ctx,
        RoutePolicyState {
            last_control_kind: row.state.last_control_kind,
            pending_required_successor: row.state.pending_required_successor,
        },
        RouteDispatchState {
            pending_request_id: row.dispatch.pending_request_id,
            awaiting_control_successor: row.dispatch.awaiting_control_successor,
            route_emitted_for_current_control: row.dispatch.route_emitted_for_current_control,
        },
    );
    assert_eq!(eval.suppression.as_ref().map(|s| s.rule), row.expected_rule, "route dispatch row {} rule mismatch", row.name);
    assert_eq!(
        eval.deterministic.as_ref().map(|d| d.rule),
        row.expected_deterministic,
        "route dispatch row {} deterministic mismatch",
        row.name
    );
}

fn assert_route_emit_row(row: &RouteEmitRow) {
    let eval = evaluate_route_emit(RouteEmitState {
        awaiting_control_successor: row.awaiting_control_successor,
        last_control_kind: row.last_control_kind,
        pending_required_successor: row.pending_required_successor,
    });
    assert_eq!(eval.rule, row.expected_rule, "route emit row {} mismatch", row.name);
}

fn assert_route_cache_row(row: &RouteCacheRow) {
    let eval = evaluate_route_cache(RouteCacheState {
        force_fresh_route_once: row.state.force_fresh_route_once,
        last_prompt_hash: row.state.last_prompt_hash,
        prompt_hash: row.state.prompt_hash,
        pending_required_successor: row.state.pending_required_successor,
        last_route_prompt_hash: row.state.last_route_prompt_hash,
        route_emitted_for_current_control: row.state.route_emitted_for_current_control,
        has_cached_route: row.state.has_cached_route,
        cached_route_is_observe: row.state.cached_route_is_observe,
        can_emit_route_selected: row.state.can_emit_route_selected,
    });
    assert_eq!(eval.rule, row.expected_rule, "route cache row {} mismatch", row.name);
}

fn assert_route_failure_row(row: &RouteFailureRow) {
    let eval = evaluate_route_failure(&RouteContext::default());
    assert_eq!(eval.rule, row.expected_rule, "route failure row {} mismatch", row.name);
}

fn assert_route_emit_effect_row(row: &RouteEmitEffectRow) {
    let decision = to_route_decision(&row.decision);
    let eval = evaluate_route_emit_effects(&decision);
    assert_eq!(eval.rules, row.expected_rules, "route emit effect row {} rules mismatch", row.name);
    assert_eq!(
        eval.clear_pending_request,
        row.expected_clear_pending_request,
        "route emit effect row {} clear request mismatch",
        row.name
    );
    assert_eq!(
        eval.clear_pending_prompt,
        row.expected_clear_pending_prompt,
        "route emit effect row {} clear prompt mismatch",
        row.name
    );
    assert_eq!(
        eval.set_halted,
        row.expected_set_halted,
        "route emit effect row {} halted mismatch",
        row.name
    );
}

fn assert_route_recovery_row(row: &RouteRecoveryRow) {
    let eval = evaluate_route_recovery(row.pending_required_successor);
    assert_eq!(eval.rule, row.expected_rule, "route recovery row {} mismatch", row.name);
}

fn assert_successor_consumption_row(row: &SuccessorConsumptionRow) {
    let event = to_runtime_event(&row.event);
    let eval = evaluate_successor_consumption(&event, row.awaiting_control_successor);
    assert_eq!(eval.rule, row.expected_rule, "successor consumption row {} mismatch", row.name);
}

fn assert_planner_judgment_row(row: &PlannerJudgmentRow) {
    let result = validate_preconditions(
        &row.actions,
        std::path::Path::new("/tmp/example"),
        &row.preconditions,
        &row.summary,
    );
    assert_eq!(
        result.is_ok(),
        row.expected_ok,
        "planner judgment row {} mismatch: {:?}",
        row.name,
        result
    );
}

fn assert_planner_objective_alignment_row(row: &PlannerObjectiveAlignmentRow) {
    let route_result = validate_objective_route_plan_alignment(
        &row.actions,
        std::path::Path::new("/tmp/example"),
        row.route_choice,
        row.primary_objective,
        &row.summary,
    );
    let trend_result = validate_trend_intent_alignment(
        &row.actions,
        std::path::Path::new("/tmp/example"),
        &row.recent_execution_results,
        &row.objective_trend_state,
    );
    let ok = route_result.is_ok() && trend_result.is_ok();
    assert_eq!(
        ok,
        row.expected_ok,
        "planner objective alignment row {} mismatch: route={:?} trend={:?}",
        row.name,
        route_result,
        trend_result
    );
}

fn assert_route_objective_alignment_row(row: &RouteObjectiveAlignmentRow) {
    assert_eq!(
        !route_choice_contradicts_primary_objective(row.route_choice, row.primary_objective, &row.summary),
        row.expected_ok,
        "route objective alignment row {} mismatch",
        row.name
    );
}

fn assert_goal_route_objective_drift_row(row: &GoalRouteObjectiveDriftRow) {
    assert_eq!(
        goal_route_objective_drift(row.goal_objective, row.route_objective),
        row.expected_drift,
        "goal/route objective drift row {} mismatch",
        row.name
    );
}

fn assert_route_semantic_actionability_row(row: &RouteSemanticActionabilityRow) {
    let mut ctx = RouteContext::default();
    ctx.semantic_summary = row.summary.clone();
    ctx.objective_trend_state = row.objective_trend_state.clone();
    assert_eq!(
        canon_route::policy::has_actionable_failure(&ctx),
        row.expected_actionable,
        "route semantic actionability row {} mismatch",
        row.name
    );
}

fn assert_run_command_outcome_row(row: &RunCommandOutcomeRow) {
    let mut ctx = RouteContext::default();
    ctx.recent_tool_results.push(run_command_result_value(row.input));
    assert_eq!(latest_run_command_outcome(&ctx), Some(row.expected), "run_command row {} mismatch", row.name);
}

fn assert_apply_patch_outcome_row(row: &ApplyPatchOutcomeRow) {
    let mut ctx = RouteContext::default();
    ctx.recent_tool_results.push(apply_patch_result_value(row.input));
    assert_eq!(latest_apply_patch_outcome(&ctx), Some(row.expected), "apply_patch row {} mismatch", row.name);
}

fn assert_verify_outcome_row(row: &VerifyOutcomeRow) {
    let mut ctx = RouteContext::default();
    apply_verify_outcome(&mut ctx, row.input);
    assert_eq!(latest_verify_outcome(&ctx), Some(row.expected), "verify row {} mismatch", row.name);
}

fn assert_invalid_plan_retry_row(row: &InvalidPlanRetryRow) {
    assert_eq!(
        classify_invalid_plan_reason(row.reason),
        row.expected_reason_class,
        "invalid-plan reason row {} class mismatch",
        row.name
    );
    assert_eq!(
        retry_policy_for_invalid_plan(row.reason, row.count),
        row.expected_retry,
        "invalid-plan retry row {} mismatch",
        row.name
    );
}

fn apply_route_outcome_context(ctx: &mut RouteContext, row: &RouteRowContext) {
    if let Some(outcome) = row.verify_outcome {
        apply_verify_outcome(ctx, outcome);
    }
    if let Some(outcome) = row.run_command_outcome {
        ctx.recent_tool_results.push(run_command_result_value(outcome));
    }
    if let Some(outcome) = row.apply_patch_outcome {
        ctx.recent_tool_results.push(apply_patch_result_value(outcome));
    }
    if row.semantic_progress {
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new(
            "module_created",
            "module file created",
            vec!["/tmp/example/src/index.rs".into()],
            true,
        ));
    }
    if row.no_semantic_progress {
        ctx.recent_execution_results.push(SemanticExecutionResultRecord::new(
            "no_semantic_progress",
            "action failed",
            Vec::new(),
            false,
        ));
    }
}

fn apply_verify_outcome(ctx: &mut RouteContext, outcome: VerifyOutcomeClass) {
    ctx.verify_seen = true;
    match outcome {
        VerifyOutcomeClass::Passed => {
            ctx.last_verify_passed = true;
            ctx.last_verify_compiler_clean = true;
        }
        VerifyOutcomeClass::CompilerFailure => {
            ctx.last_verify_passed = false;
            ctx.last_verify_compiler_clean = false;
            ctx.last_verify_diagnostics = vec!["error[E0453]: compiler failure".to_string()];
        }
        VerifyOutcomeClass::FailedNoCompilerSignal => {
            ctx.last_verify_passed = false;
            ctx.last_verify_compiler_clean = false;
            ctx.last_verify_diagnostics = vec!["failed without compiler diagnostics".to_string()];
        }
    }
}

fn assert_loop_row(row: &LoopTransitionRow) {
    let eval = evaluate_loop_transition(
        row.pending_required_successor,
        row.planning_status,
        row.error_kind,
        row.expected_successor,
    );
    assert_eq!(eval.recovery_rules, row.expected_rules, "loop row {} rules mismatch", row.name);
    assert_eq!(eval.trigger_observe, row.expected_trigger_observe, "loop row {} trigger_observe mismatch", row.name);
    assert_eq!(
        eval.force_reward_recovery,
        row.expected_force_reward_recovery,
        "loop row {} force_reward_recovery mismatch",
        row.name
    );
    assert_eq!(
        eval.observe_blocked_by_successor,
        row.expected_observe_blocked,
        "loop row {} observe_blocked mismatch",
        row.name
    );
}

fn assert_loop_runtime_row(row: &LoopRuntimeRow) {
    let eval = evaluate_loop_runtime(
        row.halted,
        row.force_observe_recovery,
        row.trigger_observe,
        row.suppress_observe_on_invariant,
        row.pending_required_successor,
        row.is_route_selected_event,
    );
    assert_eq!(eval.observe_mode, row.expected_mode, "loop runtime row {} mode mismatch", row.name);
    assert_eq!(
        eval.halt_blocks_stage,
        row.expected_halt_blocks_stage,
        "loop runtime row {} halt mismatch",
        row.name
    );
    assert_eq!(
        eval.warn_route_selected_while_halted,
        row.expected_warn_route_selected_while_halted,
        "loop runtime row {} warn mismatch",
        row.name
    );
    assert_eq!(eval.rules, row.expected_rules, "loop runtime row {} rules mismatch", row.name);
}

fn assert_recovery_event_row(row: &RecoveryEventRow) {
    let eval = evaluate_recovery_event(
        row.expected_successor,
        row.pending_required_successor,
        row.has_last_verified,
    );
    assert_eq!(eval.rule, row.expected_rule, "recovery event row {} rule mismatch", row.name);
    assert_eq!(
        eval.force_observe_recovery,
        row.expected_force_observe_recovery,
        "recovery event row {} observe mismatch",
        row.name
    );
    assert_eq!(
        eval.execute_reward_recovery,
        row.expected_execute_reward_recovery,
        "recovery event row {} reward mismatch",
        row.name
    );
}

fn assert_recovery_execution_row(row: &RecoveryExecutionRow) {
    let eval = evaluate_recovery_execution(row.operation, row.outcome);
    assert_eq!(
        eval.debug_kind,
        row.expected_debug_kind,
        "recovery execution row {} debug mismatch",
        row.name
    );
    assert_eq!(
        eval.error_kind,
        row.expected_error_kind,
        "recovery execution row {} error mismatch",
        row.name
    );
}

fn assert_bootstrap_effect_row(row: &BootstrapEffectRow) {
    let eval = evaluate_bootstrap_effects(row.action_outcome);
    assert_eq!(eval.rule, row.expected_rule, "bootstrap effect row {} rule mismatch", row.name);
    assert_eq!(
        eval.emit_refresh_required,
        row.expected_emit_refresh_required,
        "bootstrap effect row {} emit mismatch",
        row.name
    );
}

fn assert_planner_recovery_row(row: &PlannerRecoveryRow) {
    assert_eq!(
        retry_policy_for_planning_context(
            row.reason,
            row.consecutive_invalid_plan_batches,
            &row.recent_execution_results,
            &row.objective_trend_state,
        ),
        row.expected_retry,
        "planner recovery row {} mismatch",
        row.name
    );
}

fn assert_reward_semantics_row(row: &RewardSemanticsRow) {
    let mut ctx = canon_loop::LoopContext::new("/tmp/example".into(), "/tmp/tlog".into());
    ctx.last_action_kind = row.last_action_kind.to_string();
    ctx.recent_execution_results = row.recent_execution_results.clone();
    let verified = canon_event::LoopVerified {
        tick: 0,
        passed: row.compiler_clean,
        compiler_clean: row.compiler_clean,
        tlog_clean: true,
        error_count: if row.compiler_clean { 0 } else { 1 },
        diagnostics: if row.compiler_clean {
            Vec::new()
        } else {
            vec!["error".into()]
        },
        trace_id: None,
        execution_id: None,
        span_id: None,
        parent_span_id: None,
    };
    let actual = evaluate_reward_semantics(&ctx, &verified);
    assert_eq!(actual, row.expected, "reward semantics row {} mismatch", row.name);
}

fn run_command_result_value(outcome: RunCommandOutcomeClass) -> serde_json::Value {
    let (success, stderr) = match outcome {
        RunCommandOutcomeClass::BootstrapSuccess => (true, "Creating binary (application) package"),
        RunCommandOutcomeClass::ValidationFailureCompiler => (false, "error[E0453]: compiler failure"),
        RunCommandOutcomeClass::ValidationSuccess => (true, "Finished `dev` profile"),
        RunCommandOutcomeClass::SemanticFailure => (false, "test result: FAILED"),
        RunCommandOutcomeClass::Other => (false, "other failure"),
    };
    serde_json::json!({
        "action": "run_command",
        "success": success,
        "output": {"Process": {"stderr": stderr, "stdout": ""}}
    })
}

fn planned_add_file(path: &str, body: &str) -> canon_event::LoopPlanned {
    canon_event::LoopPlanned {
        tick: 0,
        action_kind: "apply_patch".to_string(),
        action_payload: serde_json::json!({
            "patch": format!("*** Begin Patch\n*** Add File: {path}\n{body}*** End Patch\n")
        }),
        reason: String::new(),
        llm_request_id: None,
        trace_id: None,
        execution_id: None,
        span_id: None,
        parent_span_id: None,
        plan_id: None,
        plan_step_id: None,
        action_id: None,
        signals: None,
        depends_on: Vec::new(),
    }
}

fn planned_run_command(cmd: &str, cwd: &str) -> canon_event::LoopPlanned {
    canon_event::LoopPlanned {
        tick: 0,
        action_kind: "run_command".to_string(),
        action_payload: serde_json::json!({
            "cmd": cmd,
            "cwd": cwd,
        }),
        reason: String::new(),
        llm_request_id: None,
        trace_id: None,
        execution_id: None,
        span_id: None,
        parent_span_id: None,
        plan_id: None,
        plan_step_id: None,
        action_id: None,
        signals: None,
        depends_on: Vec::new(),
    }
}

fn planned_update_file(path: &str, added_line: &str) -> canon_event::LoopPlanned {
    canon_event::LoopPlanned {
        tick: 0,
        action_kind: "apply_patch".to_string(),
        action_payload: serde_json::json!({
            "patch": format!("*** Begin Patch\n*** Update File: {path}\n@@\n{added_line}*** End Patch\n")
        }),
        reason: String::new(),
        llm_request_id: None,
        trace_id: None,
        execution_id: None,
        span_id: None,
        parent_span_id: None,
        plan_id: None,
        plan_step_id: None,
        action_id: None,
        signals: None,
        depends_on: Vec::new(),
    }
}

fn apply_patch_result_value(outcome: ApplyPatchOutcomeClass) -> serde_json::Value {
    let (success, stdout) = match outcome {
        ApplyPatchOutcomeClass::Success => (true, "apply_patch ok: added 1 modified 0 deleted 0"),
        ApplyPatchOutcomeClass::MissingTargetFile => (
            false,
            "apply_patch failed: Failed to read file to update src/lib.rs: No such file or directory (os error 2)",
        ),
        ApplyPatchOutcomeClass::PatchApplyFailure => (
            false,
            "apply_patch failed: invalid hunk at line 12, unexpected line in update chunk",
        ),
        ApplyPatchOutcomeClass::OtherFailure => (false, "patch tool exited with unknown failure"),
    };
    serde_json::json!({
        "action": "apply_patch",
        "success": success,
        "output": {"stdout": stdout, "stderr": ""}
    })
}

fn to_runtime_event(event: &RouteRowEvent) -> RuntimeEvent {
    match event {
        RouteRowEvent::LoopActed { action_kind } => RuntimeEvent::LoopActed(LoopActed {
            tick: 0,
            action_id: None,
            action_kind: (*action_kind).to_string(),
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
        }),
        RouteRowEvent::PlanningCompleted { status, planned_count } => {
            RuntimeEvent::PlanningCompleted(PlanningCompleted {
                tick: 0,
                llm_request_id: Some(String::new()),
                planned_count: *planned_count,
                status: (*status).to_string(),
            })
        }
    }
}

fn to_route_decision(decision: &RouteRowDecision) -> RouteDecision {
    RouteDecision {
        lane: decision.lane,
        suggested_route: decision.suggested_route,
        rationale: String::new(),
        confidence: Some(0.99),
        changed: false,
        note: decision.note.to_string(),
        gate_rules_fired: Vec::new(),
        should_stop: false,
        prompt: String::new(),
    }
}

fn push_unique<T: PartialEq + Copy>(vec: &mut Vec<T>, item: T) {
    if !vec.contains(&item) {
        vec.push(item);
    }
}

fn missing_families<T: PartialEq + Copy>(all: &[T], covered: &[T]) -> Vec<T> {
    all.iter().copied().filter(|family| !covered.contains(family)).collect()
}

fn coverage_section<T, F>(title: &str, covered: &[T], missing: &[T], fmt: F) -> String
where
    F: Fn(&T) -> String,
{
    let mut out = String::new();
    out.push_str(&format!("## {}\n", title));
    out.push_str(&format!("- covered: {}\n", covered.len()));
    out.push_str(&format!("- missing: {}\n", missing.len()));
    if !covered.is_empty() {
        out.push_str("- covered families:\n");
        for item in covered {
            out.push_str(&format!("  - {}\n", fmt(item)));
        }
    }
    if !missing.is_empty() {
        out.push_str("- missing families:\n");
        for item in missing {
            out.push_str(&format!("  - {}\n", fmt(item)));
        }
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_transition_matrix_rows_match_policy_evaluators() {
        let rows = baseline_transition_rows();
        assert_transition_rows(&rows);
    }

    #[test]
    fn baseline_transition_matrix_has_full_family_coverage() {
        let rows = baseline_transition_rows();
        let report = coverage_report(&rows);
        assert!(report.route_missing.is_empty(), "missing route families: {:?}", report.route_missing);
        assert!(report.loop_missing.is_empty(), "missing loop families: {:?}", report.loop_missing);
        assert!(report.run_command_missing.is_empty(), "missing run_command families: {:?}", report.run_command_missing);
        assert!(report.apply_patch_missing.is_empty(), "missing apply_patch families: {:?}", report.apply_patch_missing);
        assert!(report.verify_missing.is_empty(), "missing verify families: {:?}", report.verify_missing);
        assert!(
            report.invalid_plan_retry_missing.is_empty(),
            "missing invalid-plan retry families: {:?}",
            report.invalid_plan_retry_missing
        );
        assert!(
            report.judgment_missing.is_empty(),
            "missing judgment families: {:?}",
            report.judgment_missing
        );
        assert_eq!(report.planner_generated_total, 1056);
        assert_eq!(report.planner_generated_valid, 165);
        assert_eq!(report.route_generated_total, 64);
        assert_eq!(report.route_generated_valid, 33);
    }

    #[test]
    fn generated_judgment_rows_are_consistent_with_expected_counts() {
        let planner_rows = planner_judgment_rows();
        let route_rows = route_semantic_actionability_rows();
        assert_eq!(planner_rows.len(), 154);
        assert_eq!(route_rows.len(), 31);
    }
}
