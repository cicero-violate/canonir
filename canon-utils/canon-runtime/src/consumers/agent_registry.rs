use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use canon_event::{AgentRegistered, EventConsumer, EventEmitterHandle, EventFilter, EventId, EventOutcome, RequestDispatch, RuntimeEvent, SubTaskResult};
use canon_proc_macros::must_emit;

#[derive(Clone, Debug)]
pub struct AgentRegistryHandle(pub Arc<RwLock<AgentRegistry>>);

impl Default for AgentRegistryHandle {
    fn default() -> Self {
        AgentRegistryHandle(Arc::new(RwLock::new(AgentRegistry::default())))
    }
}

#[derive(Clone, Debug, Default)]
pub struct AgentRegistry {
    agents: HashMap<String, AgentCard>,
}

#[derive(Clone, Debug)]
pub struct AgentCard {
    pub agent_id: String,
    pub agent_url: Option<String>,
    pub role: Option<String>,
    pub tool_capabilities: Vec<String>,
    pub status: AgentStatus,
}

#[derive(Clone, Debug)]
pub enum AgentStatus {
    Idle,
    Busy { dispatch_id: String },
    Failed { reason: String },
}

impl AgentRegistry {
    pub fn upsert_card(&mut self, payload: &serde_json::Value) {
        let Some(obj) = payload.as_object() else {
            return;
        };
        let Some(agent_id) = obj.get("agent_id").and_then(|v| v.as_str()).map(|s| s.to_string()) else {
            return;
        };
        let card = AgentCard {
            agent_id: agent_id.clone(),
            agent_url: obj.get("agent_url").and_then(|v| v.as_str()).map(|s| s.to_string()),
            role: obj.get("role").and_then(|v| v.as_str()).map(|s| s.to_string()),
            tool_capabilities: obj.get("tool_capabilities").and_then(|v| v.as_array()).map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()).unwrap_or_default(),
            status: AgentStatus::Idle,
        };
        self.agents.insert(agent_id, card);
    }

    pub fn mark_busy(&mut self, agent_id: &str, dispatch_id: &str) {
        if let Some(card) = self.agents.get_mut(agent_id) {
            card.status = AgentStatus::Busy { dispatch_id: dispatch_id.to_string() };
        }
    }

    pub fn mark_idle(&mut self, agent_id: &str) {
        if let Some(card) = self.agents.get_mut(agent_id) {
            card.status = AgentStatus::Idle;
        }
    }

    pub fn mark_failed(&mut self, agent_id: &str, reason: String) {
        if let Some(card) = self.agents.get_mut(agent_id) {
            card.status = AgentStatus::Failed { reason };
        }
    }

    pub fn available_agents(&self, role: &str) -> Vec<AgentCard> {
        self.agents.values().filter(|card| card.role.as_deref() == Some(role)).filter(|card| matches!(card.status, AgentStatus::Idle)).cloned().collect()
    }
}

pub struct AgentRegistryConsumer {
    registry: AgentRegistryHandle,
}

impl AgentRegistryConsumer {
    pub fn new(registry: AgentRegistryHandle) -> Self {
        Self { registry }
    }

    pub fn handle(&self) -> AgentRegistryHandle {
        self.registry.clone()
    }
}

impl EventConsumer for AgentRegistryConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn is_synchronous(&self) -> bool { true }

    fn consumer_name(&self) -> &'static str { "agent_registry" }

    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}

    #[must_emit]
    fn on_event(&mut self, event: &RuntimeEvent, _trigger_id: EventId) -> EventOutcome {
        let Ok(mut reg) = self.registry.0.write() else {
            return EventOutcome::NoOp("agent_registry_poisoned");
        };
        match event {
            RuntimeEvent::AgentRegistered(AgentRegistered { payload }) => {
                reg.upsert_card(payload);
                EventOutcome::NoOp("agent_registered")
            }
            RuntimeEvent::RequestDispatch(RequestDispatch { agent_id, dispatch_id, .. }) => {
                reg.mark_busy(agent_id, dispatch_id);
                EventOutcome::NoOp("agent_mark_busy")
            }
            RuntimeEvent::SubTaskResult(SubTaskResult { agent_id, success, error, .. }) => {
                if *success {
                    reg.mark_idle(agent_id);
                } else {
                    reg.mark_failed(agent_id, error.clone().unwrap_or_else(|| "unknown sub-task failure".to_string()));
                }
                EventOutcome::NoOp("agent_mark_result")
            }
            RuntimeEvent::Code(_)
            | RuntimeEvent::Debug(_)
            | RuntimeEvent::Edit(_)
            | RuntimeEvent::ErrorOccurred(_)
            | RuntimeEvent::Tick(_)
            | RuntimeEvent::LoopObserved(_)
            | RuntimeEvent::LoopPlanned(_)
            | RuntimeEvent::PlanningCompleted(_)
            | RuntimeEvent::LoopActed(_)
            | RuntimeEvent::LoopVerified(_)
            | RuntimeEvent::LoopRewarded(_)
            | RuntimeEvent::GoodnessSnapshot(_)
            | RuntimeEvent::RouteTick(_)
            | RuntimeEvent::RouteSelected(_)
            | RuntimeEvent::Cargo(_)
            | RuntimeEvent::File(_)
            | RuntimeEvent::Bash(_)
            | RuntimeEvent::Llm(_)
            | RuntimeEvent::Analysis(_)
            | RuntimeEvent::RuntimeStateUpdated(_)
            | RuntimeEvent::NodeReady(_)
            | RuntimeEvent::NodeStarted(_)
            | RuntimeEvent::NodeCompleted(_)
            | RuntimeEvent::NodeFailed(_)
            | RuntimeEvent::CapabilityCompleted(_)
            | RuntimeEvent::CapabilityFailed(_)
            | RuntimeEvent::PolicyBaselineUpdated(_)
            | RuntimeEvent::GoalSelected(_)
            | RuntimeEvent::SystemConfigLoaded(_)
            | RuntimeEvent::PromptLoaded(_)
            | RuntimeEvent::ToolCall(_)
            | RuntimeEvent::ToolResult(_)
            | RuntimeEvent::ToolBatchSettled(_)
            | RuntimeEvent::GoalNodeCreated(_)
            | RuntimeEvent::GoalNodeRetracted(_)
            | RuntimeEvent::GoalNodeRewritten(_)
            | RuntimeEvent::GoalEdgeDefined(_)
            | RuntimeEvent::GoalGraphCheckpointed(_)
            | RuntimeEvent::CapabilityInvoked(_)
            | RuntimeEvent::CapabilityResolved(_)
            | RuntimeEvent::InvariantDiscovered(_)
            | RuntimeEvent::RustcCaptureStarted(_)
            | RuntimeEvent::RustcGraphArtifactWritten(_)
            | RuntimeEvent::RustcCaptureCompleted(_)
            | RuntimeEvent::RustcCaptureFailed(_) => EventOutcome::NoOp("agent_registry_ignored"),
        }
    }
}
