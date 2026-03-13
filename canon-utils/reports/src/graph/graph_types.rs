pub use canon_tlog_replay::{NodeRow, EdgeRow};

#[derive(Clone)]
pub struct ModuleNode {
    pub id: u32,
    pub symbol: String,
    pub file: String,
}
