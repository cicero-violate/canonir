#[derive(Debug, Clone)]
pub struct CodeNode {
    pub id: u32,
    pub kind: String,
    pub symbol: String,
    pub file_id: Option<u32>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct CodeEdge {
    pub src: u32,
    pub dst: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Default)]
pub struct CodeGraphState {
    pub nodes: Vec<CodeNode>,
    pub edges: Vec<CodeEdge>,
    pub files: Vec<String>,
}
