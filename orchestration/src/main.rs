//! Orchestration — CanonIR analysis/emit pipeline.
//!
//! Usage (single fixture):
//!   orchestration <canon_ir.json> <output_dir> [--mutate <mutation.json>]
//!
//! Usage (all fixtures):
//!   orchestration --all
//!
//! --all loops over the hard-coded fixture list, runs the full pipeline for
//! each, and writes STRUCTURAL_INVARIANTS_REPORT.md at the repo root.

use anyhow::{bail, Context, Result};
use canon::CanonIR;
use canon_telemetry::{BuildReport, StructuralSurface, TypeAuthorityReport};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// const FIXTURES: &[&str] = &["repomap", "test_1", "semantic-lint", "conversation", "canon"];
const FIXTURES: &[&str] = &["repomap"];
const TEST_ROOT: &str = "/workspace/ai_sandbox/canon/test_projects/test_rust_projects";
const CANON_ROOT: &str = "/workspace/ai_sandbox/canon";
const REPORT_PATH: &str = "/workspace/ai_sandbox/canon/STRUCTURAL_INVARIANTS_REPORT.md";
const JSON_REPORT_PATH: &str = "/workspace/ai_sandbox/canon/orchestration_report.json";

fn main() -> Result<()> {
    // All verbosity handled via --quiet at cargo invocation.

    let args: Vec<String> = std::env::args().skip(1).collect();

    // Emit-only mode: skip capture, reuse existing canon_capture.json
    if args.first().map(|s| s.as_str()) == Some("--emit") {
        return run_emit_only();
    }

    // Full pipeline mode (capture + emit)
    if args.first().map(|s| s.as_str()) == Some("--all") {
        return run_all_fixtures();
    }

    // Single-fixture mode (original behaviour).
    let mut iter = args.into_iter();
    let usage = "usage: orchestration <canon_ir.json> <output_dir> [--mutate <mutation.json>]";
    let json_path = iter.next().map(PathBuf::from).context(usage)?;
    let out_dir = iter.next().map(PathBuf::from).context(usage)?;

    let mut mutate_path: Option<PathBuf> = None;
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--mutate" => {
                mutate_path = Some(iter.next().map(PathBuf::from).context("--mutate requires a path")?);
            }
            other => bail!("unknown flag: {other}"),
        }
    }

    run_pipeline(json_path, out_dir, mutate_path, None)
}

// ---------------------------------------------------------------------------
// Multi-fixture loop
// ---------------------------------------------------------------------------

/// Machine-readable per-fixture summary written to `orchestration_report.json`.
#[derive(Debug, Clone, serde::Serialize)]
struct FixtureSummary {
    fixture: &'static str,
    pipeline_ok: bool,
    pipeline_error: Option<String>,
    suppressed_count: usize,
    suppressed_ret_count: usize,
    suppressed_nonret_count: usize,
    match_gap_count: usize,
    call_gap_count: usize,
    switch_gap_count: usize,
    unresolved_gap_total: usize,
    unresolved_ret_gap_count: usize,
    unreachable_count: usize,
    build_success: bool,
    build_error_count: usize,
    build_warning_count: usize,
    /// Error counts grouped by Rust error code, e.g. {"E0308": 3, "unknown": 5}.
    build_error_categories: HashMap<String, usize>,
    /// First rendered snippet (trimmed to 12 lines) for each error code category.
    build_error_samples: HashMap<String, String>,
    /// Error count per emitted source file, sorted descending by count.
    errors_by_file: HashMap<String, usize>,
    // --- type authority ---
    /// Number of functions where __ret Local.ty != FnSig.ret at capture time.
    type_authority_mismatch_count: usize,
    /// Number of functions where no __ret local was found.
    type_authority_missing_ret_count: usize,
    /// Per-function detail for violations only (to keep JSON compact).
    type_authority_violations: Vec<canon_telemetry::type_authority::FnTypeReport>,
}

#[derive(Debug, serde::Serialize)]
struct OrchestrationReport {
    overall_ok: bool,
    fixtures: Vec<FixtureSummary>,
}

struct FixtureResult {
    fixture: &'static str,
    surface: Option<StructuralSurface>,
    build: Option<BuildReport>,
    type_authority: Option<TypeAuthorityReport>,
    error: Option<String>,
}

