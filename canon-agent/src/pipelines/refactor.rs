//! RefactorPipeline — data types + Observe→Reason→Prove→Judge→Mutate logic.
//!
//! Consolidates what was previously split across `refactor.rs` (data types)
//! and `pipeline.rs` (stage logic) into one cohesive module.

// ---------------------------------------------------------------------------
// Data types (was src/refactor.rs)
// ---------------------------------------------------------------------------

use crate::ir::PipelineStage;
use serde::{Deserialize, Serialize};

/// What kind of structural change the refactor proposes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefactorKind {
    SplitModule,
    MergeModules,
    MoveArtifact,
    RenameArtifact,
    AddEdge,
    RemoveEdge,
    PromoteCapability,
}

/// What artifact the refactor targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorTarget {
    pub artifact_id: String,
    pub artifact_kind: String,
}

/// A refactor proposal produced by a Reasoner capability node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorProposal {
    pub id: String,
    pub kind: RefactorKind,
    pub target: RefactorTarget,
    pub destination_id: Option<String>,
    pub rationale: String,
    pub ir_proposal_id: Option<String>,
    pub proof_id: Option<String>,
    pub stage: PipelineStage,
}

impl RefactorProposal {
    pub fn new(id: impl Into<String>, kind: RefactorKind, target: RefactorTarget, rationale: impl Into<String>, stage: PipelineStage) -> Self {
        Self { id: id.into(), kind, target, destination_id: None, rationale: rationale.into(), ir_proposal_id: None, proof_id: None, stage }
    }

    pub fn is_proven(&self) -> bool {
        self.proof_id.is_some()
    }

    pub fn is_registered(&self) -> bool {
        self.ir_proposal_id.is_some()
    }
}

// ---------------------------------------------------------------------------
// Pipeline stage logic (was src/pipeline.rs)
// ---------------------------------------------------------------------------

use super::super::call::AgentCallOutput;
use super::super::capability::AgentGraph;
use super::super::dispatcher::AgentScheduler;
use super::super::evolution::{apply_admitted_deltas, enforce_lyapunov_bound, EvolutionError, DEFAULT_TOPOLOGY_THETA};
use super::super::ir::{ChangePayload, CodeDelta, DeltaKind, PipelineStage as IrPipelineStage, StateChange, SystemState};
use super::super::layout::FileTopology;
use super::super::llm_provider::call_llm;
use super::super::reward::{NodeRewardLedger, PipelineNodeOutcome};
use super::super::runtime::reward::compute_pipeline_reward;
use super::super::ws_server::WsBridge;
use super::{Pipeline, PipelineContext, PipelineOutcome};
use serde_json::Value;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Stage enum
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum RefactorError {
    MissingPayloadField { stage: RefactorStage, field: String },
    MissingProof,
    Rejected { rationale: String },
    TopologyDrift(super::super::evolution::StructureDriftError),
    Evolution(EvolutionError),
    MissingAdmission,
    StageSkipped { stage: RefactorStage },
}

impl std::fmt::Display for RefactorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefactorError::MissingPayloadField { stage, field } => write!(f, "stage {stage}: missing payload field `{field}`"),
            RefactorError::MissingProof => write!(f, "Prove stage: proof_id not populated"),
            RefactorError::Rejected { rationale } => write!(f, "Judge stage: proposal rejected — {rationale}"),
            RefactorError::TopologyDrift(e) => write!(f, "Mutate stage: {e}"),
            RefactorError::Evolution(e) => write!(f, "Mutate stage: {e}"),
            RefactorError::MissingAdmission => write!(f, "Judge stage: admission_id not found in payload"),
            RefactorError::StageSkipped { stage } => write!(f, "stage {stage}: node skipped (insufficient trust)"),
        }
    }
}

impl std::error::Error for RefactorError {}

// ---------------------------------------------------------------------------
// Result
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct RefactorResult {
    pub ir: SystemState,
    pub layout: FileTopology,
    pub proposal: RefactorProposal,
    pub admission_id: String,
    pub reward: f64,
    pub code_deltas: Vec<CodeDelta>,
}

