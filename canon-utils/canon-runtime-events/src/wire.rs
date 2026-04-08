use serde::{Deserialize, Serialize};

/// Canonical event identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(String);

impl EventId {
    pub fn new(v: impl Into<String>) -> Self {
        Self(v.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Canonical event kind — one variant per RuntimeEvent variant.
/// Wire strings are snake_case. Old aliases (rustc_event, edit_event, llm_call)
/// are accepted by FromStr for backwards-compatible log reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    // Loop lifecycle
    LoopObserved,
    LoopPlanned,
    PlanningCompleted,
    LoopActed,
    LoopVerified,
    VerifierPolicyUpdated,
    LoopRewarded,
    GoodnessSnapshot,
    InvariantDiscovered,
    // Routing
    RouteTick,
    RouteSelected,
    // Capability execution
    CapabilityInvoked,
    CapabilityResolved,
    CapabilityCompleted,
    CapabilityFailed,
    CapabilityRequested,
    // Goal graph
    GoalNodeCreated,
    GoalNodeRetracted,
    GoalNodeRewritten,
    GoalEdgeDefined,
    GoalGraphCheckpointed,
    GoalSelected,
    // Tool / LLM
    ToolCall,
    ToolResult,
    ToolBatchSettled,
    // Agent & dispatch
    AgentRegistered,
    SubTaskResult,
    // Capability sub-events (matched to RuntimeEvent variants)
    Cargo,
    File,
    Bash,
    Analysis,
    Llm,
    // Runtime state
    RuntimeStarted,
    RuntimeStateUpdated,
    PolicyBaselineUpdated,
    SystemConfigLoaded,
    PromptLoaded,
    RustcCaptureStarted,
    RustcGraphArtifactWritten,
    RustcCaptureCompleted,
    RustcCaptureFailed,
    // Node lifecycle
    NodeReady,
    NodeStarted,
    NodeCompleted,
    NodeFailed,
    // Ticks
    Tick,
    // Code / edit
    Code,
    Edit,
    // Misc
    Debug,
    ErrorOccurred,
    SupervisorEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventClass {
    Control,
    Effect,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::LoopObserved => "loop_observed",
            EventKind::LoopPlanned => "loop_planned",
            EventKind::PlanningCompleted => "planning_completed",
            EventKind::LoopActed => "loop_acted",
            EventKind::LoopVerified => "loop_verified",
            EventKind::VerifierPolicyUpdated => "verifier_policy_updated",
            EventKind::LoopRewarded => "loop_rewarded",
            EventKind::GoodnessSnapshot => "goodness_snapshot",
            EventKind::InvariantDiscovered => "invariant_discovered",
            EventKind::RouteTick => "route_tick",
            EventKind::RouteSelected => "route_selected",
            EventKind::CapabilityInvoked => "capability_invoked",
            EventKind::CapabilityResolved => "capability_resolved",
            EventKind::CapabilityCompleted => "capability_completed",
            EventKind::CapabilityFailed => "capability_failed",
            EventKind::CapabilityRequested => "capability_requested",
            EventKind::GoalNodeCreated => "goal_node_created",
            EventKind::GoalNodeRetracted => "goal_node_retracted",
            EventKind::GoalNodeRewritten => "goal_node_rewritten",
            EventKind::GoalEdgeDefined => "goal_edge_defined",
            EventKind::GoalGraphCheckpointed => "goal_graph_checkpointed",
            EventKind::GoalSelected => "goal_selected",
            EventKind::ToolCall => "tool_call",
            EventKind::ToolResult => "tool_result",
            EventKind::ToolBatchSettled => "tool_batch_settled",
            EventKind::AgentRegistered => "agent_registered",
            EventKind::SubTaskResult => "sub_task_result",
            EventKind::Cargo => "cargo",
            EventKind::File => "file",
            EventKind::Bash => "bash",
            EventKind::Analysis => "analysis",
            EventKind::Llm => "llm",
            EventKind::RuntimeStarted => "runtime_started",
            EventKind::RuntimeStateUpdated => "runtime_state_updated",
            EventKind::PolicyBaselineUpdated => "policy_baseline_updated",
            EventKind::SystemConfigLoaded => "system_config_loaded",
            EventKind::PromptLoaded => "prompt_loaded",
            EventKind::RustcCaptureStarted => "rustc_capture_started",
            EventKind::RustcGraphArtifactWritten => "rustc_graph_artifact_written",
            EventKind::RustcCaptureCompleted => "rustc_capture_completed",
            EventKind::RustcCaptureFailed => "rustc_capture_failed",
            EventKind::NodeReady => "node_ready",
            EventKind::NodeStarted => "node_started",
            EventKind::NodeCompleted => "node_completed",
            EventKind::NodeFailed => "node_failed",
            EventKind::Tick => "tick",
            EventKind::Code => "code",
            EventKind::Edit => "edit",
            EventKind::Debug => "debug",
            EventKind::ErrorOccurred => "error_occurred",
            EventKind::SupervisorEvent => "supervisor_event",
        }
    }

    pub fn class(self) -> EventClass {
        match self {
            EventKind::Tick
            | EventKind::RouteTick
            | EventKind::RouteSelected
            | EventKind::LoopObserved
            | EventKind::PlanningCompleted
            | EventKind::LoopActed
            | EventKind::LoopVerified
            | EventKind::VerifierPolicyUpdated
            | EventKind::LoopRewarded => EventClass::Control,
            _ => EventClass::Effect,
        }
    }

    pub fn allowed_next(self) -> &'static [EventKind] {
        use EventKind::*;
        match self {
            Tick => &[RouteTick],
            RouteTick => &[
                RouteSelected,
                LoopObserved, // allow observe cycle immediately after RouteTick
            ],
            RouteSelected => &[LoopObserved, PlanningCompleted, LoopActed, LoopVerified, VerifierPolicyUpdated, LoopRewarded],
            LoopObserved => &[RouteSelected],
            PlanningCompleted => &[RouteSelected],
            LoopActed => &[RouteSelected, LoopVerified],
            LoopVerified => &[VerifierPolicyUpdated],
            VerifierPolicyUpdated => &[LoopRewarded],
            LoopRewarded => &[RouteSelected, LoopObserved],
            _ => &[],
        }
    }
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for EventKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "loop_observed" => Ok(EventKind::LoopObserved),
            "loop_planned" => Ok(EventKind::LoopPlanned),
            "planning_completed" => Ok(EventKind::PlanningCompleted),
            "loop_acted" => Ok(EventKind::LoopActed),
            "loop_verified" => Ok(EventKind::LoopVerified),
            "verifier_policy_updated" => Ok(EventKind::VerifierPolicyUpdated),
            "loop_rewarded" => Ok(EventKind::LoopRewarded),
            "goodness_snapshot" => Ok(EventKind::GoodnessSnapshot),
            "route_tick" => Ok(EventKind::RouteTick),
            "route_selected" => Ok(EventKind::RouteSelected),
            "capability_invoked" => Ok(EventKind::CapabilityInvoked),
            "capability_resolved" => Ok(EventKind::CapabilityResolved),
            "capability_completed" => Ok(EventKind::CapabilityCompleted),
            "capability_failed" => Ok(EventKind::CapabilityFailed),
            "capability_requested" => Ok(EventKind::CapabilityRequested),
            "goal_node_created" => Ok(EventKind::GoalNodeCreated),
            "goal_node_retracted" => Ok(EventKind::GoalNodeRetracted),
            "goal_node_rewritten" => Ok(EventKind::GoalNodeRewritten),
            "goal_edge_defined" => Ok(EventKind::GoalEdgeDefined),
            "goal_graph_checkpointed" => Ok(EventKind::GoalGraphCheckpointed),
            "goal_selected" => Ok(EventKind::GoalSelected),
            "tool_call" => Ok(EventKind::ToolCall),
            "tool_result" => Ok(EventKind::ToolResult),
            "tool_batch_settled" => Ok(EventKind::ToolBatchSettled),
            "agent_registered" => Ok(EventKind::AgentRegistered),
            "sub_task_result" => Ok(EventKind::SubTaskResult),
            "cargo" => Ok(EventKind::Cargo),
            "file" => Ok(EventKind::File),
            "bash" => Ok(EventKind::Bash),
            "analysis" => Ok(EventKind::Analysis),
            "llm" | "llm_call" => Ok(EventKind::Llm),
            "runtime_started" => Ok(EventKind::RuntimeStarted),
            "runtime_state_updated" | "runtime_state.updated" => Ok(EventKind::RuntimeStateUpdated),
            "policy_baseline_updated" => Ok(EventKind::PolicyBaselineUpdated),
            "system_config_loaded" => Ok(EventKind::SystemConfigLoaded),
            "prompt_loaded" => Ok(EventKind::PromptLoaded),
            "rustc_capture_started" => Ok(EventKind::RustcCaptureStarted),
            "rustc_graph_artifact_written" => Ok(EventKind::RustcGraphArtifactWritten),
            "rustc_capture_completed" => Ok(EventKind::RustcCaptureCompleted),
            "rustc_capture_failed" => Ok(EventKind::RustcCaptureFailed),
            "node_ready" => Ok(EventKind::NodeReady),
            "node_started" => Ok(EventKind::NodeStarted),
            "node_completed" => Ok(EventKind::NodeCompleted),
            "node_failed" => Ok(EventKind::NodeFailed),
            "tick" => Ok(EventKind::Tick),
            "code" | "rustc_event" => Ok(EventKind::Code),
            "edit" | "edit_event" => Ok(EventKind::Edit),
            "debug" => Ok(EventKind::Debug),
            "error_occurred" => Ok(EventKind::ErrorOccurred),
            "supervisor_event" => Ok(EventKind::SupervisorEvent),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonPayloadMeta {
    pub file: String,
    pub line: u32,
}

/// Canonical payload with non-null slots.
/// input/output/delta must be JSON objects, never null.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonPayload {
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub delta: serde_json::Value,
    pub meta: CanonPayloadMeta,
    pub data: serde_json::Value,
}

