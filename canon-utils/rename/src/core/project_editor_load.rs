use crate::core::oracle::StructuralEditOracle;
use crate::core::oracle::StructuralEditOracleApi;
use super::project_editor_helpers::{
    determine_source_root, module_path_from_file, symbol_kind_from_str,
};
use crate::core::rustc_session::RustcSession;
use crate::core::symbol_id::normalize_symbol_id;
use crate::fs::collect_rs_files;
use crate::structured::{FieldMutation, NodeOp, SymbolHandle, SymbolKind};
use anyhow::{anyhow, Result};
use proc_macro2::Span;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use syn::{Item, ItemConst, ItemEnum, ItemFn, ItemMod, ItemStatic, ItemStruct, ItemTrait, ItemType};

use super::project_editor_types::{DirRename, ModuleRename, NodeRegistry, ProjectEditor, QueuedOp};

impl ProjectEditor {
    pub fn load_with_rustc(project: &Path) -> Result<Self> {
        let oracle = Box::new(StructuralEditOracle);
        let session = Arc::new(RustcSession::build(project)?);
        Self::load_with_session_inner(project, oracle, session)
    }

    pub fn load_with_session(project: &Path, session: Arc<RustcSession>) -> Result<Self> {
        let oracle = Box::new(StructuralEditOracle);
        Self::load_with_session_inner(project, oracle, session)
    }

    fn load_with_session_inner(
        project: &Path,
        oracle: Box<dyn StructuralEditOracleApi>,
        session: Arc<RustcSession>,
    ) -> Result<Self> {
        let source_root = determine_source_root(project);
        let files = collect_rs_files(&source_root)?;
        let mut registry = NodeRegistry::default();
        let mut original_sources = HashMap::new();
        for file in files {
            let content = std::fs::read_to_string(&file)?;
            let module_path = module_path_from_file(&source_root, &file)?;
            registry.module_files.insert(module_path.clone(), file.clone());
            let ast = match syn::parse_file(&content) {
                Ok(ast) => {
                    index_file_symbols(&ast, &file, &module_path, &mut registry.handles);
                    ast
                }
                Err(_) => {
                    index_file_symbols_by_text(
                        &content,
                        &file,
                        &module_path,
                        &mut registry.handles,
                    );
                    syn::File {
                        shebang: None,
                        attrs: Vec::new(),
                        items: Vec::new(),
                    }
                }
            };
            registry.asts.insert(file.clone(), ast);
            registry.sources.insert(file.clone(), content.clone());
            original_sources.insert(file, content);
        }
        Ok(Self {
            registry,
            changesets: HashMap::new(),
            oracle,
            original_sources,
            last_applied_sources: HashMap::new(),
            project_root: project.to_path_buf(),
            source_root,
            pending_module_renames: Vec::new(),
            pending_dir_renames: Vec::new(),
            pending_file_moves: Vec::new(),
            last_touched_files: HashSet::new(),
            session: Some(session),
        })
    }

    pub fn queue(&mut self, symbol_id: &str, op: NodeOp) -> Result<()> {
        let norm = normalize_symbol_id(symbol_id);
        let handle = match &op {
            NodeOp::MutateField { handle, .. } | NodeOp::MoveSymbol { handle, .. } => {
                Some(handle)
            }
        };
        if let Some(handle) = handle {
            if !self.registry.handles.contains_key(&norm) {
                self.registry.handles.insert(norm.clone(), handle.clone());
            }
        }
        let file = match &op {
            NodeOp::MutateField { handle, .. } | NodeOp::MoveSymbol { handle, .. } => {
                handle.file.clone()
            }
        };
        self.changesets.entry(file).or_default().push(QueuedOp { symbol_id: norm, op });
        Ok(())
    }

    pub fn queue_by_id(
        &mut self,
        symbol_id: &str,
        mutation: FieldMutation,
    ) -> Result<()> {
        let norm = normalize_symbol_id(symbol_id);
        let handle = if let Some(handle) = self.registry.handles.get(&norm).cloned() {
            handle
        } else if matches!(mutation, FieldMutation::RenameIdent(_)) {
            self.synthetic_handle_from_symbol_id(&norm)?
        } else {
            return Err(anyhow!("no handle found for {symbol_id}"));
        };
        let op = NodeOp::MutateField {
            handle,
            mutation,
        };
        self.queue(&norm, op)
    }

    fn synthetic_handle_from_symbol_id(&self, symbol_id: &str) -> Result<SymbolHandle> {
        let (module_path, name) = symbol_id
            .rsplit_once("::")
            .ok_or_else(|| anyhow!("invalid symbol id: {symbol_id}"))?;
        let kind = self
            .session
            .as_ref()
            .and_then(|session| session.symbol_kind(symbol_id))
            .map(symbol_kind_from_str)
            .unwrap_or(SymbolKind::Fn);
        let file = self
            .registry
            .module_files
            .get(module_path)
            .cloned()
            .unwrap_or_else(PathBuf::new);
        Ok(SymbolHandle {
            file,
            module_path: module_path.to_string(),
            name: name.to_string(),
            kind,
        })
    }