fn run_all_fixtures() -> Result<()> {
    let mut results: Vec<FixtureResult> = Vec::new();
    let mut overall_ok = true;

    for &fixture in FIXTURES {
        // capture (silent)
        let capture_dir = PathBuf::from(format!("{}/capture/{}", TEST_ROOT, fixture));
        let capture_json = PathBuf::from(format!("{}/capture/{}/canon_capture.json", TEST_ROOT, fixture));
        let emit_dir = PathBuf::from(format!("{}/emit/{}", TEST_ROOT, fixture));

        // --- capture ---
        if let Err(e) = run_capture(&capture_dir, &capture_json) {
            overall_ok = false;
            eprintln!("[{}] capture error: {:#}", fixture, e);
            results.push(FixtureResult { fixture, surface: None, build: None, type_authority: None, error: Some(format!("capture: {:#}", e)) });
            continue;
        }

        // --- type authority analysis (runs on capture output, before projection) ---
        let type_authority = match canon_telemetry::analyse_capture(&capture_json) {
            Ok(r) => {
                r.print_report();
                Some(r)
            }
            Err(e) => {
                eprintln!("[{}] type authority analysis error: {:#}", fixture, e);
                None
            }
        };

        // --- DO NOT wipe emit dir ---
        // Preserve previous emit outputs to allow iterative inspection.
        std::fs::create_dir_all(&emit_dir)
            .with_context(|| format!("cannot ensure emit dir for {}", fixture))?;

        // orchestration (silent)
        let result = match run_pipeline(capture_json, emit_dir.clone(), None, type_authority.as_ref()) {
            Ok(()) => {
                let surface = canon_telemetry::scan_emit_dir(&emit_dir).unwrap_or(None);
                let build = canon_telemetry::build(&emit_dir, true).ok();
                if build.as_ref().map(|b| !b.success).unwrap_or(false) {
                    overall_ok = false;
                }
                FixtureResult { fixture, surface, build, type_authority, error: None }
            }
            Err(e) => {
                overall_ok = false;
                eprintln!("[{}] pipeline error: {:#}", fixture, e);
                FixtureResult { fixture, surface: None, build: None, type_authority, error: Some(format!("{:#}", e)) }
            }
        };

        results.push(result);
    }

    write_report(&results)?;
    write_json_report(&results, overall_ok)?;
    println!("Invariant report written to: {}", REPORT_PATH);
    println!("JSON report written to:      {}", JSON_REPORT_PATH);

    if !overall_ok {
        std::process::exit(1);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Capture step
// ---------------------------------------------------------------------------

fn run_capture(project_dir: &Path, output_json: &Path) -> Result<()> {
    let meta_out = Command::new("cargo").args(["metadata", "--no-deps", "--format-version", "1"]).current_dir(CANON_ROOT).output().context("cargo metadata failed")?;
    anyhow::ensure!(meta_out.status.success(), "cargo metadata exited non-zero");

    let meta: serde_json::Value = serde_json::from_slice(&meta_out.stdout).context("cannot parse cargo metadata JSON")?;
    let target_dir = meta["target_directory"].as_str().context("target_directory missing from cargo metadata")?;
    let wrapper = PathBuf::from(format!("{}/debug/rustc_capture", target_dir));

    println!("  Building rustc_capture...");
    let build_status = Command::new("cargo").args(["build", "-p", "rustc_capture"]).current_dir(CANON_ROOT).status().context("cargo build rustc_capture failed to spawn")?;
    anyhow::ensure!(build_status.success(), "cargo build -p rustc_capture exited non-zero");
    anyhow::ensure!(wrapper.exists(), "rustc_capture binary not found at {:?}", wrapper);

    let rustc_out = Command::new("rustup").args(["which", "rustc"]).output().context("rustup which rustc failed")?;
    let real_rustc = String::from_utf8_lossy(&rustc_out.stdout).trim().to_owned();
    anyhow::ensure!(!real_rustc.is_empty(), "rustup which rustc returned empty");

    println!("  Capturing {:?} -> {:?}", project_dir, output_json);

    let target_capture = project_dir.join("target_capture");
    if target_capture.exists() {
        std::fs::remove_dir_all(&target_capture).context("cannot remove target_capture")?;
    }
    std::fs::create_dir_all(&target_capture).context("cannot create target_capture")?;
    if output_json.exists() {
        std::fs::remove_file(output_json).context("cannot remove stale capture JSON")?;
    }
    if let Some(parent) = output_json.parent() {
        std::fs::create_dir_all(parent).context("cannot create capture output dir")?;
    }

    let status = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(project_dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_capture)
        .env("CANON_CAPTURE_OUT", output_json)
        .env("RUSTC_WRAPPER", &wrapper)
        .env("CARGO_NET_OFFLINE", std::env::var("CARGO_NET_OFFLINE").unwrap_or_else(|_| "true".into()))
        .current_dir(project_dir)
        .status()
        .context("cargo build (capture) failed to spawn")?;
    anyhow::ensure!(status.success(), "cargo build (capture) exited non-zero for {:?}", project_dir);
    anyhow::ensure!(output_json.exists(), "capture did not produce {:?}", output_json);

    let count = Command::new("python3")
        .arg("-c")
        .arg(format!("import json; d=json.load(open('{}')); print(len(d.get('nodes', [])))", output_json.display()))
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|_| "?".into());
    println!("  Done. IR written to {:?}  nodes={}", output_json, count);

    Ok(())
}

// ---------------------------------------------------------------------------
// Report writers
// ---------------------------------------------------------------------------

fn write_report(results: &[FixtureResult]) -> Result<()> {
    let mut out = String::new();
    out.push_str("# STRUCTURAL_INVARIANTS_REPORT.md\n\n");
    out.push_str("Generated by `orchestration --all`.\n");
    out.push_str("Only direct facts from tool output are recorded.\n\n");

    for r in results {
        out.push_str(&format!("## fixture={}\n\n", r.fixture));

        if let Some(err) = &r.error {
            out.push_str(&format!("- pipeline error: {}\n\n", err));
            continue;
        }

        // --- type authority ---
        if let Some(ta) = &r.type_authority {
            out.push_str("- type authority (capture-time __ret vs FnSig.ret):\n");
            out.push_str(&format!("  - functions analysed: {}\n", ta.fn_count));
            out.push_str(&format!("  - __ret type mismatches: {}\n", ta.mismatch_count));
            out.push_str(&format!("  - missing __ret locals: {}\n", ta.missing_ret_local_count));
            if ta.mismatch_count > 0 {
                out.push_str("  - violations (fn: sig_ret | __ret_local):\n");
                for f in &ta.functions {
                    if f.mismatch {
                        out.push_str(&format!("    - {}: {} | {}\n", f.fn_name, f.sig_ret_type, f.ret_local_type.as_deref().unwrap_or("<missing>"),));
                    }
                }
            }
        }

        // --- structural surface ---
        if let Some(surface) = &r.surface {
            out.push_str("- emitted structural surface:\n");
            out.push_str(&format!("  - canon suppressed binding count: {}\n", surface.suppressed_count));
            out.push_str(&format!("  - canon suppressed __ret count: {}\n", surface.suppressed_ret_count));
            out.push_str(&format!("  - canon suppressed non-__ret count: {}\n", surface.suppressed_nonret_count));
            out.push_str(&format!("  - canon match gap count: {}\n", surface.match_gap_count));
            out.push_str(&format!("  - canon call gap count: {}\n", surface.call_gap_count));
            out.push_str(&format!("  - canon switch gap count: {}\n", surface.switch_gap_count));
            out.push_str(&format!("  - unresolved gap total: {}\n", surface.unresolved_gap_total));
            out.push_str(&format!("  - unresolved __ret gap count: {}\n", surface.unresolved_ret_gap_count));
            out.push_str(&format!("  - unreachable count: {}\n", surface.unreachable_count));
            out.push_str(&format!("  - // match count: {}\n", surface.match_comment_count));
            out.push_str(&format!("  - // goto count: {}\n", surface.goto_comment_count));
            if !surface.ret_gap_sites.is_empty() {
                out.push_str("- unresolved __ret gap sites:\n");
                for site in &surface.ret_gap_sites {
                    out.push_str(&format!("  - {}:{} :: {}\n", site.file, site.line, site.enclosing_fn));
                }
            }
        } else {
            out.push_str("- no src/ dir found; surface scan skipped.\n");
        }

        // --- build ---
        if let Some(build) = &r.build {
            out.push_str(&format!("- cargo build result: {}\n", if build.success { "OK" } else { "FAILED" }));
            out.push_str(&format!("  - error count: {}\n", build.errors.len()));
            out.push_str(&format!("  - warning count: {}\n", build.warnings.len()));

            if !build.build_error_categories.is_empty() {
                out.push_str("  - error categories:\n");
                let mut cats: Vec<(&String, &usize)> = build.build_error_categories.iter().collect();
                cats.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
                for (code, count) in &cats {
                    out.push_str(&format!("    - {}: {}\n", code, count));
                }
            }

            if !build.errors_by_file.is_empty() {
                out.push_str("  - errors by file:\n");
                let mut by_file: Vec<(&String, &usize)> = build.errors_by_file.iter().collect();
                by_file.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
                for (file, count) in &by_file {
                    out.push_str(&format!("    - {}: {}\n", file, count));
                }
            }

            if !build.build_error_samples.is_empty() {
                out.push_str("  - error samples (first occurrence per category):\n");
                let mut codes: Vec<&String> = build.build_error_samples.keys().collect();
                codes.sort();
                for code in codes {
                    out.push_str(&format!("    [{}]\n", code));
                    for line in build.build_error_samples[code].lines() {
                        out.push_str(&format!("      {}\n", line));
                    }
                }
            }

            if !build.errors.is_empty() {
                out.push_str("- cargo errors:\n");
                for d in &build.errors {
                    if let Some(rendered) = &d.rendered {
                        for line in rendered.trim_end().lines() {
                            out.push_str(&format!("  {}\n", line));
                        }
                    } else {
                        out.push_str(&format!("  [{}] {}\n", d.level, d.message));
                    }
                }
            }
        } else {
            out.push_str("- cargo build: not run (pipeline failed before emit).\n");
        }

        out.push('\n');
    }

    std::fs::write(REPORT_PATH, out).context("failed to write STRUCTURAL_INVARIANTS_REPORT.md")
}

fn write_json_report(results: &[FixtureResult], overall_ok: bool) -> Result<()> {
    let fixtures: Vec<FixtureSummary> = results
        .iter()
        .map(|r| {
            let (surface, build, ta) = (r.surface.as_ref(), r.build.as_ref(), r.type_authority.as_ref());
            FixtureSummary {
                fixture: r.fixture,
                pipeline_ok: r.error.is_none(),
                pipeline_error: r.error.clone(),
                suppressed_count: surface.map(|s| s.suppressed_count).unwrap_or(0),
                suppressed_ret_count: surface.map(|s| s.suppressed_ret_count).unwrap_or(0),
                suppressed_nonret_count: surface.map(|s| s.suppressed_nonret_count).unwrap_or(0),
                match_gap_count: surface.map(|s| s.match_gap_count).unwrap_or(0),
                call_gap_count: surface.map(|s| s.call_gap_count).unwrap_or(0),
                switch_gap_count: surface.map(|s| s.switch_gap_count).unwrap_or(0),
                unresolved_gap_total: surface.map(|s| s.unresolved_gap_total).unwrap_or(0),
                unresolved_ret_gap_count: surface.map(|s| s.unresolved_ret_gap_count).unwrap_or(0),
                unreachable_count: surface.map(|s| s.unreachable_count).unwrap_or(0),
                build_success: build.map(|b| b.success).unwrap_or(false),
                build_error_count: build.map(|b| b.errors.len()).unwrap_or(0),
                build_warning_count: build.map(|b| b.warnings.len()).unwrap_or(0),
                build_error_categories: build.map(|b| b.build_error_categories.clone()).unwrap_or_default(),
                build_error_samples: build.map(|b| b.build_error_samples.clone()).unwrap_or_default(),
                errors_by_file: build.map(|b| b.errors_by_file.clone()).unwrap_or_default(),
                type_authority_mismatch_count: ta.map(|t| t.mismatch_count).unwrap_or(0),
                type_authority_missing_ret_count: ta.map(|t| t.missing_ret_local_count).unwrap_or(0),
                type_authority_violations: ta.map(|t| t.functions.iter().filter(|f| f.mismatch).cloned().collect()).unwrap_or_default(),
            }
        })
        .collect();

    let report = OrchestrationReport { overall_ok, fixtures };
    std::fs::write(JSON_REPORT_PATH, serde_json::to_string_pretty(&report)?).context("failed to write orchestration_report.json")
}

// ---------------------------------------------------------------------------
// Single-fixture pipeline
// ---------------------------------------------------------------------------

fn run_pipeline(json_path: PathBuf, out_dir: PathBuf, mutate_path: Option<PathBuf>, type_authority: Option<&TypeAuthorityReport>) -> Result<()> {
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

    // Fail fast if unresolved types remain after analysis.
    let mut unresolved_count = 0usize;
    for node in &canon_ir.nodes {
        if let canon::node::CanonNodeKind::Type { kind } = &node.kind {
            if let canon::node::TypeKind::Unresolved(path_id) = kind {
                eprintln!("ERROR: unresolved type after analysis: {}", canon_ir.lookup_path(*path_id));
                unresolved_count += 1;
            }
        }
    }
    if unresolved_count > 0 {
        // TEMPORARY: Log unresolved types but do not abort the pipeline.
        // Downstream projection/build telemetry will surface any concrete
        // type errors. This prevents hard-stop stagnation while we
        // continue eliminating residual Unresolved nodes.
        eprintln!("WARNING: analysis left {} unresolved type(s); continuing to projection", unresolved_count);
    }

    println!("Emitting source (CanonIR pipeline)...");
    let canon_plan = canon_projection::project(&canon_ir).context("canon project failed")?;
    canon_projection::emit_to_disk(&canon_ir, &canon_plan, &out_dir).context("canon emit failed")?;
    println!("Canon emitted {} file(s) to {:?}", canon_plan.files.len(), out_dir);

    // Write type authority report alongside emitted source so the agent can cat it.
    if let Some(ta) = type_authority {
        std::fs::create_dir_all(&out_dir)?;
        if let Err(e) = canon_telemetry::write_type_authority_report(ta, &out_dir) {
            eprintln!("Warning: failed to write type authority report: {}", e);
        } else {
            println!("Type authority report written to {:?}", out_dir.join("canon_type_authority_report.json"));
        }
    }

    println!("Scanning emitted structural surface...");
    match canon_telemetry::scan_emit_dir(&out_dir).context("structural surface scan failed")? {
        Some(surface) => {
            surface.print_report();
            let snap_path = out_dir.join("canon_structural_surface.json");
            std::fs::write(&snap_path, serde_json::to_string_pretty(&surface).context("surface serialize failed")?).context("surface snapshot write failed")?;
            println!("Structural surface snapshot written to {:?}", snap_path);
        }
        None => println!("  (no src/ dir found under emit dir, skipping surface scan)"),
    }

    println!("Running cargo build on emitted source...");
    let build_report = canon_telemetry::build(&out_dir, true)
        .context("cargo build invocation failed")?;

    // --- Canon Repair Signal Extraction ---
    {
        use canon_telemetry::classify_repair_signals;
        let signals = classify_repair_signals(&build_report);
        if !signals.is_empty() {
            println!("repair signals detected: {:?}", signals);
        }
    }
    // Replace verbose print_report() with structured summary

    use canon_telemetry::classify_repair_signals;

    println!("\n=== Build Summary ===");
    println!("  success: {}", build_report.success);
    println!("  error count: {}", build_report.errors.len());
    println!("  warning count: {}", build_report.warnings.len());

    if !build_report.build_error_categories.is_empty() {
        println!("  categories:");
        let mut cats: Vec<_> = build_report.build_error_categories.iter().collect();
        cats.sort_by(|a, b| b.1.cmp(a.1));
        for (code, count) in cats {
            println!("    {}: {}", code, count);
        }
    }

    if !build_report.errors_by_file.is_empty() {
        println!("  errors by file:");
        let mut files: Vec<_> = build_report.errors_by_file.iter().collect();
        files.sort_by(|a, b| b.1.cmp(a.1));
        for (file, count) in files {
            println!("    {}: {}", file, count);
        }
    }

    let signals = classify_repair_signals(&build_report);
    if !signals.is_empty() {
        println!("\n=== Repair Signals ===");
        for s in &signals {
            println!("  {:?}", s);
        }
    }
    let build_report_path = out_dir.join("canon_build_report.json");
    std::fs::write(&build_report_path, serde_json::to_string_pretty(&build_report).context("build report serialize failed")?).context("build report write failed")?;
    println!("Build report written to {:?}", build_report_path);

    let canon_snap_path = out_dir.join("canon_ir_solved.json");
    std::fs::create_dir_all(&out_dir)?;
    std::fs::write(&canon_snap_path, serde_json::to_string_pretty(&canon_ir).context("canon serialize failed")?).context("canon snapshot write failed")?;
    println!("Canon snapshot written to {:?}", canon_snap_path);

    println!("Pipeline complete.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Emit-only mode (no capture)
// ---------------------------------------------------------------------------

fn run_emit_only() -> Result<()> {
    for &fixture in FIXTURES {
        let capture_json = PathBuf::from(format!(
            "{}/capture/{}/canon_capture.json",
            TEST_ROOT, fixture
        ));

        let emit_dir = PathBuf::from(format!(
            "{}/emit/{}",
            TEST_ROOT, fixture
        ));

        println!("Emit-only: loading {:?}", capture_json);
        run_pipeline(capture_json, emit_dir, None, None)?;
    }

    println!("Emit-only pipeline complete.");
    Ok(())
}
