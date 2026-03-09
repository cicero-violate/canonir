use super::project_editor_helpers::{build_full_path, canonicalize_relative, join_module_path, module_path_for_dir, split_module_path};
use crate::core::syn_patcher;
use crate::core::syn_patcher::SpanReplacement;
use crate::structured::{NodeOp, SymbolHandle, SymbolKind};
use anyhow::{anyhow, Result};
use proc_macro2::Span;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use syn::Item;

use super::project_editor_helpers::module_path_from_file;
use super::project_editor_rewrite::{rewrite_string_attrs_in_file, PathRewriter};
use super::project_editor_types::{ChangeReport, EditConflict, ProjectEditor, QueuedOp};

impl ProjectEditor {
    pub fn apply(&mut self) -> Result<ChangeReport> {
        let mut touched_files = HashSet::new();
        let conflicts = Vec::new();
        let mut file_moves = Vec::new();

        let mut queued_renames: Vec<(String, String)> = Vec::new();
        let mut remaining_changesets: std::collections::HashMap<PathBuf, Vec<QueuedOp>> = std::collections::HashMap::new();
        for (file, ops) in self.changesets.clone() {
            let mut remaining_ops = Vec::new();
            for op in ops {
                match &op.op {
                    NodeOp::MutateField { handle: _, mutation } => match mutation {
                        crate::structured::FieldMutation::RenameIdent(new_name) => {
                            queued_renames.push((op.symbol_id.clone(), new_name.clone()));
                        }
                    },
                    _ => remaining_ops.push(op),
                }
            }
            if !remaining_ops.is_empty() {
                remaining_changesets.insert(file, remaining_ops);
            }
        }
        self.changesets = remaining_changesets;

        for op in self.pending_dir_renames.clone() {
            let (touched, moves) = self.apply_dir_rename(&op.old_dir, &op.new_dir)?;
            touched_files.extend(touched);
            file_moves.extend(moves);
        }
        self.pending_dir_renames.clear();

        for op in self.pending_module_renames.clone() {
            let (touched, moves) = self.apply_module_rename(&op.old_module_path, &op.new_name)?;
            touched_files.extend(touched);
            file_moves.extend(moves);
        }
        self.pending_module_renames.clear();

        if !queued_renames.is_empty() {
            let touched = self.apply_symbol_renames_bulk(&queued_renames)?;
            touched_files.extend(touched);
        }

        for (_file, ops) in self.changesets.clone() {
            for op in ops {
                match &op.op {
                    NodeOp::MoveSymbol { handle, new_module_path, .. } => {
                        let touched = self.apply_move_symbol(handle, new_module_path)?;
                        touched_files.extend(touched);
                    }
                    NodeOp::DeleteSymbol { handle, symbol_id } => {
                        let touched = self.apply_delete_symbol(handle, symbol_id)?;
                        touched_files.extend(touched);
                    }
                    NodeOp::MutateField { .. } => {}
                }
            }
        }
        self.changesets.clear();

        self.rewrite_sources_for(&touched_files)?;
        self.last_applied_sources.clear();
        for path in &touched_files {
            if let Some(source) = self.registry.sources.get(path) {
                self.last_applied_sources.insert(path.clone(), source.clone());
            }
        }
        self.rebuild_registry()?;

        self.pending_file_moves.extend(file_moves.clone());
        self.last_touched_files = touched_files.clone();

        Ok(ChangeReport { touched_files: touched_files.into_iter().collect(), conflicts, file_moves })
    }

    pub fn validate(&self) -> Result<Vec<EditConflict>> {
        let mut conflicts = Vec::new();
        for (_file, ops) in &self.changesets {
            for queued in ops {
                match &queued.op {
                    NodeOp::MutateField { handle, mutation: crate::structured::FieldMutation::RenameIdent(new_name) } => {
                        let module = &handle.module_path;
                        let candidate = format!("{module}::{new_name}");
                        if self.registry.handles.contains_key(&candidate) {
                            conflicts.push(EditConflict { symbol_id: queued.symbol_id.clone(), reason: format!("rename would conflict with existing symbol {candidate}") });
                        }
                    }
                    NodeOp::MoveSymbol { handle, new_module_path, .. } => {
                        let candidate = format!("{new_module_path}::{}", handle.name);
                        if self.registry.handles.contains_key(&candidate) {
                            conflicts.push(EditConflict { symbol_id: queued.symbol_id.clone(), reason: format!("move would conflict with existing symbol {candidate}") });
                        }
                    }
                    NodeOp::DeleteSymbol { .. } => {}
                }
            }
        }
        Ok(conflicts)
    }

