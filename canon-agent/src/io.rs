use std::fs;
use std::path::Path;
use super::capability::AgentGraph;
/// Loads a CapabilityGraph from disk. Returns a default empty graph if the
/// file does not exist (first run).
pub fn load_capability_graph(
    path: &Path,
) -> Result<AgentGraph, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(AgentGraph::default());
    }
    let data = fs::read(path)?;
    Ok(serde_json::from_slice(&data)?)
}
/// Saves a CapabilityGraph to disk as pretty-printed JSON.
pub fn save_capability_graph(
    graph: &AgentGraph,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(graph)?;
    fs::write(path, json)?;
    Ok(())
}
