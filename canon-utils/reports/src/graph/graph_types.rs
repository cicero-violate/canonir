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

#[derive(Clone)]
pub struct ModuleNode {
    pub id: u32,
    pub symbol: String,
    pub file: String,
}
