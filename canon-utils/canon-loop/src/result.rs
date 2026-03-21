use canon_event::RuntimeEvent;

#[derive(Debug)]
pub enum LoopStageResult {
    Emit(RuntimeEvent),
    EmitMany(Vec<RuntimeEvent>),
    Noop,
    Deferred,
}
