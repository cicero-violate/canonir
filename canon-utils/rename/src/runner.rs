use crate::core::project_editor::ProjectEditor;
use crate::core::rustc_session::RustcSession;
use crate::core::symbol_id::normalize_symbol_id;
use crate::structured::FieldMutation;
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy)]
pub enum RenameSelfMode {
    Incremental,
    Bulk,
}

impl RenameSelfMode {
    fn from_env() -> Self {
        match std::env::var("RENAME_MODE").unwrap_or_else(|_| "incremental".to_string()).to_lowercase().as_str() {
            "bulk" => RenameSelfMode::Bulk,
            _ => RenameSelfMode::Incremental,
        }
    }
}

pub struct RenameSelfConfig {
    pub project: PathBuf,
    pub symbols_json: PathBuf,
    pub report_dir: PathBuf,
    pub offset: usize,
    pub limit: usize,
    pub mode: RenameSelfMode,
}

impl RenameSelfConfig {
    pub fn from_env() -> Self {
        let project = PathBuf::from("/workspace/ai_sandbox/canon/canon-agent-v2");
        let symbols_json = PathBuf::from("/workspace/ai_sandbox/canon/canon-agent-v2/symbols.json");
        let report_dir = PathBuf::from("/workspace/ai_sandbox/canon/canon-utils/rename");
        let offset = std::env::var("RENAME_OFFSET").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
        let limit = std::env::var("RENAME_LIMIT").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(usize::MAX);
        Self { project, symbols_json, report_dir, offset, limit, mode: RenameSelfMode::from_env() }
    }
}

pub struct RenameSelfResult {
    pub report_path: PathBuf,
    pub status: String,
}

pub fn run_rename_self_from_env() -> Result<RenameSelfResult, Box<dyn std::error::Error>> {
    run_rename_self(RenameSelfConfig::from_env())
}

