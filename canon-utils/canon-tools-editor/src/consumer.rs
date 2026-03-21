use crate::edit::ProjectEditor;
use crate::structured::{EditOp, FieldMutation};
use crate::symbol_index::SymbolIndex;
use anyhow::{anyhow, Result};
use canon_event::{RuntimeEvent, EditEvent, EventConsumer, EventFilter};
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
            EditEvent::RenameSymbol(canon_event::RenameSymbol { old, new, .. }) => {
                editor.queue_by_id(&old, FieldMutation::RenameIdent(new))?;
            }
            EditEvent::MoveSymbol(canon_event::MoveSymbol { symbol, module, .. }) => {
                let handle = editor.synthetic_handle_from_symbol_id(&symbol)?;
                let op = EditOp::MoveSymbol { handle, symbol_id: symbol.clone(), new_module_path: module, new_crate: None };
                editor.queue(&symbol, op)?;
            }
            EditEvent::DeleteSymbol(canon_event::DeleteSymbol { symbol, .. }) => {
                let handle = editor.synthetic_handle_from_symbol_id(&symbol)?;
                let op = EditOp::DeleteSymbol { handle, symbol_id: symbol.clone() };
                editor.queue(&symbol, op)?;
            }
            EditEvent::RenameModule(canon_event::RenameModule { old, new, .. }) => {
                editor.queue_module_rename(&old, &new);
            }
            EditEvent::RenameDir(canon_event::RenameDir { old, new, .. }) => {
                editor.queue_directory_rename(&old, &new);
            }
            EditEvent::InlineModule(_) => {
                return Err(anyhow!("InlineModule is not supported in EditConsumer yet"));
            }
            EditEvent::ExtractModule(_) => {
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

        let _written = editor.commit()?;
        Ok(())
    }
}

impl EventConsumer for EditConsumer {
    fn filter(&self) -> EventFilter {
        EventFilter::EditOnly
    }

    fn on_event(&mut self, event: &RuntimeEvent) {
        let RuntimeEvent::Edit(edit) = event else {
            return;
        };
        let _ = self.apply_event(edit.clone());
    }
}

trait EditEventProject {
    fn project(&self) -> &str;
}

impl EditEventProject for EditEvent {
    fn project(&self) -> &str {
        match self {
            EditEvent::RenameSymbol(canon_event::RenameSymbol { project, .. }) => project,
            EditEvent::MoveSymbol(canon_event::MoveSymbol { project, .. }) => project,
            EditEvent::DeleteSymbol(canon_event::DeleteSymbol { project, .. }) => project,
            EditEvent::RenameModule(canon_event::RenameModule { project, .. }) => project,
            EditEvent::RenameDir(canon_event::RenameDir { project, .. }) => project,
            EditEvent::InlineModule(canon_event::InlineModule { project, .. }) => project,
            EditEvent::ExtractModule(canon_event::ExtractModule { project, .. }) => project,
        }
    }
}
