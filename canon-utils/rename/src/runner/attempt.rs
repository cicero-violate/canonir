use super::check::{
    accumulate_error_counts_json, compute_delta_error_counts, run_cargo_check_json,
    sum_counts, sum_counts_i64, summarize_error_messages,
};
use super::shell::run_cmd;
use super::verify::verify_renames_applied;
use crate::core::ProjectEditor;
use crate::core::rustc_session::RustcSession;
use crate::structured::FieldMutation;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub(crate) struct IncrementalOutcome {
    pub(crate) result: String,
    pub(crate) rename_applied: bool,
    pub(crate) verify_pairs_checked: usize,
    pub(crate) verify_pairs_changed: usize,
    pub(crate) touched_files: Vec<String>,
    pub(crate) error_types: BTreeMap<String, usize>,
    pub(crate) error_messages: Vec<serde_json::Value>,
    pub(crate) error_total_after: usize,
    pub(crate) delta_error_types: BTreeMap<String, i64>,
    pub(crate) delta_total: i64,
    pub(crate) decision_reason: String,
    pub(crate) accept: bool,
    pub(crate) transform_ms: u128,
    pub(crate) compile_ms: u128,
}

pub(crate) struct BulkOutcome {
    pub(crate) result: String,
    pub(crate) rename_applied: bool,
    pub(crate) verify_pairs_checked: usize,
    pub(crate) verify_pairs_changed: usize,
    pub(crate) touched_files: Vec<String>,
    pub(crate) error_types: BTreeMap<String, usize>,
    pub(crate) error_messages: Vec<serde_json::Value>,
    pub(crate) error_total_after: usize,
    pub(crate) delta_error_types: BTreeMap<String, i64>,
    pub(crate) delta_total: i64,
    pub(crate) decision_reason: String,
    pub(crate) accept: bool,
    pub(crate) transform_ms: u128,
    pub(crate) compile_ms: u128,
}

pub(crate) fn run_bulk_attempt(
    project: &Path,
    session: &Arc<RustcSession>,
    renames: &[(String, String)],
    baseline_error_counts: &BTreeMap<String, usize>,
) -> Result<BulkOutcome, Box<dyn std::error::Error>> {
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
    let skip_restore = std::env::var("RENAME_SKIP_RESTORE").ok().as_deref() == Some("1");
    if !accept && !skip_restore {
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

pub(crate) fn run_incremental_attempt(
    project: &Path,
    session: &Arc<RustcSession>,
    old_symbol: &str,
    new_symbol: &str,
    baseline_error_counts: &BTreeMap<String, usize>,
) -> Result<IncrementalOutcome, Box<dyn std::error::Error>> {
    let transform_started = Instant::now();
    let mut editor = ProjectEditor::load_with_session(project, session.clone())?;
    let new_ident = new_symbol
        .rsplit_once("::")
        .map(|(_, s)| s)
        .unwrap_or(new_symbol);
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
    let skip_restore = std::env::var("RENAME_SKIP_RESTORE").ok().as_deref() == Some("1");
    if !accept && !skip_restore {
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

pub(crate) fn restore_project_src(project: &Path) -> bool {
    run_cmd(project, "git", &["restore", "--source=HEAD", "--worktree", "--staged", "src"])
}