    pub fn preview(&self) -> Result<String> {
        let mut out = String::new();
        if !self.pending_file_moves.is_empty() {
            out.push_str("file moves:\n");
            for (from, to) in &self.pending_file_moves {
                out.push_str(&format!("- {} -> {}\n", from.display(), to.display()));
            }
            out.push('\n');
        }
        for path in &self.last_touched_files {
            if let Some(ast) = self.registry.asts.get(path) {
                let rendered = if ast.items.is_empty() { self.registry.sources.get(path).cloned().unwrap_or_default() } else { prettyplease::unparse(ast) };
                out.push_str(&format!("=== {} ===\n", path.display()));
                out.push_str(&rendered);
                out.push('\n');
            }
        }
        Ok(out)
    }

    pub fn commit(&self) -> Result<Vec<PathBuf>> {
        let move_map: std::collections::HashMap<PathBuf, PathBuf> = self.pending_file_moves.iter().cloned().collect();
        let mut written = Vec::new();
        for path in &self.last_touched_files {
            if let Some(ast) = self.registry.asts.get(path) {
                let rendered = if ast.items.is_empty() { self.registry.sources.get(path).cloned().unwrap_or_default() } else { prettyplease::unparse(ast) };
                std::fs::write(path, rendered)?;
            } else if let Some(source) = self.registry.sources.get(path) {
                std::fs::write(path, source)?;
            }
            let final_path = move_map.get(path).cloned().unwrap_or_else(|| path.clone());
            written.push(final_path);
        }
        for (from, to) in &self.pending_file_moves {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(from, to)?;
        }
        Ok(written)
    }

    fn apply_symbol_renames_bulk(&mut self, renames: &[(String, String)]) -> Result<HashSet<PathBuf>> {
        let Some(session) = &self.session else {
            return Err(anyhow!("rustc session not initialized; use ProjectEditor::load_with_rustc"));
        };

        let mut per_file: std::collections::HashMap<PathBuf, Vec<SpanReplacement>> = std::collections::HashMap::new();
        let mut per_file_attr_pairs: std::collections::HashMap<PathBuf, Vec<(String, String)>> = std::collections::HashMap::new();

        for (symbol_id, new_name) in renames {
            let old_ident = symbol_id.rsplit_once("::").map(|(_, s)| s).unwrap_or(symbol_id.as_str()).to_string();
            let norm = crate::core::symbol_id::normalize_symbol_id(symbol_id);
            let occurrences = session.spans_for(&norm).ok_or_else(|| anyhow!("symbol not found via rustc: {symbol_id}"))?;
            for (path, spans) in occurrences {
                let entry = per_file.entry(path.clone()).or_default();
                for span in spans {
                    entry.push(SpanReplacement { span: span.clone(), replacement: new_name.clone() });
                }
                per_file_attr_pairs.entry(path.clone()).or_default().push((old_ident.clone(), new_name.clone()));
            }
        }

        let mut touched = HashSet::new();
        for (path, mut replacements) in per_file {
            replacements.sort_by(|a, b| a.span.lo.cmp(&b.span.lo).then_with(|| a.span.hi.cmp(&b.span.hi)).then_with(|| a.replacement.cmp(&b.replacement)));
            replacements.dedup_by(|a, b| a.span.lo == b.span.lo && a.span.hi == b.span.hi && a.replacement == b.replacement);

            for window in replacements.windows(2) {
                let a = &window[0];
                let b = &window[1];
                if a.span.lo == b.span.lo && a.span.hi == b.span.hi && a.replacement != b.replacement {
                    return Err(anyhow!("conflicting replacements at {}..{} in {}", a.span.lo, a.span.hi, path.display()));
                }
            }

            let source = match self.registry.sources.get(&path) {
                Some(content) => content.clone(),
                None => continue,
            };
            let source = match session.normalized_source(&path) {
                Some(s) => s.clone(),
                None => source,
            };
            let mut updated = syn_patcher::patch_file(&source, &replacements)?;
            let mut changed = updated != source;
            if let Ok(ast) = syn::parse_file(&updated) {
                let mut ast = ast;
                let mut attr_pairs = per_file_attr_pairs.get(&path).cloned().unwrap_or_default();
                attr_pairs.sort();
                attr_pairs.dedup();
                let mut attr_changed = false;
                for (old_name, new_name) in &attr_pairs {
                    if rewrite_string_attrs_in_file(&mut ast, old_name, new_name) {
                        attr_changed = true;
                    }
                }
                if attr_changed {
                    updated = prettyplease::unparse(&ast);
                    changed = true;
                }
                if changed {
                    self.registry.sources.insert(path.to_path_buf(), updated.clone());
                    self.registry.asts.insert(path.to_path_buf(), ast);
                }
            } else if changed {
                self.registry.sources.insert(path.to_path_buf(), updated.clone());
            }
            if changed {
                touched.insert(path);
            }
        }

        Ok(touched)
    }

