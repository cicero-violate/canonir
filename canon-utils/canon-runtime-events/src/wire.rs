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

/// Canonical event kind (snake_case on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    LoopObserved,
    LoopPlanned,
    LoopActed,
    LoopVerified,
    LoopRewarded,
    RuntimeStarted,
    RouteTick,
    RouteSelected,
    CapabilityCompleted,
    CapabilityFailed,
    CapabilityInvoked,
    CapabilityResolved,
    CapabilityRequested,
    ErrorOccurred,
    Debug,
    PromptLoaded,
    RuntimeStateUpdated,
    ToolCall,
    ToolResult,
    AgentRegistered,
    RequestDispatch,
    SubTaskResult,
    RustcEvent,
    EditEvent,
    SupervisorEvent,
    GoalNodeCreated,
    GoalNodeRetracted,
    GoalNodeRewritten,
    GoalEdgeDefined,
    GoalGraphCheckpointed,
    GoodnessSnapshot,
    LlmCall,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventKind::LoopObserved => "loop_observed",
            EventKind::LoopPlanned => "loop_planned",
            EventKind::LoopActed => "loop_acted",
            EventKind::LoopVerified => "loop_verified",
            EventKind::LoopRewarded => "loop_rewarded",
            EventKind::RuntimeStarted => "runtime_started",
            EventKind::RouteTick => "route_tick",
            EventKind::RouteSelected => "route_selected",
            EventKind::CapabilityCompleted => "capability_completed",
            EventKind::CapabilityFailed => "capability_failed",
            EventKind::CapabilityInvoked => "capability_invoked",
            EventKind::CapabilityResolved => "capability_resolved",
            EventKind::CapabilityRequested => "capability_requested",
            EventKind::ErrorOccurred => "error_occurred",
            EventKind::Debug => "debug",
            EventKind::PromptLoaded => "prompt_loaded",
            EventKind::RuntimeStateUpdated => "runtime_state.updated",
            EventKind::ToolCall => "tool_call",
            EventKind::ToolResult => "tool_result",
            EventKind::AgentRegistered => "agent_registered",
            EventKind::RequestDispatch => "request_dispatch",
            EventKind::SubTaskResult => "sub_task_result",
            EventKind::RustcEvent => "rustc_event",
            EventKind::EditEvent => "edit_event",
            EventKind::SupervisorEvent => "supervisor_event",
            EventKind::GoalNodeCreated => "goal_node_created",
            EventKind::GoalNodeRetracted => "goal_node_retracted",
            EventKind::GoalNodeRewritten => "goal_node_rewritten",
            EventKind::GoalEdgeDefined => "goal_edge_defined",
            EventKind::GoalGraphCheckpointed => "goal_graph_checkpointed",
            EventKind::GoodnessSnapshot => "goodness_snapshot",
            EventKind::LlmCall => "llm_call",
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
            "loop_acted" => Ok(EventKind::LoopActed),
            "loop_verified" => Ok(EventKind::LoopVerified),
            "loop_rewarded" => Ok(EventKind::LoopRewarded),
            "runtime_started" => Ok(EventKind::RuntimeStarted),
            "route_tick" => Ok(EventKind::RouteTick),
            "route_selected" => Ok(EventKind::RouteSelected),
            "capability_completed" => Ok(EventKind::CapabilityCompleted),
            "capability_failed" => Ok(EventKind::CapabilityFailed),
            "capability_invoked" => Ok(EventKind::CapabilityInvoked),
            "capability_resolved" => Ok(EventKind::CapabilityResolved),
            "capability_requested" => Ok(EventKind::CapabilityRequested),
            "error_occurred" => Ok(EventKind::ErrorOccurred),
            "debug" => Ok(EventKind::Debug),
            "prompt_loaded" => Ok(EventKind::PromptLoaded),
            "runtime_state.updated" => Ok(EventKind::RuntimeStateUpdated),
            "tool_call" => Ok(EventKind::ToolCall),
            "tool_result" => Ok(EventKind::ToolResult),
            "agent_registered" => Ok(EventKind::AgentRegistered),
            "request_dispatch" => Ok(EventKind::RequestDispatch),
            "sub_task_result" => Ok(EventKind::SubTaskResult),
            "rustc_event" => Ok(EventKind::RustcEvent),
            "edit_event" => Ok(EventKind::EditEvent),
            "supervisor_event" => Ok(EventKind::SupervisorEvent),
            "goal_node_created" => Ok(EventKind::GoalNodeCreated),
            "goal_node_retracted" => Ok(EventKind::GoalNodeRetracted),
            "goal_node_rewritten" => Ok(EventKind::GoalNodeRewritten),
            "goal_edge_defined" => Ok(EventKind::GoalEdgeDefined),
            "goal_graph_checkpointed" => Ok(EventKind::GoalGraphCheckpointed),
            "goodness_snapshot" => Ok(EventKind::GoodnessSnapshot),
            "llm_call" => Ok(EventKind::LlmCall),
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
}

impl CanonEvent {
    pub fn new(id: EventId, parent_ids: Vec<EventId>, actor: impl Into<String>, kind: EventKind, ts: u64, payload: CanonPayload, root: bool) -> Self {
        if !root {
            assert!(!parent_ids.is_empty(), "CanonEvent requires at least one parent_id unless root=true");
        }
        Self { id, parent_ids, actor: actor.into(), kind, ts, payload }
    }
}
/// Trait describing how to populate CanonPayload slots from an event struct.
pub trait CanonPayloadShape {
    fn payload_input(&self) -> serde_json::Value;
    fn payload_output(&self) -> serde_json::Value;
    fn payload_delta(&self) -> serde_json::Value;
    fn payload_data(&self) -> serde_json::Value;
}

impl<T> CanonPayloadShape for T
where
    T: serde::Serialize,
{
    fn payload_input(&self) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }
    fn payload_output(&self) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }
    fn payload_delta(&self) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }
    fn payload_data(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}
