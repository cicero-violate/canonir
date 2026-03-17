#[derive(Debug, Clone)]
pub struct CodeGraphNode {
    pub id: u32,
    pub kind: String,
    pub symbol: String,
    pub file_id: Option<u32>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct CodeGraphEdge {
    pub src: u32,
    pub dst: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Default)]
pub struct CodeGraphProjection {
    pub nodes: Vec<CodeGraphNode>,
    pub edges: Vec<CodeGraphEdge>,
    pub files: Vec<String>,
}
