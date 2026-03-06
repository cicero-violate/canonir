use canon::csr_graph::CsrGraph;
use canon::edge::EdgeKind;
use canon::node::{CanonId, CanonNodeKind};
use canon::CanonIR;
use canon_analyzer::solver::constraint_solver::build_problem;
use canon_analyzer::solver::search_optimizer_solver::optimize;
use rename::core::oracle::StructuralEditOracle;
use rename::core::project_editor::ProjectEditor;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let project = Path::new("/workspace/ai_sandbox/canon/canon-agent");
    let project_name = project.file_name().and_then(|n| n.to_str()).unwrap_or("canon-agent-v1").to_string();
    let renames_md = "/workspace/ai_sandbox/canon/canon-agent/src/pipelines/capability/RENAMES.md";
    let report_path = "/workspace/ai_sandbox/canon/canon-utils/rename/rename_report.jsonl";
    let run_id = format!("run-{}", now_unix_secs());
    let run_started_at_unix = now_unix_secs();
    let run_started_at = Instant::now();
    let baseline_commit = git_head_commit(project)?;
    let baseline_check = run_cargo_check_json(project)?;
    let mut baseline_error_counts = BTreeMap::new();
    accumulate_error_counts_json(&baseline_check.diagnostics, &mut baseline_error_counts);
    let baseline_error_total = sum_counts(&baseline_error_counts);

    let ordered = parse_simple_ident_mappings(renames_md)?;
    let offset = std::env::var("RENAME_OFFSET").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let limit = std::env::var("RENAME_LIMIT").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(usize::MAX);
    let bounded: Vec<(String, String)> = ordered.into_iter().skip(offset).take(limit).collect();
    let solver_plan = build_solver_plan(&bounded);

    append_report_line(
        report_path,
        &json!({
            "type": "run_start",
            "run_id": run_id,
            "project": project_name,
            "baseline": {
                "state": "S0",
                "commit": baseline_commit,
                "error_types": baseline_error_counts,
                "error_total": baseline_error_total
            },
            "config": {
                "offset": offset,
                "limit": if limit == usize::MAX { serde_json::Value::Null } else { json!(limit) },
                "runner": "rename_self",
                "compile_cmd": "cargo check"
            },
            "started_at_unix": run_started_at_unix,
            "ts": now_iso_utc()
        }),
    )?;

    append_report_line(
        report_path,
        &json!({
            "type": "solver_summary",
            "run_id": run_id,
            "solver": {
                "input_total": solver_plan.input_total,
                "transform_total": solver_plan.transform_total,
                "dependency_count": solver_plan.dependency_count,
                "conflict_count": solver_plan.conflict_count,
                "cyclic_component_count": solver_plan.cyclic_component_count,
                "sat_selected_total": solver_plan.sat_selected_total,
                "selected_total": solver_plan.selected_total
            },
            "ts": now_iso_utc()
        }),
    )?;

    let mut introduced_summary: BTreeMap<String, usize> = BTreeMap::new();
    let mut kind_stats: BTreeMap<String, KindStats> = BTreeMap::new();
    let mut total_attempts = 0usize;
    let mut total_pass = 0usize;
    let mut total_fail = 0usize;
    let mut total_skipped = 0usize;
    let mut attempt_id = 0usize;

    for (old_ident, new_ident) in solver_plan.selected_pairs {
        if is_degenerate_rename(&old_ident, &new_ident) {
            total_skipped += 1;
            attempt_id += 1;
            append_report_line(
                report_path,
                &json!({
                    "type": "attempt",
                    "run_id": run_id,
                    "attempt_id": attempt_id,
                    "transform": {
                        "kind": "rename",
                        "transform_type": "symbol_rename",
                        "symbol_kind": "unknown",
                        "state_from": "S0",
                        "state_to": format!("S{}", attempt_id),
                        "symbol_id": old_ident,
                        "old_name": old_ident,
                        "new_name": new_ident,
                        "rename_applied": false,
                        "touched_files": []
                    },
                    "compile": {
                        "status": "skipped",
                        "invoked": false,
                        "error_types_after": {},
                        "error_total_after": 0
                    },
                    "delta": {
                        "delta_error_types": {},
                        "delta_total": 0
                    },
                    "decision": {
                        "accept": false,
                        "reason": "invalid_transform_name"
                    },
                    "timing_ms": {
                        "transform": 0,
                        "compile": 0
                    },
                    "ts": now_iso_utc()
                }),
            )?;
            update_kind_stats(&mut kind_stats, "unknown", false, 0);
            continue;
        }

        let symbol_ids = load_symbol_ids(project)?;
        let mut candidates: Vec<(String, String)> = symbol_ids.into_iter().filter(|(id, _)| id.ends_with(&format!("::{old_ident}"))).collect();
        candidates.sort();

        if candidates.is_empty() {
            total_skipped += 1;
            attempt_id += 1;
            append_report_line(
                report_path,
                &json!({
                    "type": "attempt",
                    "run_id": run_id,
                    "attempt_id": attempt_id,
                    "transform": {
                        "kind": "rename",
                        "transform_type": "symbol_rename",
                        "symbol_kind": "unknown",
                        "state_from": "S0",
                        "state_to": format!("S{}", attempt_id),
                        "symbol_id": old_ident,
                        "old_name": old_ident,
                        "new_name": new_ident,
                        "rename_applied": false,
                        "touched_files": []
                    },
                    "compile": {
                        "status": "skipped",
                        "invoked": false,
                        "error_types_after": {},
                        "error_total_after": 0
                    },
                    "delta": {
                        "delta_error_types": {},
                        "delta_total": 0
                    },
                    "decision": {
                        "accept": false,
                        "reason": "missing_symbol"
                    },
                    "timing_ms": {
                        "transform": 0,
                        "compile": 0
                    },
                    "ts": now_iso_utc()
                }),
            )?;
            update_kind_stats(&mut kind_stats, "unknown", false, 0);
            continue;
        }

        for (old_symbol, symbol_kind) in candidates {
            let prefix = old_symbol.rsplit_once("::").map(|(p, _)| p).unwrap_or("");
            let new_symbol = if prefix.is_empty() { new_ident.clone() } else { format!("{prefix}::{new_ident}") };
            total_attempts += 1;
            attempt_id += 1;

            // Keep each attempt independent from baseline source state.
            let _ = restore_project_src(project);
            let outcome = run_incremental_attempt(project, &old_symbol, &new_symbol, &baseline_error_counts)?;
            let _ = restore_project_src(project);

            match outcome.result.as_str() {
                "pass" => total_pass += 1,
                "fail" => total_fail += 1,
                _ => {}
            }
            merge_counts(&outcome.error_types, &mut introduced_summary);
            update_kind_stats(&mut kind_stats, &symbol_kind, outcome.accept, outcome.delta_total.max(0) as usize);

            append_report_line(
                report_path,
                &json!({
                    "type": "attempt",
                    "run_id": run_id,
                    "attempt_id": attempt_id,
                    "transform": {
                        "kind": "rename",
                        "transform_type": "symbol_rename",
                        "symbol_kind": symbol_kind,
                        "state_from": "S0",
                        "state_to": format!("S{}", attempt_id),
                        "symbol_id": old_symbol.as_str(),
                        "old_name": old_symbol.rsplit_once("::").map(|(_, s)| s).unwrap_or(old_symbol.as_str()),
                        "new_name": new_symbol.rsplit_once("::").map(|(_, s)| s).unwrap_or(new_symbol.as_str()),
                        "rename_applied": outcome.rename_applied,
                        "touched_files": outcome.touched_files
                    },
                    "compile": {
                        "status": if outcome.compile_invoked { "finished" } else { "skipped" },
                        "invoked": outcome.compile_invoked,
                        "error_types_after": outcome.error_types_after,
                        "error_total_after": outcome.error_total_after,
                        "messages": outcome.compile_messages
                    },
                    "delta": {
                        "introduced_errors": outcome.error_types,
                        "delta_total": outcome.delta_total
                    },
                    "decision": {
                        "accept": outcome.accept,
                        "reason": outcome.decision_reason
                    },
                    "timing_ms": {
                        "transform": outcome.transform_ms,
                        "compile": outcome.compile_ms
                    },
                    "ts": now_iso_utc()
                }),
            )?;
        }
    }

    for (symbol_kind, stats) in &kind_stats {
        let attempts = stats.attempts as f64;
        let success_rate = if stats.attempts == 0 { 0.0 } else { stats.accepted as f64 / attempts };
        let error_rate = if stats.attempts == 0 { 0.0 } else { stats.introduced_errors as f64 / attempts };
        append_report_line(
            report_path,
            &json!({
                "type": "transform_stats",
                "run_id": run_id,
                "symbol_kind": symbol_kind,
                "attempts": stats.attempts,
                "accepted": stats.accepted,
                "introduced_errors": stats.introduced_errors,
                "success_rate": round3(success_rate),
                "score": round3(success_rate - error_rate),
                "ts": now_iso_utc()
            }),
        )?;
    }

    let final_check = run_cargo_check_json(project)?;

    append_report_line(
        report_path,
        &json!({
            "type": "run_summary",
            "run_id": run_id,
            "stats": {
                "attempted": total_attempts,
                "passed": total_pass,
                "failed": total_fail,
                "skipped": total_skipped
            },
            "introduced_error_types": introduced_summary,
            "final_compile": {
                "status": if final_check.ok { "ok" } else { "failed" }
            },
            "started_at_unix": run_started_at_unix,
            "finished_at_unix": now_unix_secs(),
            "duration_ms": run_started_at.elapsed().as_millis(),
            "ts": now_iso_utc()
        }),
    )?;

    println!("status: {}", if final_check.ok { "ok" } else { "failed" });
    println!("report: {}", report_path);
    Ok(())
}

