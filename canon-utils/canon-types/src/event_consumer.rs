use crate::{EventDelta, KernelEvent, KernelState};

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

    pub fn for_event(event: &KernelEvent) -> EventMask {
        match event {
            KernelEvent::NodeDefined { .. } => Self::NODE_DEFINED,
            KernelEvent::NodeUpdated { .. } => Self::NODE_UPDATED,
            KernelEvent::NodeRemoved { .. } => Self::NODE_REMOVED,
            KernelEvent::EdgeDefined { .. } => Self::EDGE_DEFINED,
            KernelEvent::EdgeRemoved { .. } => Self::EDGE_REMOVED,
            KernelEvent::FileSeen { .. } => Self::FILE_SEEN,
            KernelEvent::CallsiteObserved { .. } => Self::CALLSITE_OBSERVED,
            KernelEvent::SymbolDefined { .. } => Self::SYMBOL_DEFINED,
            KernelEvent::SpanDefined { .. } => Self::SPAN_DEFINED,
            KernelEvent::PanicCaptured { .. } => Self::PANIC_CAPTURED,
            KernelEvent::WarningCaptured { .. } => Self::WARNING_CAPTURED,
            KernelEvent::SessionStart { .. } => Self::SESSION_START,
            KernelEvent::CompilationUnitFinished { .. } => Self::COMPILATION_UNIT_FINISHED,
            KernelEvent::InvariantViolation { .. } => Self::INVARIANT_VIOLATION,
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

pub trait KernelEventConsumer: Send + Sync {
    fn mask(&self) -> EventMask;
    fn on_event(&mut self, delta: &EventDelta, state: &KernelState);
}
