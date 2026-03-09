use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;

use rename::api::{dispatch, ApiOp, ApiRequest, ApiResult};
use rename::check::{accumulate_error_counts_json, compute_delta_error_counts, run_cargo_check_json, summarize_error_categories};
use rename::core::ProjectEditor;
use rename::core::rustc_session::RustcSession;
use rename::git::restore_project_src;
use rename::verify::verify_renames_applied;

#[derive(Serialize)]
struct ApiResponse {
    results: Vec<ApiResult>,
    report: Option<serde_json::Value>,
    verify: Option<serde_json::Value>,
    check: Option<serde_json::Value>,
    apply_error: Option<String>,
    commit_error: Option<String>,
    invariant_error: Option<String>,
    restore_error: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;

    let req: ApiRequest = serde_json::from_str(&buf)?;
    let project = Path::new(&req.project);

    let session = Arc::new(RustcSession::build(project)?);
    let mut editor = ProjectEditor::load_with_session(project, session.clone())?;

    let results: Vec<ApiResult> = req.ops.iter().map(|op| dispatch(&mut editor, op)).collect();

    let baseline = if req.check {
        run_cargo_check_json(project)
    } else {
        Ok(rename::check::CargoCheckJson {
            diagnostics: Vec::new(),
            success: true,
        })
    };

    let mut apply_error = None;
    let mut commit_error = None;
    let mut restore_error = None;
    let mut invariant_error = None;
    let mut moved_files: Vec<std::path::PathBuf> = Vec::new();
    let report = match editor.apply() {
        Ok(r) => {
            moved_files = r.file_moves.iter().map(|(_, dst)| dst.clone()).collect();
            if let Err(e) = editor.commit() {
                commit_error = Some(e.to_string());
            }
            Some(serde_json::to_value(r).unwrap())
        }
        Err(e) => {
            apply_error = Some(e.to_string());
            None
        }
    };

    if apply_error.is_none() && commit_error.is_none() {
        let validate = std::env::var("RENAME_VALIDATE_UPG").ok().as_deref() == Some("1");
        if validate {
            let output_dir = project.join("analysis");
            if let Err(err) = validate_upg_invariants(&output_dir) {
                invariant_error = Some(err);
            }
        }
    }

    let rename_pairs: Vec<(String, String)> = req
        .ops
        .iter()
        .filter_map(|op| match op {
            ApiOp::RenameSymbol { old, new } => Some((old.clone(), new.clone())),
            _ => None,
        })
        .collect();

    let verify_result = if req.verify && apply_error.is_none() {
        Some(serde_json::to_value(verify_renames_applied(
            &session,
            &editor,
            &rename_pairs,
        ))?)
    } else {
        None
    };

    let check_result = if req.check && apply_error.is_none() && commit_error.is_none() && invariant_error.is_none() {
        let baseline_success = baseline.as_ref().map(|c| c.success).unwrap_or(false);
        let baseline_res = baseline
            .as_ref()
            .map(|c| &c.diagnostics)
            .cloned()
            .unwrap_or_default();
        let after = run_cargo_check_json(project);
        let after_success = after.as_ref().map(|c| c.success).unwrap_or(false);
        let after_diags = after.as_ref().map(|c| &c.diagnostics).cloned().unwrap_or_default();
        let mut base_counts = BTreeMap::new();
        let mut after_counts = BTreeMap::new();
        accumulate_error_counts_json(&baseline_res, &mut base_counts);
        accumulate_error_counts_json(&after_diags, &mut after_counts);
        if !baseline_success && base_counts.is_empty() {
            base_counts.insert("unknown".to_string(), 1);
        }
        if !after_success && after_counts.is_empty() {
            after_counts.insert("unknown".to_string(), 1);
        }
        let delta = compute_delta_error_counts(&base_counts, &after_counts);
        let delta_total: i64 = delta.values().copied().sum();
        let mut baseline_errors = summarize_error_categories(&baseline_res);
        if !baseline_success && baseline_errors.is_empty() {
            baseline_errors.push(serde_json::json!({
                "code": "unknown",
                "description": "cargo check failed without JSON diagnostics",
                "count": 1
            }));
        }
        let mut after_errors = summarize_error_categories(&after_diags);
        if !after_success && after_errors.is_empty() {
            after_errors.push(serde_json::json!({
                "code": "unknown",
                "description": "cargo check failed without JSON diagnostics",
                "count": 1
            }));
        }
        let skip_restore = std::env::var("RENAME_SKIP_RESTORE").ok().as_deref() == Some("1");
        if delta_total != 0 && !skip_restore {
            if let Err(err) = restore_project_src(project) {
                restore_error = Some(err.to_string());
            } else {
                for moved in &moved_files {
                    if moved.exists() {
                        if let Err(err) = std::fs::remove_file(moved) {
                            restore_error = Some(err.to_string());
                            break;
                        }
                    }
                }
            }
        }
        Some(serde_json::json!({
            "baseline_success": baseline_success,
            "after_success": after_success,
            "error_total_before": base_counts.values().sum::<usize>(),
            "error_total_after": after_counts.values().sum::<usize>(),
            "delta_error_types": delta,
            "baseline_errors": baseline_errors,
            "after_errors": after_errors
        }))
    } else {
        None
    };

    let response = ApiResponse {
        results,
        report,
        verify: verify_result,
        check: check_result,
        apply_error,
        commit_error,
        invariant_error,
        restore_error,
    };
    println!("{}", serde_json::to_string_pretty(&response)?);

    Ok(())
}

fn validate_upg_invariants(output_dir: &Path) -> Result<(), String> {
    let path = output_dir.join("upg_invariants.json");
    let payload = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
    let obj = value.as_object().ok_or_else(|| "upg_invariants.json is not an object".to_string())?;
    for (key, val) in obj {
        if !key.starts_with("must_") {
            continue;
        }
        match val.as_bool() {
            Some(true) => {}
            Some(false) => return Err(key.clone()),
            None => return Err(format!("{key} not boolean")),
        }
    }
    Ok(())
}