#[derive(Default)]
struct AttemptOutcome {
    result: String,
    accept: bool,
    error_types: BTreeMap<String, usize>,
    error_types_after: BTreeMap<String, usize>,
    error_total_after: usize,
    compile_messages: Vec<serde_json::Value>,
    delta_total: i64,
    decision_reason: String,
    touched_files: Vec<String>,
    transform_ms: u128,
    compile_ms: u128,
    compile_invoked: bool,
    rename_applied: bool,
    rename_error: Option<String>,
}

#[derive(Default)]
struct KindStats {
    attempts: usize,
    accepted: usize,
    introduced_errors: usize,
}

fn run_incremental_attempt(project: &Path, old_symbol: &str, new_symbol: &str, baseline_error_counts: &BTreeMap<String, usize>) -> Result<AttemptOutcome, Box<dyn std::error::Error>> {
    let transform_start = Instant::now();
    let mut outcome = AttemptOutcome { result: "fail".to_string(), rename_applied: false, decision_reason: "introduced_errors".to_string(), ..Default::default() };

    let rename_report = rename::rename_symbol_pairs(project, &[(old_symbol.to_string(), new_symbol.to_string())]);

    if let Some(err) = rename_report.error {
        outcome.rename_error = Some(err);
        outcome.decision_reason = "rename_error".to_string();
        outcome.transform_ms = transform_start.elapsed().as_millis();
        return Ok(outcome);
    }

    outcome.transform_ms = transform_start.elapsed().as_millis();
    outcome.rename_applied = src_has_diff(project)?;
    outcome.touched_files = touched_src_files(project)?;
    if !outcome.rename_applied {
        outcome.decision_reason = "rename_not_applied".to_string();
        return Ok(outcome);
    }

    let compile_start = Instant::now();
    let check = run_cargo_check_json(project)?;
    outcome.compile_ms = compile_start.elapsed().as_millis();
    outcome.compile_invoked = true;
    outcome.compile_messages = check.diagnostics.clone();
    accumulate_error_counts_json(&check.diagnostics, &mut outcome.error_types_after);
    outcome.error_total_after = sum_counts(&outcome.error_types_after);
    let delta_error_types = compute_delta_error_counts(baseline_error_counts, &outcome.error_types_after);

    let mut introduced = BTreeMap::new();
    let mut delta_total = 0i64;
    for (k, v) in &delta_error_types {
        if *v > 0 {
            introduced.insert(k.clone(), *v as usize);
            delta_total += *v;
        }
    }
    outcome.error_types = introduced;
    outcome.delta_total = delta_total;
    outcome.accept = outcome.rename_applied && outcome.delta_total == 0;
    if outcome.accept {
        outcome.decision_reason = "accepted".to_string();
    } else {
        outcome.decision_reason = "introduced_errors".to_string();
    }

    if outcome.accept {
        outcome.result = "pass".to_string();
    } else {
        outcome.result = "fail".to_string();
    }

    Ok(outcome)
}

