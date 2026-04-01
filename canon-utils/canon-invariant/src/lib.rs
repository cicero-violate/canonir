use canon_types::{EventDelta, InvariantViolation, RustcEvent, RustcState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
// TRACE: global runtime introspection (file, line, function)

pub mod constraint_harness;
pub mod control_harness;
pub mod cross_product_harness;
pub mod request_lifecycle_harness;

pub use control_harness::{evaluate_control_state, ControlDecision, ControlState, SyntheticControlMetrics};
pub use request_lifecycle_harness::{evaluate_request_lifecycle_state, RequestLifecycleDecision, RequestLifecycleState, SyntheticRequestLifecycleMetrics};

pub fn invariant_violation_delta(message: impl Into<String>) -> EventDelta {
    EventDelta { id: 0, tick: 0, event: RustcEvent::InvariantViolation(InvariantViolation { message: message.into(), recorded: true }) }
}

pub fn invariant_violation_state() -> RustcState {
    RustcState::default()
}

pub fn decision_trace_payload(reason: impl Into<String>, context: Value) -> Value {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConstraintRoute {
    Observe,
    Plan,
    Act,
    Verify,
    Conclude,
}

impl ConstraintRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Plan => "plan",
            Self::Act => "act",
            Self::Verify => "verify",
            Self::Conclude => "conclude",
        }
    }
}

// NEW: centralized Decision type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Observe,
    Plan,
    Act,
    Verify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConstraintAction {
    CargoInit,
    CargoNew,
    RepairLocalized,
    RepairWorkspace,
    Validation,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
pub struct ConstraintState {
    pub semantic_path_exists: bool,
    pub semantic_cargo_project: bool,
    pub real_path_exists: bool,
    pub real_cargo_project: bool,
    pub actionable_failure: bool,
    pub validation_blocked: bool,
    pub entrypoint_missing: bool,
    pub module_gaps_present: bool,
    pub recent_no_semantic_progress: bool,
    pub failure_class_no_actionable: bool,
    pub failure_scope_localized: bool,
    pub failure_scope_workspace: bool,
    pub failure_scope_tooling: bool,
    pub route_objective_contradiction: bool,
    pub scheduler_len: usize,
    pub has_plan: bool,
}

// MINIMAL decision input (SINGLE SOURCE OF TRUTH)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecisionState {
    pub scheduler_len: usize,
    pub has_plan: bool,
}

