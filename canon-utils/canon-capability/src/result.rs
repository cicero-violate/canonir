use canon_event::CanonEvent;

#[derive(Debug, Clone)]
pub enum CapabilityExecutionResult {
    Emit(CanonEvent),
    EmitMany(Vec<CanonEvent>),
    /// Work dispatched asynchronously; completion will arrive later via the emitter.
    /// The runtime must NOT emit a fallback completion for this case.
    Deferred,
    NoOp,
}
