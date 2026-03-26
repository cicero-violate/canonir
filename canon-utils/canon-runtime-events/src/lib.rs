pub mod emit;
pub mod events;
pub mod wire;
pub mod macros;
pub mod schema;
pub mod tlog;
pub mod invariants;

/// Generate a [`RustcEventConsumer`] impl for a concrete type.
///
/// The named method must have signature:
/// `fn NAME(&mut self, delta: &canon_types::EventDelta, state: &canon_types::RustcState)`
///
/// Example:
/// ```ignore
/// impl_rustc_consumer!(MyConsumer, EventMask::ALL, handle_event);
/// ```
#[macro_export]
macro_rules! impl_rustc_consumer {
    ($type:ty, $mask:expr, $handler:ident) => {
        impl $crate::RustcEventConsumer for $type {
            fn mask(&self) -> $crate::EventMask {
                $mask
            }
            fn on_event(&mut self, delta: &canon_types::EventDelta, state: &canon_types::RustcState) {
                self.$handler(delta, state);
            }
        }
    };
}

pub use emit::*;
pub use events::{
    new_error_occurred, AgentRegistered, AnalysisEvent, AnalysisRun, AnalysisWorkspace, BashInvoke, CapabilityCompleted, CapabilityFailed, CapabilityInvoked, CapabilityResolved, CapabilityResult,
    CargoBuild, CargoCheck, CargoEvent, CargoRun, Code, DebugEvent, DeleteSymbol, EditEvent, ErrorOccurred, EventConsumer, EventEmitter, EventEmitterHandle, EventFilter, EventMask, EventOutcome, ExtractModule,
    FileEvent, FilePatch, FileRead, FileWrite, GoalEdgeDefined, GoalGraphCheckpointed, GoalNodeCreated, GoalNodeRetracted, GoalNodeRewritten, GoalSelected, InlineModule, InvariantDiscovered, LlmCall, LlmResult,
    LocatedEvent, LoopActed, LoopObserved, LoopPlanned, LoopRewarded, LoopVerified, MoveSymbol, NodeCompleted, NodeFailed, NodeReady, NodeStarted, PolicyBaselineUpdated, ProcessResult, PromptLoaded,
    RenameDir, RenameModule, RenameSymbol, RequestDispatch, RouteSelected, RouteTick, RuntimeEvent, RuntimeStateUpdated, RustcEventConsumer, SubTaskResult, SystemConfigLoaded, Tick, ToolBatchSettled,
    ToolCall, ToolResult, GoodnessSnapshot,
    event_kind_str,
    EVENT_SCHEMA_VERSION,
};
pub use wire::{CanonEvent, CanonPayload, CanonPayloadMeta, EventId, EventKind, CanonPayloadShape};
pub use macros::emit::canon_emit;
pub use schema::*;
pub use tlog::{is_binary_tlog, maybe_rotate, BinarySegmentWriter, RotateConfig, SegmentConfig};
