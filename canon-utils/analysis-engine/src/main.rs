use anyhow::{anyhow, Result};
use std::env;
use std::path::PathBuf;

use analysis_engine::anomalies::analyze_anomalies;
use analysis_engine::duplicates::find_duplicates;
use analysis_engine::emit::write_json;
use analysis_engine::invariants::analyze_invariants;
use analysis_engine::loader::load_dir;
use analysis_engine::refactoring::analyze_refactoring;

#[derive(Debug)]
struct Args {
    dir: PathBuf,
    phase: String,
    epsilon: f32,
}

fn parse_args() -> Result<Args> {
    let mut dir = PathBuf::from("analysis");
    let mut phase = String::from("all");
    let mut epsilon = 0.1f32;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => {
                let value = args.next().ok_or_else(|| anyhow!("--dir requires a path"))?;
                dir = PathBuf::from(value);
            }
            "--phase" => {
                phase = args.next().ok_or_else(|| anyhow!("--phase requires a value"))?;
            }
            "--epsilon" => {
                let raw = args.next().ok_or_else(|| anyhow!("--epsilon requires a value"))?;
                epsilon = raw.parse::<f32>()?;
            }
            _ => return Err(anyhow!("unknown argument: {}", arg)),
        }
    }
    Ok(Args { dir, phase, epsilon })
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let graph = load_dir(&args.dir)?;
    match args.phase.as_str() {
        "all" => {
            let (dup_res, (inv_report, anom_report)) = rayon::join(
                || find_duplicates(&graph, args.epsilon),
                || rayon::join(|| analyze_invariants(&graph), || analyze_anomalies(&graph)),
            );
            let dup_report = dup_res?;
            let refactoring = analyze_refactoring(&graph, &dup_report);
            write_json(&args.dir, "semantic_duplicates.json", &dup_report)?;
            write_json(&args.dir, "invariants.json", &inv_report)?;
            write_json(&args.dir, "anomalies.json", &anom_report)?;
            write_json(&args.dir, "refactoring_candidates.json", &refactoring)?;
        }
        "duplicates" => {
            let dup_report = find_duplicates(&graph, args.epsilon)?;
            write_json(&args.dir, "semantic_duplicates.json", &dup_report)?;
        }
        "invariants" => {
            let inv_report = analyze_invariants(&graph);
            write_json(&args.dir, "invariants.json", &inv_report)?;
        }
        "anomalies" => {
            let anom_report = analyze_anomalies(&graph);
            write_json(&args.dir, "anomalies.json", &anom_report)?;
        }
        "refactoring" => {
            let dup_report = find_duplicates(&graph, args.epsilon)?;
            let refactoring = analyze_refactoring(&graph, &dup_report);
            write_json(&args.dir, "semantic_duplicates.json", &dup_report)?;
            write_json(&args.dir, "refactoring_candidates.json", &refactoring)?;
        }
        other => return Err(anyhow!("unknown phase: {}", other)),
    }
    Ok(())
}
