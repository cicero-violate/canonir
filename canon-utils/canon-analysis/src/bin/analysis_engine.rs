use anyhow::{anyhow, Result};
use std::env;
use std::fs;
use std::path::PathBuf;

use canon_analysis::smt::anomalies::analyze_anomalies;
use canon_analysis::smt::duplicates::find_duplicates;
use canon_analysis::smt::emit::write_json;
use canon_analysis::smt::invariants::analyze_invariants;
use canon_analysis::smt::loader::load_dir;
use canon_analysis::smt::refactoring::analyze_refactoring;
use canon_analysis::smt::augment::augment_with_errors;
use canon_analysis::smt::smt::equivalence::check_equivalence;
use canon_analysis::smt::smt::invariants::prove_invariants;
use canon_analysis::smt::smt::reachability::check_repair_surface;
use canon_analysis::smt::smt::repair::build_repair_surface_smt;
use canon_analysis::smt::smt::SmtSession;
use canon_types::ReportLayout;

#[derive(Debug)]
struct Args {
    dir: PathBuf,
    out_dir: PathBuf,
    phase: String,
    epsilon: f32,
    clear_cache: bool,
    #[allow(dead_code)]
    dir_mode: bool,
    crate_name: Option<String>,
    reports_root: Option<PathBuf>,
    crate_root: Option<PathBuf>,
    workspace_mode: bool,
}

fn parse_args() -> Result<Args> {
    let mut dir = PathBuf::from("analysis");
    let mut out_dir: Option<PathBuf> = None;
    let mut phase = String::from("all");
    let mut epsilon = 0.1f32;
    let mut clear_cache = false;
    let mut dir_mode = false;
    let mut crate_name: Option<String> = None;
    let mut reports_root: Option<PathBuf> = None;
    let mut crate_root: Option<PathBuf> = None;
    let mut workspace_mode = false;
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
            "--crate-name" => {
                crate_name = args.next();
            }
            "--reports-root" => {
                let value = args.next().ok_or_else(|| anyhow!("--reports-root requires a path"))?;
                reports_root = Some(PathBuf::from(value));
            }
            "--crate-root" => {
                let value = args.next().ok_or_else(|| anyhow!("--crate-root requires a path"))?;
                crate_root = Some(PathBuf::from(value));
            }
            "--workspace" => {
                workspace_mode = true;
            }
            _ => return Err(anyhow!("unknown argument: {}", arg)),
        }
    }
    let out_dir = out_dir.unwrap_or_else(|| dir.join("post_analysis"));
    Ok(Args {
        dir,
        out_dir,
        phase,
        epsilon,
        clear_cache,
        dir_mode,
        crate_name,
        reports_root,
        crate_root,
        workspace_mode,
    })
}

fn main() -> Result<()> {
    let mut args = parse_args()?;
    if let Some(crate_root) = args.crate_root.clone() {
        let layout = ReportLayout::from_crate_root(crate_root);
        args.dir = layout.graph_dir();
        args.out_dir = layout.analysis_dir();
    } else if let (Some(root), Some(name)) = (args.reports_root.clone(), args.crate_name.clone()) {
        let layout = ReportLayout::from_workspace_root(root, name);
        args.dir = layout.graph_dir();
        args.out_dir = layout.analysis_dir();
    } else if args.workspace_mode {
        let root = args
            .reports_root
            .clone()
            .unwrap_or_else(|| PathBuf::from("state/reports_out"));
        let layout = ReportLayout::from_workspace_root(root, "workspace".to_string());
        args.dir = layout.workspace_root();
        args.out_dir = layout.workspace_root().join("analysis");
    }

    fs::create_dir_all(&args.out_dir)?;
    let errors_json = args.dir.join("errors.json");
    if errors_json.exists() {
        augment_with_errors(&args.dir, &errors_json, &args.out_dir)?;
    }
    let graph = load_dir(&args.dir)?;
    match args.phase.as_str() {
        "all" => {
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
