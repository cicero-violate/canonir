use canon_event::RuntimeEvent;

#[derive(Debug, Clone)]
pub enum CapabilityResult {
    Emit(RuntimeEvent),
    EmitMany(Vec<RuntimeEvent>),
    NoOp,
}
