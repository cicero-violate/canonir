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

pub fn classify_planned_action_class(action_kind: &str, action_payload: &Value) -> PlannedActionClass {
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

pub fn is_localized_repair_action(action_kind: &str) -> bool {
    matches!(
        action_kind,
        "edit.add_import"
            | "edit.define_symbol_stub"
            | "edit.rename_symbol"
            | "apply_patch"
    )
}

pub fn semantic_summary_has_actionable_failure(
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

pub fn failure_scope_is_sufficient(
    compiler_repair_required: bool,
    compiler_hints_len: usize,
    failure_scope: Option<&str>,
) -> bool {
    if !compiler_repair_required || compiler_hints_len == 0 {
        return true;
    }
    matches!(failure_scope, Some("localized") | Some("workspace") | Some("tooling"))
}

pub fn high_invalid_plan_pressure_requires_single_action(
    invalid_plan_rate: f32,
    planning_attempts: u32,
) -> bool {
    invalid_plan_rate > 0.5 && planning_attempts >= 3
}

pub fn stalled_loop_forbids_action_class(
    no_progress_streak: u32,
    action_class: PlannedActionClass,
) -> bool {
    no_progress_streak >= 2
        && matches!(action_class, PlannedActionClass::PassiveDiscovery | PlannedActionClass::Verification)
}