fn restore_project_src(project: &Path) -> bool {
    run_cmd(project, "git", &["restore", "--source=HEAD", "--worktree", "--staged", "src"])
}

fn parse_simple_ident_mappings(path: &str) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cols: Vec<&str> = trimmed.split('|').map(str::trim).collect();
        if cols.len() < 4 {
            continue;
        }
        let old = cols[1].trim_matches('`');
        let new = cols[2].trim_matches('`');
        if old.is_empty() || new.is_empty() || old == "Old Name" || old == "---" || new == "---" {
            continue;
        }
        if old.contains("::") || new.contains("::") {
            continue;
        }
        if is_ident(old) && is_ident(new) {
            out.push((old.to_string(), new.to_string()));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn is_ident(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn load_symbol_ids(project: &Path) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let editor = ProjectEditor::load(project, Box::new(StructuralEditOracle))?;
    Ok(editor.symbol_catalog())
}

fn run_cmd(project: &Path, cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).current_dir(project).status().map(|s| s.success()).unwrap_or(false)
}

struct OutputCapture {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn run_capture(project: &Path, cmd: &str, args: &[&str]) -> Result<OutputCapture, Box<dyn std::error::Error>> {
    let out = Command::new(cmd).args(args).current_dir(project).output()?;
    Ok(OutputCapture { ok: out.status.success(), stdout: String::from_utf8_lossy(&out.stdout).to_string(), stderr: String::from_utf8_lossy(&out.stderr).to_string() })
}

#[derive(Default)]
struct CargoCheckJson {
    ok: bool,
    diagnostics: Vec<serde_json::Value>,
}

fn run_cargo_check_json(project: &Path) -> Result<CargoCheckJson, Box<dyn std::error::Error>> {
    let out = run_capture(project, "cargo", &["check", "--message-format=json"])?;
    let mut diagnostics = Vec::new();
    for line in out.stdout.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(|v| v.as_str()) != Some("compiler-message") {
            continue;
        }
        let message = value.get("message").and_then(|v| v.as_object());
        if let Some(message) = message {
            let code = message.get("code").and_then(|c| c.get("code")).and_then(|c| c.as_str()).map(|s| s.to_string());
            let level = message.get("level").and_then(|v| v.as_str()).map(|s| s.to_string());
            let text = message.get("message").and_then(|v| v.as_str()).map(|s| s.to_string());
            let rendered = message.get("rendered").and_then(|v| v.as_str()).map(|s| s.to_string());

            let mut file = None;
            let mut line = None;
            let mut column = None;
            let mut is_primary = None;
            if let Some(spans) = message.get("spans").and_then(|v| v.as_array()) {
                let primary = spans.iter().find(|s| s.get("is_primary").and_then(|v| v.as_bool()) == Some(true));
                if let Some(span) = primary.or_else(|| spans.first()) {
                    file = span.get("file_name").and_then(|v| v.as_str()).map(|s| s.to_string());
                    line = span.get("line_start").and_then(|v| v.as_u64());
                    column = span.get("column_start").and_then(|v| v.as_u64());
                    is_primary = span.get("is_primary").and_then(|v| v.as_bool());
                }
            }

            diagnostics.push(json!({
                "code": code,
                "level": level,
                "message": text,
                "rendered": rendered,
                "file": file,
                "line": line,
                "column": column,
                "is_primary": is_primary
            }));
        }
    }
    Ok(CargoCheckJson { ok: out.ok, diagnostics })
}

