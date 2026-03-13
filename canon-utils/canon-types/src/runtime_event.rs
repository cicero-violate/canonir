use crate::{EditEvent, EventDelta, EventMask, KernelState};

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    Kernel { delta: EventDelta, state: KernelState },
    Edit(EditEvent),
}

#[derive(Debug, Clone, Copy)]
pub enum RuntimeEventFilter {
    All,
    Kernel(EventMask),
    EditOnly,
}

pub trait RuntimeConsumer: Send + Sync {
    fn filter(&self) -> RuntimeEventFilter;
    fn on_event(&mut self, event: &RuntimeEvent);
}
