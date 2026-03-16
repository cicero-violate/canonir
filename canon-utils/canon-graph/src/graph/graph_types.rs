pub use canon_event_store::{CodeEdge, CodeNode};

#[derive(Clone)]
pub struct ModuleNode {
    pub id: u32,
    pub symbol: String,
    pub file: String,
}
