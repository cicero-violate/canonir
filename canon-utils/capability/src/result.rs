use canon_types::RuntimeEvent;

#[derive(Debug, Clone)]
pub enum CapabilityResult {
    Emit(RuntimeEvent),
    EmitMany(Vec<RuntimeEvent>),
    NoOp,
}
