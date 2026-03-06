#![cfg_attr(feature = "rustc_frontend", feature(rustc_private))]

pub mod core;
pub mod fs;
pub mod structured;

use std::path::Path;

use core::oracle::StructuralEditOracle;
use core::project_editor::ProjectEditor;
use structured::FieldMutation;

#[derive(Debug, Clone, Default)]
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

#[cfg(feature = "rustc_frontend")]
pub fn rename_symbol_pairs(project: &Path, renames: &[(String, String)]) -> RenameRunReport {
    let mut report = RenameRunReport::default();
    let mut editor = match ProjectEditor::load(project, Box::new(StructuralEditOracle)) {
        Ok(editor) => editor,
        Err(err) => {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    };

    for (old_symbol, new_symbol) in renames {
        if !editor.has_symbol(old_symbol) {
            report.error = Some(format!("symbol not found in registry: {old_symbol}"));
            return report;
        }
        let new_ident = new_symbol.rsplit("::").next().unwrap_or(new_symbol.as_str());
        if let Err(err) =
            editor.queue_by_id(old_symbol, FieldMutation::RenameIdent(new_ident.to_string()))
        {
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

#[cfg(not(feature = "rustc_frontend"))]
pub fn rename_symbol_pairs(_project: &Path, _renames: &[(String, String)]) -> RenameRunReport {
    RenameRunReport {
        rustc_args: Vec::new(),
        def_paths: Vec::new(),
        error: Some("rustc_frontend feature disabled".to_string()),
    }
}
