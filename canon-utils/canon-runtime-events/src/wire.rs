use crate::{LoopObserved, RouteSelected, RouteTick};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMeta {
    pub ts: u64,
    pub source: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum CanonPayload {
    LoopObserved(LoopObserved),
    LoopPlanned(serde_json::Value),
    LoopActed(serde_json::Value),
    LoopVerified(serde_json::Value),
    LoopRewarded(serde_json::Value),
    RuntimeStarted(serde_json::Value),
    RouteTick(RouteTick),
    RouteSelected(RouteSelected),
    CapabilityCompleted(serde_json::Value),
    CapabilityFailed(serde_json::Value),
    CapabilityInvoked(serde_json::Value),
    CapabilityResolved(serde_json::Value),
    CapabilityRequested(serde_json::Value),
    ErrorOccurred(serde_json::Value),
    Debug(serde_json::Value),
    PromptLoaded(serde_json::Value),
    RuntimeStateUpdated(serde_json::Value),
    ToolCall(serde_json::Value),
    ToolResult(serde_json::Value),
    AgentRegistered(serde_json::Value),
    RequestDispatch(serde_json::Value),
    SubTaskResult(serde_json::Value),
    RustcEvent(serde_json::Value),
    EditEvent(serde_json::Value),
    SupervisorEvent(serde_json::Value),
    GoalNodeCreated(serde_json::Value),
    GoalNodeRetracted(serde_json::Value),
    GoalNodeRewritten(serde_json::Value),
    GoalEdgeDefined(serde_json::Value),
    GoalGraphCheckpointed(serde_json::Value),
    GoodnessSnapshot(serde_json::Value),
    Llm(serde_json::Value),
    #[serde(other)]
    Unknown,
}

impl CanonPayload {
    pub fn kind_str(&self) -> &'static str {
        match self {
            CanonPayload::LoopObserved(_) => "loop_observed",
            CanonPayload::LoopPlanned(_) => "loop_planned",
            CanonPayload::LoopActed(_) => "loop_acted",
            CanonPayload::LoopVerified(_) => "loop_verified",
            CanonPayload::LoopRewarded(_) => "loop_rewarded",
            CanonPayload::RuntimeStarted(_) => "runtime_started",
            CanonPayload::RouteTick(_) => "route_tick",
            CanonPayload::RouteSelected(_) => "route_selected",
            CanonPayload::CapabilityCompleted(_) => "capability_completed",
            CanonPayload::CapabilityFailed(_) => "capability_failed",
            CanonPayload::CapabilityInvoked(_) => "capability_invoked",
            CanonPayload::CapabilityResolved(_) => "capability_resolved",
            CanonPayload::CapabilityRequested(_) => "capability_requested",
            CanonPayload::ErrorOccurred(_) => "error_occurred",
            CanonPayload::Debug(_) => "debug",
            CanonPayload::PromptLoaded(_) => "prompt_loaded",
            CanonPayload::RuntimeStateUpdated(_) => "runtime_state.updated",
            CanonPayload::ToolCall(_) => "tool_call",
            CanonPayload::ToolResult(_) => "tool_result",
            CanonPayload::AgentRegistered(_) => "agent_registered",
            CanonPayload::RequestDispatch(_) => "request_dispatch",
            CanonPayload::SubTaskResult(_) => "sub_task_result",
            CanonPayload::RustcEvent(_) => "rustc_event",
            CanonPayload::EditEvent(_) => "edit_event",
            CanonPayload::SupervisorEvent(_) => "supervisor_event",
            CanonPayload::GoalNodeCreated(_) => "goal_node_created",
            CanonPayload::GoalNodeRetracted(_) => "goal_node_retracted",
            CanonPayload::GoalNodeRewritten(_) => "goal_node_rewritten",
            CanonPayload::GoalEdgeDefined(_) => "goal_edge_defined",
            CanonPayload::GoalGraphCheckpointed(_) => "goal_graph_checkpointed",
            CanonPayload::GoodnessSnapshot(_) => "goodness_snapshot",
            CanonPayload::Llm(_) => "llm_call",
            CanonPayload::Unknown => "unknown",
        }
    }

    pub fn as_value(&self) -> Option<Value> {
        match self {
            CanonPayload::LoopObserved(v) => serde_json::to_value(v).ok(),
            CanonPayload::RuntimeStarted(v) => Some(v.clone()),
            CanonPayload::RouteTick(v) => serde_json::to_value(v).ok(),
            CanonPayload::RouteSelected(v) => serde_json::to_value(v).ok(),
            CanonPayload::LoopPlanned(v)
            | CanonPayload::LoopActed(v)
            | CanonPayload::LoopVerified(v)
            | CanonPayload::LoopRewarded(v)
            | CanonPayload::CapabilityCompleted(v)
            | CanonPayload::CapabilityFailed(v)
            | CanonPayload::CapabilityInvoked(v)
            | CanonPayload::CapabilityResolved(v)
            | CanonPayload::CapabilityRequested(v)
            | CanonPayload::ErrorOccurred(v)
            | CanonPayload::Debug(v)
            | CanonPayload::PromptLoaded(v)
            | CanonPayload::RuntimeStateUpdated(v)
            | CanonPayload::ToolCall(v)
            | CanonPayload::ToolResult(v)
            | CanonPayload::AgentRegistered(v)
            | CanonPayload::RequestDispatch(v)
            | CanonPayload::SubTaskResult(v)
            | CanonPayload::RustcEvent(v)
            | CanonPayload::EditEvent(v)
            | CanonPayload::SupervisorEvent(v)
            | CanonPayload::GoalNodeCreated(v)
            | CanonPayload::GoalNodeRetracted(v)
            | CanonPayload::GoalNodeRewritten(v)
            | CanonPayload::GoalEdgeDefined(v)
            | CanonPayload::GoalGraphCheckpointed(v)
            | CanonPayload::GoodnessSnapshot(v)
            | CanonPayload::Llm(v) => Some(v.clone()),
            CanonPayload::Unknown => None,
        }
    }

    pub fn from_kind(kind: &str, data: serde_json::Value) -> Self {
        match kind {
            "loop_observed" => CanonPayload::LoopObserved(serde_json::from_value(data).unwrap_or_default()),
            "loop_planned" => CanonPayload::LoopPlanned(data),
            "loop_acted" => CanonPayload::LoopActed(data),
            "loop_verified" => CanonPayload::LoopVerified(data),
            "loop_rewarded" => CanonPayload::LoopRewarded(data),
            "runtime_started" => CanonPayload::RuntimeStarted(data),
            "route_tick" => CanonPayload::RouteTick(serde_json::from_value(data).unwrap_or_default()),
            "route_selected" => CanonPayload::RouteSelected(serde_json::from_value(data).unwrap_or_default()),
            "capability_completed" => CanonPayload::CapabilityCompleted(data),
            "capability_failed" => CanonPayload::CapabilityFailed(data),
            "capability_invoked" => CanonPayload::CapabilityInvoked(data),
            "capability_resolved" => CanonPayload::CapabilityResolved(data),
            "capability_requested" => CanonPayload::CapabilityRequested(data),
            "error_occurred" => CanonPayload::ErrorOccurred(data),
            "debug" => CanonPayload::Debug(data),
            "prompt_loaded" => CanonPayload::PromptLoaded(data),
            "runtime_state.updated" => CanonPayload::RuntimeStateUpdated(data),
            "tool_call" => CanonPayload::ToolCall(data),
            "tool_result" => CanonPayload::ToolResult(data),
            "agent_registered" => CanonPayload::AgentRegistered(data),
            "request_dispatch" => CanonPayload::RequestDispatch(data),
            "sub_task_result" => CanonPayload::SubTaskResult(data),
            "rustc_event" => CanonPayload::RustcEvent(data),
            "edit_event" => CanonPayload::EditEvent(data),
            "supervisor_event" => CanonPayload::SupervisorEvent(data),
            "goal_node_created" => CanonPayload::GoalNodeCreated(data),
            "goal_node_retracted" => CanonPayload::GoalNodeRetracted(data),
            "goal_node_rewritten" => CanonPayload::GoalNodeRewritten(data),
            "goal_edge_defined" => CanonPayload::GoalEdgeDefined(data),
            "goal_graph_checkpointed" => CanonPayload::GoalGraphCheckpointed(data),
            "goodness_snapshot" => CanonPayload::GoodnessSnapshot(data),
            "llm_call" => CanonPayload::Llm(data),
            _ => CanonPayload::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonEvent {
    pub event_id: Option<u64>,
    pub meta: EventMeta,
    #[serde(flatten)]
    pub payload: CanonPayload,
}
