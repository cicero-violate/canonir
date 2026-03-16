pub mod events;
pub mod emit;
pub mod schema;
pub mod emit_debug;
pub mod tlog;

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

pub use events::*;
pub use emit::*;
pub use schema::*;
pub use emit_debug::*;
pub use tlog::{
    emit_event_json, is_binary_tlog,
    BinarySegmentWriter, BinaryTlogWriter, CanonEvent, RotateConfig, SegmentConfig, TlogWriter,
    maybe_rotate,
};
