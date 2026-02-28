//! RefactorProposal pipeline: Observe → Reason → Prove → Judge → Mutate.
//!
//! Pure data pipeline — no LLM calls, no async.
//! Each stage validates the previous stage's output before proceeding.
//! If any stage fails the pipeline halts with a typed PipelineError.
//!
//! The caller drives each stage by supplying AgentCallOutputs from their
//! LLM client. The pipeline assembles them into IR mutations via the
//! existing accept_proposal + apply_deltas + Lyapunov gate chain.
//!
//! If nodes were skipped due to insufficient trust the pipeline short-circuits
//! at the first missing stage and returns PipelineError::StageSkipped so the
//! runner can log the incomplete tick without penalising the ledger.
use super::call::AgentCallOutput;
use super::refactor::RefactorProposal;
use super::reward::{NodeRewardLedger, PipelineNodeOutcome};
use crate::runtime::reward::compute_pipeline_reward;
use crate::{
    evolution::{
        apply_admitted_deltas, enforce_lyapunov_bound, EvolutionError,
        DEFAULT_TOPOLOGY_THETA,
    },
    ir::{SystemState, CodeDelta},
    layout::FileTopology,
};
use serde_json::Value;
/// Which stage the pipeline is currently at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefactorStage {
    Observe,
    Reason,
    Prove,
    Judge,
    Mutate,
    Complete,
}
impl std::fmt::Display for RefactorStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefactorStage::Observe => write!(f, "Observe"),
            RefactorStage::Reason => write!(f, "Reason"),
            RefactorStage::Prove => write!(f, "Prove"),
            RefactorStage::Judge => write!(f, "Judge"),
            RefactorStage::Mutate => write!(f, "Mutate"),
            RefactorStage::Complete => write!(f, "Complete"),
        }
    }
}
/// Typed error for each pipeline stage.
#[derive(Debug)]
pub enum RefactorError {
    /// A required field was missing from an AgentCallOutput payload.
    MissingPayloadField { stage: RefactorStage, field: String },
    /// The Prover stage did not populate proof_id on the proposal.
    MissingProof,
    /// The Judge stage rejected the proposal.
    Rejected { rationale: String },
    /// The Lyapunov gate blocked the mutation.
    TopologyDrift(crate::evolution::StructureDriftError),
    /// apply_deltas failed.
    Evolution(EvolutionError),
    /// No admission_id was found in the Judge output payload.
    MissingAdmission,
    /// One or more nodes were skipped due to insufficient trust; pipeline
    /// cannot proceed past this stage in the current tick.
    StageSkipped { stage: RefactorStage },
}
impl std::fmt::Display for RefactorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefactorError::MissingPayloadField { stage, field } => {
                write!(f, "stage {stage}: missing payload field `{field}`")
            }
            RefactorError::MissingProof => {
                write!(f, "Prove stage: proof_id not populated")
            }
            RefactorError::Rejected { rationale } => {
                write!(f, "Judge stage: proposal rejected — {rationale}")
            }
            RefactorError::TopologyDrift(e) => write!(f, "Mutate stage: {e}"),
            RefactorError::Evolution(e) => write!(f, "Mutate stage: {e}"),
            RefactorError::MissingAdmission => {
                write!(f, "Judge stage: admission_id not found in payload")
            }
            RefactorError::StageSkipped { stage } => {
                write!(f, "stage {stage}: node skipped (insufficient trust)")
            }
        }
    }
}
impl std::error::Error for RefactorError {}
/// Result of a completed pipeline run.
#[derive(Debug)]
pub struct RefactorResult {
    /// The mutated IR after all stages completed successfully.
    pub ir: SystemState,
    /// The layout after mutation.
    pub layout: FileTopology,
    /// The refactor proposal that was applied.
    pub proposal: RefactorProposal,
    /// Admission id recorded in the IR.
    pub admission_id: String,
    /// Reward signal from this pipeline run.
    pub reward: f64,
    /// Concrete code deltas emitted by this mutation.
    pub code_deltas: Vec<CodeDelta>,
}
/// Drives a RefactorProposal through the Observe→Reason→Prove→Judge→Mutate pipeline.
///
/// Each `stage_output` corresponds to one LLM call result in order:
/// [0] = Observer output, [1] = Reasoner output, [2] = Prover output,
/// [3] = Judge output. The Mutate stage is driven internally.
///
/// If a node was skipped by the dispatcher (insufficient trust) its slot will
/// be absent from `stage_outputs`. The pipeline short-circuits at that point
/// with `PipelineError::StageSkipped` rather than treating it as a hard
/// payload error. The runner should log it and continue to the next tick.
///
/// Returns PipelineResult on success, PipelineError on first failure.
pub fn run_refactor_pipeline(
    ir: &SystemState,
    layout: &FileTopology,
    mut proposal: RefactorProposal,
    stage_outputs: &[AgentCallOutput],
) -> Result<RefactorResult, RefactorError> {
    let observer_out = require_stage(stage_outputs, 0, RefactorStage::Observe)?;
    extract_str_field(&observer_out.payload, "observation", RefactorStage::Observe)?;
    let reasoner_out = require_stage(stage_outputs, 1, RefactorStage::Reason)?;
    let rationale = extract_str_field(
        &reasoner_out.payload,
        "rationale",
        RefactorStage::Reason,
    )?;
    proposal.rationale = rationale;
    let prover_out = require_stage(stage_outputs, 2, RefactorStage::Prove)?;
    let proof_id = prover_out
        .proof_id
        .clone()
        .or_else(|| {
            prover_out
                .payload
                .get("proof_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .ok_or(RefactorError::MissingProof)?;
    proposal.proof_id = Some(proof_id);
    let judge_out = require_stage(stage_outputs, 3, RefactorStage::Judge)?;
    let decision = extract_str_field(
        &judge_out.payload,
        "decision",
        RefactorStage::Judge,
    )?;
    if decision.to_lowercase() != "accept" {
        let rationale = judge_out
            .payload
            .get("rationale")
            .and_then(|v| v.as_str())
            .unwrap_or("no rationale provided")
            .to_string();
        return Err(RefactorError::Rejected {
            rationale,
        });
    }
    let admission_id = judge_out
        .payload
        .get("admission_id")
        .and_then(|v| v.as_str())
        .ok_or(RefactorError::MissingAdmission)?
        .to_string();
    let proof_ids: Vec<String> = ir.proofs.iter().map(|p| p.id.clone()).collect();
    let (candidate, code_deltas) =
        apply_admitted_deltas(ir, &[admission_id.clone()])
            .map_err(RefactorError::Evolution)?;
    enforce_lyapunov_bound(ir, &candidate, &proof_ids, DEFAULT_TOPOLOGY_THETA)
        .map_err(RefactorError::TopologyDrift)?;
    let next_layout = layout.clone();
    let reward = compute_pipeline_reward(ir, &candidate, 0.0, 0.0);
    Ok(RefactorResult {
        ir: candidate,
        layout: next_layout,
        proposal,
        admission_id,
        reward,
        code_deltas,
    })
}
/// Records a pipeline result into the ledger and returns the updated
/// trust threshold for the primary node that drove this pipeline.
pub fn record_refactor_reward(
    ledger: &mut NodeRewardLedger,
    node_id: &str,
    result: Result<&RefactorResult, &RefactorError>,
) -> f64 {
    let outcome = match result {
        Ok(r) => {
            PipelineNodeOutcome::Accepted {
                reward: r.reward,
            }
        }
        Err(RefactorError::Rejected { .. }) => {
            PipelineNodeOutcome::Rejected {
                penalty: 1.0,
            }
        }
        Err(RefactorError::StageSkipped { .. }) => {
            return ledger.trust_threshold_for(node_id);
        }
        Err(_) => {
            PipelineNodeOutcome::Halted {
                penalty: 0.5,
            }
        }
    };
    ledger.record(node_id, outcome);
    ledger.trust_threshold_for(node_id)
}
/// Returns the output at `idx` if present, or `StageSkipped` if the node
/// was not dispatched this tick (trust gate). Distinct from `MissingPayloadField`
/// which signals a structural problem in an output that *was* returned.
fn require_stage(
    outputs: &[AgentCallOutput],
    idx: usize,
    stage: RefactorStage,
) -> Result<&AgentCallOutput, RefactorError> {
    outputs
        .get(idx)
        .ok_or_else(|| RefactorError::StageSkipped {
            stage,
        })
}
fn extract_str_field<'a>(
    payload: &'a Value,
    field: &str,
    stage: RefactorStage,
) -> Result<String, RefactorError> {
    payload
        .get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| RefactorError::MissingPayloadField {
            stage,
            field: field.to_string(),
        })
}