fn src_has_diff(project: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let diff = run_capture(project, "git", &["diff", "--name-only", "--", "src"])?;
    Ok(!diff.stdout.trim().is_empty())
}

fn accumulate_error_counts_json(messages: &[serde_json::Value], counts: &mut BTreeMap<String, usize>) {
    for msg in messages {
        if let Some(code) = msg.get("code").and_then(|v| v.as_str()) {
            *counts.entry(code.to_string()).or_insert(0) += 1;
        }
    }
}

fn merge_counts(from: &BTreeMap<String, usize>, into: &mut BTreeMap<String, usize>) {
    for (k, v) in from {
        *into.entry(k.clone()).or_insert(0) += *v;
    }
}

fn update_kind_stats(stats: &mut BTreeMap<String, KindStats>, symbol_kind: &str, accepted: bool, introduced_errors: usize) {
    let entry = stats.entry(symbol_kind.to_string()).or_default();
    entry.attempts += 1;
    if accepted {
        entry.accepted += 1;
    }
    entry.introduced_errors += introduced_errors;
}

fn compute_delta_error_counts(baseline: &BTreeMap<String, usize>, after: &BTreeMap<String, usize>) -> BTreeMap<String, i64> {
    let mut keys: HashSet<String> = HashSet::new();
    keys.extend(baseline.keys().cloned());
    keys.extend(after.keys().cloned());

    let mut delta = BTreeMap::new();
    for key in keys {
        let b = *baseline.get(&key).unwrap_or(&0) as i64;
        let a = *after.get(&key).unwrap_or(&0) as i64;
        let d = a - b;
        if d != 0 {
            delta.insert(key, d);
        }
    }
    delta
}