    fn apply_delete_symbol(&mut self, handle: &SymbolHandle, symbol_id: &str) -> Result<HashSet<PathBuf>> {
        let mut touched = HashSet::new();
        let file = handle.file.clone();
        let ast = self
            .registry
            .asts
            .get_mut(&file)
            .ok_or_else(|| anyhow!("missing AST for {}", file.display()))?;
        let removed = remove_top_level_item(ast, &handle.name, &handle.kind);
        if removed.is_some() {
            touched.insert(file);
            Ok(touched)
        } else {
            Err(anyhow!("symbol not found in AST: {symbol_id}"))
        }
    }

    fn apply_move_symbol(&mut self, handle: &SymbolHandle, new_module_path: &str) -> Result<HashSet<PathBuf>> {
        let mut touched = HashSet::new();
        let old_file = handle.file.clone();
        let old_module_path = handle.module_path.clone();
        let symbol_name = handle.name.clone();
        let removed = {
            let old_ast = self.registry.asts.get_mut(&old_file).ok_or_else(|| anyhow!("missing AST for {}", old_file.display()))?;
            let removed = remove_top_level_item(old_ast, &symbol_name, &handle.kind).ok_or_else(|| anyhow!("symbol not found for move: {}", symbol_name))?;
            touched.insert(old_file.clone());
            removed
        };
        let new_file = self.registry.module_files.get(new_module_path).cloned().ok_or_else(|| anyhow!("no file for module path {new_module_path}"))?;
        {
            let new_ast = self.registry.asts.get_mut(&new_file).ok_or_else(|| anyhow!("missing AST for {}", new_file.display()))?;
            new_ast.items.push(removed);
            touched.insert(new_file.clone());
        }
        let old_full = build_full_path(&old_module_path, &symbol_name);
        let new_full = build_full_path(new_module_path, &symbol_name);
        for (path, ast) in self.registry.asts.iter_mut() {
            let mut rewriter = PathRewriter::replace_full(&old_full, &new_full);
            if rewriter.visit_file(ast) {
                touched.insert(path.clone());
            }
        }
        Ok(touched)
    }

    fn apply_module_rename(&mut self, old_module_path: &str, new_name: &str) -> Result<(HashSet<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        let mut touched = HashSet::new();
        let mut file_moves = Vec::new();
        let old_segments = split_module_path(old_module_path);
        if old_segments.len() < 2 {
            return Err(anyhow!("module path must include crate and name"));
        }
        let parent_segments = &old_segments[..old_segments.len() - 1];
        let parent_module_path = join_module_path(parent_segments);
        let old_name = old_segments.last().unwrap().to_string();
        let mut new_segments = parent_segments.to_vec();
        new_segments.push(new_name.to_string());
        if let Some(parent_file) = self.registry.module_files.get(&parent_module_path).cloned() {
            if let Some(ast) = self.registry.asts.get_mut(&parent_file) {
                let mut changed = rename_mod_decl(ast, &old_name, new_name);
                if strip_mod_path_attr(ast, &old_name) {
                    changed = true;
                }
                if changed {
                    touched.insert(parent_file);
                }
            }
        }
        if let Some(module_file) = self.registry.module_files.get(old_module_path).cloned() {
            if module_file.file_name().and_then(|n| n.to_str()) == Some("mod.rs") {
                if let Some(parent) = module_file.parent() {
                    if let Some(grand) = parent.parent() {
                        let new_dir = grand.join(new_name);
                        file_moves.push((parent.to_path_buf(), new_dir));
                    }
                }
            } else {
                let new_file_name = format!("{new_name}.rs");
                let new_file = module_file.with_file_name(new_file_name);
                file_moves.push((module_file, new_file));
            }
        } else {
            return Err(anyhow!("no file for module path {old_module_path}"));
        }
        for (path, ast) in self.registry.asts.iter_mut() {
            let mut rewriter = PathRewriter::replace_prefix(&old_segments, &new_segments);
            if rewriter.visit_file(ast) {
                touched.insert(path.clone());
            }
        }
        Ok((touched, file_moves))
    }

