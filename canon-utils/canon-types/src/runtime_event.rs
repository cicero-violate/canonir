use crate::{EditEvent, EventDelta, EventMask, KernelState};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    Kernel { delta: EventDelta, state: KernelState },
    Edit(EditEvent),
    Tick { tick: u64 },
    RuntimeStateUpdated { payload: serde_json::Value },
    NodeReady(NodeReady),
    NodeStarted(NodeStarted),
    NodeCompleted(NodeCompleted),
    NodeFailed(NodeFailed),
    CapabilityRequested(CapabilityRequested),
    CapabilityCompleted(CapabilityCompleted),
    CapabilityFailed(CapabilityFailed),
}

pub trait RuntimeEmitter: Send + Sync {
    fn emit(&self, event: RuntimeEvent);
}

pub type RuntimeEmitterHandle = Arc<dyn RuntimeEmitter>;

#[derive(Debug, Clone, Copy)]
pub enum RuntimeEventFilter {
    All,
    Kernel(EventMask),
    EditOnly,
    CapabilityOnly,
}

pub trait RuntimeConsumer: Send + Sync {
    fn filter(&self) -> RuntimeEventFilter;
    fn on_event(&mut self, event: &RuntimeEvent);
    fn set_emitter(&mut self, _emitter: RuntimeEmitterHandle) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRequested {
    pub request_id: String,
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityCompleted {
    pub request_id: String,
    pub name: String,
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityFailed {
    pub request_id: String,
    pub name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeReady {
    pub node_id: String,
    pub capability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStarted {
    pub node_id: String,
    pub capability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCompleted {
    pub node_id: String,
    pub capability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeFailed {
    pub node_id: String,
    pub capability: String,
    pub error: Option<String>,
}
