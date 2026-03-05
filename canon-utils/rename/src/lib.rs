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
        let (source_symbol, target_symbol) = if editor.has_symbol(old_symbol) {
            (old_symbol.as_str(), new_symbol.as_str())
        } else {
            (new_symbol.as_str(), old_symbol.as_str())
        };
        let new_ident = target_symbol.rsplit("::").next().unwrap_or(target_symbol);
        if let Err(err) =
            editor.queue_by_id(source_symbol, FieldMutation::RenameIdent(new_ident.to_string()))
        {
            report.error = Some(format!("{err:?}"));
            return report;
        }
    }

    if let Err(err) = editor
        .validate()
        .and_then(|conflicts| {
            if conflicts.is_empty() {
                Ok(())
            } else {
                Err(anyhow::anyhow!("validation conflicts: {conflicts:?}"))
            }
        })
        .and_then(|_| editor.apply().map(|_| ()))
        .and_then(|_| editor.preview().map(|_| ()))
        .and_then(|_| editor.commit().map(|_| ()))
    {
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
