use crate::{LoopObserved, RouteSelected, RouteTick};
use serde::{Deserialize, Serialize};

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
    RouteTick(RouteTick),
    RouteSelected(RouteSelected),
    CapabilityCompleted(serde_json::Value),
    CapabilityFailed(serde_json::Value),
    CapabilityInvoked(serde_json::Value),
    CapabilityResolved(serde_json::Value),
    ErrorOccurred(serde_json::Value),
    Debug(serde_json::Value),
    PromptLoaded(serde_json::Value),
    RuntimeStateUpdated(serde_json::Value),
    ToolCall(serde_json::Value),
    ToolResult(serde_json::Value),
    GoalNodeCreated(serde_json::Value),
    GoalNodeRetracted(serde_json::Value),
    GoalNodeRewritten(serde_json::Value),
    GoalEdgeDefined(serde_json::Value),
    GoalGraphCheckpointed(serde_json::Value),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonEvent {
    pub event_id: Option<u64>,
    pub meta: EventMeta,
    #[serde(flatten)]
    pub payload: CanonPayload,
}