pub fn run_rename_self(config: RenameSelfConfig) -> Result<RenameSelfResult, Box<dyn std::error::Error>> {
    let project = config.project;
    let project_name = project.file_name().and_then(|n| n.to_str()).unwrap_or("canon-agent-v1").to_string();
    let symbols_json = config.symbols_json.to_string_lossy().to_string();
    let report_name = format!("rename_report_{}_{}.jsonl", project_name, now_compact_utc());
    let report_path = config.report_dir.join(report_name);
    let run_id = format!("run-{}", now_unix_secs());
    let run_started_at_unix = now_unix_secs();
    let run_started_at = Instant::now();
    let baseline_commit = git_head_commit(&project)?;
    let baseline_check = run_cargo_check_json(&project)?;
    let mut baseline_error_counts = BTreeMap::new();
    accumulate_error_counts_json(&baseline_check.diagnostics, &mut baseline_error_counts);
    let baseline_error_total = sum_counts(&baseline_error_counts);

    let session = Arc::new(RustcSession::build(&project)?);
    let ordered = parse_symbols_json(symbols_json)?;
    let bounded: Vec<(String, String)> = ordered.into_iter().skip(config.offset).take(config.limit).collect();
    let solver_plan = SolverPlan {
        input_total: bounded.len(),
        transform_total: bounded.len(),
        dependency_count: 0,
        conflict_count: 0,
        cyclic_component_count: 0,
        sat_selected_total: bounded.len(),
        selected_total: bounded.len(),
        selected_pairs: bounded.clone(),
    };

    append_report_line(
        report_path.to_string_lossy().as_ref(),
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
                "offset": config.offset,
                "limit": if config.limit == usize::MAX { serde_json::Value::Null } else { json!(config.limit) },
                "runner": "rename_self",
                "compile_cmd": "cargo check",
                "mode": match config.mode { RenameSelfMode::Incremental => "incremental", RenameSelfMode::Bulk => "bulk" }
            },
            "started_at_unix": run_started_at_unix,
            "ts": now_iso_utc()
        }),
    )?;

    append_report_line(
        report_path.to_string_lossy().as_ref(),
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
    let mut degenerate_pairs: Vec<(String, String)> = Vec::new();
    let mut wrote_degenerate_report = false;

    let symbol_ids = load_symbol_ids(&session)?;
    println!("registry: {} symbols loaded", symbol_ids.len());

    if matches!(config.mode, RenameSelfMode::Bulk) {
        let resolved = solver_plan.selected_pairs.clone();
        total_attempts = resolved.len();
        println!("bulk: requested={} resolved={} (offset={} limit={})", solver_plan.selected_pairs.len(), resolved.len(), config.offset, config.limit);
        let _ = restore_project_src(&project);
        let mut degenerate: Vec<(String, String)> = Vec::new();
        let non_degenerate: Vec<(String, String)> = resolved
            .iter()
            .filter(|(old, new)| {
                let old_ident = old.rsplit("::").next().unwrap_or(old.as_str());
                let new_ident = new.rsplit("::").next().unwrap_or(new.as_str());
                let is_deg = is_degenerate_rename(old_ident, new_ident);
                if is_deg {
                    degenerate.push((old.clone(), new.clone()));
                    degenerate_pairs.push((old_ident.to_string(), new_ident.to_string()));
                }
                !is_deg
            })
            .cloned()
            .collect();
        total_skipped += resolved.len().saturating_sub(non_degenerate.len());
        if !degenerate.is_empty() {
            append_report_line(
                report_path.to_string_lossy().as_ref(),
                &json!({
                    "type": "skipped",
                    "run_id": run_id,
                    "reason": "degenerate",
                    "count": degenerate.len(),
                    "pairs": degenerate.iter().map(|(old, new)| json!({
                        "symbol_id": old,
                        "old_name": old.rsplit_once("::").map(|(_, s)| s).unwrap_or(old.as_str()),
                        "new_name": new.rsplit_once("::").map(|(_, s)| s).unwrap_or(new.as_str())
                    })).collect::<Vec<_>>(),
                    "ts": now_iso_utc()
                }),
            )?;
            wrote_degenerate_report = true;
        }
        let outcome = run_bulk_attempt(&project, &session, &non_degenerate, &baseline_error_counts)?;
        if !outcome.accept {
            let _ = restore_project_src(&project);
        }

        match outcome.result.as_str() {
            "pass" => total_pass += 1,
            "fail" => total_fail += 1,
            _ => {}
        }
        let tag = if outcome.accept { "PASS" } else { "FAIL" };
        println!("{tag}  applied={}  delta={}  reason={}  ({}ms)", outcome.rename_applied, outcome.delta_total, outcome.decision_reason, outcome.transform_ms + outcome.compile_ms);
        println!("bulk errors: total_after={} delta_total={} types={:?} touched_files={}", outcome.error_total_after, outcome.delta_total, outcome.delta_error_types, outcome.touched_files.len());

        append_report_line(
            report_path.to_string_lossy().as_ref(),
            &json!({
                "type": "attempt",
                "run_id": run_id,
                "attempt_id": 1,
                "transform": {
                    "kind": "rename",
                    "transform_type": "bulk_symbol_rename",
                    "symbol_kind": "mixed",
                    "state_from": "S0",
                    "state_to": "S1",
                    "symbol_id": "bulk",
                    "old_name": "bulk",
                    "new_name": "bulk",
                    "rename_applied": outcome.rename_applied,
                    "touched_files": outcome.touched_files,
                    "verification": {
                        "method": "span_match",
                        "pairs_checked": outcome.verify_pairs_checked,
                        "pairs_changed": outcome.verify_pairs_changed
                    }
                },
                "compile": {
                    "status": outcome.result,
                    "invoked": true,
                    "error_types_after": outcome.error_types,
                    "error_total_after": outcome.error_total_after,
                    "messages": outcome.error_messages
                },
                "delta": {
                    "delta_error_types": outcome.delta_error_types,
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
    } else {
        for (old_ident, new_ident) in solver_plan.selected_pairs {
            if is_degenerate_rename(&old_ident, &new_ident) {
                total_skipped += 1;
                degenerate_pairs.push((old_ident.to_string(), new_ident.to_string()));
                attempt_id += 1;
                println!("[{attempt_id:>4}] SKIP  {old_ident} -> {new_ident}  (degenerate)");
                append_report_line(
                    report_path.to_string_lossy().as_ref(),
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
                            "error_total_after": 0,
                            "messages": []
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

            let old_symbol = old_ident.clone();
            let new_symbol = {
                let prefix = old_symbol.rsplit_once("::").map(|(p, _)| p).unwrap_or("");
                if prefix.is_empty() { new_ident.clone() } else { format!("{prefix}::{new_ident}") }
            };
            let symbol_kind = symbol_ids
                .iter()
                .find(|(id, _)| id == &old_symbol)
                .map(|(_, k)| k.as_str())
                .unwrap_or("unknown")
                .to_string();

            if !session.symbol_ids().iter().any(|id| id == &old_symbol) {
                total_skipped += 1;
                attempt_id += 1;
                println!("[{attempt_id:>4}] SKIP  {old_symbol} -> {new_symbol}  (missing_symbol)");
                let old_name = old_symbol
                    .rsplit_once("::")
                    .map(|(_, s)| s)
                    .unwrap_or(old_symbol.as_str());
                let new_name = new_symbol
                    .rsplit_once("::")
                    .map(|(_, s)| s)
                    .unwrap_or(new_symbol.as_str());
                append_report_line(
                    report_path.to_string_lossy().as_ref(),
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
                            "symbol_id": old_symbol.as_str(),
                            "old_name": old_name,
                            "new_name": new_name,
                            "rename_applied": false,
                            "touched_files": []
                        },
                        "compile": {
                            "status": "skipped",
                            "invoked": false,
                            "error_types_after": {},
                            "error_total_after": 0,
                            "messages": []
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

            total_attempts += 1;
            attempt_id += 1;

            print!("[{attempt_id:>4}] TRY   {old_symbol} -> {new_symbol} ... ");
            let _ = std::io::stdout().flush();

            let _ = restore_project_src(&project);
            let outcome = run_incremental_attempt(&project, &session, &old_symbol, &new_symbol, &baseline_error_counts)?;
            let _ = restore_project_src(&project);

            match outcome.result.as_str() {
                "pass" => total_pass += 1,
                "fail" => total_fail += 1,
                _ => {}
            }
            let tag = if outcome.accept { "PASS" } else { "FAIL" };
            println!("{tag}  applied={}  delta={}  reason={}  ({}ms)", outcome.rename_applied, outcome.delta_total, outcome.decision_reason, outcome.transform_ms + outcome.compile_ms);

            merge_counts(&outcome.error_types, &mut introduced_summary);
            update_kind_stats(&mut kind_stats, &symbol_kind, outcome.accept, outcome.delta_total.max(0) as usize);

            append_report_line(
                report_path.to_string_lossy().as_ref(),
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
                        "touched_files": outcome.touched_files,
                        "verification": {
                            "method": "span_match",
                            "pairs_checked": outcome.verify_pairs_checked,
                            "pairs_changed": outcome.verify_pairs_changed
                        }
                    },
                    "compile": {
                        "status": outcome.result,
                        "invoked": true,
                        "error_types_after": outcome.error_types,
                        "error_total_after": outcome.error_total_after,
                        "messages": outcome.error_messages
                    },
                    "delta": {
                        "delta_error_types": outcome.delta_error_types,
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

    let timing_total_ms = run_started_at.elapsed().as_millis();
    let summary = json!({
        "type": "run_summary",
        "run_id": run_id,
        "baseline_error_total": baseline_error_total,
        "attempts": total_attempts,
        "pass": total_pass,
        "fail": total_fail,
        "skipped": total_skipped,
        "introduced_error_types": introduced_summary,
        "timing_ms": {
            "total": timing_total_ms,
        },
        "ts": now_iso_utc()
    });
    append_report_line(report_path.to_string_lossy().as_ref(), &summary)?;

    let status = if total_fail == 0 { "ok" } else { "fail" }.to_string();
    println!(
        "summary: attempts={} pass={} fail={} skipped={} baseline_errors={} introduced_errors={}",
        total_attempts,
        total_pass,
        total_fail,
        total_skipped,
        baseline_error_total,
        introduced_summary.values().sum::<usize>()
    );
    if !degenerate_pairs.is_empty() {
        println!("degenerate: {}", degenerate_pairs.len());
        for (old_name, new_name) in &degenerate_pairs {
            println!("  {old_name} -> {new_name}");
        }
    }
    println!("status: {}", status);
    println!("report: {}", report_path.display());

    if !degenerate_pairs.is_empty() && !wrote_degenerate_report {
        append_report_line(
            report_path.to_string_lossy().as_ref(),
            &json!({
                "type": "skipped",
                "run_id": run_id,
                "reason": "degenerate",
                "count": degenerate_pairs.len(),
                "pairs": degenerate_pairs.iter().map(|(old, new)| json!({
                    "old_name": old,
                    "new_name": new
                })).collect::<Vec<_>>(),
                "ts": now_iso_utc()
            }),
        )?;
    }
    Ok(RenameSelfResult { report_path, status })
}

struct OutputCapture {
    stdout: String,
    stderr: String,
}

struct CargoCheckJson {
    diagnostics: Vec<serde_json::Value>,
}

struct IncrementalOutcome {
    result: String,
    rename_applied: bool,
    verify_pairs_checked: usize,
    verify_pairs_changed: usize,
    touched_files: Vec<String>,
    error_types: BTreeMap<String, usize>,
    error_messages: Vec<serde_json::Value>,
    error_total_after: usize,
    delta_error_types: BTreeMap<String, i64>,
    delta_total: i64,
    decision_reason: String,
    accept: bool,
    transform_ms: u128,
    compile_ms: u128,
}

struct BulkOutcome {
    result: String,
    rename_applied: bool,
    verify_pairs_checked: usize,
    verify_pairs_changed: usize,
    touched_files: Vec<String>,
    error_types: BTreeMap<String, usize>,
    error_messages: Vec<serde_json::Value>,
    error_total_after: usize,
    delta_error_types: BTreeMap<String, i64>,
    delta_total: i64,
    decision_reason: String,
    accept: bool,
    transform_ms: u128,
    compile_ms: u128,
}

fn run_bulk_attempt(project: &Path, session: &Arc<RustcSession>, renames: &[(String, String)], baseline_error_counts: &BTreeMap<String, usize>) -> Result<BulkOutcome, Box<dyn std::error::Error>> {
    let transform_started = Instant::now();
    let mut editor = ProjectEditor::load_with_session(project, session.clone())?;
    let mut touched_files = Vec::new();
    for (old_symbol, new_symbol) in renames {
        let new_ident = new_symbol
            .rsplit_once("::")
            .map(|(_, s)| s)
            .unwrap_or(new_symbol.as_str());
        if session.symbol_kind(old_symbol) == Some("module") {
            editor.queue_module_rename(old_symbol, new_ident);
        } else {
            editor.queue_by_id(old_symbol, FieldMutation::RenameIdent(new_ident.to_string()))?;
        }
    }
    let mut rename_applied = false;
    let mut verify_pairs_checked = 0usize;
    let mut verify_pairs_changed = 0usize;
    if editor.validate()?.is_empty() {
        let report = editor.apply()?;
        touched_files = report.touched_files.iter().map(|p| p.display().to_string()).collect();
        let verify = verify_renames_applied(session, &editor, renames);
        rename_applied = report.conflicts.is_empty() && verify.applied;
        verify_pairs_checked = verify.pairs_checked;
        verify_pairs_changed = verify.pairs_changed;
    }
    let transform_ms = transform_started.elapsed().as_millis();

    let compile_started = Instant::now();
    if rename_applied {
        match editor.commit() {
            Ok(written) => println!("commit: wrote {} files", written.len()),
            Err(e) => println!("commit ERROR: {e}"),
        }
    }

    let check = run_cargo_check_json(project)?;
    let mut error_counts = BTreeMap::new();
    accumulate_error_counts_json(&check.diagnostics, &mut error_counts);
    let error_messages = summarize_error_messages(&check.diagnostics);
    let error_total_after = sum_counts(&error_counts);
    let delta_error_types = compute_delta_error_counts(baseline_error_counts, &error_counts);
    let delta_total = sum_counts_i64(&delta_error_types);
    let compile_ms = compile_started.elapsed().as_millis();

    let accept = delta_total == 0 && rename_applied;
    let decision_reason = if accept {
        "accepted"
    } else if !rename_applied {
        "no_changes"
    } else {
        "introduced_errors"
    }
    .to_string();
    if !accept {
        restore_project_src(project);
    }

    Ok(BulkOutcome {
        result: if accept { "pass" } else { "fail" }.to_string(),
        rename_applied,
        verify_pairs_checked,
        verify_pairs_changed,
        touched_files,
        error_types: error_counts,
        error_messages,
        error_total_after,
        delta_error_types,
        delta_total,
        decision_reason,
        accept,
        transform_ms,
        compile_ms,
    })
}

fn run_incremental_attempt(
    project: &Path, session: &Arc<RustcSession>, old_symbol: &str, new_symbol: &str, baseline_error_counts: &BTreeMap<String, usize>,
) -> Result<IncrementalOutcome, Box<dyn std::error::Error>> {
    let transform_started = Instant::now();
    let mut editor = ProjectEditor::load_with_session(project, session.clone())?;
    let new_ident = new_symbol.rsplit_once("::").map(|(_, s)| s).unwrap_or(new_symbol);
    editor.queue_by_id(old_symbol, FieldMutation::RenameIdent(new_ident.to_string()))?;

    let mut rename_applied = false;
    let mut verify_pairs_checked = 0usize;
    let mut verify_pairs_changed = 0usize;
    let mut touched_files = Vec::new();
    if editor.validate()?.is_empty() {
        let report = editor.apply()?;
        touched_files = report.touched_files.iter().map(|p| p.display().to_string()).collect();
        let verify = verify_renames_applied(session, &editor, &[(old_symbol.to_string(), new_symbol.to_string())]);
        rename_applied = report.conflicts.is_empty() && verify.applied;
        verify_pairs_checked = verify.pairs_checked;
        verify_pairs_changed = verify.pairs_changed;
    }
    let transform_ms = transform_started.elapsed().as_millis();

    if rename_applied {
        match editor.commit() {
            Ok(written) => println!("commit: wrote {} files", written.len()),
            Err(e) => println!("commit ERROR: {e}"),
        }
    }

    let compile_started = Instant::now();
    let check = run_cargo_check_json(project)?;
    let mut error_counts = BTreeMap::new();
    accumulate_error_counts_json(&check.diagnostics, &mut error_counts);
    let error_messages = summarize_error_messages(&check.diagnostics);
    let error_total_after = sum_counts(&error_counts);
    let delta_error_types = compute_delta_error_counts(baseline_error_counts, &error_counts);
    let delta_total = sum_counts_i64(&delta_error_types);
    let compile_ms = compile_started.elapsed().as_millis();

    let accept = delta_total == 0 && rename_applied;
    let decision_reason = if accept {
        "accepted"
    } else if !rename_applied {
        "no_changes"
    } else {
        "introduced_errors"
    }
    .to_string();
    if !accept {
        restore_project_src(project);
    }

    Ok(IncrementalOutcome {
        result: if accept { "pass" } else { "fail" }.to_string(),
        rename_applied,
        verify_pairs_checked,
        verify_pairs_changed,
        touched_files,
        error_types: error_counts,
        error_messages,
        error_total_after,
        delta_error_types,
        delta_total,
        decision_reason,
        accept,
        transform_ms,
        compile_ms,
    })
}

fn restore_project_src(project: &Path) -> bool {
    run_cmd(project, "git", &["restore", "--source=HEAD", "--worktree", "--staged", "src"])
}

fn parse_symbols_json(path: String) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(&path)?;
    let entries: Vec<serde_json::Value> = serde_json::from_str(&content)?;
    let mut pairs = Vec::new();
    for entry in &entries {
        if entry.get("kind").and_then(|v| v.as_str()) == Some("file") {
            continue;
        }
        let old = entry
            .get("old")
            .and_then(|v| v.as_str())
            .or_else(|| entry.get("symbol_id").and_then(|v| v.as_str()))
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "missing 'old' or 'symbol_id' field")
            })?;
        let new = entry
            .get("new")
            .and_then(|v| v.as_str())
            .or_else(|| entry.get("new_name").and_then(|v| v.as_str()))
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "missing 'new' or 'new_name' field")
            })?;
        pairs.push((old.to_string(), new.to_string()));
    }
    Ok(pairs)
}

fn load_symbol_ids(session: &RustcSession) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    Ok(session.symbol_catalog())
}

fn run_cmd(project: &Path, cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).current_dir(project).status().map(|s| s.success()).unwrap_or(false)
}

fn run_capture(project: &Path, cmd: &str, args: &[&str]) -> Result<OutputCapture, Box<dyn std::error::Error>> {
    let output = Command::new(cmd).args(args).current_dir(project).output()?;
    Ok(OutputCapture { stdout: String::from_utf8_lossy(&output.stdout).to_string(), stderr: String::from_utf8_lossy(&output.stderr).to_string() })
}

fn run_cargo_check_json(project: &Path) -> Result<CargoCheckJson, Box<dyn std::error::Error>> {
    let mut cmd = Command::new("cargo");
    cmd.arg("check").arg("--message-format=json").current_dir(project);
    let output = cmd.output()?;
    let mut diagnostics = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value.get("reason").and_then(|v| v.as_str()) == Some("compiler-message") {
                if let Some(message) = value.get("message") {
                    diagnostics.push(message.clone());
                }
            }
        }
    }
    Ok(CargoCheckJson { diagnostics })
}

