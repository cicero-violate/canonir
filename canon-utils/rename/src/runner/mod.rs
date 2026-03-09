mod attempt;
mod check;
mod config;
mod report;
mod shell;
mod solver;
mod suggest;
mod symbols;
mod verify;

pub use config::{RenameSelfConfig, RenameSelfMode, RenameSelfResult, SuggestConfig};

use attempt::{run_bulk_attempt, run_incremental_attempt, restore_project_src};
use check::{accumulate_error_counts_json, merge_counts, run_cargo_check_json, sum_counts};
use report::{
    append_report_line, git_head_commit, now_compact_utc, now_iso_utc, now_unix_secs, KindStats,
    SolverPlan, update_kind_stats,
};
use solver::{build_rename_groups, is_degenerate_rename};
use suggest::{apply_suggestions_from_stdin, run_suggest_names};
use symbols::{load_symbol_ids, parse_symbols_json};
use crate::core::rustc_session::RustcSession;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub fn run_rename_self_from_env() -> Result<RenameSelfResult, Box<dyn std::error::Error>> {
    if std::env::var("RENAME_SUGGEST_NAMES").ok().as_deref() == Some("1") {
        if std::env::var("RENAME_SUGGEST_STDIN").ok().as_deref() == Some("1") {
            let config = SuggestConfig::from_env();
            eprintln!("stdin: applying suggestions to {}", config.symbols_json.display());
            apply_suggestions_from_stdin(&config.symbols_json)?;
            eprintln!("stdin: suggestions applied; running bulk rename");
            let mut rename_config = RenameSelfConfig::from_env();
            rename_config.mode = RenameSelfMode::Bulk;
            return run_rename_self(rename_config);
        }
        let config = SuggestConfig::from_env();
        run_suggest_names(config)?;
        return Ok(RenameSelfResult {
            report_path: PathBuf::new(),
            status: "suggested".to_string(),
        });
    }
    if std::env::var("RENAME_LIST_SYMBOLS").ok().as_deref() == Some("1") {
        let config = RenameSelfConfig::from_env();
        let session = RustcSession::build(&config.project)?;
        let filter = std::env::var("RENAME_LIST_FILTER").ok();
        let limit = std::env::var("RENAME_LIST_LIMIT")
            .ok()
            .and_then(|v| v.parse::<usize>().ok());
        let mut count = 0usize;
        for (symbol_id, kind) in session.symbol_catalog() {
            if let Some(ref needle) = filter {
                if !symbol_id.contains(needle) && !kind.contains(needle) {
                    continue;
                }
            }
            println!("{}\t{}", symbol_id, kind);
            count += 1;
            if let Some(max) = limit {
                if count >= max {
                    break;
                }
            }
        }
        return Ok(RenameSelfResult {
            report_path: PathBuf::new(),
            status: "listed".to_string(),
        });
    }
    run_rename_self(RenameSelfConfig::from_env())
}

pub fn run_rename_self(config: RenameSelfConfig) -> Result<RenameSelfResult, Box<dyn std::error::Error>> {
    let project = config.project;
    let project_name = project
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("canon-agent-v1")
        .to_string();
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
    let bounded: Vec<(String, String)> = ordered
        .into_iter()
        .skip(config.offset)
        .take(config.limit)
        .collect();
    let (selected_pairs, skipped_groups) = if matches!(config.mode, RenameSelfMode::Bulk) {
        build_rename_groups(&bounded, &session)
    } else {
        (bounded.clone(), 0)
    };
    let solver_plan = SolverPlan {
        input_total: bounded.len(),
        transform_total: selected_pairs.len(),
        dependency_count: 0,
        conflict_count: 0,
        cyclic_component_count: 0,
        sat_selected_total: selected_pairs.len(),
        selected_total: selected_pairs.len(),
        selected_pairs: selected_pairs.clone(),
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
                "selected_total": solver_plan.selected_total,
                "skipped_trait_groups": skipped_groups
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
        println!(
            "bulk: requested={} resolved={} (offset={} limit={})",
            solver_plan.selected_pairs.len(),
            resolved.len(),
            config.offset,
            config.limit
        );
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
        println!(
            "{tag}  applied={}  delta={}  reason={}  ({}ms)",
            outcome.rename_applied,
            outcome.delta_total,
            outcome.decision_reason,
            outcome.transform_ms + outcome.compile_ms
        );
        println!(
            "bulk errors: total_after={} delta_total={} types={:?} touched_files={}",
            outcome.error_total_after,
            outcome.delta_total,
            outcome.delta_error_types,
            outcome.touched_files.len()
        );

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
                if prefix.is_empty() {
                    new_ident.clone()
                } else {
                    format!("{prefix}::{new_ident}")
                }
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
            let outcome = run_incremental_attempt(
                &project,
                &session,
                &old_symbol,
                &new_symbol,
                &baseline_error_counts,
            )?;
            let _ = restore_project_src(&project);

            match outcome.result.as_str() {
                "pass" => total_pass += 1,
                "fail" => total_fail += 1,
                _ => {}
            }
            let tag = if outcome.accept { "PASS" } else { "FAIL" };
            println!(
                "{tag}  applied={}  delta={}  reason={}  ({}ms)",
                outcome.rename_applied,
                outcome.delta_total,
                outcome.decision_reason,
                outcome.transform_ms + outcome.compile_ms
            );

            merge_counts(&outcome.error_types, &mut introduced_summary);
            update_kind_stats(
                &mut kind_stats,
                &symbol_kind,
                outcome.accept,
                outcome.delta_total.max(0) as usize,
            );

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
                "pairs": degenerate_pairs
                    .iter()
                    .map(|(old, new)| json!({
                        "old_name": old,
                        "new_name": new
                    }))
                    .collect::<Vec<_>>(),
                "ts": now_iso_utc()
            }),
        )?;
    }
    Ok(RenameSelfResult { report_path, status })
}