    pub fn has_symbol(&self, symbol_id: &str) -> bool {
        let norm = normalize_symbol_id(symbol_id);
        if let Some(session) = &self.session {
            return session.spans_for(&norm).is_some();
        }
        self.registry.handles.contains_key(&norm)
    }

    pub fn symbol_ids(&self) -> Vec<String> {
        if let Some(session) = &self.session {
            return session.symbol_ids();
        }
        self.registry.handles.keys().cloned().collect()
    }

    pub fn symbol_catalog(&self) -> Vec<(String, String)> {
        if let Some(session) = &self.session {
            return session.symbol_catalog();
        }
        self.registry
            .handles
            .values()
            .map(|handle| {
                let kind = match handle.kind {
                    SymbolKind::Fn => "fn",
                    SymbolKind::Struct => "struct",
                    SymbolKind::Enum => "enum",
                    SymbolKind::Const => "const",
                    SymbolKind::Static => "static",
                    SymbolKind::Type => "type",
                    SymbolKind::Trait => "trait",
                    SymbolKind::Module => "module",
                };
                (format!("{}::{}", handle.module_path, handle.name), kind.to_string())
            })
            .collect()
    }

    pub fn queue_module_rename(&mut self, old_module_path: &str, new_name: &str) {
        self.pending_module_renames.push(ModuleRename {
            old_module_path: old_module_path.to_string(),
            new_name: new_name.to_string(),
        });
    }

    pub fn queue_directory_rename(&mut self, old_dir: &Path, new_dir: &Path) {
        self.pending_dir_renames.push(DirRename {
            old_dir: old_dir.to_path_buf(),
            new_dir: new_dir.to_path_buf(),
        });
    }
}

pub(crate) fn index_file_symbols(
    ast: &syn::File,
    file: &Path,
    module_path: &str,
    handles: &mut HashMap<String, SymbolHandle>,
) {
    index_items(&ast.items, file, module_path, handles);
}

pub(crate) fn index_file_symbols_by_text(
    content: &str,
    file: &Path,
    module_path: &str,
    handles: &mut HashMap<String, SymbolHandle>,
) {
    let re = regex::Regex::new(
            r"(?m)^\\s*(pub\\s+)?(fn|struct|enum|const|static|type|trait|mod)\\s+([A-Za-z_][A-Za-z0-9_]*)",
        )
        .ok();
    let Some(re) = re else {
        return;
    };
    for cap in re.captures_iter(content) {
        let kind = match &cap[2] {
            "fn" => SymbolKind::Fn,
            "struct" => SymbolKind::Struct,
            "enum" => SymbolKind::Enum,
            "const" => SymbolKind::Const,
            "static" => SymbolKind::Static,
            "type" => SymbolKind::Type,
            "trait" => SymbolKind::Trait,
            "mod" => SymbolKind::Module,
            _ => continue,
        };
        let name = &cap[3];
        let ident = syn::Ident::new(name, Span::call_site());
        insert_handle(file, module_path, &ident, kind, handles);
    }
}

pub(crate) fn index_items(
    items: &[Item],
    file: &Path,
    module_path: &str,
    handles: &mut HashMap<String, SymbolHandle>,
) {
    for item in items {
        match item {
            Item::Fn(ItemFn { sig, .. }) => {
                insert_handle(file, module_path, &sig.ident, SymbolKind::Fn, handles);
            }
            Item::Struct(ItemStruct { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Struct, handles);
            }
            Item::Enum(ItemEnum { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Enum, handles);
            }
            Item::Const(ItemConst { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Const, handles);
            }
            Item::Static(ItemStatic { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Static, handles);
            }
            Item::Type(ItemType { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Type, handles);
            }
            Item::Trait(ItemTrait { ident, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Trait, handles);
            }
            Item::Mod(ItemMod { ident, content, .. }) => {
                insert_handle(file, module_path, ident, SymbolKind::Module, handles);
                if let Some((_, inner_items)) = content {
                    let next_module_path = format!("{module_path}::{}", ident);
                    index_items(inner_items, file, &next_module_path, handles);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn insert_handle(
    file: &Path,
    module_path: &str,
    ident: &syn::Ident,
    kind: SymbolKind,
    handles: &mut HashMap<String, SymbolHandle>,
) {
    let symbol_id = format!("{module_path}::{ident}");
    handles.insert(
        symbol_id,
        SymbolHandle {
            file: file.to_path_buf(),
            module_path: module_path.to_string(),
            name: ident.to_string(),
            kind,
        },
    );
}