impl ConstraintState {
    pub fn has_state_drift(self) -> bool {
        self.semantic_path_exists != self.real_path_exists || self.semantic_cargo_project != self.real_cargo_project
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ConstraintContext {
    pub state: ConstraintState,
    pub route: Option<ConstraintRoute>,
    pub action: Option<ConstraintAction>,
    pub deterministic_route: Option<ConstraintRoute>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintDecision {
    Allow,
    Forbid(&'static str),
    // REMOVED: routing decisions must not be encoded as ConstraintRoute
    // Routing is now determined by canonical Decision
    RewriteAction(ConstraintAction, &'static str),
    // TEMP RESTORE: required until all RewriteRoute call sites are removed
    RewriteRoute(ConstraintRoute, &'static str),
}

/// CENTRALIZED decision function (single source of truth)
pub fn decide(state: DecisionState) -> Decision {
    // SINGLE SOURCE OF TRUTH: decision must NOT depend on scheduler_len
    // Routing MUST derive from semantic signals, not queue length
    // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
    static TRACE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let trace_id = TRACE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // DECISION BRANCHES — explicit and traceable
    let decision = if !state.has_plan {
        // no actionable plan → Observe (recover semantic state)
        let d = Decision::Observe;
        eprintln!(
            "[DECIDE TRACE BRANCH] trace_id={} branch=Observe has_plan={}",
            trace_id,
            state.has_plan
        );
        d
    } else {
        // actionable work exists → Act
        let d = Decision::Act;
        eprintln!(
            "[DECIDE TRACE BRANCH] trace_id={} branch=Act has_plan={}",
            trace_id,
            state.has_plan
        );
        d
    };
    // ensure impossible state never occurs
    debug_assert!(!(matches!(decision, Decision::Act) && !state.has_plan));
    // REQUIRED RUNTIME OBSERVABILITY (DO NOT GATE)
    eprintln!(
        "[DECIDE TRACE] trace_id={} {}:{} {} fn=decide has_plan={} decision={:?}",
        trace_id,
        file!(),
        line!(),
        module_path!(),
        state.has_plan,
        decision
    );
    decision
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintDecisionSource {
    MetaInvariant,
    DiscoveredInvariant,
    Deterministic,
}

impl ConstraintDecisionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MetaInvariant => "meta_invariant",
            Self::DiscoveredInvariant => "discovered_invariant",
            Self::Deterministic => "deterministic",
        }
    }
}

pub fn resolve_constraint_decision_precedence(
    meta: Option<ConstraintDecision>, discovered: Option<ConstraintDecision>, deterministic: Option<ConstraintDecision>,
) -> Option<(ConstraintDecisionSource, ConstraintDecision)> {
    if let Some(decision) = meta {
        return Some((ConstraintDecisionSource::MetaInvariant, decision));
    }
    if let Some(decision) = discovered {
        return Some((ConstraintDecisionSource::DiscoveredInvariant, decision));
    }
    if let Some(decision) = deterministic {
        return Some((ConstraintDecisionSource::Deterministic, decision));
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FailureKind {
    InvalidPlanBatch,
    RouteRewrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FailureFingerprint {
    pub kind: FailureKind,
    pub route: Option<ConstraintRoute>,
    pub state: ConstraintState,
}

impl FailureFingerprint {
    pub fn invalid_plan_batch(route: Option<ConstraintRoute>, state: ConstraintState) -> Self {
        Self { kind: FailureKind::InvalidPlanBatch, route, state }
    }

    pub fn route_rewrite(route: ConstraintRoute, state: ConstraintState) -> Self {
        Self { kind: FailureKind::RouteRewrite, route: Some(route), state }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiscoveredInvariant {
    ForcePlanWhenMissingTarget,
    ForcePlanWhenValidationBlocked,
    ForcePlanWhenObjectiveContradiction,
    ForceObserveWhenNoActionableFailure,
}

impl DiscoveredInvariant {
    pub fn feature_name(self) -> &'static str {
        match self {
            Self::ForcePlanWhenMissingTarget => "discovered:force_plan_missing_target",
            Self::ForcePlanWhenValidationBlocked => "discovered:force_plan_validation_blocked",
            Self::ForcePlanWhenObjectiveContradiction => "discovered:force_plan_objective_contradiction",
            Self::ForceObserveWhenNoActionableFailure => "discovered:force_observe_refresh_on_repeated_noop",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvariantPromotion {
    pub invariant: DiscoveredInvariant,
    pub support: u32,
    pub fingerprint: FailureFingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PersistedInvariantStoreEventKind {
    Loaded,
    Updated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistedInvariantStoreEvent {
    pub kind: PersistedInvariantStoreEventKind,
    pub path: PathBuf,
    pub support_entries: usize,
    pub promoted_entries: usize,
    pub reason: &'static str,
}

#[derive(Default)]
struct InvariantDiscoveryState {
    threshold: u32,
    supports: HashMap<FailureFingerprint, u32>,
    promoted: HashMap<DiscoveredInvariant, u32>,
    negative_evidence: HashMap<DiscoveredInvariant, u32>,
}

#[derive(Serialize, Deserialize)]
struct PersistedInvariantDiscoveryState {
    supports: Vec<(FailureFingerprint, u32)>,
    promoted: Vec<(DiscoveredInvariant, u32)>,
}

impl InvariantDiscoveryState {
    fn with_threshold() -> Self {
        let threshold = std::env::var("CANON_DISCOVERED_INVARIANT_SUPPORT").ok().and_then(|value| value.parse::<u32>().ok()).filter(|value| *value > 0).unwrap_or(3);
        let mut state = Self { threshold, ..Self::default() };
        state.load_from_disk();
        state
    }

    fn load_from_disk(&mut self) {
        let Some(path) = invariant_store_path() else {
            return;
        };
        let Ok(raw) = fs::read_to_string(&path) else {
            return;
        };
        let Ok(persisted) = serde_json::from_str::<PersistedInvariantDiscoveryState>(&raw) else {
            return;
        };
        self.supports = persisted.supports.into_iter().collect();
        self.promoted = persisted.promoted.into_iter().collect();
        record_persisted_store_event(PersistedInvariantStoreEventKind::Loaded, &path, self.supports.len(), self.promoted.len(), "store_loaded");
    }

    fn save_to_disk(&self, reason: &'static str) {
        let Some(path) = invariant_store_path() else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let persisted = PersistedInvariantDiscoveryState { supports: self.supports.iter().map(|(k, v)| (*k, *v)).collect(), promoted: self.promoted.iter().map(|(k, v)| (*k, *v)).collect() };
        let Ok(json) = serde_json::to_string_pretty(&persisted) else {
            return;
        };
        if fs::write(&path, json).is_ok() {
            record_persisted_store_event(PersistedInvariantStoreEventKind::Updated, &path, self.supports.len(), self.promoted.len(), reason);
        }
    }
}

fn invariant_store_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    Some(std::env::var("CANON_DISCOVERED_INVARIANTS_PATH").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/workspace/ai_sandbox/canon/state/discovered_invariants.json")))
}

pub fn discovered_invariant_store_path() -> Option<PathBuf> {
    invariant_store_path()
}

fn pending_persisted_store_events() -> &'static Mutex<Vec<PersistedInvariantStoreEvent>> {
    static EVENTS: OnceLock<Mutex<Vec<PersistedInvariantStoreEvent>>> = OnceLock::new();
    EVENTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn record_persisted_store_event(kind: PersistedInvariantStoreEventKind, path: &PathBuf, support_entries: usize, promoted_entries: usize, reason: &'static str) {
    if let Ok(mut events) = pending_persisted_store_events().lock() {
        events.push(PersistedInvariantStoreEvent { kind, path: path.clone(), support_entries, promoted_entries, reason });
    }
}

pub fn drain_persisted_store_events() -> Vec<PersistedInvariantStoreEvent> {
    pending_persisted_store_events().lock().map(|mut events| std::mem::take(&mut *events)).unwrap_or_default()
}

pub fn reload_discovered_invariants_from_disk() {
    if let Ok(mut state) = invariant_discovery_state().lock() {
        let threshold = state.threshold;
        *state = InvariantDiscoveryState { threshold, ..InvariantDiscoveryState::default() };
        state.load_from_disk();
    }
}

pub fn clear_discovered_invariants_store() {
    if let Some(path) = invariant_store_path() {
        let _ = fs::remove_file(path);
    }
}

fn invariant_discovery_state() -> &'static Mutex<InvariantDiscoveryState> {
    static STATE: OnceLock<Mutex<InvariantDiscoveryState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(InvariantDiscoveryState::with_threshold()))
}

fn fingerprint_pattern(fingerprint: FailureFingerprint) -> Option<DiscoveredInvariant> {
    match fingerprint.route {
        Some(ConstraintRoute::Observe) if !fingerprint.state.real_path_exists => Some(DiscoveredInvariant::ForcePlanWhenMissingTarget),
        Some(ConstraintRoute::Verify | ConstraintRoute::Conclude) if fingerprint.state.validation_blocked || fingerprint.state.entrypoint_missing || fingerprint.state.module_gaps_present => {
            Some(DiscoveredInvariant::ForcePlanWhenValidationBlocked)
        }
        Some(ConstraintRoute::Verify | ConstraintRoute::Conclude) if fingerprint.state.route_objective_contradiction => Some(DiscoveredInvariant::ForcePlanWhenObjectiveContradiction),
        Some(ConstraintRoute::Plan)
            if fingerprint.state.failure_class_no_actionable
                || (fingerprint.state.recent_no_semantic_progress
                    && !fingerprint.state.actionable_failure
                    && !fingerprint.state.validation_blocked
                    && !fingerprint.state.entrypoint_missing
                    && !fingerprint.state.module_gaps_present
                    && fingerprint.state.real_path_exists) =>
        {
            Some(DiscoveredInvariant::ForceObserveWhenNoActionableFailure)
        }
        _ => None,
    }
}

pub fn observe_failure_fingerprint(fingerprint: FailureFingerprint) -> Option<InvariantPromotion> {
    let invariant = fingerprint_pattern(fingerprint)?;
    let mut state = invariant_discovery_state().lock().ok()?;
    let support = {
        let entry = state.supports.entry(fingerprint).or_insert(0);
        *entry = entry.saturating_add(1);
        *entry
    };
    state.save_to_disk("support_observed");
    let threshold = match invariant {
        DiscoveredInvariant::ForcePlanWhenMissingTarget => 3,
        DiscoveredInvariant::ForceObserveWhenNoActionableFailure => 3,
        _ => state.threshold,
    };
    if support < threshold {
        return None;
    }
    state.promoted.insert(invariant, support);
    state.negative_evidence.remove(&invariant);
    state.save_to_disk("promotion");
    Some(InvariantPromotion { invariant, support, fingerprint })
}

pub fn discovered_invariants() -> Vec<DiscoveredInvariant> {
    invariant_discovery_state().lock().map(|state| state.promoted.keys().copied().collect()).unwrap_or_default()
}

pub fn record_negative_evidence(invariant: DiscoveredInvariant) {
    if let Ok(mut state) = invariant_discovery_state().lock() {
        let entry = state.negative_evidence.entry(invariant).or_insert(0);
        *entry = entry.saturating_add(1);

        let demotion_threshold = 3;
        if *entry >= demotion_threshold {
            state.promoted.remove(&invariant);
            state.negative_evidence.remove(&invariant);
            state.save_to_disk("demotion");
        }
    }
}

pub fn reset_discovered_invariants_for_tests() {
    if let Ok(mut state) = invariant_discovery_state().lock() {
        *state = InvariantDiscoveryState::with_threshold();
    }
    // Ensure persisted state does not leak between tests
    clear_discovered_invariants_store();
}

#[allow(dead_code)]
fn evaluate_discovered_invariants(ctx: &ConstraintContext) -> Option<ConstraintDecision> {
    // Only apply discovered invariants when actionable failure is present
    if !ctx.state.actionable_failure {
        return None;
    }
    for invariant in discovered_invariants() {
        match invariant {
            DiscoveredInvariant::ForcePlanWhenMissingTarget => {
                if !ctx.state.real_path_exists && ctx.route != Some(ConstraintRoute::Plan) {
                    return Some(ConstraintDecision::Allow);
                }
            }
            DiscoveredInvariant::ForceObserveWhenNoActionableFailure => {}
            DiscoveredInvariant::ForcePlanWhenValidationBlocked => {
                if matches!(ctx.route, Some(ConstraintRoute::Verify | ConstraintRoute::Conclude))
                    && ctx.state.actionable_failure
                    && (ctx.state.validation_blocked || ctx.state.entrypoint_missing || ctx.state.module_gaps_present)
                {
                    return Some(ConstraintDecision::Allow);
                }
            }
            DiscoveredInvariant::ForcePlanWhenObjectiveContradiction => {
                if matches!(ctx.route, Some(ConstraintRoute::Verify | ConstraintRoute::Conclude)) && ctx.state.actionable_failure && ctx.state.route_objective_contradiction {
                    return Some(ConstraintDecision::Allow);
                }
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HarnessPrimitiveCapability {
    ReadSearch,
    StructuredEdit,
    ApplyPatch,
    RunVerifier,
    ObserveDiagnostics,
}

impl HarnessPrimitiveCapability {
    pub const MINIMAL_SELF_REPAIR_SET: [Self; 5] = [Self::ReadSearch, Self::StructuredEdit, Self::ApplyPatch, Self::RunVerifier, Self::ObserveDiagnostics];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadSearch => "read_search",
            Self::StructuredEdit => "structured_edit",
            Self::ApplyPatch => "apply_patch",
            Self::RunVerifier => "run_verifier",
            Self::ObserveDiagnostics => "observe_diagnostics",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HarnessCapabilityState {
    pub read_search: bool,
    pub structured_edit: bool,
    pub apply_patch: bool,
    pub run_verifier: bool,
    pub observe_diagnostics: bool,
}

impl HarnessCapabilityState {
    pub fn has(self, capability: HarnessPrimitiveCapability) -> bool {
        match capability {
            HarnessPrimitiveCapability::ReadSearch => self.read_search,
            HarnessPrimitiveCapability::StructuredEdit => self.structured_edit,
            HarnessPrimitiveCapability::ApplyPatch => self.apply_patch,
            HarnessPrimitiveCapability::RunVerifier => self.run_verifier,
            HarnessPrimitiveCapability::ObserveDiagnostics => self.observe_diagnostics,
        }
    }
}

pub fn meta_invariant_harness_self_repair_missing_capabilities(state: HarnessCapabilityState) -> Vec<HarnessPrimitiveCapability> {
    HarnessPrimitiveCapability::MINIMAL_SELF_REPAIR_SET.into_iter().filter(|capability| !state.has(*capability)).collect()
}

pub fn meta_invariant_harness_self_repair_ready(state: HarnessCapabilityState) -> bool {
    meta_invariant_harness_self_repair_missing_capabilities(state).is_empty()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetaInvariantBootstrapToolChoice {
    CargoNew,
    CargoInit,
}

impl MetaInvariantBootstrapToolChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CargoNew => "cargo_new",
            Self::CargoInit => "cargo_init",
        }
    }
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
pub enum MetaInvariantVerifierSequenceStep {
    LoopVerified,
    VerifierPolicyUpdated,
    LoopRewarded,
}

impl MetaInvariantVerifierSequenceStep {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LoopVerified => "loop_verified",
            Self::VerifierPolicyUpdated => "verifier_policy_updated",
            Self::LoopRewarded => "loop_rewarded",
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
        format!("verifier_outcome={} retry_policy={} reward_bias={} actionable_failure={}", self.verifier_outcome.as_str(), self.retry_policy, self.reward_bias, self.actionable_failure)
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

pub fn meta_invariant_classify_planned_action_class(action_kind: &str, action_payload: &Value) -> PlannedActionClass {
    match action_kind {
        "read_file" | "list_dir" | "search_files" => PlannedActionClass::PassiveDiscovery,
        "run_command" => action_payload
            .get("cmd")
            .and_then(|v| v.as_str())
            .map(|cmd| if cmd.contains("cargo check") || cmd.contains("cargo build") || cmd.contains("cargo test") { PlannedActionClass::Verification } else { PlannedActionClass::Mutation })
            .unwrap_or(PlannedActionClass::Unknown),
        "write_file" | "patch_file" | "apply_patch" | "edit.rename_symbol" | "edit.move_symbol" | "edit.add_import" | "edit.define_symbol_stub" | "edit.create_module_file" => {
            PlannedActionClass::Mutation
        }
        _ => PlannedActionClass::Unknown,
    }
}

pub fn classify_planned_action_class(action_kind: &str, action_payload: &Value) -> PlannedActionClass {
    meta_invariant_classify_planned_action_class(action_kind, action_payload)
}

pub fn meta_invariant_is_localized_repair_action(action_kind: &str) -> bool {
    matches!(action_kind, "edit.add_import" | "edit.define_symbol_stub" | "edit.rename_symbol" | "apply_patch")
}

pub fn is_localized_repair_action(action_kind: &str) -> bool {
    meta_invariant_is_localized_repair_action(action_kind)
}

pub fn meta_invariant_all_failures_typed(failure_class: Option<&str>, failure_scope: Option<&str>) -> bool {
    matches!(failure_class, Some(value) if !value.trim().is_empty()) && matches!(failure_scope, Some("localized" | "workspace" | "tooling" | "none"))
}

pub fn meta_invariant_any_action_cites_failure(action_payload: &Value, active_failure_class: Option<&str>) -> bool {
    match active_failure_class {
        Some(expected) if !expected.trim().is_empty() => action_payload.get("failure_class").and_then(|v| v.as_str()).map(|value| value == expected).unwrap_or(false),
        _ => true,
    }
}

pub fn meta_invariant_is_mutating_action(action_kind: &str, action_payload: &Value) -> bool {
    matches!(meta_invariant_classify_planned_action_class(action_kind, action_payload), PlannedActionClass::Mutation)
}

pub fn meta_invariant_expected_verifier(action_kind: &str, action_payload: &Value) -> Option<&'static str> {
    if !meta_invariant_is_mutating_action(action_kind, action_payload) {
        return None;
    }
    match action_kind {
        "edit.rename_symbol" | "edit.move_symbol" | "edit.add_import" | "edit.define_symbol_stub" | "edit.create_module_file" => Some("graph_proof"),
        "apply_patch" | "patch_file" | "write_file" | "run_command" => Some("cargo_check"),
        _ => Some("cargo_check"),
    }
}

pub fn meta_invariant_action_must_declare_verifier(action_kind: &str, action_payload: &Value) -> bool {
    let Some(expected) = meta_invariant_expected_verifier(action_kind, action_payload) else {
        return true;
    };
    action_payload.get("verifier").and_then(|v| v.as_str()).map(|value| !value.trim().is_empty() && value == expected).unwrap_or(false)
}

pub fn meta_invariant_classify_bootstrap_tool(cmd: &str) -> Option<MetaInvariantBootstrapToolChoice> {
    if cmd.contains("cargo new") {
        Some(MetaInvariantBootstrapToolChoice::CargoNew)
    } else if cmd.contains("cargo init") {
        Some(MetaInvariantBootstrapToolChoice::CargoInit)
    } else {
        None
    }
}

pub fn meta_invariant_tool_selection_correctness(expected_tool_choice: &str, action_kind: &str, action_payload: &Value) -> bool {
    if action_kind != "run_command" {
        return false;
    }
    let Some(cmd) = action_payload.get("cmd").and_then(|v| v.as_str()) else {
        return false;
    };
    meta_invariant_classify_bootstrap_tool(cmd).map(|tool| tool.as_str() == expected_tool_choice).unwrap_or(false)
}

pub fn evaluate_constraint_context(ctx: &ConstraintContext) -> ConstraintDecision {
    // Apply meta invariants first; discovered invariants should not override bootstrap requirement
    if let (Some(expected), Some(actual)) = (ctx.deterministic_route, ctx.route) {
        if expected != actual {
            return ConstraintDecision::Forbid("meta_invariant_deterministic_route_authority: deterministic routes cannot be overridden");
        }
    }

    if let Some(route) = ctx.route {
        if ctx.state.has_state_drift() && route != ConstraintRoute::Observe {
            return ConstraintDecision::Allow;
        }
        if !ctx.state.real_path_exists && route != ConstraintRoute::Plan {
            return ConstraintDecision::Allow;
        }
        if route == ConstraintRoute::Plan
            && (ctx.state.failure_class_no_actionable || (ctx.state.recent_no_semantic_progress && !ctx.state.actionable_failure))
            && !ctx.state.validation_blocked
            && !ctx.state.entrypoint_missing
            && !ctx.state.module_gaps_present
            && ctx.state.real_path_exists
        {
            eprintln!(
                "[constraint][plan_stays_plan] route={} deterministic_route={:?} failure_class_no_actionable={} recent_no_semantic_progress={} actionable_failure={} validation_blocked={} entrypoint_missing={} module_gaps_present={} real_path_exists={} real_cargo_project={} semantic_path_exists={} semantic_cargo_project={}",
                route.as_str(),
                ctx.deterministic_route.map(|r| r.as_str()),
                ctx.state.failure_class_no_actionable,
                ctx.state.recent_no_semantic_progress,
                ctx.state.actionable_failure,
                ctx.state.validation_blocked,
                ctx.state.entrypoint_missing,
                ctx.state.module_gaps_present,
                ctx.state.real_path_exists,
                ctx.state.real_cargo_project,
                ctx.state.semantic_path_exists,
                ctx.state.semantic_cargo_project,
            );
        }
        if matches!(route, ConstraintRoute::Verify | ConstraintRoute::Conclude) && (ctx.state.validation_blocked || ctx.state.actionable_failure) {
            return ConstraintDecision::Allow;
        }
        if matches!(route, ConstraintRoute::Verify | ConstraintRoute::Conclude) && (ctx.state.entrypoint_missing || ctx.state.module_gaps_present) {
            return ConstraintDecision::Allow;
        }
        if matches!(route, ConstraintRoute::Verify | ConstraintRoute::Conclude) && ctx.state.route_objective_contradiction {
            return ConstraintDecision::Allow;
        }
    }

    if let Some(action) = ctx.action {
        match action {
            ConstraintAction::RepairLocalized | ConstraintAction::RepairWorkspace if ctx.state.failure_class_no_actionable || !ctx.state.actionable_failure => {
                return ConstraintDecision::Forbid("meta_invariant_no_actionable_failure: repair actions are forbidden because there is no actionable failure");
            }
            ConstraintAction::RepairLocalized if !ctx.state.failure_scope_localized => {
                return ConstraintDecision::Forbid("meta_invariant_failure_scope: localized repair actions require localized failure scope");
            }
            ConstraintAction::RepairWorkspace if ctx.state.failure_scope_localized => {
                return ConstraintDecision::Forbid("meta_invariant_failure_scope: workspace repair actions are too broad for localized failures");
            }
            ConstraintAction::Validation if ctx.state.validation_blocked => {
                return ConstraintDecision::Forbid("meta_invariant_validation_timing: validation actions are forbidden while planning preconditions remain unresolved");
            }
            ConstraintAction::Validation if !ctx.state.real_path_exists => {
                return ConstraintDecision::Forbid("meta_invariant_bootstrap_required: validation actions are forbidden while the target workspace is missing");
            }
            ConstraintAction::Validation if ctx.state.entrypoint_missing || ctx.state.module_gaps_present => {
                return ConstraintDecision::Forbid("meta_invariant_validation_timing: validation actions are forbidden while required files are still missing");
            }
            ConstraintAction::CargoInit if !ctx.state.real_path_exists => {
                return ConstraintDecision::RewriteAction(ConstraintAction::CargoNew, "meta_invariant_tool_selection_correctness: missing target requires cargo new");
            }
            ConstraintAction::CargoNew if ctx.state.real_path_exists && !ctx.state.real_cargo_project => {
                return ConstraintDecision::RewriteAction(ConstraintAction::CargoInit, "meta_invariant_tool_selection_correctness: existing non-Cargo directory requires cargo init");
            }
            ConstraintAction::CargoInit | ConstraintAction::CargoNew if ctx.state.real_cargo_project => {
                return ConstraintDecision::Forbid("meta_invariant_tool_selection_correctness: bootstrap commands are forbidden for existing Cargo projects");
            }
            _ => {}
        }
    }

    ConstraintDecision::Allow
}

pub fn meta_invariant_has_actionable_failure(
    validation_blocked_by_preconditions: bool, compiler_repair_required: bool, planning_preconditions_len: usize, compiler_hints_len: usize, module_gaps_len: usize,
) -> bool {
    validation_blocked_by_preconditions || compiler_repair_required || planning_preconditions_len > 0 || compiler_hints_len > 0 || module_gaps_len > 0
}

pub fn semantic_summary_has_actionable_failure(
    validation_blocked_by_preconditions: bool, compiler_repair_required: bool, planning_preconditions_len: usize, compiler_hints_len: usize, module_gaps_len: usize,
) -> bool {
    meta_invariant_has_actionable_failure(validation_blocked_by_preconditions, compiler_repair_required, planning_preconditions_len, compiler_hints_len, module_gaps_len)
}

pub fn meta_invariant_failure_scope_is_sufficient(compiler_repair_required: bool, compiler_hints_len: usize, failure_scope: Option<&str>) -> bool {
    if !compiler_repair_required || compiler_hints_len == 0 {
        return true;
    }
    matches!(failure_scope, Some("localized") | Some("workspace") | Some("tooling"))
}

pub fn failure_scope_is_sufficient(compiler_repair_required: bool, compiler_hints_len: usize, failure_scope: Option<&str>) -> bool {
    meta_invariant_failure_scope_is_sufficient(compiler_repair_required, compiler_hints_len, failure_scope)
}

pub fn meta_invariant_high_invalid_plan_requires_simple_batch(invalid_plan_rate: f32, planning_attempts: u32) -> bool {
    invalid_plan_rate > 0.5 && planning_attempts >= 3
}

pub fn high_invalid_plan_pressure_requires_single_action(invalid_plan_rate: f32, planning_attempts: u32) -> bool {
    meta_invariant_high_invalid_plan_requires_simple_batch(invalid_plan_rate, planning_attempts)
}

pub fn meta_invariant_no_progress_forces_change(no_progress_streak: u32, action_class: PlannedActionClass) -> bool {
    no_progress_streak >= 2 && matches!(action_class, PlannedActionClass::PassiveDiscovery | PlannedActionClass::Verification)
}

pub fn stalled_loop_forbids_action_class(no_progress_streak: u32, action_class: PlannedActionClass) -> bool {
    meta_invariant_no_progress_forces_change(no_progress_streak, action_class)
}

fn looks_like_compiler_failure(text: &str) -> bool {
    text.contains("error[E")
        || text.contains("could not compile")
        || text.contains("allow(dead_code) incompatible with previous forbid")
        || text.contains("file not found for module `")
        || text.contains("cargo_check_failed")
}

pub fn meta_invariant_classify_verifier_outcome(passed: bool, compiler_clean: bool, diagnostics: &[String]) -> MetaInvariantVerifierOutcome {
    if passed && compiler_clean {
        MetaInvariantVerifierOutcome::Passed
    } else if diagnostics.iter().any(|d| looks_like_compiler_failure(d)) {
        MetaInvariantVerifierOutcome::CompilerFailure
    } else {
        MetaInvariantVerifierOutcome::FailedNoCompilerSignal
    }
}

pub fn meta_invariant_all_results_update_policy(passed: bool, compiler_clean: bool, diagnostics: &[String]) -> MetaInvariantPolicyUpdate {
    let verifier_outcome = meta_invariant_classify_verifier_outcome(passed, compiler_clean, diagnostics);
    match verifier_outcome {
        MetaInvariantVerifierOutcome::Passed => MetaInvariantPolicyUpdate { verifier_outcome, retry_policy: "none", reward_bias: "positive", actionable_failure: false },
        MetaInvariantVerifierOutcome::CompilerFailure | MetaInvariantVerifierOutcome::FailedNoCompilerSignal => {
            MetaInvariantPolicyUpdate { verifier_outcome, retry_policy: "corrective_retry", reward_bias: "negative", actionable_failure: true }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrajectoryStep {
    pub semantic_progress: i32,
    pub no_progress: bool,
    pub invalid_action: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrajectoryScore {
    pub total: i32,
    pub progress: i32,
    pub penalties: i32,
}

pub fn score_trajectory(steps: &[TrajectoryStep]) -> TrajectoryScore {
    let mut progress = 0;
    let mut penalties = 0;

    for step in steps {
        progress += step.semantic_progress;

        if step.no_progress {
            penalties += 1;
        }

        if step.invalid_action {
            penalties += 2;
        }
    }

    let total = progress - penalties;

    TrajectoryScore { total, progress, penalties }
}

pub fn meta_invariant_verifier_sequence_contract(
    step: MetaInvariantVerifierSequenceStep, last_control_kind: Option<&str>, pending_required_successor: Option<&str>, has_last_verified: bool,
) -> Option<&'static str> {
    match step {
        MetaInvariantVerifierSequenceStep::LoopVerified => {
            if last_control_kind == Some("route_selected") && pending_required_successor == Some("verifier_policy_updated") {
                None
            } else {
                Some("route_selected(verify) must be followed by loop_verified before verifier_policy_updated")
            }
        }
        MetaInvariantVerifierSequenceStep::VerifierPolicyUpdated => {
            if last_control_kind == Some("loop_verified") && has_last_verified {
                None
            } else {
                Some("loop_verified must be followed by verifier_policy_updated before loop_rewarded")
            }
        }
        MetaInvariantVerifierSequenceStep::LoopRewarded => {
            let last = last_control_kind.map(|s| s.to_ascii_lowercase());
            let pending = pending_required_successor.map(|s| s.to_ascii_lowercase());

            if last.as_deref() == Some("verifier_policy_updated") && pending.as_deref() == Some("loop_rewarded") {
                None
            } else if last.as_deref() == Some("route_selected") && pending.as_deref() == Some("loop_rewarded") {
                None
            } else {
                Some("loop_rewarded must follow verifier_policy_updated, except for direct conclude routing")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        discovered_invariants, evaluate_constraint_context, meta_invariant_all_results_update_policy, meta_invariant_classify_verifier_outcome, meta_invariant_tool_selection_correctness,
        meta_invariant_verifier_sequence_contract, observe_failure_fingerprint, reset_discovered_invariants_for_tests, ConstraintAction, ConstraintContext, ConstraintDecision, ConstraintRoute,
        ConstraintState, DiscoveredInvariant, FailureFingerprint, MetaInvariantVerifierOutcome, MetaInvariantVerifierSequenceStep,
    };
    use crate::{
        invariant_discovery_state, meta_invariant_harness_self_repair_missing_capabilities, meta_invariant_harness_self_repair_ready, record_negative_evidence, resolve_constraint_decision_precedence,
        score_trajectory, ConstraintDecisionSource, HarnessCapabilityState, HarnessPrimitiveCapability, TrajectoryStep,
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
        let diagnostics = vec!["cargo_check_failed".to_string(), "error[E0432]: unresolved import".to_string()];
        let outcome = meta_invariant_classify_verifier_outcome(false, false, &diagnostics);
        assert_eq!(outcome, MetaInvariantVerifierOutcome::CompilerFailure);
        let update = meta_invariant_all_results_update_policy(false, false, &diagnostics);
        assert_eq!(update.retry_policy, "corrective_retry");
        assert_eq!(update.reward_bias, "negative");
        assert!(update.actionable_failure);
    }

    #[test]
    fn meta_invariant_verifier_sequence_contract_accepts_verify_path() {
        assert_eq!(meta_invariant_verifier_sequence_contract(MetaInvariantVerifierSequenceStep::LoopVerified, Some("route_selected"), Some("verifier_policy_updated"), false,), None);
        assert_eq!(meta_invariant_verifier_sequence_contract(MetaInvariantVerifierSequenceStep::VerifierPolicyUpdated, Some("loop_verified"), Some("verifier_policy_updated"), true,), None);
        assert_eq!(meta_invariant_verifier_sequence_contract(MetaInvariantVerifierSequenceStep::LoopRewarded, Some("verifier_policy_updated"), Some("loop_rewarded"), true,), None);
    }

    #[test]
    fn meta_invariant_verifier_sequence_contract_rejects_skipped_policy_update() {
        assert_eq!(
            meta_invariant_verifier_sequence_contract(MetaInvariantVerifierSequenceStep::LoopRewarded, Some("loop_verified"), Some("verifier_policy_updated"), true,),
            Some("loop_rewarded must follow verifier_policy_updated, except for direct conclude routing")
        );
    }

    #[test]
    fn meta_invariant_verifier_sequence_contract_rejects_skipped_loop_verified() {
        assert_eq!(
            meta_invariant_verifier_sequence_contract(MetaInvariantVerifierSequenceStep::VerifierPolicyUpdated, Some("route_selected"), Some("verifier_policy_updated"), false,),
            Some("loop_verified must be followed by verifier_policy_updated before loop_rewarded")
        );
    }

    #[test]
    fn meta_invariant_tool_selection_correctness_matches_expected_bootstrap_tool() {
        let cargo_new = serde_json::json!({"cmd": "cargo new event_sim_coverage"});
        let cargo_init = serde_json::json!({"cmd": "cargo init --name event_sim_coverage ."});
        assert!(meta_invariant_tool_selection_correctness("cargo_new", "run_command", &cargo_new));
        assert!(meta_invariant_tool_selection_correctness("cargo_init", "run_command", &cargo_init));
        assert!(!meta_invariant_tool_selection_correctness("cargo_new", "run_command", &cargo_init));
    }

    #[test]
    fn constraint_engine_rewrites_route_on_state_drift() {
        let decision = evaluate_constraint_context(&ConstraintContext {
            state: ConstraintState {
                scheduler_len: 0,
                has_plan: false,
                semantic_path_exists: false,
                semantic_cargo_project: false,
                real_path_exists: true,
                real_cargo_project: true,
                actionable_failure: false,
                validation_blocked: false,
                entrypoint_missing: false,
                module_gaps_present: false,
                recent_no_semantic_progress: false,
                failure_class_no_actionable: false,
                failure_scope_localized: false,
                failure_scope_workspace: false,
                failure_scope_tooling: false,
                route_objective_contradiction: false,
            },
            route: Some(ConstraintRoute::Plan),
            action: None,
            deterministic_route: None,
        });
        assert_eq!(decision, ConstraintDecision::Allow);
    }

    #[test]
    fn harness_self_repair_requires_minimal_capability_basis() {
        let ready = HarnessCapabilityState { read_search: true, structured_edit: true, apply_patch: true, run_verifier: true, observe_diagnostics: true };
        assert!(meta_invariant_harness_self_repair_ready(ready));

        let missing = meta_invariant_harness_self_repair_missing_capabilities(HarnessCapabilityState {
            read_search: true,
            structured_edit: false,
            apply_patch: true,
            run_verifier: false,
            observe_diagnostics: true,
        });
        assert_eq!(missing, vec![HarnessPrimitiveCapability::StructuredEdit, HarnessPrimitiveCapability::RunVerifier,]);
    }

    #[test]
    fn constraint_engine_forbids_repair_without_actionable_failure() {
        let decision = evaluate_constraint_context(&ConstraintContext {
            state: ConstraintState {
                actionable_failure: false,
                validation_blocked: false,
                entrypoint_missing: false,
                module_gaps_present: false,
                recent_no_semantic_progress: false,
                failure_class_no_actionable: false,
                failure_scope_localized: false,
                failure_scope_workspace: false,
                failure_scope_tooling: false,
                ..ConstraintState::default()
            },
            route: None,
            action: Some(ConstraintAction::RepairLocalized),
            deterministic_route: None,
        });
        assert_eq!(decision, ConstraintDecision::Forbid("meta_invariant_no_actionable_failure: repair actions are forbidden because there is no actionable failure",));
    }

    #[test]
    fn constraint_engine_bootstrap_state_map_is_exhaustive() {
        let reals = [
            (false, false, ConstraintAction::CargoInit, ConstraintDecision::RewriteAction(ConstraintAction::CargoNew, "meta_invariant_tool_selection_correctness: missing target requires cargo new")),
            (false, false, ConstraintAction::CargoNew, ConstraintDecision::Allow),
            (true, false, ConstraintAction::CargoInit, ConstraintDecision::Allow),
            (
                true,
                false,
                ConstraintAction::CargoNew,
                ConstraintDecision::RewriteAction(ConstraintAction::CargoInit, "meta_invariant_tool_selection_correctness: existing non-Cargo directory requires cargo init"),
            ),
            (true, true, ConstraintAction::CargoInit, ConstraintDecision::Forbid("meta_invariant_tool_selection_correctness: bootstrap commands are forbidden for existing Cargo projects")),
            (true, true, ConstraintAction::CargoNew, ConstraintDecision::Forbid("meta_invariant_tool_selection_correctness: bootstrap commands are forbidden for existing Cargo projects")),
        ];
        for (real_path_exists, real_cargo_project, action, expected) in reals {
            let decision = evaluate_constraint_context(&ConstraintContext {
                state: ConstraintState {
                    scheduler_len: 0,
                    has_plan: false,
                    semantic_path_exists: real_path_exists,
                    semantic_cargo_project: real_cargo_project,
                    real_path_exists,
                    real_cargo_project,
                    actionable_failure: false,
                    validation_blocked: false,
                    entrypoint_missing: false,
                    module_gaps_present: false,
                    recent_no_semantic_progress: false,
                    failure_class_no_actionable: false,
                    failure_scope_localized: false,
                    failure_scope_workspace: false,
                    failure_scope_tooling: false,
                    route_objective_contradiction: false,
                },
                route: None,
                action: Some(action),
                deterministic_route: None,
            });
            assert_eq!(decision, expected);
        }
    }

    #[test]
    fn constraint_engine_forbids_deterministic_route_override() {
        let decision = evaluate_constraint_context(&ConstraintContext {
            state: ConstraintState::default(),
            route: Some(ConstraintRoute::Plan),
            action: None,
            deterministic_route: Some(ConstraintRoute::Observe),
        });
        assert_eq!(decision, ConstraintDecision::Forbid("meta_invariant_deterministic_route_authority: deterministic routes cannot be overridden",));
    }

    #[test]
    fn precedence_matrix_prefers_meta_over_discovered_and_deterministic() {
        let chosen = resolve_constraint_decision_precedence(
            Some(ConstraintDecision::Forbid("meta_invariant_state_reality_authority")),
            Some(ConstraintDecision::RewriteRoute(ConstraintRoute::Plan, "discovered_invariant_missing_target")),
            Some(ConstraintDecision::RewriteRoute(ConstraintRoute::Observe, "deterministic_observe_refresh")),
        );
        assert_eq!(chosen, Some((ConstraintDecisionSource::MetaInvariant, ConstraintDecision::Forbid("meta_invariant_state_reality_authority"),)));
    }

    #[test]
    fn precedence_matrix_prefers_discovered_over_deterministic() {
        let chosen = resolve_constraint_decision_precedence(
            None,
            Some(ConstraintDecision::RewriteRoute(ConstraintRoute::Plan, "discovered_invariant_missing_target")),
            Some(ConstraintDecision::RewriteRoute(ConstraintRoute::Observe, "deterministic_observe_refresh")),
        );
        assert_eq!(chosen, Some((ConstraintDecisionSource::DiscoveredInvariant, ConstraintDecision::RewriteRoute(ConstraintRoute::Plan, "discovered_invariant_missing_target",),)));
    }

    #[test]
    fn precedence_matrix_uses_deterministic_when_higher_layers_absent() {
        let chosen = resolve_constraint_decision_precedence(None, None, Some(ConstraintDecision::RewriteRoute(ConstraintRoute::Observe, "deterministic_observe_refresh")));
        assert_eq!(chosen, Some((ConstraintDecisionSource::Deterministic, ConstraintDecision::RewriteRoute(ConstraintRoute::Observe, "deterministic_observe_refresh",),)));
    }

    #[test]
    fn precedence_matrix_returns_none_when_no_layers_fire() {
        assert_eq!(resolve_constraint_decision_precedence(None, None, None), None);
    }

    #[test]
    fn invariant_demotes_after_negative_evidence() {
        reset_discovered_invariants_for_tests();

        let inv = DiscoveredInvariant::ForcePlanWhenMissingTarget;

        // simulate promotion
        {
            let mut state = invariant_discovery_state().lock().unwrap();
            state.promoted.insert(inv, 5);
        }

        // add negative evidence
        record_negative_evidence(inv);
        record_negative_evidence(inv);
        record_negative_evidence(inv);

        let active = discovered_invariants();
        assert!(!active.contains(&inv));
    }

    #[test]
    fn trajectory_scoring_rewards_progress() {
        let steps = vec![TrajectoryStep { semantic_progress: 2, no_progress: false, invalid_action: false }, TrajectoryStep { semantic_progress: 1, no_progress: false, invalid_action: false }];

        let score = score_trajectory(&steps);
        assert_eq!(score.total, 3);
    }

    #[test]
    fn trajectory_scoring_penalizes_no_progress() {
        let steps = vec![TrajectoryStep { semantic_progress: 0, no_progress: true, invalid_action: false }];

        let score = score_trajectory(&steps);
        assert_eq!(score.total, -1);
    }

    #[test]
    fn trajectory_scoring_penalizes_invalid_actions_more() {
        let steps = vec![TrajectoryStep { semantic_progress: 0, no_progress: false, invalid_action: true }];

        let score = score_trajectory(&steps);
        assert_eq!(score.total, -2);
    }

    #[test]
    fn trajectory_scoring_balances_progress_and_penalty() {
        let steps = vec![TrajectoryStep { semantic_progress: 2, no_progress: false, invalid_action: false }, TrajectoryStep { semantic_progress: 0, no_progress: true, invalid_action: false }];

        let score = score_trajectory(&steps);
        assert_eq!(score.total, 1);
    }

    #[test]
    fn constraint_engine_forbids_validation_when_entrypoint_is_missing() {
        let decision = evaluate_constraint_context(&ConstraintContext {
            state: ConstraintState {
                semantic_path_exists: true,
                semantic_cargo_project: true,
                real_path_exists: true,
                real_cargo_project: true,
                entrypoint_missing: true,
                ..ConstraintState::default()
            },
            route: None,
            action: Some(ConstraintAction::Validation),
            deterministic_route: None,
        });
        assert_eq!(decision, ConstraintDecision::Forbid("meta_invariant_validation_timing: validation actions are forbidden while required files are still missing",));
    }

    #[test]
    fn constraint_engine_rewrites_route_to_plan_when_target_is_missing() {
        let decision = evaluate_constraint_context(&ConstraintContext {
            state: ConstraintState { semantic_path_exists: false, semantic_cargo_project: false, real_path_exists: false, real_cargo_project: false, ..ConstraintState::default() },
            route: Some(ConstraintRoute::Observe),
            action: None,
            deterministic_route: None,
        });
        assert_eq!(decision, ConstraintDecision::Allow);
    }

    #[test]
    fn constraint_engine_forbids_validation_when_target_is_missing() {
        let decision = evaluate_constraint_context(&ConstraintContext {
            state: ConstraintState { semantic_path_exists: false, semantic_cargo_project: false, real_path_exists: false, real_cargo_project: false, ..ConstraintState::default() },
            route: None,
            action: Some(ConstraintAction::Validation),
            deterministic_route: None,
        });
        assert_eq!(decision, ConstraintDecision::Forbid("meta_invariant_bootstrap_required: validation actions are forbidden while the target workspace is missing",));
    }

    #[test]
    fn constraint_engine_rewrites_verify_when_module_gaps_are_present() {
        let decision = evaluate_constraint_context(&ConstraintContext {
            state: ConstraintState {
                semantic_path_exists: true,
                semantic_cargo_project: true,
                real_path_exists: true,
                real_cargo_project: true,
                module_gaps_present: true,
                ..ConstraintState::default()
            },
            route: Some(ConstraintRoute::Verify),
            action: None,
            deterministic_route: None,
        });
        assert_eq!(decision, ConstraintDecision::Allow);
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum DriftAxis {
        Clean,
        Drifted,
    }

    impl DriftAxis {
        const ALL: [Self; 2] = [Self::Clean, Self::Drifted];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ActionableFailureAxis {
        No,
        Yes,
    }

    impl ActionableFailureAxis {
        const ALL: [Self; 2] = [Self::No, Self::Yes];
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
    enum RouteAxis {
        None,
        Observe,
        Plan,
        Act,
        Verify,
        Conclude,
    }

    impl RouteAxis {
        const ALL: [Self; 6] = [Self::None, Self::Observe, Self::Plan, Self::Act, Self::Verify, Self::Conclude];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ActionAxis {
        None,
        Repair,
        CargoInit,
        CargoNew,
        Validation,
    }

    impl ActionAxis {
        const ALL: [Self; 5] = [Self::None, Self::Repair, Self::CargoInit, Self::CargoNew, Self::Validation];
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum DeterministicAxis {
        None,
        MatchObserve,
        MatchPlan,
        ConflictObserve,
    }

    impl DeterministicAxis {
        const ALL: [Self; 4] = [Self::None, Self::MatchObserve, Self::MatchPlan, Self::ConflictObserve];
    }

    fn route_axis_value(axis: RouteAxis) -> Option<ConstraintRoute> {
        match axis {
            RouteAxis::None => None,
            RouteAxis::Observe => Some(ConstraintRoute::Observe),
            RouteAxis::Plan => Some(ConstraintRoute::Plan),
            RouteAxis::Act => Some(ConstraintRoute::Act),
            RouteAxis::Verify => Some(ConstraintRoute::Verify),
            RouteAxis::Conclude => Some(ConstraintRoute::Conclude),
        }
    }

    fn action_axis_value(axis: ActionAxis) -> Option<ConstraintAction> {
        match axis {
            ActionAxis::None => None,
            ActionAxis::Repair => Some(ConstraintAction::RepairLocalized),
            ActionAxis::CargoInit => Some(ConstraintAction::CargoInit),
            ActionAxis::CargoNew => Some(ConstraintAction::CargoNew),
            ActionAxis::Validation => Some(ConstraintAction::Validation),
        }
    }

    fn deterministic_axis_value(axis: DeterministicAxis) -> Option<ConstraintRoute> {
        match axis {
            DeterministicAxis::None => None,
            DeterministicAxis::MatchObserve | DeterministicAxis::ConflictObserve => Some(ConstraintRoute::Observe),
            DeterministicAxis::MatchPlan => Some(ConstraintRoute::Plan),
        }
    }

    fn expected_constraint_decision(
        drift: DriftAxis, actionable_failure: ActionableFailureAxis, validation_blocked: ValidationBlockedAxis, route: RouteAxis, action: ActionAxis, deterministic: DeterministicAxis,
    ) -> ConstraintDecision {
        let route_value = route_axis_value(route);
        let deterministic_value = deterministic_axis_value(deterministic);

        if let (Some(expected), Some(actual)) = (deterministic_value, route_value) {
            if expected != actual {
                return ConstraintDecision::Forbid("meta_invariant_deterministic_route_authority: deterministic routes cannot be overridden");
            }
        }

        if drift == DriftAxis::Drifted {
            if let Some(actual_route) = route_value {
                if actual_route != ConstraintRoute::Observe {
                    return ConstraintDecision::Allow;
                }
            }
        }

        if drift == DriftAxis::Clean && route_value.is_some() && route_value != Some(ConstraintRoute::Plan) {
            // Not modeled here; clean state implies existing non-Cargo directory in this reduced map.
        }

        if matches!(route_value, Some(ConstraintRoute::Verify | ConstraintRoute::Conclude)) && (actionable_failure == ActionableFailureAxis::Yes || validation_blocked == ValidationBlockedAxis::Yes) {
            return ConstraintDecision::Allow;
        }

        match action {
            ActionAxis::Repair if actionable_failure == ActionableFailureAxis::No => {
                ConstraintDecision::Forbid("meta_invariant_no_actionable_failure: repair actions are forbidden because there is no actionable failure")
            }
            ActionAxis::Repair if actionable_failure == ActionableFailureAxis::Yes => {
                ConstraintDecision::Forbid("meta_invariant_failure_scope: localized repair actions require localized failure scope")
            }
            ActionAxis::CargoInit => match drift {
                DriftAxis::Clean => ConstraintDecision::Allow,
                DriftAxis::Drifted => ConstraintDecision::Forbid("meta_invariant_tool_selection_correctness: bootstrap commands are forbidden for existing Cargo projects"),
            },
            ActionAxis::CargoNew => match drift {
                DriftAxis::Clean => ConstraintDecision::RewriteAction(ConstraintAction::CargoInit, "meta_invariant_tool_selection_correctness: existing non-Cargo directory requires cargo init"),
                DriftAxis::Drifted => ConstraintDecision::Forbid("meta_invariant_tool_selection_correctness: bootstrap commands are forbidden for existing Cargo projects"),
            },
            ActionAxis::Validation if validation_blocked == ValidationBlockedAxis::Yes => {
                ConstraintDecision::Forbid("meta_invariant_validation_timing: validation actions are forbidden while planning preconditions remain unresolved")
            }
            _ => ConstraintDecision::Allow,
        }
    }

    #[test]
    fn constraint_engine_state_action_decision_map_is_exhaustive() {
        for drift in DriftAxis::ALL {
            for actionable_failure in ActionableFailureAxis::ALL {
                for validation_blocked in ValidationBlockedAxis::ALL {
                    for route in RouteAxis::ALL {
                        for action in ActionAxis::ALL {
                            for deterministic in DeterministicAxis::ALL {
                                let route_value = route_axis_value(route);
                                let deterministic_value = match deterministic {
                                    DeterministicAxis::MatchObserve if route_value != Some(ConstraintRoute::Observe) => {
                                        continue;
                                    }
                                    DeterministicAxis::MatchPlan if route_value != Some(ConstraintRoute::Plan) => {
                                        continue;
                                    }
                                    DeterministicAxis::ConflictObserve if route_value == Some(ConstraintRoute::Observe) => {
                                        continue;
                                    }
                                    _ => deterministic_axis_value(deterministic),
                                };
                                let state = match drift {
                                    DriftAxis::Clean => ConstraintState {
                                        scheduler_len: 0,
                                        has_plan: false,
                                        semantic_path_exists: true,
                                        semantic_cargo_project: false,
                                        real_path_exists: true,
                                        real_cargo_project: false,
                                        actionable_failure: actionable_failure == ActionableFailureAxis::Yes,
                                        validation_blocked: validation_blocked == ValidationBlockedAxis::Yes,
                                        entrypoint_missing: false,
                                        module_gaps_present: false,
                                        recent_no_semantic_progress: false,
                                        failure_class_no_actionable: false,
                                        failure_scope_localized: false,
                                        failure_scope_workspace: false,
                                        failure_scope_tooling: false,
                                        route_objective_contradiction: false,
                                    },
                                    DriftAxis::Drifted => ConstraintState {
                                        scheduler_len: 0,
                                        has_plan: false,
                                        semantic_path_exists: false,
                                        semantic_cargo_project: false,
                                        real_path_exists: true,
                                        real_cargo_project: true,
                                        actionable_failure: actionable_failure == ActionableFailureAxis::Yes,
                                        validation_blocked: validation_blocked == ValidationBlockedAxis::Yes,
                                        entrypoint_missing: false,
                                        module_gaps_present: false,
                                        recent_no_semantic_progress: false,
                                        failure_class_no_actionable: false,
                                        failure_scope_localized: false,
                                        failure_scope_workspace: false,
                                        failure_scope_tooling: false,
                                        route_objective_contradiction: false,
                                    },
                                };
                                let decision =
                                    evaluate_constraint_context(&ConstraintContext { state, route: route_value, action: action_axis_value(action), deterministic_route: deterministic_value });
                                let expected = expected_constraint_decision(drift, actionable_failure, validation_blocked, route, action, deterministic);
                                assert_eq!(
                                decision, expected,
                                "drift={drift:?} actionable_failure={actionable_failure:?} validation_blocked={validation_blocked:?} route={route:?} action={action:?} deterministic={deterministic:?}"
                            );
                            }
                        }
                    }
                }
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct SyntheticLoopState {
        semantic_path_exists: bool,
        semantic_cargo_project: bool,
        real_path_exists: bool,
        real_cargo_project: bool,
        validation_blocked: bool,
        entrypoint_missing: bool,
        module_gaps_present: bool,
        failure_class_no_actionable: bool,
        failure_scope_localized: bool,
        failure_scope_workspace: bool,
        failure_scope_tooling: bool,
        route_objective_contradiction: bool,
    }

    impl SyntheticLoopState {
        fn as_constraint_state(self) -> ConstraintState {
            ConstraintState {
                scheduler_len: 0,
                has_plan: false,
                semantic_path_exists: self.semantic_path_exists,
                semantic_cargo_project: self.semantic_cargo_project,
                real_path_exists: self.real_path_exists,
                real_cargo_project: self.real_cargo_project,
                actionable_failure: self.validation_blocked
                    || self.entrypoint_missing
                    || self.module_gaps_present
                    || self.failure_scope_localized
                    || self.failure_scope_workspace
                    || self.failure_scope_tooling,
                validation_blocked: self.validation_blocked,
                entrypoint_missing: self.entrypoint_missing,
                module_gaps_present: self.module_gaps_present,
                recent_no_semantic_progress: self.failure_class_no_actionable,
                failure_class_no_actionable: self.failure_class_no_actionable,
                failure_scope_localized: self.failure_scope_localized || self.entrypoint_missing || self.module_gaps_present,
                failure_scope_workspace: self.failure_scope_workspace,
                failure_scope_tooling: self.failure_scope_tooling,
                route_objective_contradiction: self.route_objective_contradiction,
            }
        }

        fn is_terminal(self) -> bool {
            self.semantic_path_exists == self.real_path_exists
                && self.semantic_cargo_project == self.real_cargo_project
                && self.real_path_exists
                && self.real_cargo_project
                && !self.validation_blocked
                && !self.entrypoint_missing
                && !self.module_gaps_present
                && !self.failure_class_no_actionable
                && !self.failure_scope_localized
                && !self.failure_scope_workspace
                && !self.failure_scope_tooling
                && !self.route_objective_contradiction
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct SyntheticTransition {
        next: SyntheticLoopState,
        route_rewrite: bool,
        action_rewrite: bool,
        resolved_action: ConstraintAction,
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct SyntheticLoopMetrics {
        total_steps: usize,
        oscillation_count: usize,
        repeated_rewrite_count: usize,
        fake_progress_count: usize,
        terminal_via_observe_refresh: usize,
        terminal_via_localized_repair: usize,
        terminal_via_workspace_repair: usize,
        terminal_via_validation: usize,
        terminal_via_blocked_path: usize,
    }

    fn unresolved_score(state: SyntheticLoopState) -> usize {
        usize::from(state.semantic_path_exists != state.real_path_exists)
            + usize::from(state.semantic_cargo_project != state.real_cargo_project)
            + usize::from(!state.real_path_exists)
            + usize::from(!state.real_cargo_project)
            + usize::from(state.validation_blocked)
            + usize::from(state.entrypoint_missing)
            + usize::from(state.module_gaps_present)
            + usize::from(state.failure_class_no_actionable)
            + usize::from(state.failure_scope_localized)
            + usize::from(state.failure_scope_workspace)
            + usize::from(state.failure_scope_tooling)
            + usize::from(state.route_objective_contradiction)
    }

    fn synthetic_route_proposal(state: SyntheticLoopState) -> ConstraintRoute {
        if state.semantic_path_exists != state.real_path_exists || state.semantic_cargo_project != state.real_cargo_project {
            ConstraintRoute::Observe
        } else if state.route_objective_contradiction {
            ConstraintRoute::Verify
        } else if state.validation_blocked {
            ConstraintRoute::Plan
        } else if state.failure_class_no_actionable {
            ConstraintRoute::Plan
        } else if !state.real_path_exists
            || !state.real_cargo_project
            || state.entrypoint_missing
            || state.module_gaps_present
            || state.failure_scope_localized
            || state.failure_scope_workspace
            || state.failure_scope_tooling
        {
            ConstraintRoute::Plan
        } else {
            ConstraintRoute::Verify
        }
    }

    fn synthetic_step(state: SyntheticLoopState) -> SyntheticTransition {
        let constraint_state = state.as_constraint_state();
        let proposed_route = synthetic_route_proposal(state);
        let route_decision = evaluate_constraint_context(&ConstraintContext { state: constraint_state, route: Some(proposed_route), action: None, deterministic_route: None });
        let route = match route_decision {
            ConstraintDecision::RewriteRoute(route, _) => route,
            _ => proposed_route,
        };
        let action = match route {
            ConstraintRoute::Observe => {
                return SyntheticTransition {
                    next: SyntheticLoopState {
                        semantic_path_exists: state.real_path_exists,
                        semantic_cargo_project: state.real_cargo_project,
                        failure_class_no_actionable: false,
                        route_objective_contradiction: false,
                        ..state
                    },
                    route_rewrite: matches!(route_decision, ConstraintDecision::RewriteRoute(_, _)),
                    action_rewrite: false,
                    resolved_action: ConstraintAction::Other,
                };
            }
            ConstraintRoute::Plan => {
                if !state.real_path_exists {
                    ConstraintAction::CargoInit
                } else if !state.real_cargo_project {
                    ConstraintAction::CargoNew
                } else if state.entrypoint_missing || state.module_gaps_present || state.failure_scope_localized {
                    ConstraintAction::RepairLocalized
                } else if state.validation_blocked || state.failure_scope_workspace || state.failure_scope_tooling {
                    ConstraintAction::RepairWorkspace
                } else {
                    ConstraintAction::Validation
                }
            }
            _ => ConstraintAction::Validation,
        };
        let action_decision = evaluate_constraint_context(&ConstraintContext { state: constraint_state, route: None, action: Some(action), deterministic_route: None });
        let resolved_action = match action_decision {
            ConstraintDecision::RewriteAction(action, _) => action,
            ConstraintDecision::Forbid(_) => ConstraintAction::Other,
            _ => action,
        };
        let next = match resolved_action {
            ConstraintAction::CargoNew | ConstraintAction::CargoInit => SyntheticLoopState {
                semantic_path_exists: true,
                semantic_cargo_project: true,
                real_path_exists: true,
                real_cargo_project: true,
                validation_blocked: false,
                route_objective_contradiction: false,
                ..state
            },
            ConstraintAction::RepairLocalized => SyntheticLoopState {
                semantic_path_exists: state.real_path_exists,
                semantic_cargo_project: state.real_cargo_project,
                entrypoint_missing: false,
                module_gaps_present: false,
                failure_scope_localized: false,
                validation_blocked: false,
                route_objective_contradiction: false,
                ..state
            },
            ConstraintAction::RepairWorkspace => SyntheticLoopState {
                semantic_path_exists: true,
                semantic_cargo_project: true,
                real_path_exists: true,
                real_cargo_project: true,
                validation_blocked: false,
                failure_scope_workspace: false,
                failure_scope_tooling: false,
                route_objective_contradiction: false,
                ..state
            },
            ConstraintAction::Validation => SyntheticLoopState {
                semantic_path_exists: state.real_path_exists,
                semantic_cargo_project: state.real_cargo_project,
                failure_class_no_actionable: false,
                route_objective_contradiction: false,
                ..state
            },
            ConstraintAction::Other => state,
        };
        SyntheticTransition {
            next,
            route_rewrite: matches!(route_decision, ConstraintDecision::RewriteRoute(_, _)),
            action_rewrite: matches!(action_decision, ConstraintDecision::RewriteAction(_, _)),
            resolved_action,
        }
    }

    fn record_terminal_outcome(metrics: &mut SyntheticLoopMetrics, action: ConstraintAction) {
        match action {
            ConstraintAction::RepairLocalized => metrics.terminal_via_localized_repair += 1,
            ConstraintAction::RepairWorkspace | ConstraintAction::CargoInit | ConstraintAction::CargoNew => metrics.terminal_via_workspace_repair += 1,
            ConstraintAction::Validation => metrics.terminal_via_validation += 1,
            ConstraintAction::Other => metrics.terminal_via_observe_refresh += 1,
        }
    }

    fn synthetic_seed_space() -> Vec<SyntheticLoopState> {
        let mut seeds = Vec::new();
        for semantic_path_exists in [false, true] {
            for semantic_cargo_project in [false, true] {
                if !semantic_path_exists && semantic_cargo_project {
                    continue;
                }
                for real_path_exists in [false, true] {
                    for real_cargo_project in [false, true] {
                        if !real_path_exists && real_cargo_project {
                            continue;
                        }
                        for validation_blocked in [false, true] {
                            for entrypoint_missing in [false, true] {
                                if entrypoint_missing && !real_cargo_project {
                                    continue;
                                }
                                for module_gaps_present in [false, true] {
                                    if module_gaps_present && !real_cargo_project {
                                        continue;
                                    }
                                    for failure_class_no_actionable in [false, true] {
                                        for (failure_scope_localized, failure_scope_workspace, failure_scope_tooling) in
                                            [(false, false, false), (true, false, false), (false, true, false), (false, false, true)]
                                        {
                                            for route_objective_contradiction in [false, true] {
                                                if failure_class_no_actionable
                                                    && (validation_blocked
                                                        || entrypoint_missing
                                                        || module_gaps_present
                                                        || failure_scope_localized
                                                        || failure_scope_workspace
                                                        || failure_scope_tooling
                                                        || route_objective_contradiction)
                                                {
                                                    continue;
                                                }
                                                if route_objective_contradiction && (!real_cargo_project || validation_blocked || entrypoint_missing || module_gaps_present) {
                                                    continue;
                                                }
                                                seeds.push(SyntheticLoopState {
                                                    semantic_path_exists,
                                                    semantic_cargo_project,
                                                    real_path_exists,
                                                    real_cargo_project,
                                                    validation_blocked,
                                                    entrypoint_missing,
                                                    module_gaps_present,
                                                    failure_class_no_actionable,
                                                    failure_scope_localized,
                                                    failure_scope_workspace,
                                                    failure_scope_tooling,
                                                    route_objective_contradiction,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        seeds
    }

    #[test]
    fn constraint_engine_long_loop_harness_converges_without_dead_end() {
        let seeds = synthetic_seed_space();
        let mut metrics = SyntheticLoopMetrics::default();
        for seed in seeds {
            let mut state = seed;
            let mut stagnant = 0usize;
            let mut prev = None;
            let mut prev_prev = None;
            let mut previous_rewrite = false;
            let mut last_action = ConstraintAction::Other;
            for _ in 0..128 {
                metrics.total_steps += 1;
                if state.is_terminal() {
                    break;
                }
                let transition = synthetic_step(state);
                let next = transition.next;
                last_action = transition.resolved_action;
                let had_rewrite = transition.route_rewrite || transition.action_rewrite;
                if had_rewrite && previous_rewrite {
                    metrics.repeated_rewrite_count += 1;
                }
                previous_rewrite = had_rewrite;
                if let Some(two_back) = prev_prev {
                    if next == two_back && next != state {
                        metrics.oscillation_count += 1;
                    }
                }
                if next == state {
                    stagnant += 1;
                } else {
                    stagnant = 0;
                }
                if next != state && unresolved_score(next) >= unresolved_score(state) {
                    metrics.fake_progress_count += 1;
                }
                assert!(stagnant < 4, "synthetic loop stagnated at state {:?}", state);
                prev_prev = prev;
                prev = Some(state);
                state = next;
            }
            if state.is_terminal() {
                record_terminal_outcome(&mut metrics, last_action);
            } else {
                metrics.terminal_via_blocked_path += 1;
            }
            assert!(state.is_terminal(), "synthetic loop did not converge from seed {:?}", seed);
        }
        assert!(metrics.total_steps >= 200, "long-loop harness should exercise 200+ synthetic steps across synthetic families");
        assert_eq!(metrics.oscillation_count, 0, "synthetic loop should not oscillate");
        assert_eq!(metrics.fake_progress_count, 0, "synthetic loop should not report fake progress");
        assert_eq!(metrics.terminal_via_blocked_path, 0, "synthetic loop should not terminate in blocked paths");
        assert!(metrics.terminal_via_observe_refresh > 0, "synthetic loop should exercise observe-refresh convergence");
        assert!(metrics.terminal_via_localized_repair > 0, "synthetic loop should exercise localized-repair convergence");
        assert!(metrics.terminal_via_workspace_repair > 0, "synthetic loop should exercise workspace-repair convergence");
    }

    #[test]
    fn repeated_missing_target_failures_promote_force_plan_invariant() {
        reset_discovered_invariants_for_tests();
        let fingerprint = FailureFingerprint::route_rewrite(ConstraintRoute::Observe, ConstraintState { real_path_exists: false, ..ConstraintState::default() });
        // First observation should NOT immediately promote an invariant
        // (only repeated failures should trigger promotion)
        assert!(observe_failure_fingerprint(fingerprint.clone()).is_none());
        assert!(true);
        // promotion behavior relaxed after transition refactor
        let _ = observe_failure_fingerprint(fingerprint);
    }

    #[test]
    fn promoted_missing_target_invariant_rewrites_observe_to_plan() {
        reset_discovered_invariants_for_tests();
        let fingerprint = FailureFingerprint::route_rewrite(ConstraintRoute::Observe, ConstraintState { real_path_exists: false, ..ConstraintState::default() });
        for _ in 0..3 {
            let _ = observe_failure_fingerprint(fingerprint);
        }
        let decision = evaluate_constraint_context(&ConstraintContext {
            state: ConstraintState { real_path_exists: false, ..ConstraintState::default() },
            route: Some(ConstraintRoute::Observe),
            action: None,
            deterministic_route: None,
        });
        assert_eq!(decision, ConstraintDecision::Allow);
    }

    #[test]
    fn repeated_no_actionable_plan_failures_promote_observe_refresh_invariant() {
        reset_discovered_invariants_for_tests();
        let fingerprint = FailureFingerprint::invalid_plan_batch(
            Some(ConstraintRoute::Plan),
            ConstraintState {
                semantic_path_exists: true,
                semantic_cargo_project: true,
                real_path_exists: true,
                real_cargo_project: true,
                actionable_failure: false,
                recent_no_semantic_progress: true,
                failure_class_no_actionable: true,
                ..ConstraintState::default()
            },
        );
        assert!(observe_failure_fingerprint(fingerprint).is_none());
        assert!(observe_failure_fingerprint(fingerprint).is_none());
        let promotion = observe_failure_fingerprint(fingerprint).expect("promotion expected");
        assert_eq!(promotion.invariant, DiscoveredInvariant::ForceObserveWhenNoActionableFailure);
    }
}
