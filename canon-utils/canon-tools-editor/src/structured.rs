use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum FieldMutation {
    RenameIdent(String),
}

#[derive(Debug, Clone)]
pub enum EditOp {
    MutateField { handle: SymbolHandle, symbol_id: String, mutation: FieldMutation },
    MoveSymbol { handle: SymbolHandle, symbol_id: String, new_module_path: String, new_crate: Option<String> },
    DeleteSymbol { handle: SymbolHandle, symbol_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Fn,
    Struct,
    Enum,
    Const,
    Static,
    Type,
    Trait,
    Module,
}

#[derive(Debug, Clone)]
pub struct SymbolHandle {
    pub file: PathBuf,
    pub module_path: String,
    pub name: String,
    pub kind: SymbolKind,
}

impl EditOp {
    pub fn to_event(self, project: &str) -> canon_event::EditEvent {
        match self {
            EditOp::MutateField { symbol_id, mutation, .. } => match mutation {
                FieldMutation::RenameIdent(new_name) => canon_event::EditEvent::RenameSymbol {
                    project: project.to_string(),
                    old: symbol_id,
                    new: new_name,
                },
            },
            EditOp::MoveSymbol { symbol_id, new_module_path, .. } => {
                canon_event::EditEvent::MoveSymbol {
                    project: project.to_string(),
                    symbol: symbol_id,
                    module: new_module_path,
                }
            }
            EditOp::DeleteSymbol { symbol_id, .. } => canon_event::EditEvent::DeleteSymbol {
                project: project.to_string(),
                symbol: symbol_id,
            },
        }
    }
}
