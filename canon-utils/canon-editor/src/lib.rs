pub mod edit;
pub mod fs;
pub mod structured;
pub mod api;
pub mod check;
pub mod git;
pub mod verify;
pub mod query;
pub mod symbol_index;

use std::path::Path;
use std::sync::Arc;

use edit::ProjectEditor;
use crate::symbol_index::SymbolIndex;
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

pub fn rename_symbol_pairs_with_session(
    project: &Path,
    session: Arc<SymbolIndex>,
    renames: &[(String, String)],
) -> RenameRunReport {
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
        let op = EditOp::MutateField {
            handle,
            mutation: FieldMutation::RenameIdent(new_ident.to_string()),
        };
        if let Err(err) = editor.queue(old_symbol, op) {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    }

    let preview_output = editor
        .validate()
        .and_then(|conflicts| {
            if conflicts.is_empty() {
                Ok(())
            } else {
                Err(anyhow::anyhow!("validation conflicts: {conflicts:?}"))
            }
        })
        .and_then(|_| editor.apply().and_then(|report| {
            if report.conflicts.is_empty() {
                Ok(())
            } else {
                Err(anyhow::anyhow!("apply conflicts: {:?}", report.conflicts))
            }
        }))
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
