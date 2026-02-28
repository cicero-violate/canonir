use serde::{Deserialize, Serialize};
use std::collections::HashMap;
/// Which top-level fields of CanonicalIr this node is allowed to read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IrField {
    Modules,
    ModuleEdges,
    Structs,
    Enums,
    Traits,
    ImplBlocks,
    Functions,
    CallEdges,
    TickGraphs,
    SystemGraphs,
    LoopPolicies,
    Ticks,
    TickEpochs,
    PolicyParameters,
    Plans,
    Executions,
    Admissions,
    AppliedDeltas,
    GpuFunctions,
    Proposals,
    Judgments,
    JudgmentPredicates,
    Deltas,
    Proofs,
    Learning,
    Errors,
    Dependencies,
    FileHashes,
    RewardDeltas,
    WorldModel,
    GoalMutations,
}
/// The role a capability node plays in the L3 graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    /// Reads IR, emits observation deltas.
    Observer,
    /// Proposes structural or behavioural refactors.
    Reasoner,
    /// Generates or checks SMT proofs.
    Prover,
    /// Accepts or rejects proposals via judgment.
    Judge,
    /// Applies admitted deltas to produce new IR.
    Mutator,
    /// Computes reward signal from execution records.
    Evaluator,
    /// Rewrites graph topology (L7 only).
    MetaAgent,
}
/// A single node in the L3 capability graph.
/// Each node corresponds to one stateless LLM call invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNode {
    pub id: String,
    pub kind: CapabilityKind,
    /// Human-readable label for this node.
    pub label: String,
    /// Exact IR fields this node may read. Enforced at call-slice time.
    pub reads: Vec<IrField>,
    /// Exact IR fields this node may write (via emitted deltas only).
    pub writes: Vec<IrField>,
    /// Which pipeline stage this node belongs to.
    pub stage: crate::ir::PipelineStage,
}
/// A directed edge in the capability graph.
/// Data flows from `from` to `to` — the output of one call feeds the next.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityEdge {
    pub from: String,
    pub to: String,
    /// Proof confidence on this edge: 0.0 = unverified, 1.0 = fully proven.
    pub proof_confidence: f64,
}
/// The L3 capability graph.
/// Lives beside CanonicalIr and LayoutGraph — not inside either.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentGraph {
    pub nodes: Vec<AgentNode>,
    pub edges: Vec<CapabilityEdge>,
}
impl AgentGraph {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_node(&mut self, node: AgentNode) {
        self.nodes.push(node);
    }
    pub fn add_edge(&mut self, edge: CapabilityEdge) {
        self.edges.push(edge);
    }
    pub fn node(&self, id: &str) -> Option<&AgentNode> {
        self.nodes.iter().find(|n| n.id == id)
    }
    /// Returns all nodes that feed into `id` (predecessors).
    pub fn predecessors(&self, id: &str) -> Vec<&AgentNode> {
        self.edges
            .iter()
            .filter(|e| e.to == id)
            .filter_map(|e| self.node(&e.from))
            .collect()
    }
    /// Returns all nodes that `id` feeds into (successors).
    pub fn successors(&self, id: &str) -> Vec<&AgentNode> {
        self.edges
            .iter()
            .filter(|e| e.from == id)
            .filter_map(|e| self.node(&e.to))
            .collect()
    }
    /// Validates that every edge references existing node ids.
    pub fn validate_edges(&self) -> Vec<String> {
        let ids: std::collections::HashSet<&str> = self
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        let mut violations = Vec::new();
        for edge in &self.edges {
            if !ids.contains(edge.from.as_str()) {
                violations.push(format!("edge from unknown node: {}", edge.from));
            }
            if !ids.contains(edge.to.as_str()) {
                violations.push(format!("edge to unknown node: {}", edge.to));
            }
        }
        violations
    }
    /// Computes graph entropy H(G) = -Σ π(v,u) log π(v,u)
    /// where π is uniform over outgoing edges per node.
    pub fn entropy(&self) -> f64 {
        let mut h = 0.0_f64;
        for node in &self.nodes {
            let out_edges: Vec<&CapabilityEdge> = self
                .edges
                .iter()
                .filter(|e| e.from == node.id)
                .collect();
            let n = out_edges.len() as f64;
            if n > 1.0 {
                let p = 1.0 / n;
                h += -n * p * p.ln();
            }
        }
        h
    }
    /// Returns node trust scores τ(v) propagated from proof_confidence on edges.
    /// Nodes with no predecessors get trust = 1.0 (root axiom).
    pub fn trust_scores(&self) -> HashMap<String, f64> {
        let mut scores: HashMap<String, f64> = self
            .nodes
            .iter()
            .map(|n| (n.id.clone(), 1.0_f64))
            .collect();
        for _ in 0..self.nodes.len() {
            for node in &self.nodes {
                let preds: Vec<&CapabilityEdge> = self
                    .edges
                    .iter()
                    .filter(|e| e.to == node.id)
                    .collect();
                if preds.is_empty() {
                    continue;
                }
                let sum: f64 = preds
                    .iter()
                    .map(|e| {
                        e.proof_confidence
                            * scores.get(e.from.as_str()).copied().unwrap_or(1.0)
                    })
                    .sum();
                scores.insert(node.id.clone(), sum / preds.len() as f64);
            }
        }
        scores
    }
}
