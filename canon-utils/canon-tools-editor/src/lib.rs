pub mod api;
pub mod check;
pub mod consumer;
pub mod edit;
pub mod fs;
pub mod git;
pub mod query;
pub mod structured;
pub mod symbol_index;
pub mod tlog;
pub mod verify;

use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;

use crate::symbol_index::SymbolIndex;
pub use consumer::EditConsumer;
use edit::ProjectEditor;
use structured::{EditOp, FieldMutation};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RenameRunReport {
    pub rustc_args: Vec<String>,
    pub def_paths: Vec<String>,
    pub error: Option<String>,
}

impl RenameRunReport {
    pub fn status(&self) -> &'static str {
        if self.error.is_some() {
            "error"
        } else {
            "ok"
        }
    }
}

pub fn rename_symbol_pairs(project: &Path, renames: &[(String, String)]) -> RenameRunReport {
    let mut report = RenameRunReport::default();
    let session = match SymbolIndex::build(project) {
        Ok(session) => Arc::new(session),
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    };
    rename_symbol_pairs_with_session(project, session, renames)
}

pub fn rename_symbol_pairs_from_graph_candidates(
    project: &Path,
    candidates: &[canon_analysis::GraphRenameCandidate],
) -> RenameRunReport {
    let renames: Vec<(String, String)> = candidates
        .iter()
        .map(|candidate| (candidate.symbol_path.clone(), candidate.suggested_path.clone()))
        .collect();
    rename_symbol_pairs(project, &renames)
}

pub fn rename_duplicate_symbols_from_latest_graph(
    project: &Path,
    limit: usize,
) -> RenameRunReport {
    match canon_analysis::graph_backed_rename_candidates(project, limit) {
        Ok(candidates) => rename_symbol_pairs_from_graph_candidates(project, &candidates),
        Err(err) => RenameRunReport {
            error: Some(format!("{err:?}")),
            ..RenameRunReport::default()
        },
    }
}

pub fn move_symbol_pairs(project: &Path, moves: &[(String, String)]) -> RenameRunReport {
    let mut report = RenameRunReport::default();
    let session = match SymbolIndex::build(project) {
        Ok(session) => Arc::new(session),
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    };
    let mut editor = match ProjectEditor::load_with_session(project, session) {
        Ok(editor) => editor,
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    };

    for (symbol_id, new_module_path) in moves {
        let handle = match editor.synthetic_handle_from_symbol_id(symbol_id) {
            Ok(handle) => handle,
            Err(err) => {
                report.error = Some(format!("{err:?}"));
                return report;
            }
        };
        let op = EditOp::MoveSymbol {
            handle,
            symbol_id: symbol_id.clone(),
            new_module_path: new_module_path.clone(),
            new_crate: None,
        };
        if let Err(err) = editor.queue(symbol_id, op) {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    }

    let preview_output = editor
        .validate()
        .and_then(|conflicts| if conflicts.is_empty() { Ok(()) } else { Err(anyhow!("validation conflicts: {conflicts:?}")) })
        .and_then(|_| editor.apply().and_then(|report| if report.conflicts.is_empty() { Ok(()) } else { Err(anyhow!("apply conflicts: {:?}", report.conflicts)) }))
        .and_then(|_| editor.preview());

    match preview_output {
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
        Ok(preview) => report.def_paths.push(preview),
    }

    if let Err(err) = editor.commit() {
        report.error = Some(format!("{err:?}"));
    }

    report
}

pub fn move_symbols_from_graph_candidates(
    project: &Path,
    candidates: &[canon_analysis::GraphModuleMoveCandidate],
) -> RenameRunReport {
    let moves: Vec<(String, String)> = candidates
        .iter()
        .map(|candidate| (candidate.symbol_path.clone(), candidate.to_module_path.clone()))
        .collect();
    move_symbol_pairs(project, &moves)
}

pub fn restructure_modules_from_latest_graph(project: &Path, limit: usize) -> RenameRunReport {
    match canon_analysis::graph_backed_module_moves(project, limit) {
        Ok(candidates) => move_symbols_from_graph_candidates(project, &candidates),
        Err(err) => RenameRunReport {
            error: Some(format!("{err:?}")),
            ..RenameRunReport::default()
        },
    }
}

pub fn rename_symbol_pairs_with_session(project: &Path, session: Arc<SymbolIndex>, renames: &[(String, String)]) -> RenameRunReport {
    let mut report = RenameRunReport::default();
    let mut editor = match ProjectEditor::load_with_session(project, session) {
        Ok(editor) => editor,
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    };

    for (old_symbol, new_symbol) in renames {
        let Some(handle) = editor.registry.handles.get(old_symbol).cloned() else {
            report.error = Some(format!("symbol not found in registry: {old_symbol}"));
            return report;
        };
        let new_ident = new_symbol.rsplit("::").next().unwrap_or(new_symbol.as_str());
        let op = EditOp::MutateField { handle, symbol_id: old_symbol.to_string(), mutation: FieldMutation::RenameIdent(new_ident.to_string()) };
        if let Err(err) = editor.queue(old_symbol, op) {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    }

    let preview_output = editor
        .validate()
        .and_then(|conflicts| if conflicts.is_empty() { Ok(()) } else { Err(anyhow!("validation conflicts: {conflicts:?}")) })
        .and_then(|_| editor.apply().and_then(|report| if report.conflicts.is_empty() { Ok(()) } else { Err(anyhow!("apply conflicts: {:?}", report.conflicts)) }))
        .and_then(|_| editor.preview());

    match preview_output {
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
        Ok(preview) => report.def_paths.push(preview),
    }

    if let Err(err) = editor.commit() {
        report.error = Some(format!("{err:?}"));
    }

    report
}