fn summarize_error_messages(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    const MAX_MESSAGES: usize = 50;
    let mut out = Vec::new();
    for msg in messages {
        if msg.get("level").and_then(|v| v.as_str()) != Some("error") {
            continue;
        }
        let code = msg.get("code").and_then(|c| c.get("code")).and_then(|c| c.as_str());
        let message = msg.get("message").and_then(|m| m.as_str());
        let mut file: Option<&str> = None;
        let mut line: Option<u64> = None;
        if let Some(spans) = msg.get("spans").and_then(|s| s.as_array()) {
            let primary = spans
                .iter()
                .find(|s| s.get("is_primary").and_then(|v| v.as_bool()) == Some(true))
                .or_else(|| spans.first());
            if let Some(span) = primary {
                file = span.get("file_name").and_then(|v| v.as_str());
                line = span.get("line_start").and_then(|v| v.as_u64());
            }
        }
        out.push(json!({
            "level": "error",
            "code": code,
            "message": message,
            "file": file,
            "line": line,
        }));
        if out.len() >= MAX_MESSAGES {
            break;
        }
    }
    out
}

fn accumulate_error_counts_json(messages: &[serde_json::Value], counts: &mut BTreeMap<String, usize>) {
    for msg in messages {
        if msg.get("level").and_then(|v| v.as_str()) != Some("error") {
            continue;
        }
        if let Some(code) = msg.get("code").and_then(|c| c.get("code")).and_then(|c| c.as_str()) {
            *counts.entry(code.to_string()).or_default() += 1;
        }
    }
}

