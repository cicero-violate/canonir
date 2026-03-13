#[derive(Debug, Clone)]
pub struct NodeRow {
    pub id: u32,
    pub kind: String,
    pub symbol: String,
    pub file_id: Option<u32>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct EdgeRow {
    pub src: u32,
    pub dst: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Default)]
pub struct ReplayGraph {
    pub nodes: Vec<NodeRow>,
    pub edges: Vec<EdgeRow>,
    pub files: Vec<String>,
}
