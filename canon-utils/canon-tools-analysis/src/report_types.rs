use serde::Serialize;

#[derive(Serialize)]
pub struct BranchComplexityEntry {
    pub symbol: String,
    pub file: String,
    pub line: Option<u32>,
    pub branch_count: usize,
    pub duplicate_block_count: usize,
    pub score: usize,
}

#[derive(Serialize)]
pub struct CallgraphCentralityEntry {
    pub symbol: String,
    pub file: String,
    pub caller_count: usize,
    pub callee_count: usize,
    pub centrality_score: usize,
}

#[derive(Serialize)]
pub struct DeadCodeEntry {
    pub symbol: String,
    pub file: String,
    pub line: Option<u32>,
    pub reason: String,
}

#[derive(Serialize)]
pub struct DependencyCycleEntry {
    pub cycle_id: usize,
    pub nodes: Vec<String>,
    pub files: Vec<String>,
    pub cycle_length: usize,
}

#[derive(Serialize)]
pub struct StructuralHotspotEntry {
    pub symbol: String,
    pub file: String,
    pub line: Option<u32>,
    pub branch_count: usize,
    pub duplicate_blocks: usize,
    pub callers: Vec<String>,
    pub score: usize,
}

#[derive(Serialize)]
pub struct DataflowFanoutEntry {
    pub symbol: String,
    pub file: String,
    pub line: Option<u32>,
    pub outgoing_edges: usize,
    pub mutation_edges: usize,
    pub io_edges: usize,
}

#[derive(Serialize)]
pub struct BranchPressureEntry {
    pub symbol: String,
    pub file: String,
    pub line: Option<u32>,
    pub branch_nodes: usize,
    pub branch_pressure: usize,
}

#[derive(Serialize)]
pub struct MergeCandidateEntry {
    pub function: String,
    pub branch_block: u32,
    pub successors: Vec<u32>,
    pub candidate_blocks: Vec<u32>,
}

#[derive(Serialize)]
pub struct ReachabilityEntry {
    pub symbol: String,
    pub file: String,
    pub line: Option<u32>,
    pub reachable_blocks: usize,
    pub total_blocks: usize,
    pub reachable_ratio: f64,
}

#[derive(Serialize)]
pub struct PathRedundancyEntry {
    pub symbol: String,
    pub file: String,
    pub line: Option<u32>,
    pub paths_total: usize,
    pub paths_unique: usize,
    pub redundancy_ratio: f64,
}

#[derive(Serialize)]
pub struct RuntimeReachabilityEntry {
    pub symbol: String,
    pub file: String,
    pub line: Option<u32>,
}

#[derive(Serialize)]
pub struct RuntimeReachabilityReport {
    pub entry_symbol: String,
    pub entry_node_id: Option<u32>,
    pub total_functions: usize,
    pub reachable_functions: usize,
    pub coverage_ratio: f64,
    pub unreachable: Vec<RuntimeReachabilityEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}
