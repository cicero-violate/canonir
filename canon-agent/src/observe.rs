//! Observer capability node — concrete IR analysis logic.
//!
//! Reads declared IrFields from a slice, produces a structured observation:
//!   - hottest modules   (most call-edge targets)
//!   - largest structs   (most fields)
//!   - deepest call chains (longest path in call graph from each function)
//!
//! Output is a serde_json::Value suitable for feeding directly to the
//! Reasoner as structured JSON (not free text).
//!
//! No LLM calls. No unsafe. Pure graph analysis.
use crate::ir::SystemState;
use serde_json::{json, Value};
use std::collections::HashMap;
/// Top-N limit for each observation category.
const TOP_N: usize = 5;
/// A fully structured observation produced by the Observer node.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct IrAnalysisReport {
    /// Modules ranked by in-degree in the call graph (most-called first).
    pub hottest_modules: Vec<ModuleHeat>,
    /// Structs ranked by field count (largest first).
    pub largest_structs: Vec<StructSize>,
    /// Functions ranked by longest outgoing call chain depth (deepest first).
    pub deepest_call_chains: Vec<CallChainDepth>,
    /// Total counts snapshot.
    pub totals: IrTotals,
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ModuleHeat {
    pub module_id: String,
    pub module_name: String,
    /// Number of call edges that target functions inside this module.
    pub in_degree: usize,
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct StructSize {
    pub struct_id: String,
    pub struct_name: String,
    pub field_count: usize,
    pub module_id: String,
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CallChainDepth {
    pub function_id: String,
    pub function_name: String,
    pub depth: usize,
    pub module_id: String,
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct IrTotals {
    pub modules: usize,
    pub functions: usize,
    pub structs: usize,
    pub traits: usize,
    pub call_edges: usize,
    pub deltas: usize,
    pub proofs: usize,
}
/// Produce a structured IrObservation from a CanonicalIr.
///
/// This is the concrete implementation for the Observer CapabilityNode.
/// The result serialises directly to JSON and is placed in AgentCallOutput.payload.
pub fn analyze_ir(ir: &SystemState) -> IrAnalysisReport {
    IrAnalysisReport {
        hottest_modules: hottest_modules(ir),
        largest_structs: largest_structs(ir),
        deepest_call_chains: deepest_call_chains(ir),
        totals: IrTotals {
            modules: ir.modules.len(),
            functions: ir.functions.len(),
            structs: ir.structs.len(),
            traits: ir.traits.len(),
            call_edges: ir.call_edges.len(),
            deltas: ir.deltas.len(),
            proofs: ir.proofs.len(),
        },
    }
}
/// Serialises an IrObservation to a JSON Value for use as AgentCallOutput.payload.
pub fn ir_observation_to_json(obs: &IrAnalysisReport) -> Value {
    serde_json::to_value(obs)
        .unwrap_or_else(|_| json!({ "error" : "serialisation failed" }))
}
fn hottest_modules(ir: &SystemState) -> Vec<ModuleHeat> {
    let fn_to_module: HashMap<&str, &str> = ir
        .functions
        .iter()
        .map(|f| (f.id.as_str(), f.module.as_str()))
        .collect();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for edge in &ir.call_edges {
        if let Some(&module_id) = fn_to_module.get(edge.callee.as_str()) {
            *in_degree.entry(module_id).or_insert(0) += 1;
        }
    }
    let module_name: HashMap<&str, &str> = ir
        .modules
        .iter()
        .map(|m| (m.id.as_str(), m.name.as_str()))
        .collect();
    let mut ranked: Vec<ModuleHeat> = in_degree
        .into_iter()
        .map(|(module_id, deg)| ModuleHeat {
            module_id: module_id.to_string(),
            module_name: module_name
                .get(module_id)
                .copied()
                .unwrap_or(module_id)
                .to_string(),
            in_degree: deg,
        })
        .collect();
    ranked.sort_by(|a, b| b.in_degree.cmp(&a.in_degree));
    ranked.truncate(TOP_N);
    ranked
}
fn largest_structs(ir: &SystemState) -> Vec<StructSize> {
    let mut ranked: Vec<StructSize> = ir
        .structs
        .iter()
        .map(|s| StructSize {
            struct_id: s.id.clone(),
            struct_name: s.name.as_str().to_string(),
            field_count: s.fields.len(),
            module_id: s.module.clone(),
        })
        .collect();
    ranked.sort_by(|a, b| b.field_count.cmp(&a.field_count));
    ranked.truncate(TOP_N);
    ranked
}
fn deepest_call_chains(ir: &SystemState) -> Vec<CallChainDepth> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &ir.call_edges {
        adj.entry(edge.caller.as_str()).or_default().push(edge.callee.as_str());
    }
    let fn_name: HashMap<&str, &str> = ir
        .functions
        .iter()
        .map(|f| (f.id.as_str(), f.name.as_str()))
        .collect();
    let fn_module: HashMap<&str, &str> = ir
        .functions
        .iter()
        .map(|f| (f.id.as_str(), f.module.as_str()))
        .collect();
    let mut memo: HashMap<&str, usize> = HashMap::new();
    let mut ranked: Vec<CallChainDepth> = ir
        .functions
        .iter()
        .map(|f| {
            let depth = chain_depth(f.id.as_str(), &adj, &mut memo, &mut vec![]);
            CallChainDepth {
                function_id: f.id.clone(),
                function_name: fn_name
                    .get(f.id.as_str())
                    .copied()
                    .unwrap_or(&f.id)
                    .to_string(),
                depth,
                module_id: fn_module
                    .get(f.id.as_str())
                    .copied()
                    .unwrap_or("")
                    .to_string(),
            }
        })
        .collect();
    ranked.sort_by(|a, b| b.depth.cmp(&a.depth));
    ranked.truncate(TOP_N);
    ranked
}
/// Recursive DFS with memoisation for longest chain depth.
/// `path` tracks the current DFS path to detect cycles.
fn chain_depth<'a>(
    node: &'a str,
    adj: &HashMap<&'a str, Vec<&'a str>>,
    memo: &mut HashMap<&'a str, usize>,
    path: &mut Vec<&'a str>,
) -> usize {
    if let Some(&cached) = memo.get(node) {
        return cached;
    }
    if path.contains(&node) {
        return 0;
    }
    let callees = match adj.get(node) {
        Some(cs) => cs.clone(),
        None => {
            memo.insert(node, 0);
            return 0;
        }
    };
    path.push(node);
    let max_child = callees
        .iter()
        .map(|callee| chain_depth(callee, adj, memo, path))
        .max()
        .unwrap_or(0);
    path.pop();
    let depth = 1 + max_child;
    memo.insert(node, depth);
    depth
}
