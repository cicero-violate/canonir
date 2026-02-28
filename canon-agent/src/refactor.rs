//! RefactorProposal — the unit of work for the self-modification pipeline.
//!
//! A RefactorProposal is what the Reasoner node produces.
//! It is pure data — no LLM calls, no side effects.

use serde::{Deserialize, Serialize};

use crate::ir::PipelineStage;

/// What kind of structural change the refactor proposes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefactorKind {
    /// Split one module into two.
    SplitModule,
    /// Merge two modules into one.
    MergeModules,
    /// Move a struct/trait/function to a different module.
    MoveArtifact,
    /// Rename a module, struct, trait, or function.
    RenameArtifact,
    /// Add a new module edge (dependency declaration).
    AddEdge,
    /// Remove an existing module edge.
    RemoveEdge,
    /// Promote a capability node to its own subgraph (L7 only).
    PromoteCapability,
}

/// What artifact the refactor targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorTarget {
    /// The IR artifact id being targeted (module id, struct id, etc.).
    pub artifact_id: String,
    /// Human-readable kind label: "module", "struct", "trait", "function".
    pub artifact_kind: String,
}

/// A refactor proposal produced by a Reasoner capability node.
/// Carries enough information for the Prover and Judge nodes to evaluate it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorProposal {
    pub id: String,
    pub kind: RefactorKind,
    pub target: RefactorTarget,
    /// For moves/merges: the destination artifact id.
    pub destination_id: Option<String>,
    /// Human-readable rationale from the Reasoner node.
    pub rationale: String,
    /// The proposal_id in CanonicalIr this maps to (set after IR registration).
    pub ir_proposal_id: Option<String>,
    /// The proof_id that the Prover node must populate before Judge runs.
    pub proof_id: Option<String>,
    /// Pipeline stage this proposal was created in.
    pub stage: PipelineStage,
}

impl RefactorProposal {
    pub fn new(id: impl Into<String>, kind: RefactorKind, target: RefactorTarget, rationale: impl Into<String>, stage: PipelineStage) -> Self {
        Self { id: id.into(), kind, target, destination_id: None, rationale: rationale.into(), ir_proposal_id: None, proof_id: None, stage }
    }

    /// Returns true if the Prover has populated a proof_id.
    pub fn is_proven(&self) -> bool {
        self.proof_id.is_some()
    }

    /// Returns true if the proposal has been registered in the IR.
    pub fn is_registered(&self) -> bool {
        self.ir_proposal_id.is_some()
    }
}
