use crate::{EditEvent, EventDelta, EventMask, KernelState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    Kernel { delta: EventDelta, state: KernelState },
    Edit(EditEvent),
    CapabilityRequested(CapabilityRequested),
    CapabilityCompleted(CapabilityCompleted),
    CapabilityFailed(CapabilityFailed),
}

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
