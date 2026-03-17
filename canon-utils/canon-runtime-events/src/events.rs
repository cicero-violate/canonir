use canon_types::{EventDelta, RustcEvent, RustcState};
use std::sync::Arc;

use canon_macros::{canon_event_struct, canon_event_enum};

canon_event_struct!(RenameSymbol { project: String, old: String, new: String });
canon_event_struct!(MoveSymbol { project: String, symbol: String, module: String });
canon_event_struct!(DeleteSymbol { project: String, symbol: String });
canon_event_struct!(RenameModule { project: String, old: String, new: String });
canon_event_struct!(RenameDir { project: String, old: std::path::PathBuf, new: std::path::PathBuf });
canon_event_struct!(InlineModule { project: String, module: String });
canon_event_struct!(ExtractModule { project: String, symbol: String, module: String });

canon_event_enum!(#[derive(serde::Serialize, serde::Deserialize)]
EditEvent {
    RenameSymbol(RenameSymbol),
    MoveSymbol(MoveSymbol),
    DeleteSymbol(DeleteSymbol),
    RenameModule(RenameModule),
    RenameDir(RenameDir),
    InlineModule(InlineModule),
    ExtractModule(ExtractModule),
});

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
            RustcEvent::NodeDefined(_) => Self::NODE_DEFINED,
            RustcEvent::NodeUpdated(_) => Self::NODE_UPDATED,
            RustcEvent::NodeRemoved(_) => Self::NODE_REMOVED,
            RustcEvent::EdgeDefined(_) => Self::EDGE_DEFINED,
            RustcEvent::EdgeRemoved(_) => Self::EDGE_REMOVED,
            RustcEvent::FileSeen(_) => Self::FILE_SEEN,
            RustcEvent::CallsiteObserved(_) => Self::CALLSITE_OBSERVED,
            RustcEvent::SymbolDefined(_) => Self::SYMBOL_DEFINED,
            RustcEvent::SpanDefined(_) => Self::SPAN_DEFINED,
            RustcEvent::PanicCaptured(_) => Self::PANIC_CAPTURED,
            RustcEvent::WarningCaptured(_) => Self::WARNING_CAPTURED,
            RustcEvent::SessionStart(_) => Self::SESSION_START,
            RustcEvent::CompilationUnitFinished(_) => Self::COMPILATION_UNIT_FINISHED,
            RustcEvent::InvariantViolation(_) => Self::INVARIANT_VIOLATION,
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
pub struct Code {
    pub delta: canon_types::EventDelta,
    pub state: canon_types::RustcState,
}
canon_event_struct!(DebugEvent {
    source: String,
    kind: String,
    payload: serde_json::Value,
});
canon_event_struct!(Tick { tick: u64 });
canon_event_struct!(RuntimeStateUpdated { payload: serde_json::Value });
canon_event_struct!(PolicyBaselineUpdated { payload: serde_json::Value });
canon_event_struct!(GoalSelected { payload: serde_json::Value });
canon_event_struct!(SystemConfigLoaded { payload: serde_json::Value });
canon_event_struct!(AgentRegistered { payload: serde_json::Value });
canon_event_struct!(PromptLoaded { payload: serde_json::Value });
canon_event_struct!(ToolCall {
    node_id: String,
    request_id: String,
    kind: String,
    payload: serde_json::Value,
});
canon_event_struct!(ToolResult {
    node_id: String,
    request_id: String,
    kind: String,
    output: serde_json::Value,
    success: bool,
});
canon_event_struct!(GoalNodeCreated {
    node_id: String,
    description: String,
    deps: Vec<String>,
    caps: Vec<String>,
    node_type: String,
    priority: u8,
    #[serde(default)]
    budget: Option<u32>,
});
canon_event_struct!(GoalNodeRetracted { node_id: String });
canon_event_struct!(GoalNodeRewritten {
    node_id: String,
    new_description: String,
    new_caps: Vec<String>,
});
canon_event_struct!(GoalEdgeDefined { from_node_id: String, to_node_id: String });
canon_event_struct!(GoalGraphCheckpointed { tlog_seq: u64 });
canon_event_struct!(CapabilityInvoked { capability_id: String, name: String, node_id: String });
canon_event_struct!(CapabilityResolved { capability_id: String, success: bool, duration_ms: u64 });

canon_event_enum!(CanonEvent {
    Code(Code),
    Debug(DebugEvent),
    Edit(EditEvent),
    Tick(Tick),
    RuntimeStateUpdated(RuntimeStateUpdated),
    NodeReady(NodeReady),
    NodeStarted(NodeStarted),
    NodeCompleted(NodeCompleted),
    NodeFailed(NodeFailed),
    CapabilityRequested(CapabilityRequested),
    CapabilityCompleted(CapabilityCompleted),
    CapabilityFailed(CapabilityFailed),
    PolicyBaselineUpdated(PolicyBaselineUpdated),
    GoalSelected(GoalSelected),
    SystemConfigLoaded(SystemConfigLoaded),
    AgentRegistered(AgentRegistered),
    PromptLoaded(PromptLoaded),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    GoalNodeCreated(GoalNodeCreated),
    GoalNodeRetracted(GoalNodeRetracted),
    GoalNodeRewritten(GoalNodeRewritten),
    GoalEdgeDefined(GoalEdgeDefined),
    GoalGraphCheckpointed(GoalGraphCheckpointed),
    CapabilityInvoked(CapabilityInvoked),
    CapabilityResolved(CapabilityResolved),
});

pub trait EventEmitter: Send + Sync {
    fn emit(&self, event: CanonEvent);
}

pub type EventEmitterHandle = Arc<dyn EventEmitter>;

#[derive(Debug, Clone, Copy)]
pub enum EventFilter {
    All,
    Code(EventMask),
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
