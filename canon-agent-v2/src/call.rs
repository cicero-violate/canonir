use serde::{Deserialize, Serialize};

use crate::ir::PipelineStage;

/// Unique identifier for a single agent call invocation.
pub type AgentCallId = String;

/// The input payload handed to one stateless LLM call.
/// Contains only the IR slice the node is permitted to read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCallInput {
    pub call_id: AgentCallId,
    pub node_id: String,
    /// Serialised IR slice (only the fields declared in CapabilityNode::reads).
    pub ir_slice: serde_json::Value,
    /// Any outputs from predecessor nodes in this tick's call chain.
    pub predecessor_outputs: Vec<AgentCallOutput>,
    pub stage: PipelineStage,
}

/// The output produced by one stateless LLM call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCallOutput {
    pub call_id: AgentCallId,
    pub node_id: String,
    /// Structured result payload — interpretation depends on CapabilityKind.
    pub payload: serde_json::Value,
    /// Proof id generated during this call, if any.
    pub proof_id: Option<String>,
    /// Delta ids emitted during this call, if any.
    pub emitted_delta_ids: Vec<String>,
    pub stage: PipelineStage,
}

/// Result of dispatching a single agent call.
#[derive(Debug, Clone)]
pub enum AgentCallResult {
    Ok(AgentCallOutput),
    Err(AgentCallError),
}

/// Errors that can occur when dispatching an agent call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCallError {
    /// The node id referenced does not exist in the capability graph.
    UnknownNode(String),
    /// The IR slice could not be constructed (field access violation).
    SliceError(String),
    /// The LLM call returned a payload that failed schema validation.
    InvalidPayload(String),
    /// A predecessor node's output was missing when required.
    MissingPredecessorOutput(String),
    /// Proof confidence on an incoming edge was below the required threshold.
    InsufficientTrust { node_id: String, score: f64, required: f64 },
}

impl std::fmt::Display for AgentCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentCallError::UnknownNode(id) => write!(f, "unknown capability node: {id}"),
            AgentCallError::SliceError(msg) => write!(f, "IR slice error: {msg}"),
            AgentCallError::InvalidPayload(msg) => write!(f, "invalid payload: {msg}"),
            AgentCallError::MissingPredecessorOutput(id) => {
                write!(f, "missing predecessor output from node: {id}")
            }
            AgentCallError::InsufficientTrust { node_id, score, required } => {
                write!(f, "insufficient trust on node {node_id}: score={score:.3} required={required:.3}")
            }
        }
    }
}

impl std::error::Error for AgentCallError {}
