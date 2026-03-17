use canon_event::CanonEvent;

#[derive(Debug, Clone)]
pub enum CapabilityExecutionResult {
    Emit(CanonEvent),
    EmitMany(Vec<CanonEvent>),
    NoOp,
}
