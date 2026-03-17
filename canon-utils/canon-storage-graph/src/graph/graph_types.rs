pub use canon_event_store::{CodeGraphEdge, CodeGraphNode};

#[derive(Clone)]
pub struct ModuleNode {
    pub id: u32,
    pub symbol: String,
    pub file: String,
}
