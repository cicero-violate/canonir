use canon_types::{EventDelta, RustcEvent, RustcState};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::canon_event_struct;

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
pub enum CanonEvent {
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
    PolicyBaselineUpdated { payload: serde_json::Value },
    GoalSelected { payload: serde_json::Value },
    SystemConfigLoaded { payload: serde_json::Value },
    AgentRegistered { payload: serde_json::Value },
    PromptLoaded { payload: serde_json::Value },
    ToolCall { node_id: String, request_id: String, kind: String, payload: serde_json::Value },
    ToolResult { node_id: String, request_id: String, kind: String, output: serde_json::Value, success: bool },
    // Goal Graph: emitted when apply_graph_patch mutates the goal graph
    GoalNodeCreated { node_id: String, description: String, deps: Vec<String>, caps: Vec<String>, node_type: String, priority: u8, budget: Option<u32> },
    GoalNodeRetracted { node_id: String },
    GoalNodeRewritten { node_id: String, new_description: String, new_caps: Vec<String> },
    GoalEdgeDefined { from_node_id: String, to_node_id: String },
    GoalGraphCheckpointed { tlog_seq: u64 },
    // Capability Graph: emitted around capability dispatch
    CapabilityInvoked { capability_id: String, name: String, node_id: String },
    CapabilityResolved { capability_id: String, success: bool, duration_ms: u64 },
}

pub trait EventEmitter: Send + Sync {
    fn emit(&self, event: CanonEvent);
}

pub type EventEmitterHandle = Arc<dyn EventEmitter>;

#[derive(Debug, Clone, Copy)]
pub enum EventFilter {
    All,
    Kernel(EventMask),
    EditOnly,
    CapabilityOnly,
}

pub trait EventConsumer: Send + Sync {
    fn filter(&self) -> EventFilter;
    fn on_event(&mut self, event: &CanonEvent);
    fn set_emitter(&mut self, _emitter: EventEmitterHandle) {}
}

canon_event_struct!(CapabilityRequested {
    request_id: String,
    name: String,
    args: serde_json::Value,
});

canon_event_struct!(CapabilityCompleted {
    request_id: String,
    name: String,
    result: serde_json::Value,
});

canon_event_struct!(CapabilityFailed {
    request_id: String,
    name: String,
    error: String,
});

canon_event_struct!(NodeReady {
    node_id: String,
    capability: String,
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    args: serde_json::Value,
});

canon_event_struct!(NodeStarted {
    node_id: String,
    capability: String,
    #[serde(default)]
    request_id: String,
});

canon_event_struct!(NodeCompleted {
    node_id: String,
    capability: String,
    #[serde(default)]
    request_id: String,
});

canon_event_struct!(NodeFailed {
    node_id: String,
    capability: String,
    error: Option<String>,
    #[serde(default)]
    request_id: String,
});
