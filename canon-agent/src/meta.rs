//! L7 Meta-tick — CapabilityGraph self-rewrite driven by reward signal.
//!
//! The meta-tick fires every N pipeline runs. It reads ranked_nodes()
//! from the RewardLedger, identifies underperforming nodes, and proposes
//! topology mutations to the CapabilityGraph itself.
//!
//! Safety: every proposed graph mutation is checked against:
//!   1. Entropy bound  — H(G') must not deviate from H(G) by more than η
//!   2. Minimum nodes  — graph must retain at least MIN_NODES after mutation
//!   3. Connectivity   — every remaining node must have a path to at least one successor
//!
//! This is the Φ: G → G' operator made concrete.
//! No LLM calls. No unsafe. Pure graph arithmetic.
use super::capability::{CapabilityEdge, AgentGraph, CapabilityKind, AgentNode};
use super::reward::NodeRewardLedger;
use crate::ir::PipelineStage;
/// Minimum number of nodes the capability graph must retain after a meta-tick.
pub const MIN_NODES: usize = 3;
/// Maximum entropy deviation allowed between G and Φ(G).
pub const MAX_ENTROPY_DELTA: f64 = 0.5;
/// EMA reward below this threshold marks a node as underperforming.
pub const UNDERPERFORM_THRESHOLD: f64 = -0.1;
/// A single proposed mutation to the CapabilityGraph topology.
#[derive(Debug, Clone)]
pub enum GraphMutation {
    /// Remove a node and all its edges.
    RemoveNode { node_id: String },
    /// Add a new edge between two existing nodes.
    AddEdge { from: String, to: String, proof_confidence: f64 },
    /// Remove an edge between two nodes.
    RemoveEdge { from: String, to: String },
    /// Promote a node to MetaAgent kind.
    PromoteToMetaAgent { node_id: String },
}
/// Error returned when a meta-tick mutation is rejected.
#[derive(Debug, Clone)]
pub enum GraphEvolutionError {
    /// Graph entropy deviated beyond MAX_ENTROPY_DELTA.
    EntropyBoundExceeded { before: f64, after: f64, delta: f64 },
    /// Mutation would drop graph below MIN_NODES.
    TooFewNodes { remaining: usize },
    /// Target node does not exist.
    UnknownNode(String),
    /// Mutation would disconnect a node entirely.
    DisconnectedNode(String),
    /// No mutations were proposed (graph is already optimal).
    NothingToDo,
}
impl std::fmt::Display for GraphEvolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphEvolutionError::EntropyBoundExceeded { before, after, delta } => {
                write!(
                    f,
                    "entropy deviation {delta:.4} exceeds bound {MAX_ENTROPY_DELTA:.4} \
                 (before={before:.4} after={after:.4})"
                )
            }
            GraphEvolutionError::TooFewNodes { remaining } => {
                write!(
                    f,
                    "mutation would leave only {remaining} nodes (minimum {MIN_NODES})"
                )
            }
            GraphEvolutionError::UnknownNode(id) => write!(f, "unknown node: {id}"),
            GraphEvolutionError::DisconnectedNode(id) => {
                write!(f, "mutation would disconnect node: {id}")
            }
            GraphEvolutionError::NothingToDo => write!(f, "no mutations proposed"),
        }
    }
}
impl std::error::Error for GraphEvolutionError {}
/// Result of a successful meta-tick.
#[derive(Debug)]
pub struct GraphEvolutionResult {
    /// The mutated capability graph.
    pub graph: AgentGraph,
    /// Mutations that were applied.
    pub applied: Vec<GraphMutation>,
    /// Mutations that were rejected by safety checks.
    pub rejected: Vec<(GraphMutation, GraphEvolutionError)>,
    /// Entropy before mutation.
    pub entropy_before: f64,
    /// Entropy after mutation.
    pub entropy_after: f64,
}
/// Drives one meta-tick over the capability graph.
///
/// Reads the reward ledger to identify underperforming nodes,
/// proposes graph mutations, applies each through safety checks,
/// and returns the mutated graph.
///
/// Returns Err(GraphEvolutionError::NothingToDo) if no mutations are warranted.
pub fn evolve_capability_graph(
    graph: &AgentGraph,
    ledger: &NodeRewardLedger,
) -> Result<GraphEvolutionResult, GraphEvolutionError> {
    let mutations = propose_mutations(graph, ledger);
    if mutations.is_empty() {
        return Err(GraphEvolutionError::NothingToDo);
    }
    let entropy_before = graph.entropy();
    let mut current = graph.clone();
    let mut applied = Vec::new();
    let mut rejected = Vec::new();
    for mutation in mutations {
        match apply_mutation(&current, &mutation) {
            Ok(candidate) => {
                let entropy_after = candidate.entropy();
                let delta = (entropy_after - entropy_before).abs();
                if delta > MAX_ENTROPY_DELTA {
                    rejected
                        .push((
                            mutation,
                            GraphEvolutionError::EntropyBoundExceeded {
                                before: entropy_before,
                                after: entropy_after,
                                delta,
                            },
                        ));
                    continue;
                }
                if candidate.nodes.len() < MIN_NODES {
                    rejected
                        .push((
                            mutation,
                            GraphEvolutionError::TooFewNodes {
                                remaining: candidate.nodes.len(),
                            },
                        ));
                    continue;
                }
                if let Some(id) = find_disconnected(&candidate) {
                    rejected.push((mutation, GraphEvolutionError::DisconnectedNode(id)));
                    continue;
                }
                applied.push(mutation);
                current = candidate;
            }
            Err(e) => {
                rejected.push((mutation, e));
            }
        }
    }
    let entropy_after = current.entropy();
    Ok(GraphEvolutionResult {
        graph: current,
        applied,
        rejected,
        entropy_before,
        entropy_after,
    })
}
/// Proposes graph mutations based on reward ledger state.
fn propose_mutations(
    graph: &AgentGraph,
    ledger: &NodeRewardLedger,
) -> Vec<GraphMutation> {
    let mut mutations = Vec::new();
    let ranked = ledger.ranked_nodes();
    for entry in &ranked {
        if entry.ema_reward < UNDERPERFORM_THRESHOLD {
            if let Some(node) = graph.node(&entry.node_id) {
                if node.kind != CapabilityKind::MetaAgent {
                    mutations
                        .push(GraphMutation::RemoveNode {
                            node_id: entry.node_id.clone(),
                        });
                }
            }
        }
    }
    let has_meta = graph.nodes.iter().any(|n| n.kind == CapabilityKind::MetaAgent);
    if !has_meta {
        if let Some(top) = ranked.first() {
            if top.ema_reward > 0.0 {
                if let Some(node) = graph.node(&top.node_id) {
                    if node.kind != CapabilityKind::MetaAgent {
                        mutations
                            .push(GraphMutation::PromoteToMetaAgent {
                                node_id: top.node_id.clone(),
                            });
                    }
                }
            }
        }
    }
    mutations
}
/// Applies a single GraphMutation to a cloned graph, returning the candidate.
fn apply_mutation(
    graph: &AgentGraph,
    mutation: &GraphMutation,
) -> Result<AgentGraph, GraphEvolutionError> {
    let mut next = graph.clone();
    match mutation {
        GraphMutation::RemoveNode { node_id } => {
            if !next.nodes.iter().any(|n| n.id == *node_id) {
                return Err(GraphEvolutionError::UnknownNode(node_id.clone()));
            }
            next.nodes.retain(|n| n.id != *node_id);
            next.edges.retain(|e| e.from != *node_id && e.to != *node_id);
        }
        GraphMutation::AddEdge { from, to, proof_confidence } => {
            if !next.nodes.iter().any(|n| &n.id == from) {
                return Err(GraphEvolutionError::UnknownNode(from.clone()));
            }
            if !next.nodes.iter().any(|n| &n.id == to) {
                return Err(GraphEvolutionError::UnknownNode(to.clone()));
            }
            next.edges
                .push(CapabilityEdge {
                    from: from.clone(),
                    to: to.clone(),
                    proof_confidence: *proof_confidence,
                });
        }
        GraphMutation::RemoveEdge { from, to } => {
            next.edges.retain(|e| !(e.from == *from && e.to == *to));
        }
        GraphMutation::PromoteToMetaAgent { node_id } => {
            let node = next
                .nodes
                .iter_mut()
                .find(|n| n.id == *node_id)
                .ok_or_else(|| GraphEvolutionError::UnknownNode(node_id.clone()))?;
            node.kind = CapabilityKind::MetaAgent;
            node.stage = PipelineStage::Act;
        }
    }
    Ok(next)
}
/// Returns the id of the first node that has no edges at all, or None.
fn find_disconnected(graph: &AgentGraph) -> Option<String> {
    if graph.nodes.len() <= 1 {
        return None;
    }
    for node in &graph.nodes {
        let has_edge = graph.edges.iter().any(|e| e.from == node.id || e.to == node.id);
        if !has_edge {
            return Some(node.id.clone());
        }
    }
    None
}