fn merge_counts(from: &BTreeMap<String, usize>, into: &mut BTreeMap<String, usize>) {
    for (k, v) in from {
        *into.entry(k.clone()).or_default() += *v;
    }
}

fn update_kind_stats(stats: &mut BTreeMap<String, KindStats>, symbol_kind: &str, accepted: bool, introduced_errors: usize) {
    let entry = stats.entry(symbol_kind.to_string()).or_insert_with(KindStats::default);
    entry.attempts += 1;
    if accepted {
        entry.accepted += 1;
    } else if introduced_errors > 0 {
        entry.introduced_errors += 1;
    }
}

struct VerifySummary {
    applied: bool,
    pairs_checked: usize,
    pairs_changed: usize,
}

fn verify_renames_applied(session: &RustcSession, editor: &ProjectEditor, renames: &[(String, String)]) -> VerifySummary {
    let mut pairs_checked = 0usize;
    let mut pairs_changed = 0usize;
    let sources = &editor.last_applied_sources;
    if sources.is_empty() || renames.is_empty() {
        return VerifySummary { applied: false, pairs_checked, pairs_changed };
    }

    for (old_symbol, new_symbol) in renames {
        let old_norm = normalize_symbol_id(old_symbol);
        let old_ident = old_symbol.rsplit_once("::").map(|(_, s)| s).unwrap_or(old_symbol.as_str());
        let new_ident = new_symbol.rsplit_once("::").map(|(_, s)| s).unwrap_or(new_symbol.as_str());
        pairs_checked += 1;

        let Some(spans_by_file) = session.spans_for(&old_norm) else {
            continue;
        };

        let mut saw_file = false;
        let mut all_files_match = true;
        for (path, spans) in spans_by_file {
            if spans.is_empty() {
                continue;
            }
            saw_file = true;
            let Some(after) = sources.get(path) else {
                all_files_match = false;
                break;
            };
            // New ident must appear somewhere in the patched file.
            if !after.contains(new_ident) {
                all_files_match = false;
                break;
            }
            // Spot-check: old ident must not still sit at the first span position.
            if let Some(first_span) = spans.first() {
                let lo = first_span.lo;
                let hi = lo + old_ident.len();
                if after.as_bytes().get(lo..hi) == Some(old_ident.as_bytes()) {
                    all_files_match = false;
                    break;
                }
            }
        }

        if saw_file && all_files_match {
            pairs_changed += 1;
        }
    }

    VerifySummary { applied: pairs_checked > 0, pairs_checked, pairs_changed }
}

