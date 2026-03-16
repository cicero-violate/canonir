use canon_types::{EventDelta, RustcEvent, RustcState};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditEvent {
    RenameSymbol { project: String, old: String, new: String },
    MoveSymbol { project: String, symbol: String, module: String },
    DeleteSymbol { project: String, symbol: String },
    RenameModule { project: String, old: String, new: String },
    RenameDir { project: String, old: PathBuf, new: PathBuf },
    InlineModule { project: String, module: String },
    ExtractModule { project: String, symbol: String, module: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventMask(pub(crate) u16);

impl EventMask {
    pub const NONE: EventMask = EventMask(0);
    pub const NODE_DEFINED: EventMask = EventMask(1 << 0);
    pub const EDGE_DEFINED: EventMask = EventMask(1 << 1);
    pub const FILE_SEEN: EventMask = EventMask(1 << 2);
    pub const NODE_UPDATED: EventMask = EventMask(1 << 3);
    pub const NODE_REMOVED: EventMask = EventMask(1 << 4);
    pub const EDGE_REMOVED: EventMask = EventMask(1 << 5);
    pub const PANIC_CAPTURED: EventMask = EventMask(1 << 6);
    pub const WARNING_CAPTURED: EventMask = EventMask(1 << 7);
    pub const SESSION_START: EventMask = EventMask(1 << 8);
    pub const COMPILATION_UNIT_FINISHED: EventMask = EventMask(1 << 9);
    pub const INVARIANT_VIOLATION: EventMask = EventMask(1 << 10);
    pub const CALLSITE_OBSERVED: EventMask = EventMask(1 << 11);
    pub const SYMBOL_DEFINED: EventMask = EventMask(1 << 12);
    pub const SPAN_DEFINED: EventMask = EventMask(1 << 13);
    pub const ALL: EventMask = EventMask(
        Self::NODE_DEFINED.0
            | Self::EDGE_DEFINED.0
            | Self::FILE_SEEN.0
            | Self::NODE_UPDATED.0
            | Self::NODE_REMOVED.0
            | Self::EDGE_REMOVED.0
            | Self::PANIC_CAPTURED.0
            | Self::WARNING_CAPTURED.0
            | Self::SESSION_START.0
            | Self::COMPILATION_UNIT_FINISHED.0
            | Self::INVARIANT_VIOLATION.0
            | Self::CALLSITE_OBSERVED.0
            | Self::SYMBOL_DEFINED.0
            | Self::SPAN_DEFINED.0,
    );

    pub fn contains(self, other: EventMask) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn for_event(event: &RustcEvent) -> EventMask {
        match event {
            RustcEvent::NodeDefined { .. } => Self::NODE_DEFINED,
            RustcEvent::NodeUpdated { .. } => Self::NODE_UPDATED,
            RustcEvent::NodeRemoved { .. } => Self::NODE_REMOVED,
            RustcEvent::EdgeDefined { .. } => Self::EDGE_DEFINED,
            RustcEvent::EdgeRemoved { .. } => Self::EDGE_REMOVED,
            RustcEvent::FileSeen { .. } => Self::FILE_SEEN,
            RustcEvent::CallsiteObserved { .. } => Self::CALLSITE_OBSERVED,
            RustcEvent::SymbolDefined { .. } => Self::SYMBOL_DEFINED,
            RustcEvent::SpanDefined { .. } => Self::SPAN_DEFINED,
            RustcEvent::PanicCaptured { .. } => Self::PANIC_CAPTURED,
            RustcEvent::WarningCaptured { .. } => Self::WARNING_CAPTURED,
            RustcEvent::SessionStart { .. } => Self::SESSION_START,
            RustcEvent::CompilationUnitFinished { .. } => Self::COMPILATION_UNIT_FINISHED,
            RustcEvent::InvariantViolation { .. } => Self::INVARIANT_VIOLATION,
        }
    }
}

impl std::ops::BitOr for EventMask {
    type Output = EventMask;

    fn bitor(self, rhs: EventMask) -> EventMask {
        EventMask(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for EventMask {
    fn bitor_assign(&mut self, rhs: EventMask) {
        self.0 |= rhs.0;
    }
}

pub trait RustcEventConsumer: Send + Sync {
    fn mask(&self) -> EventMask;
    fn on_event(&mut self, delta: &EventDelta, state: &RustcState);
}

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    Kernel { delta: EventDelta, state: RustcState },
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
    AgentState { payload: serde_json::Value },
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
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStarted {
    pub node_id: String,
    pub capability: String,
    #[serde(default)]
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCompleted {
    pub node_id: String,
    pub capability: String,
    #[serde(default)]
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeFailed {
    pub node_id: String,
    pub capability: String,
    pub error: Option<String>,
    #[serde(default)]
    pub request_id: String,
}
