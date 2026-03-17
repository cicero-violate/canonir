/// Generate a serializable event struct with standard derives.
/// Supports optional per-field attributes (e.g. `#[serde(default)]`).
///
/// # Example
/// ```rust,ignore
/// canon_event_struct!(NodeReady {
///     node_id: String,
///     capability: String,
///     #[serde(default)]
///     request_id: String,
///     #[serde(default)]
///     args: serde_json::Value,
/// });
/// ```
#[macro_export]
macro_rules! canon_event_struct {
    ($name:ident { $($(#[$meta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        pub struct $name {
            $($(#[$meta])* pub $field: $ty),*
        }
    };
}

/// Generate a CanonEvent-style enum where each variant wraps its namesake struct.
///
/// # Example
/// ```rust,ignore
/// canon_event_enum!(CapabilityRequested, CapabilityCompleted, CapabilityFailed);
/// ```
#[macro_export]
macro_rules! canon_event_enum {
    ($($name:ident),* $(,)?) => {
        #[derive(Debug, Clone)]
        pub enum CanonEvent {
            $($name($name)),*
        }
    };
}
