use crate::edit::ProjectEditor;
use crate::structured::{EditOp, FieldMutation};
use crate::symbol_index::SymbolIndex;
use anyhow::{anyhow, Result};
use canon_event_log::{error, info};
use canon_types::{EditEvent, RuntimeConsumer, RuntimeEvent, RuntimeEventFilter};
use std::path::PathBuf;
use std::sync::Arc;

pub struct EditConsumer;

impl EditConsumer {
    pub fn new() -> Self {
        Self
    }

    fn apply_event(&self, event: EditEvent) -> Result<()> {
        let project_root = PathBuf::from(event.project().to_string());
        let session = Arc::new(SymbolIndex::build(&project_root)?);
        let mut editor = ProjectEditor::load_with_session(&project_root, session)?;

        match event {
            EditEvent::RenameSymbol { old, new, .. } => {
                editor.queue_by_id(&old, FieldMutation::RenameIdent(new))?;
            }
            EditEvent::MoveSymbol { symbol, module, .. } => {
                let handle = editor.synthetic_handle_from_symbol_id(&symbol)?;
                let op = EditOp::MoveSymbol {
                    handle,
                    symbol_id: symbol.clone(),
                    new_module_path: module,
                    new_crate: None,
                };
                editor.queue(&symbol, op)?;
            }
            EditEvent::DeleteSymbol { symbol, .. } => {
                let handle = editor.synthetic_handle_from_symbol_id(&symbol)?;
                let op = EditOp::DeleteSymbol {
                    handle,
                    symbol_id: symbol.clone(),
                };
                editor.queue(&symbol, op)?;
            }
            EditEvent::RenameModule { old, new, .. } => {
                editor.queue_module_rename(&old, &new);
            }
            EditEvent::RenameDir { old, new, .. } => {
                editor.queue_directory_rename(&old, &new);
            }
            EditEvent::InlineModule { .. } => {
                return Err(anyhow!("InlineModule is not supported in EditConsumer yet"));
            }
            EditEvent::ExtractModule { .. } => {
                return Err(anyhow!("ExtractModule is not supported in EditConsumer yet"));
            }
        }

        let conflicts = editor.validate()?;
        if !conflicts.is_empty() {
            return Err(anyhow!("validation conflicts: {:?}", conflicts));
        }

        let report = editor.apply()?;
        if !report.conflicts.is_empty() {
            return Err(anyhow!("apply conflicts: {:?}", report.conflicts));
        }

        let written = editor.commit()?;
        info(
            "edit_consumer",
            "edit_applied",
            serde_json::json!({ "files": written.len() }),
        );
        Ok(())
    }
}

impl RuntimeConsumer for EditConsumer {
    fn filter(&self) -> RuntimeEventFilter {
        RuntimeEventFilter::EditOnly
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        let RuntimeEvent::Edit(edit) = event else {
            return;
        };
        if let Err(err) = self.apply_event(edit.clone()) {
            error(
                "edit_consumer",
                "edit_apply_failed",
                serde_json::json!({ "error": err.to_string() }),
            );
        }
    }
}

trait EditEventProject {
    fn project(&self) -> &str;
}

impl EditEventProject for EditEvent {
    fn project(&self) -> &str {
        match self {
            EditEvent::RenameSymbol { project, .. } => project,
            EditEvent::MoveSymbol { project, .. } => project,
            EditEvent::DeleteSymbol { project, .. } => project,
            EditEvent::RenameModule { project, .. } => project,
            EditEvent::RenameDir { project, .. } => project,
            EditEvent::InlineModule { project, .. } => project,
            EditEvent::ExtractModule { project, .. } => project,
        }
    }
}
