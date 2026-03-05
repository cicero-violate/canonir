use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum FieldMutation {
    RenameIdent(String),
}

#[derive(Debug, Clone)]
pub enum NodeOp {
    MutateField { handle: SymbolHandle, mutation: FieldMutation },
    MoveSymbol { handle: SymbolHandle, symbol_id: String, new_module_path: String, new_crate: Option<String> },
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
