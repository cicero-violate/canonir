use anyhow::{anyhow, Result};
use std::env;
use std::fs;
use std::path::PathBuf;

use analysis_engine::anomalies::analyze_anomalies;
use analysis_engine::duplicates::find_duplicates;
use analysis_engine::emit::write_json;
use analysis_engine::invariants::analyze_invariants;
use analysis_engine::loader::load_dir;
use analysis_engine::refactoring::analyze_refactoring;
use analysis_engine::augment::augment_with_errors;
use analysis_engine::smt::equivalence::check_equivalence;
use analysis_engine::smt::invariants::prove_invariants;
use analysis_engine::smt::reachability::check_repair_surface;
use analysis_engine::smt::repair::build_repair_surface_smt;
use analysis_engine::smt::SmtSession;

#[derive(Debug)]
struct Args {
    dir: PathBuf,
    out_dir: PathBuf,
    phase: String,
    epsilon: f32,
    clear_cache: bool,
    #[allow(dead_code)]
    dir_mode: bool,
}

fn parse_args() -> Result<Args> {
    let mut dir = PathBuf::from("analysis");
    let mut out_dir: Option<PathBuf> = None;
    let mut phase = String::from("all");
    let mut epsilon = 0.1f32;
    let mut clear_cache = false;
    let mut dir_mode = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => {
                let value = args.next().ok_or_else(|| anyhow!("--dir requires a path"))?;
                dir = PathBuf::from(value);
            }
            "--out" => {
                let value = args.next().ok_or_else(|| anyhow!("--out requires a path"))?;
                out_dir = Some(PathBuf::from(value));
            }
            "--phase" => {
                phase = args.next().ok_or_else(|| anyhow!("--phase requires a value"))?;
            }
            "--epsilon" => {
                let raw = args.next().ok_or_else(|| anyhow!("--epsilon requires a value"))?;
                epsilon = raw.parse::<f32>()?;
            }
            "--clear-cache" => {
                clear_cache = true;
            }
            "--dir-mode" => {
                dir_mode = true;
            }
            _ => return Err(anyhow!("unknown argument: {}", arg)),
        }
    }
    let out_dir = out_dir.unwrap_or_else(|| dir.join("post_analysis"));
    Ok(Args { dir, out_dir, phase, epsilon, clear_cache, dir_mode })
}

fn main() -> Result<()> {
    let args = parse_args()?;
    fs::create_dir_all(&args.out_dir)?;
    let errors_json = args.dir.join("errors.json");
    if errors_json.exists() {
        augment_with_errors(&args.dir, &errors_json, &args.out_dir)?;
    }
    let graph = load_dir(&args.dir)?;
    match args.phase.as_str() {
        "all" => {
            generate_reports(&args.dir, &args.out_dir)?;
            let (dup_res, (inv_report, anom_report)) = rayon::join(
                || find_duplicates(&graph, args.epsilon),
                || rayon::join(|| analyze_invariants(&graph), || analyze_anomalies(&graph)),
            );
            let dup_report = dup_res?;
            let refactoring = analyze_refactoring(&graph, &dup_report);
            let cache_path = args.out_dir.join("smt_cache.json");
            let session = SmtSession::new(5000, cache_path, args.clear_cache);
            let reachability = check_repair_surface(&session, &graph, &graph.repair_surface);
            let inv_value = {
                let inv_value = serde_json::to_value(&inv_report).unwrap_or(serde_json::Value::Null);
                prove_invariants(&session, &graph, &inv_value)
            };
            let mut ref_value = serde_json::to_value(&refactoring).unwrap_or(serde_json::Value::Null);
            let eq = check_equivalence(&session, &graph, &ref_value);
            if let Some(obj) = ref_value.as_object_mut() {
                obj.insert("smt_equivalence".to_string(), serde_json::to_value(eq).unwrap_or(serde_json::Value::Null));
            }
            write_json(&args.out_dir, "semantic_duplicates.json", &dup_report)?;
            write_json(&args.out_dir, "invariants.json", &inv_value)?;
            write_json(&args.out_dir, "anomalies.json", &anom_report)?;
            write_json(&args.out_dir, "refactoring_candidates.json", &ref_value)?;
            if !graph.repair_surface.is_null() {
                let smt_surface = build_repair_surface_smt(&graph.repair_surface, &reachability);
                write_json(&args.out_dir, "repair_surface_smt.json", &smt_surface)?;
            }
        }
        "reachability" => {
            let cache_path = args.out_dir.join("smt_cache.json");
            let session = SmtSession::new(5000, cache_path, args.clear_cache);
            let reachability = check_repair_surface(&session, &graph, &graph.repair_surface);
            if !graph.repair_surface.is_null() {
                let smt_surface = build_repair_surface_smt(&graph.repair_surface, &reachability);
                write_json(&args.out_dir, "repair_surface_smt.json", &smt_surface)?;
            }
        }
        "duplicates" => {
            let dup_report = find_duplicates(&graph, args.epsilon)?;
            write_json(&args.out_dir, "semantic_duplicates.json", &dup_report)?;
        }
        "invariants" => {
            let inv_report = analyze_invariants(&graph);
            write_json(&args.out_dir, "invariants.json", &inv_report)?;
        }
        "anomalies" => {
            let anom_report = analyze_anomalies(&graph);
            write_json(&args.out_dir, "anomalies.json", &anom_report)?;
        }
        "refactoring" => {
            let dup_report = find_duplicates(&graph, args.epsilon)?;
            let refactoring = analyze_refactoring(&graph, &dup_report);
            write_json(&args.out_dir, "semantic_duplicates.json", &dup_report)?;
            write_json(&args.out_dir, "refactoring_candidates.json", &refactoring)?;
        }
        other => return Err(anyhow!("unknown phase: {}", other)),
    }
    Ok(())
}
use analysis_engine::reports::generate_reports;
