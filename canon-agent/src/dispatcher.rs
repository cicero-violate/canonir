//! AgentCallDispatcher — builds AgentCallInput from graph + IR.
//!
//! Does not make LLM calls. Produces a fully-formed AgentCallInput
//! ready to hand to any LLM provider. Trust threshold enforced here.
use super::call::{AgentCallError, AgentCallInput, AgentCallOutput};
use super::capability::AgentGraph;
use super::slice::slice_ir_fields;
use crate::ir::SystemState;
use std::collections::HashMap;
/// Minimum proof_confidence required on all incoming edges before
/// a node is permitted to receive a call. Nodes with no predecessors
/// are exempt (root axiom trust = 1.0).
pub const DEFAULT_TRUST_THRESHOLD: f64 = 0.5;
pub struct AgentScheduler<'a> {
    graph: &'a AgentGraph,
    ir: &'a SystemState,
    /// Completed outputs from earlier calls in this tick, keyed by node_id.
    completed: HashMap<String, AgentCallOutput>,
    trust_threshold: f64,
}
impl<'a> AgentScheduler<'a> {
    pub fn new(graph: &'a AgentGraph, ir: &'a SystemState) -> Self {
        Self {
            graph,
            ir,
            completed: HashMap::new(),
            trust_threshold: DEFAULT_TRUST_THRESHOLD,
        }
    }
    pub fn with_trust_threshold(mut self, threshold: f64) -> Self {
        self.trust_threshold = threshold;
        self
    }
    /// Record a completed call output so successors can access it.
    pub fn record_output(&mut self, output: AgentCallOutput) {
        self.completed.insert(output.node_id.clone(), output);
    }
    /// Build a fully-formed AgentCallInput for `node_id`.
    /// Returns Err if the node is unknown, trust is insufficient,
    /// or a required predecessor output is missing.
    pub fn dispatch(&self, node_id: &str) -> Result<AgentCallInput, AgentCallError> {
        let node = self
            .graph
            .node(node_id)
            .ok_or_else(|| AgentCallError::UnknownNode(node_id.to_string()))?;
        let trust_scores = self.graph.trust_scores();
        let score = trust_scores.get(node_id).copied().unwrap_or(1.0);
        let preds = self.graph.predecessors(node_id);
        if !preds.is_empty() && score < self.trust_threshold {
            return Err(AgentCallError::InsufficientTrust {
                node_id: node_id.to_string(),
                score,
                required: self.trust_threshold,
            });
        }
        let ir_slice = slice_ir_fields(self.ir, &node.reads);
        let mut predecessor_outputs = Vec::new();
        for pred in &preds {
            let output = self
                .completed
                .get(pred.id.as_str())
                .ok_or_else(|| AgentCallError::MissingPredecessorOutput(
                    pred.id.clone(),
                ))?;
            predecessor_outputs.push(output.clone());
        }
        let call_id = format!("{}#{}", node_id, self.completed.len());
        Ok(AgentCallInput {
            call_id,
            node_id: node_id.to_string(),
            ir_slice,
            predecessor_outputs,
            stage: node.stage,
        })
    }
    /// Returns node ids in topological dispatch order (predecessors first).
    /// Errors if the graph contains a cycle.
    pub fn topological_call_order(&self) -> Result<Vec<String>, AgentCallError> {
        let mut visited: std::collections::HashSet<String> = Default::default();
        let mut in_progress: std::collections::HashSet<String> = Default::default();
        let mut order: Vec<String> = Vec::new();
        for node in &self.graph.nodes {
            if !visited.contains(&node.id) {
                self.visit(&node.id, &mut visited, &mut in_progress, &mut order)?;
            }
        }
        Ok(order)
    }
    fn visit(
        &self,
        node_id: &str,
        visited: &mut std::collections::HashSet<String>,
        in_progress: &mut std::collections::HashSet<String>,
        order: &mut Vec<String>,
    ) -> Result<(), AgentCallError> {
        if in_progress.contains(node_id) {
            return Err(
                AgentCallError::SliceError(format!("cycle detected at node: {node_id}")),
            );
        }
        if visited.contains(node_id) {
            return Ok(());
        }
        in_progress.insert(node_id.to_string());
        for pred in self.graph.predecessors(node_id) {
            self.visit(&pred.id.clone(), visited, in_progress, order)?;
        }
        in_progress.remove(node_id);
        visited.insert(node_id.to_string());
        order.push(node_id.to_string());
        Ok(())
    }
}
