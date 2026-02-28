//! Orchestration — CanonIR analysis/emit pipeline.
//!
//! Usage:
//!   orchestration <canon_ir.json> <output_dir> [--mutate <mutation.json>]

use anyhow::{bail, Context, Result};
use canon::CanonIR;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: orchestration <canon_ir.json> <output_dir> [--mutate <mutation.json>]";
    let json_path = args.next().map(PathBuf::from).context(usage)?;
    let out_dir = args.next().map(PathBuf::from).context(usage)?;

    let mut mutate_path: Option<PathBuf> = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--mutate" => {
                mutate_path = Some(args.next().map(PathBuf::from).context("--mutate requires a path argument")?);
            }
            other => bail!("unknown flag: {other}"),
        }
    }

    run_pipeline(json_path, out_dir, mutate_path)
}

fn run_pipeline(json_path: PathBuf, out_dir: PathBuf, mutate_path: Option<PathBuf>) -> Result<()> {
    println!("Loading {:?}", json_path);
    let json = std::fs::read_to_string(&json_path).with_context(|| format!("cannot read {:?}", json_path))?;
    let mut canon_ir: CanonIR = serde_json::from_str(&json).with_context(|| format!("cannot parse CanonIR from {:?}", json_path))?;
    canon_ir.restore();
    println!("  canon nodes: {}", canon_ir.nodes.len());

    if let Some(mut_path) = mutate_path {
        let snap_a = canon_ir.clone();
        println!("Snapshot A: {} canon nodes", snap_a.nodes.len());

        let mut_json = std::fs::read_to_string(&mut_path).with_context(|| format!("cannot read mutation file {:?}", mut_path))?;
        let ops: Vec<canon_mutation::MutationOp> = serde_json::from_str(&mut_json).with_context(|| format!("cannot parse canon MutationOp list from {:?}", mut_path))?;
        println!("  applying {} canon mutation op(s)...", ops.len());

        for (i, op) in ops.into_iter().enumerate() {
            let id = canon_mutation::apply(&mut canon_ir, op).with_context(|| format!("canon mutation op {} failed", i))?;
            println!("  op {}: affected canon node {:?}", i, id);
        }

        println!("Verifying mutated CanonIR...");
        canon_mutation::verify(&canon_ir).context("verification failed after canon mutation")?;
        println!("  verification passed.");

        let delta = canon_mutation::diff(&snap_a, &canon_ir);
        println!(
            "ChangeSet: +{} nodes, -{} nodes, ~{} nodes, +{} edges, -{} edges",
            delta.added_nodes.len(),
            delta.removed_nodes.len(),
            delta.changed_nodes.len(),
            delta.added_edges.len(),
            delta.removed_edges.len(),
        );

        std::fs::create_dir_all(&out_dir)?;
        let diff_path = out_dir.join("canon_diff_report.json");
        std::fs::write(&diff_path, serde_json::to_string_pretty(&delta)?).context("canon diff report write failed")?;
        println!("Canon diff report written to {:?}", diff_path);
    }

    println!("Analyzing CanonIR...");
    canon_analyzer::canon_analyze(&mut canon_ir).context("canon analysis failed")?;

    println!("Emitting source (CanonIR pipeline)...");
    let canon_plan = canon_projection::project(&canon_ir).context("canon project failed")?;
    canon_projection::emit_to_disk(&canon_ir, &canon_plan, &out_dir).context("canon emit failed")?;
    println!("Canon emitted {} file(s) to {:?}", canon_plan.files.len(), out_dir);

    println!("Scanning emitted structural surface...");
    match canon_telemetry::scan_emit_dir(&out_dir).context("structural surface scan failed")? {
        Some(surface) => {
            surface.print_report();
            let snap_surface_path = out_dir.join("canon_structural_surface.json");
            std::fs::write(
                &snap_surface_path,
                serde_json::to_string_pretty(&surface).context("surface serialize failed")?,
            )
            .context("surface snapshot write failed")?;
            println!("Structural surface snapshot written to {:?}", snap_surface_path);
        }
        None => {
            println!("  (no src/ dir found under emit dir, skipping surface scan)");
        }
    }

    println!("Running cargo build on emitted source...");
    let build_report = canon_telemetry::build(&out_dir, true).context("cargo build invocation failed")?;
    build_report.print_report();
    let build_report_path = out_dir.join("canon_build_report.json");
    std::fs::write(
        &build_report_path,
        serde_json::to_string_pretty(&build_report).context("build report serialize failed")?,
    )
    .context("build report write failed")?;
    println!("Build report written to {:?}", build_report_path);

    let canon_snap_path = out_dir.join("canon_ir_solved.json");
    let canon_snap = serde_json::to_string_pretty(&canon_ir).context("canon serialize failed")?;
    std::fs::create_dir_all(&out_dir)?;
    std::fs::write(&canon_snap_path, canon_snap).context("canon snapshot write failed")?;
    println!("Canon snapshot written to {:?}", canon_snap_path);

    println!("Pipeline complete.");
    Ok(())
}
