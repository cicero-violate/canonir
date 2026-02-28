//! Lyapunov stability gate for structural IR mutations.
//!
//! Before any structural delta mutates the IR topology, this gate checks
//! that the proposed change does not exceed the drift bound θ. This is the
//! safety invariant that makes self-modification controlled rather than arbitrary.
//!
//! Formally: ||Φ(G) - G||_F ≤ θ  →  mutation permitted
//!
//! We approximate the Frobenius norm over the adjacency matrix using
//! compute_goal_drift() over topology fingerprints (module + edge counts).
//! This keeps the gate purely data-structural — no LLM calls, no SMT.
use crate::ir::{
    goals::{compute_goal_drift, GoalDriftMetric},
    SystemState,
};
/// Default drift bound θ for structural mutations.
/// Tighter than goal mutation (0.05) — topology changes must be small per tick.
pub const DEFAULT_TOPOLOGY_THETA: f64 = 0.15;
/// A fingerprint of IR topology: used to compute drift between before/after.
#[derive(Debug, Clone)]
pub struct StructureMetrics {
    pub module_count: usize,
    pub module_edge_count: usize,
    pub struct_count: usize,
    pub trait_count: usize,
    pub function_count: usize,
    pub call_edge_count: usize,
    pub tick_graph_count: usize,
    pub delta_count: usize,
}
impl StructureMetrics {
    /// Captures the current IR topology.
    pub fn of(ir: &SystemState) -> Self {
        Self {
            module_count: ir.modules.len(),
            module_edge_count: ir.module_edges.len(),
            struct_count: ir.structs.len(),
            trait_count: ir.traits.len(),
            function_count: ir.functions.len(),
            call_edge_count: ir.call_edges.len(),
            tick_graph_count: ir.tick_graphs.len(),
            delta_count: ir.deltas.len(),
        }
    }
    /// Renders as a token string for drift computation.
    /// Each count is repeated as tokens so compute_goal_drift Jaccard distance
    /// reflects relative change in topology weight.
    pub fn as_token_string(&self) -> String {
        format!(
            "{} {} {} {} {} {} {} {}", "module ".repeat(self.module_count.max(1)),
            "module_edge ".repeat(self.module_edge_count.max(1)), "struct ".repeat(self
            .struct_count.max(1)), "trait ".repeat(self.trait_count.max(1)), "function "
            .repeat(self.function_count.max(1)), "call_edge ".repeat(self.call_edge_count
            .max(1)), "tick_graph ".repeat(self.tick_graph_count.max(1)), "delta "
            .repeat(self.delta_count.max(1)),
        )
    }
    /// Frobenius-approximated drift between two fingerprints.
    /// Uses the same Jaccard-based compute_goal_drift as goal mutation.
    pub fn drift_from(&self, other: &StructureMetrics, theta: f64) -> GoalDriftMetric {
        compute_goal_drift(&self.as_token_string(), &other.as_token_string(), theta)
    }
}
/// Error returned when the Lyapunov gate rejects a mutation.
#[derive(Debug, Clone)]
pub enum StructureDriftError {
    /// Topology drift exceeded bound θ.
    DriftExceeded { cosine_distance: f64, bound_theta: f64 },
    /// No invariant proofs were provided — gate requires at least one.
    NoInvariantProofs,
}
impl std::fmt::Display for StructureDriftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StructureDriftError::DriftExceeded { cosine_distance, bound_theta } => {
                write!(
                    f,
                    "topology drift {cosine_distance:.4} exceeds bound θ={bound_theta:.4}"
                )
            }
            StructureDriftError::NoInvariantProofs => {
                write!(f, "structural mutation requires at least one invariant proof")
            }
        }
    }
}
impl std::error::Error for StructureDriftError {}
/// Gate a structural mutation by checking topology drift against θ.
///
/// Call this with the IR *before* and *after* a proposed mutation.
/// Returns Ok(GoalDriftMetric) if the mutation is within bound.
/// Returns Err(LyapunovError) if drift exceeds θ or proofs are missing.
///
/// # Arguments
/// - `before`         — IR snapshot before mutation
/// - `after`          — proposed IR after mutation
/// - `invariant_proof_ids` — proof ids that must be non-empty
/// - `theta`          — drift bound (use DEFAULT_TOPOLOGY_THETA if unsure)
pub fn enforce_lyapunov_bound(
    before: &SystemState,
    after: &SystemState,
    invariant_proof_ids: &[String],
    theta: f64,
) -> Result<GoalDriftMetric, StructureDriftError> {
    if invariant_proof_ids.is_empty() {
        return Err(StructureDriftError::NoInvariantProofs);
    }
    let fp_before = StructureMetrics::of(before);
    let fp_after = StructureMetrics::of(after);
    let mut metric = fp_before.drift_from(&fp_after, theta);
    metric.mutation_id = format!(
        "topology:modules={}->{},functions={}->{}", fp_before.module_count, fp_after
        .module_count, fp_before.function_count, fp_after.function_count,
    );
    if !metric.within_bound {
        return Err(StructureDriftError::DriftExceeded {
            cosine_distance: metric.cosine_distance,
            bound_theta: theta,
        });
    }
    Ok(metric)
}
