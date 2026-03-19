pub mod events;
pub mod emit;
pub mod schema;
pub mod tlog;
pub mod macros;

/// Generate a [`RustcEventConsumer`] impl for a concrete type.
///
/// The named method must have signature:
/// `fn NAME(&mut self, delta: &canon_types::EventDelta, state: &canon_types::RustcState)`
///
/// Example:
/// ```ignore
/// impl_rustc_consumer!(MyConsumer, EventMask::ALL, handle_event);
/// ```
#[macro_export] // TODO() need to remove this
macro_rules! impl_rustc_consumer {
    ($type:ty, $mask:expr, $handler:ident) => {
        impl $crate::RustcEventConsumer for $type {
            fn mask(&self) -> $crate::EventMask {
                $mask
            }
            fn on_event(
                &mut self,
                delta: &canon_types::EventDelta,
                state: &canon_types::RustcState,
            ) {
                self.$handler(delta, state);
            }
        }
    };
}

pub use events::{
    CanonEvent,
    EditEvent,
    EventConsumer,
    EventEmitter,
    EventEmitterHandle,
    EventFilter,
    EventMask,
    RustcEventConsumer,
    RenameSymbol,
    MoveSymbol,
    DeleteSymbol,
    RenameModule,
    RenameDir,
    InlineModule,
    ExtractModule,
    Code,
    DebugEvent,
    ErrorOccurred,
    Tick,
    RuntimeStateUpdated,
    PolicyBaselineUpdated,
    GoalSelected,
    SystemConfigLoaded,
    AgentRegistered,
    PromptLoaded,
    ToolCall,
    ToolResult,
    GoalNodeCreated,
    GoalNodeRetracted,
    GoalNodeRewritten,
    GoalEdgeDefined,
    GoalGraphCheckpointed,
    CapabilityInvoked,
    CapabilityResolved,
    CapabilityRequested,
    CapabilityCompleted,
    CapabilityFailed,
    NodeReady,
    NodeStarted,
    NodeCompleted,
    NodeFailed,
};
pub use emit::*;
pub use schema::*;
pub use macros::emit::canon_emit;
pub use tlog::{
    emit_event_json, is_binary_tlog,
    BinarySegmentWriter, BinaryTlogWriter, TlogEvent, RotateConfig, SegmentConfig, TlogWriter,
    maybe_rotate,
};
