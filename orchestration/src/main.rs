//! Orchestration — JSON ModelIR → analyze → emit Rust source.
//!
//! Usage:
//!   orchestration <model_ir.json> <output_dir> [--mutate <mutation.json>]
//!
//! Pipeline (no --mutate):
//!   load → analyze → emit → snapshot_A
//!
//! Pipeline (--mutate):
//!   load → analyze → snapshot_A → apply_mutations → diff(A, current)
//!        → verify → emit → snapshot_B → write diff report
//!
//! Variables:
//!   ir       : ModelIR        — mutable IR threaded through all stages
//!   snap_A   : ModelIR        — clone before mutations
//!   snap_B   : ModelIR        — clone after mutations
//!   Δ        : ChangeSet      — diff(snap_A, snap_B)

use anyhow::{Context, Result};
use std::path::PathBuf;
use model::ir::model_ir::ModelIR;
use mutation::{MutationOp, apply, diff, verify};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let json_path = args.next().map(PathBuf::from)
        .context("usage: orchestration <model_ir.json> <output_dir> [--mutate <mutation.json>]")?;
    let out_dir = args.next().map(PathBuf::from)
        .context("usage: orchestration <model_ir.json> <output_dir> [--mutate <mutation.json>]")?;

    // Optional --mutate <path>
    let mutate_path: Option<PathBuf> = {
        let flag = args.next();
        if flag.as_deref() == Some("--mutate") {
            Some(args.next().map(PathBuf::from)
                .context("--mutate requires a path argument")?)
        } else {
            None
        }
    };

    run_pipeline(json_path, out_dir, mutate_path)
}

fn run_pipeline(json_path: PathBuf, out_dir: PathBuf, mutate_path: Option<PathBuf>) -> Result<()> {
    // ── Stage 1: load ModelIR from JSON ─────────────────────────────────────
    println!("Loading {:?}", json_path);
    let json = std::fs::read_to_string(&json_path)
        .with_context(|| format!("cannot read {:?}", json_path))?;
    let mut ir: ModelIR = serde_json::from_str(&json)
        .with_context(|| format!("cannot parse ModelIR from {:?}", json_path))?;
    println!("  nodes: {}", ir.nodes.len());

    // ── Stage 2: analyze (derive + solve) ───────────────────────────────────
    println!("Analyzing...");
    analyzer::analyze(&mut ir).context("analysis failed")?;

    // ── Stage 3: optional mutation pipeline ─────────────────────────────────
    if let Some(mut_path) = mutate_path {
        // snapshot_A = clone before mutations
        // Equation: snap_A = clone(IR)
        let snap_a = ir.clone();
        println!("Snapshot A: {} nodes", snap_a.nodes.len());

        // Load mutation ops from JSON
        // Format: array of MutationOp
        let mut_json = std::fs::read_to_string(&mut_path)
            .with_context(|| format!("cannot read mutation file {:?}", mut_path))?;
        let ops: Vec<MutationOp> = serde_json::from_str(&mut_json)
            .with_context(|| format!("cannot parse MutationOp list from {:?}", mut_path))?;
        println!("  applying {} mutation op(s)...", ops.len());

        // Apply each op in sequence
        // Equation: IR' = fold(apply, IR, ops)
        for (i, op) in ops.into_iter().enumerate() {
            let id = apply(&mut ir, op).with_context(|| format!("mutation op {} failed", i))?;
            println!("  op {}: affected node {:?}", i, id);
        }

        // verify — re-runs analyze + invariant_solver on mutated IR
        // Equation: verify(IR') = Ok  (else abort)
        println!("Verifying mutated IR...");
        verify(&ir).context("verification failed after mutation")?;
        println!("  verification passed.");

        // diff(snap_A, IR')
        // Equation: Δ = diff(snap_A, IR')
        let delta = diff(&snap_a, &ir);
        println!("ChangeSet: +{} nodes, -{} nodes, ~{} nodes, +{} edges, -{} edges",
            delta.added_nodes.len(),
            delta.removed_nodes.len(),
            delta.changed_nodes.len(),
            delta.added_edges.len(),
            delta.removed_edges.len(),
        );

        // Write diff report
        std::fs::create_dir_all(&out_dir)?;
        let diff_path = out_dir.join("diff_report.json");
        std::fs::write(&diff_path, serde_json::to_string_pretty(&delta)?)
            .context("diff report write failed")?;
        println!("Diff report written to {:?}", diff_path);
    }

    // ── Stage 4: project → emit to disk ─────────────────────────────────────
    println!("Emitting source...");
    let plan = projection::project(&ir).context("project failed")?;
    projection::emit_to_disk(&plan, &out_dir).context("emit failed")?;
    println!("Emitted {} file(s) to {:?}", plan.files.len(), out_dir);

    // ── Stage 5: write snapshot ──────────────────────────────────────────────
    // Equation: snap_B = serialize(IR')
    let snap_path = out_dir.join("model_ir_solved.json");
    let snap = serde_json::to_string_pretty(&ir).context("json serialize failed")?;
    std::fs::create_dir_all(&out_dir)?;
    std::fs::write(&snap_path, snap).context("json write failed")?;
    println!("Snapshot written to {:?}", snap_path);

    println!("Pipeline complete.");
    Ok(())
}