fn is_degenerate_rename(old: &str, new: &str) -> bool {
    if old == new {
        return true;
    }
    new == format!("{old}{old}")
}

fn sum_counts(counts: &BTreeMap<String, usize>) -> usize {
    counts.values().sum()
}

fn touched_src_files(project: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let diff = run_capture(project, "git", &["diff", "--name-only", "--", "src"])?;
    Ok(diff.stdout.lines().map(str::trim).filter(|s| !s.is_empty()).map(|s| if s.starts_with("canon-agent/") { s.to_string() } else { format!("canon-agent/{s}") }).collect())
}

fn git_head_commit(project: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let out = run_capture(project, "git", &["rev-parse", "HEAD"])?;
    Ok(out.stdout.trim().to_string())
}

fn append_report_line(path: &str, payload: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(payload)?)?;
    Ok(())
}

struct SolverPlan {
    input_total: usize,
    transform_total: usize,
    dependency_count: usize,
    conflict_count: usize,
    cyclic_component_count: usize,
    sat_selected_total: usize,
    selected_total: usize,
    selected_pairs: Vec<(String, String)>,
}

fn build_solver_plan(pairs: &[(String, String)]) -> SolverPlan {
    let mut ir = CanonIR::new();
    let mut name_to_idx: BTreeMap<String, usize> = BTreeMap::new();
    let mut edges: Vec<(u32, u32, EdgeKind)> = Vec::new();

    for (old, new) in pairs {
        if is_degenerate_rename(old, new) {
            continue;
        }
        let src = ensure_name_node(&mut ir, &mut name_to_idx, new);
        let dst = ensure_name_node(&mut ir, &mut name_to_idx, old);
        edges.push((src as u32, dst as u32, EdgeKind::Renames));
    }

    let node_data: Vec<CanonId> = (0..ir.nodes.len()).map(|i| CanonId(i as u32)).collect();
    ir.name_graph = CsrGraph::from_edges(node_data, edges);

    let problem = build_problem(&ir);
    let optimized = optimize(&problem, 32);
    let selected_set: BTreeSet<usize> = optimized.into_iter().collect();

    let order: Vec<usize> = if problem.topo_order.len() == problem.transforms.len() { problem.topo_order.clone() } else { (0..problem.transforms.len()).collect() };

    let mut selected_pairs = Vec::new();
    for idx in order {
        if !selected_set.contains(&idx) {
            continue;
        }
        if let Some(t) = problem.transforms.get(idx) {
            selected_pairs.push((t.old_name.clone(), t.new_name.clone()));
        }
    }
    selected_pairs.sort();
    selected_pairs.dedup();

    SolverPlan {
        input_total: pairs.len(),
        transform_total: problem.transforms.len(),
        dependency_count: problem.dependencies.len(),
        conflict_count: problem.conflicts.len(),
        cyclic_component_count: problem.sccs.iter().filter(|c| c.len() > 1).count(),
        sat_selected_total: selected_set.len(),
        selected_total: selected_pairs.len(),
        selected_pairs,
    }
}

fn ensure_name_node(ir: &mut CanonIR, map: &mut BTreeMap<String, usize>, name: &str) -> usize {
    if let Some(idx) = map.get(name) {
        return *idx;
    }
    let name_id = ir.intern_name(name);
    let id = ir.push_node(CanonNodeKind::TypeRef { name_id });
    let idx = id.0 as usize;
    map.insert(name.to_string(), idx);
    idx
}

fn now_unix_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn now_iso_utc() -> String {
    let out = Command::new("date").args(["-u", "+%Y-%m-%dT%H:%M:%SZ"]).output();
    if let Ok(out) = out {
        let ts = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !ts.is_empty() {
            return ts;
        }
    }
    format!("{}", now_unix_secs())
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn tail_text(value: &str, max_chars: usize) -> Option<String> {
    if value.trim().is_empty() {
        return None;
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_chars {
        return Some(value.trim().to_string());
    }
    Some(chars[chars.len() - max_chars..].iter().collect::<String>().trim().to_string())
}