    fn apply_dir_rename(&mut self, old_dir: &Path, new_dir: &Path) -> Result<(HashSet<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        let mut touched = HashSet::new();
        let mut file_moves = Vec::new();
        let old_dir = canonicalize_relative(old_dir, &self.source_root)?;
        let new_dir = canonicalize_relative(new_dir, &self.source_root)?;
        let old_segments = module_path_for_dir(&self.source_root, &old_dir)?;
        let new_segments = module_path_for_dir(&self.source_root, &new_dir)?;
        if old_segments.len() < 2 || new_segments.len() < 2 {
            return Err(anyhow!("directory rename must be under source root"));
        }
        let old_name = old_segments.last().unwrap().to_string();
        let new_name = new_segments.last().unwrap().to_string();
        let parent_segments = &old_segments[..old_segments.len() - 1];
        let parent_module_path = join_module_path(parent_segments);
        if let Some(parent_file) = self.registry.module_files.get(&parent_module_path).cloned() {
            if let Some(ast) = self.registry.asts.get_mut(&parent_file) {
                let mut changed = rename_mod_decl(ast, &old_name, &new_name);
                if strip_mod_path_attr(ast, &old_name) {
                    changed = true;
                }
                if changed {
                    touched.insert(parent_file);
                }
            }
        }
        for (path, ast) in self.registry.asts.iter_mut() {
            let mut rewriter = PathRewriter::replace_prefix(&old_segments, &new_segments);
            if rewriter.visit_file(ast) {
                touched.insert(path.clone());
            }
        }
        file_moves.push((old_dir, new_dir));
        Ok((touched, file_moves))
    }

    fn rewrite_sources_for(&mut self, touched: &HashSet<PathBuf>) -> Result<()> {
        for path in touched {
            if let Some(ast) = self.registry.asts.get(path) {
                let rendered = prettyplease::unparse(ast);
                self.registry.sources.insert(path.clone(), rendered);
            }
        }
        Ok(())
    }

    fn rebuild_registry(&mut self) -> Result<()> {
        self.registry.handles.clear();
        self.registry.module_files.clear();
        for (file, ast) in &self.registry.asts {
            let module_path = module_path_from_file(&self.source_root, file)?;
            self.registry.module_files.insert(module_path.clone(), file.clone());
            super::project_editor_load::index_file_symbols(ast, file, &module_path, &mut self.registry.handles);
        }
        Ok(())
    }
}

fn remove_top_level_item(ast: &mut syn::File, name: &str, kind: &SymbolKind) -> Option<Item> {
    let mut idx = None;
    for (i, item) in ast.items.iter().enumerate() {
        let matches = match (item, kind) {
            (Item::Fn(item_fn), SymbolKind::Fn) => item_fn.sig.ident == name,
            (Item::Struct(item_struct), SymbolKind::Struct) => item_struct.ident == name,
            (Item::Enum(item_enum), SymbolKind::Enum) => item_enum.ident == name,
            (Item::Const(item_const), SymbolKind::Const) => item_const.ident == name,
            (Item::Static(item_static), SymbolKind::Static) => item_static.ident == name,
            (Item::Type(item_type), SymbolKind::Type) => item_type.ident == name,
            (Item::Trait(item_trait), SymbolKind::Trait) => item_trait.ident == name,
            (Item::Mod(item_mod), SymbolKind::Module) => item_mod.ident == name,
            _ => false,
        };
        if matches {
            idx = Some(i);
            break;
        }
    }
    idx.map(|i| ast.items.remove(i))
}

fn rename_mod_decl(ast: &mut syn::File, old_name: &str, new_name: &str) -> bool {
    let mut changed = false;
    for item in &mut ast.items {
        if let Item::Mod(item_mod) = item {
            if item_mod.ident == old_name {
                item_mod.ident = syn::Ident::new(new_name, Span::call_site());
                changed = true;
            }
        }
    }
    changed
}

fn strip_mod_path_attr(ast: &mut syn::File, old_name: &str) -> bool {
    let mut changed = false;
    for item in &mut ast.items {
        let Item::Mod(item_mod) = item else { continue };
        if item_mod.ident != old_name {
            continue;
        }
        let before = item_mod.attrs.len();
        item_mod.attrs.retain(|attr| !attr.path().is_ident("path"));
        if item_mod.attrs.len() != before {
            changed = true;
        }
    }
    changed
}