fn compute_delta_error_counts(baseline: &BTreeMap<String, usize>, after: &BTreeMap<String, usize>) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    for key in baseline.keys().chain(after.keys()) {
        let base = *baseline.get(key).unwrap_or(&0) as i64;
        let new = *after.get(key).unwrap_or(&0) as i64;
        let delta = new - base;
        if delta != 0 {
            out.insert(key.clone(), delta);
        }
    }
    out
}

fn is_degenerate_rename(old: &str, new: &str) -> bool {
    if old == new {
        return true;
    }
    // Skip exact self-doubling: Foo -> FooFoo
    if new == format!("{old}{old}") {
        return true;
    }
    // Skip if new == prefix + old where prefix is just old again (case-insensitive snake).
    if new.len() > old.len() {
        let prefix = &new[..new.len() - old.len()];
        if to_snake(prefix) == to_snake(old) {
            return true;
        }
    }
    false
}

fn to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        if let Some(lower) = c.to_lowercase().next() {
            out.push(lower);
        }
    }
    out
}

fn sum_counts(counts: &BTreeMap<String, usize>) -> usize {
    counts.values().sum()
}

fn sum_counts_i64(counts: &BTreeMap<String, i64>) -> i64 {
    counts.values().sum()
}

fn git_head_commit(project: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let out = run_capture(project, "git", &["rev-parse", "HEAD"])?;
    Ok(out.stdout.trim().to_string())
}

fn append_report_line(path: &str, payload: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let mut line = serde_json::to_string(payload)?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn now_unix_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn now_iso_utc() -> String {
    let now = chrono::Utc::now();
    now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn now_compact_utc() -> String {
    let now = chrono::Utc::now();
    now.format("%Y%m%dT%H%M%SZ").to_string()
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn tail_text(value: &str, max_chars: usize) -> Option<String> {
    if value.len() <= max_chars {
        Some(value.to_string())
    } else {
        Some(value.chars().rev().take(max_chars).collect::<String>().chars().rev().collect())
    }
}

#[derive(Default)]
struct KindStats {
    attempts: usize,
    accepted: usize,
    introduced_errors: usize,
}

#[derive(Debug, Clone)]
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
