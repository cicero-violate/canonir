use canon_event::CanonEvent;

#[derive(Debug)]
pub enum LoopStageResult {
    Emit(CanonEvent),
    EmitMany(Vec<CanonEvent>),
    Noop,
    Deferred,
}
