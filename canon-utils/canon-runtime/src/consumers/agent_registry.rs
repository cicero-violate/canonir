use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use canon_event::{AgentRegistered, EventConsumer, EventEmitterHandle, EventFilter, RequestDispatch, RuntimeEvent, SubTaskResult};

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
        let Some(obj) = payload.as_object() else { return; };
        let Some(agent_id) = obj.get("agent_id").and_then(|v| v.as_str()).map(|s| s.to_string()) else { return; };
        let card = AgentCard {
            agent_id: agent_id.clone(),
            agent_url: obj.get("agent_url").and_then(|v| v.as_str()).map(|s| s.to_string()),
            role: obj.get("role").and_then(|v| v.as_str()).map(|s| s.to_string()),
            tool_capabilities: obj
                .get("tool_capabilities")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default(),
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
        self.agents
            .values()
            .filter(|card| card.role.as_deref() == Some(role))
            .filter(|card| matches!(card.status, AgentStatus::Idle))
            .cloned()
            .collect()
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

    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}

    fn on_event(&mut self, event: &RuntimeEvent) {
        let Ok(mut reg) = self.registry.0.write() else { return; };
        match event {
            RuntimeEvent::AgentRegistered(AgentRegistered { payload }) => reg.upsert_card(payload),
            RuntimeEvent::RequestDispatch(RequestDispatch { agent_id, dispatch_id, .. }) => reg.mark_busy(agent_id, dispatch_id),
            RuntimeEvent::SubTaskResult(SubTaskResult { agent_id, success, error, .. }) => {
                if *success {
                    reg.mark_idle(agent_id);
                } else {
                    reg.mark_failed(agent_id, error.clone().unwrap_or_else(|| "unknown sub-task failure".to_string()));
                }
            }
            _ => {}
        }
    }
}
