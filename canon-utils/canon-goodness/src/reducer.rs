use canon_event::RuntimeEvent;

pub trait Reducer: Send {
    fn update(&mut self, event: &RuntimeEvent);
    fn value(&self) -> f32;  // always in [0.0, 1.0]
    fn reset(&mut self);
}
