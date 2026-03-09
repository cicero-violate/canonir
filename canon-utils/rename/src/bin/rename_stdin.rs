use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;

use rename::api::{dispatch, ApiOp, ApiRequest, ApiResult};
use rename::check::{accumulate_error_counts_json, compute_delta_error_counts, run_cargo_check_json};
use rename::core::ProjectEditor;
use rename::core::rustc_session::RustcSession;
use rename::verify::verify_renames_applied;

#[derive(Serialize)]
struct ApiResponse {
    results: Vec<ApiResult>,
    report: Option<serde_json::Value>,
    verify: Option<serde_json::Value>,
    check: Option<serde_json::Value>,
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
            .map(|c| c.diagnostics)
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let report = editor.apply().ok().map(|r| serde_json::to_value(r).unwrap());

    let rename_pairs: Vec<(String, String)> = req
        .ops
        .iter()
        .filter_map(|op| match op {
            ApiOp::RenameSymbol { old, new } => Some((old.clone(), new.clone())),
            _ => None,
        })
        .collect();

    let verify_result = if req.verify {
        Some(serde_json::to_value(verify_renames_applied(
            &session,
            &editor,
            &rename_pairs,
        ))?)
    } else {
        None
    };

    let check_result = if req.check {
        let after = run_cargo_check_json(project)
            .map(|c| c.diagnostics)
            .unwrap_or_default();
        let mut base_counts = BTreeMap::new();
        let mut after_counts = BTreeMap::new();
        accumulate_error_counts_json(&baseline, &mut base_counts);
        accumulate_error_counts_json(&after, &mut after_counts);
        let delta = compute_delta_error_counts(&base_counts, &after_counts);
        Some(serde_json::to_value(delta)?)
    } else {
        None
    };

    let response = ApiResponse {
        results,
        report,
        verify: verify_result,
        check: check_result,
    };
    println!("{}", serde_json::to_string_pretty(&response)?);

    Ok(())
}
