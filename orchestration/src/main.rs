//! Orchestration — JSON ModelIR -> selected analysis/emit pipeline.
//!
//! Usage:
//!   orchestration <model_ir.json> <output_dir> [--mutate <mutation.json>] [--canon]
//!
//! Pipelines:
//!   default : capture -> ModelIR -> analyzer -> projection -> <output_dir>
//!   --canon : capture -> ModelIR -> seal -> canon-analyzer -> canon-projection -> <output_dir>

use anyhow::{Context, Result};
use canon::seal;
use model::ir::model_ir::ModelIR;
use mutation::{apply, diff, verify, MutationOp};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let json_path = args.next().map(PathBuf::from).context("usage: orchestration <model_ir.json> <output_dir> [--mutate <mutation.json>] [--canon]")?;
    let out_dir = args.next().map(PathBuf::from).context("usage: orchestration <model_ir.json> <output_dir> [--mutate <mutation.json>] [--canon]")?;

    let mut mutate_path: Option<PathBuf> = None;
    let mut canon_only = false;

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--mutate" => {
                mutate_path = Some(args.next().map(PathBuf::from).context("--mutate requires a path argument")?);
            }
            "--canon" => canon_only = true,
            _ => {}
        }
    }

    run_pipeline(json_path, out_dir, mutate_path, canon_only)
}

fn run_pipeline(json_path: PathBuf, out_dir: PathBuf, mutate_path: Option<PathBuf>, canon_only: bool) -> Result<()> {
    println!("Loading {:?}", json_path);
    let json = std::fs::read_to_string(&json_path).with_context(|| format!("cannot read {:?}", json_path))?;
    let mut ir: ModelIR = serde_json::from_str(&json).with_context(|| format!("cannot parse ModelIR from {:?}", json_path))?;
    println!("  nodes: {}", ir.nodes.len());

    println!("Analyzing...");
    analyzer::analyze(&mut ir).context("analysis failed")?;

    if let Some(mut_path) = mutate_path {
        let snap_a = ir.clone();
        println!("Snapshot A: {} nodes", snap_a.nodes.len());

        let mut_json = std::fs::read_to_string(&mut_path).with_context(|| format!("cannot read mutation file {:?}", mut_path))?;
        let ops: Vec<MutationOp> = serde_json::from_str(&mut_json).with_context(|| format!("cannot parse MutationOp list from {:?}", mut_path))?;
        println!("  applying {} mutation op(s)...", ops.len());

        for (i, op) in ops.into_iter().enumerate() {
            let id = apply(&mut ir, op).with_context(|| format!("mutation op {} failed", i))?;
            println!("  op {}: affected node {:?}", i, id);
        }

        println!("Verifying mutated IR...");
        verify(&ir).context("verification failed after mutation")?;
        println!("  verification passed.");

        let delta = diff(&snap_a, &ir);
        println!(
            "ChangeSet: +{} nodes, -{} nodes, ~{} nodes, +{} edges, -{} edges",
            delta.added_nodes.len(),
            delta.removed_nodes.len(),
            delta.changed_nodes.len(),
            delta.added_edges.len(),
            delta.removed_edges.len(),
        );

        std::fs::create_dir_all(&out_dir)?;
        let diff_path = out_dir.join("diff_report.json");
        std::fs::write(&diff_path, serde_json::to_string_pretty(&delta)?).context("diff report write failed")?;
        println!("Diff report written to {:?}", diff_path);
    }

    if canon_only {
        println!("Sealing to CanonIR...");
        let mut canon_ir = seal(&ir);
        println!("Analyzing CanonIR...");
        canon_analyzer::canon_analyze(&mut canon_ir).context("canon analysis failed")?;

        println!("Emitting source (CanonIR pipeline)...");
        let canon_plan = canon_projection::project(&canon_ir).context("canon project failed")?;
        canon_projection::emit_to_disk(&canon_ir, &canon_plan, &out_dir).context("canon emit failed")?;
        println!("Canon emitted {} file(s) to {:?}", canon_plan.files.len(), out_dir);

        let canon_snap_path = out_dir.join("canon_ir_solved.json");
        let canon_snap = serde_json::to_string_pretty(&canon_ir).context("canon serialize failed")?;
        std::fs::create_dir_all(&out_dir)?;
        std::fs::write(&canon_snap_path, canon_snap).context("canon snapshot write failed")?;
        println!("Canon snapshot written to {:?}", canon_snap_path);
    } else {
        println!("Emitting source (ModelIR pipeline)...");
        let plan = projection::project(&ir).context("project failed")?;
        projection::emit_to_disk(&plan, &out_dir).context("emit failed")?;
        println!("Emitted {} file(s) to {:?}", plan.files.len(), out_dir);

        let snap_path = out_dir.join("model_ir_solved.json");
        let snap = serde_json::to_string_pretty(&ir).context("json serialize failed")?;
        std::fs::create_dir_all(&out_dir)?;
        std::fs::write(&snap_path, snap).context("json write failed")?;
        println!("ModelIR snapshot written to {:?}", snap_path);
    }

    println!("Pipeline complete.");
    Ok(())
}