impl CanonPayload {
    pub fn from_data(input: serde_json::Value, output: serde_json::Value, delta: serde_json::Value, meta: CanonPayloadMeta, data: serde_json::Value) -> Self {
        assert!(!input.is_null(), "CanonPayload.input must not be null");
        assert!(!output.is_null(), "CanonPayload.output must not be null");
        assert!(!delta.is_null(), "CanonPayload.delta must not be null");
        Self { input, output, delta, meta, data }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonEvent {
    pub id: EventId,
    pub parent_ids: Vec<EventId>,
    pub actor: String,
    pub kind: EventKind,
    pub ts: u64,
    pub payload: CanonPayload,
    /// Previous event of the same kind in this session — forms a per-kind causal chain.
    /// None for the first event of a given kind. Set by EventRuntime before writing to tlog.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub prev_event_id: Option<EventId>,
}

impl CanonEvent {
    #[track_caller]
    pub fn new(id: EventId, parent_ids: Vec<EventId>, actor: impl Into<String>, kind: EventKind, ts: u64, payload: CanonPayload, root: bool) -> Self {
        if !root {
            assert!(!parent_ids.is_empty(), "CanonEvent requires at least one parent_id unless root=true (kind={kind})");
        }
        Self { id, parent_ids, actor: actor.into(), kind, ts, payload, prev_event_id: None }
    }

    /// Construct a child event whose causal parent is `parent`.
    /// The parent chain is enforced at the type level — impossible to omit.
    pub fn from_parent(parent: &CanonEvent, id: EventId, actor: impl Into<String>, kind: EventKind, ts: u64, payload: CanonPayload) -> Self {
        Self { id, parent_ids: vec![parent.id.clone()], actor: actor.into(), kind, ts, payload, prev_event_id: None }
    }

    /// Construct a root event (legitimately parentless: Tick, PromptLoaded, etc.).
    pub fn new_root(id: EventId, actor: impl Into<String>, kind: EventKind, ts: u64, payload: CanonPayload) -> Self {
        Self { id, parent_ids: Vec::new(), actor: actor.into(), kind, ts, payload, prev_event_id: None }
    }
}

#[cfg(test)]
mod tests {
    use super::{CanonEvent, CanonPayload, CanonPayloadMeta, EventId, EventKind};
    use serde_json::json;

    fn payload() -> CanonPayload {
        CanonPayload { input: json!({"x":1}), output: json!({"y":1}), delta: json!({"z":1}), meta: CanonPayloadMeta { file: "test".to_string(), line: 1 }, data: json!({}) }
    }

    #[test]
    fn parent_causality_integrity_requires_parent_for_non_root_events() {
        let result = std::panic::catch_unwind(|| CanonEvent::new(EventId::new("child".to_string()), Vec::new(), "test", EventKind::LoopObserved, 1, payload(), false));
        assert!(result.is_err());
    }

    #[test]
    fn from_parent_constructs_reconstructable_chain() {
        let parent = CanonEvent::new_root(EventId::new("parent".to_string()), "test", EventKind::RouteSelected, 1, payload());
        let child = CanonEvent::from_parent(&parent, EventId::new("child".to_string()), "test", EventKind::LoopObserved, 2, payload());
        assert_eq!(child.parent_ids.len(), 1);
        assert_eq!(child.parent_ids[0].as_str(), parent.id.as_str());
    }
}

/// Describes how an event struct populates the canonical payload slots.
/// Generated automatically by `canon_event_struct!` via field attributes
/// `#[input]`, `#[output]`, `#[delta]`.
pub trait CanonPayloadShape {
    fn payload_input(&self) -> serde_json::Value;
    fn payload_output(&self) -> serde_json::Value;
    fn payload_delta(&self) -> serde_json::Value;
    fn payload_data(&self) -> serde_json::Value;
}