// ---------------------------------------------------------------------------
// Core pipeline function
// ---------------------------------------------------------------------------

pub fn run_refactor_pipeline(ir: &SystemState, layout: &FileTopology, mut proposal: RefactorProposal, stage_outputs: &[AgentCallOutput]) -> Result<RefactorResult, RefactorError> {
    let observer_out = require_stage(stage_outputs, 0, RefactorStage::Observe)?;
    extract_str_field(&observer_out.payload, "observation", RefactorStage::Observe)?;

    let reasoner_out = require_stage(stage_outputs, 1, RefactorStage::Reason)?;
    let rationale = extract_str_field(&reasoner_out.payload, "rationale", RefactorStage::Reason)?;
    proposal.rationale = rationale;

    let (ir_with_delta, delta_id) = {
        let mut ir_clone = ir.clone();
        let delta_id = format!("delta-tick-{}", stage_outputs.len());
        let payload: Option<ChangePayload> = reasoner_out.payload.get("change_payload").and_then(|v| serde_json::from_value(v.clone()).ok());
        if let Some(raw) = reasoner_out.payload.get("change_payload") {
            if payload.is_none() {
                eprintln!("[pipeline] WARN: change_payload present but failed to deserialize: {}", serde_json::to_string(raw).unwrap_or_default());
            } else {
                eprintln!("[pipeline] change_payload deserialized OK: type={:?}", raw.get("type").and_then(|v| v.as_str()).unwrap_or("?"));
            }
        } else {
            eprintln!("[pipeline] WARN: Reasoner emitted no change_payload field");
        }
        let proof_hint = stage_outputs
            .get(2)
            .and_then(|o| o.proof_id.clone())
            .or_else(|| stage_outputs.get(2).and_then(|o| o.payload.get("proof_id")).and_then(|v| v.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| format!("proof-{}", delta_id));
        let state_change = StateChange {
            id: delta_id.clone(),
            kind: DeltaKind::Structure,
            stage: IrPipelineStage::Act,
            append_only: true,
            proof: proof_hint,
            description: proposal.rationale.clone(),
            related_function: None,
            payload,
            proof_object_hash: None,
        };
        ir_clone.deltas.push(state_change);
        (ir_clone, delta_id)
    };

    let prover_out = require_stage(stage_outputs, 2, RefactorStage::Prove)?;
    let proof_id = prover_out.proof_id.clone().or_else(|| prover_out.payload.get("proof_id").and_then(|v| v.as_str()).map(|s| s.to_string())).ok_or(RefactorError::MissingProof)?;
    proposal.proof_id = Some(proof_id);

    let judge_out = require_stage(stage_outputs, 3, RefactorStage::Judge)?;
    let decision = extract_str_field(&judge_out.payload, "decision", RefactorStage::Judge)?;
    if decision.to_lowercase() != "accept" {
        let rationale = judge_out.payload.get("rationale").and_then(|v| v.as_str()).unwrap_or("no rationale provided").to_string();
        return Err(RefactorError::Rejected { rationale });
    }

    let admission_id = judge_out.payload.get("admission_id").and_then(|v| v.as_str()).ok_or(RefactorError::MissingAdmission)?.to_string();

    let resolved_id = if ir_with_delta.deltas.iter().any(|d| d.id == admission_id) { admission_id.clone() } else { delta_id.clone() };

    let proof_ids: Vec<String> = ir_with_delta.proofs.iter().map(|p| p.id.clone()).collect();
    let (candidate, code_deltas) = apply_admitted_deltas(&ir_with_delta, &[resolved_id]).map_err(RefactorError::Evolution)?;

    enforce_lyapunov_bound(ir, &candidate, &proof_ids, DEFAULT_TOPOLOGY_THETA).map_err(RefactorError::TopologyDrift)?;

    let next_layout = layout.clone();
    let reward = compute_pipeline_reward(ir, &candidate, 0.0, 0.0);

    Ok(RefactorResult { ir: candidate, layout: next_layout, proposal, admission_id, reward, code_deltas })
}

// ---------------------------------------------------------------------------
// Reward recording
// ---------------------------------------------------------------------------

pub fn record_refactor_reward(ledger: &mut NodeRewardLedger, node_id: &str, result: Result<&RefactorResult, &RefactorError>) -> f64 {
    let outcome = match result {
        Ok(r) => PipelineNodeOutcome::Accepted { reward: r.reward },
        Err(RefactorError::Rejected { .. }) => PipelineNodeOutcome::Rejected { penalty: 1.0 },
        Err(RefactorError::StageSkipped { .. }) => {
            return ledger.trust_threshold_for(node_id);
        }
        Err(_) => PipelineNodeOutcome::Halted { penalty: 0.5 },
    };
    ledger.record(node_id, outcome);
    ledger.trust_threshold_for(node_id)
}

// ---------------------------------------------------------------------------
// Pipeline trait impl
// ---------------------------------------------------------------------------

pub struct RefactorPipeline {
    pub graph: Arc<tokio::sync::Mutex<AgentGraph>>,
    pub ledger: Arc<tokio::sync::Mutex<NodeRewardLedger>>,
    pub proposal_seed: RefactorProposal,
    pub bridge: WsBridge,
    pub trust_threshold: f64,
}

#[async_trait::async_trait]
impl Pipeline for RefactorPipeline {
    fn name(&self) -> &str {
        "refactor"
    }

    async fn run_tick(&self, ctx: &PipelineContext, ir: &mut SystemState, layout: &mut FileTopology) -> anyhow::Result<PipelineOutcome> {
        let mut graph = self.graph.lock().await;
        let mut ledger = self.ledger.lock().await;

        let mut dispatcher = AgentScheduler::new(&graph, ir).with_trust_threshold(self.trust_threshold);
        let order = dispatcher.topological_call_order().map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut stage_outputs: Vec<AgentCallOutput> = Vec::new();
        for node_id in &order {
            let input = match dispatcher.dispatch(node_id) {
                Ok(inp) => inp,
                Err(e) => {
                    eprintln!("[refactor] dispatch skip {node_id}: {e}");
                    continue;
                }
            };
            // URL must come from AgentConfig; no hardcoded fallbacks.
            let url = &ctx.workspace.join("agent.json"); // placeholder to avoid unused warning
            let _ = url; // ensure no dead constant; actual URL supplied at higher layer
            match call_llm(&self.bridge, &input, ctx.workspace.to_str().unwrap_or("")).await {
                Ok(output) => {
                    stage_outputs.push(output.clone());
                    dispatcher.record_output(output);
                }
                Err(e) => {
                    eprintln!("[refactor] llm error on {node_id}: {e}");
                }
            }
        }

        if stage_outputs.is_empty() {
            return Ok(PipelineOutcome { reward: 0.0, summary: "no stage outputs".into(), advanced: false });
        }

        let mut proposal = self.proposal_seed.clone();
        proposal.id = format!("{}-tick-{}", self.proposal_seed.id, ctx.tick);

        let result = run_refactor_pipeline(ir, layout, proposal, &stage_outputs);
        let primary = order.first().map(|s| s.as_str()).unwrap_or("unknown");
        record_refactor_reward(&mut ledger, primary, result.as_ref());

        match result {
            Ok(r) => {
                *ir = r.ir.clone();
                *layout = r.layout.clone();
                Ok(PipelineOutcome { reward: r.reward, summary: format!("admission={} reward={:.4}", r.admission_id, r.reward), advanced: true })
            }
            Err(e) => Ok(PipelineOutcome { reward: -0.5, summary: format!("pipeline error: {e}"), advanced: false }),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn require_stage(outputs: &[AgentCallOutput], idx: usize, stage: RefactorStage) -> Result<&AgentCallOutput, RefactorError> {
    outputs.get(idx).ok_or_else(|| RefactorError::StageSkipped { stage })
}

fn extract_str_field(payload: &Value, field: &str, stage: RefactorStage) -> Result<String, RefactorError> {
    payload.get(field).and_then(|v| v.as_str()).map(|s| s.to_string()).ok_or_else(|| RefactorError::MissingPayloadField { stage, field: field.to_string() })
}
