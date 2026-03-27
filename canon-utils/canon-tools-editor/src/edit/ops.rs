use super::helper::{build_full_path, canonicalize_relative, join_module_path, module_path_for_dir, split_module_path};
use crate::edit::syn_patcher;
use crate::edit::syn_patcher::SpanReplacement;
use crate::structured::{EditOp, SymbolHandle, SymbolKind};
use crate::symbol_index::SymbolIndex;
use anyhow::{anyhow, Result};
use proc_macro2::Span;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use syn::Item;

use super::rewrite::{rewrite_string_attrs_in_file, PathRewriter};
use super::types::{ChangeReport, EditConflict, PendingEdit, ProjectEditor};

impl ProjectEditor {
    pub fn apply(&mut self) -> Result<ChangeReport> {
        let mut touched_files = HashSet::new();
        let conflicts = Vec::new();
        let mut file_moves = Vec::new();

        let mut queued_renames: Vec<(String, String)> = Vec::new();
        let mut remaining_changesets: std::collections::HashMap<PathBuf, Vec<PendingEdit>> = std::collections::HashMap::new();
        for (file, ops) in self.changesets.clone() {
            let mut remaining_ops = Vec::new();
            for op in ops {
                match &op.op {
                    EditOp::MutateField { handle: _, mutation, .. } => match mutation {
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
                    EditOp::MoveSymbol { handle, new_module_path, .. } => {
                        let touched = self.apply_move_symbol(handle, new_module_path)?;
                        touched_files.extend(touched);
                    }
                    EditOp::DeleteSymbol { handle, symbol_id } => {
                        let touched = self.apply_delete_symbol(handle, symbol_id)?;
                        touched_files.extend(touched);
                    }
                    EditOp::MutateField { .. } => {}
                }
            }
        }
        self.changesets.clear();

        self.rewrite_sources_for(&touched_files)?;

        // Rebuild registry after structural edits
        self.rebuild_registry()?;
        self.last_applied_sources.clear();
        for path in &touched_files {
            if let Some(source) = self.registry.sources.get(path) {
                self.last_applied_sources.insert(path.clone(), source.clone());
            }
        }
        // Analysis-first: registry rebuilding should come from analysis, not source scans.
        // Leave the current registry in place; callers should re-run analysis for new targets.

        self.pending_file_moves.extend(file_moves.clone());
        self.last_touched_files = touched_files.clone();

        Ok(ChangeReport { touched_files: touched_files.into_iter().collect(), conflicts, file_moves })
    }

    pub fn validate(&self) -> Result<Vec<EditConflict>> {
        let mut conflicts = Vec::new();
        for (_file, ops) in &self.changesets {
            for queued in ops {
                match &queued.op {
                    EditOp::MutateField { handle, mutation: crate::structured::FieldMutation::RenameIdent(new_name), .. } => {
                        let module = &handle.module_path;
                        let candidate = format!("{module}::{new_name}");
                        if self.registry.handles.contains_key(&candidate) {
                            conflicts.push(EditConflict { symbol_id: queued.symbol_id.clone(), reason: format!("rename would conflict with existing symbol {candidate}") });
                        }
                    }
                    EditOp::MoveSymbol { handle, new_module_path, .. } => {
                        let candidate = format!("{new_module_path}::{}", handle.name);
                        if self.registry.handles.contains_key(&candidate) {
                            conflicts.push(EditConflict { symbol_id: queued.symbol_id.clone(), reason: format!("move would conflict with existing symbol {candidate}") });
                        }
                    }
                    EditOp::DeleteSymbol { .. } => {}
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
        let Some(session) = self.session.as_ref().cloned() else {
            return Err(anyhow!("symbol index not initialized; use ProjectEditor::load"));
        };
        let (per_file, per_file_attr_pairs) = self.collect_symbol_replacements(&session, renames)?;
        if !renames.is_empty() && per_file.is_empty() {
            crate::tlog::publish_invariant_error(
                &self.project_root,
                "rename_symbol_apply",
                "apply invariant: rename produced no replacements",
                serde_json::json!({ "renames": renames }),
            );
            return Err(anyhow!("apply invariant: rename produced no replacements"));
        }
        let mut touched = HashSet::new();
        for (path, mut replacements) in per_file {
            let attr_pairs = per_file_attr_pairs.get(&path).cloned().unwrap_or_default();
            self.apply_replacements_for_file(&session, &path, &mut replacements, &attr_pairs, &mut touched)?;
        }
        Ok(touched)
    }

    fn collect_symbol_replacements(
        &self, session: &SymbolIndex, renames: &[(String, String)],
    ) -> Result<(std::collections::HashMap<PathBuf, Vec<SpanReplacement>>, std::collections::HashMap<PathBuf, Vec<(String, String)>>)> {
        let mut per_file: std::collections::HashMap<PathBuf, Vec<SpanReplacement>> = std::collections::HashMap::new();
        let mut per_file_attr_pairs: std::collections::HashMap<PathBuf, Vec<(String, String)>> = std::collections::HashMap::new();
        for (symbol_id, new_name) in renames {
            let old_ident = symbol_id.rsplit_once("::").map(|(_, s)| s).unwrap_or(symbol_id.as_str()).to_string();
            let norm = crate::edit::symbol_id::normalize_symbol_id(symbol_id);
            let Some(occurrences) = session.spans_for(&norm) else {
                crate::tlog::publish_invariant_error(
                    &self.project_root,
                    "rename_symbol_index",
                    &format!("collect invariant: symbol not found via kernel index: {symbol_id}"),
                    serde_json::json!({ "symbol_id": symbol_id, "normalized": norm }),
                );
                return Err(anyhow!("collect invariant: symbol not found via kernel index: {symbol_id}"));
            };
            if occurrences.is_empty() {
                crate::tlog::publish_invariant_error(
                    &self.project_root,
                    "rename_symbol_apply",
                    &format!("apply invariant: symbol has no references to rename: {symbol_id}"),
                    serde_json::json!({ "symbol_id": symbol_id, "new_name": new_name }),
                );
                return Err(anyhow!("apply invariant: symbol has no references to rename: {symbol_id}"));
            }
            for (path, spans) in occurrences {
                let entry = per_file.entry(path.clone()).or_default();
                for span in spans {
                    entry.push(SpanReplacement { span: span.clone(), replacement: new_name.clone() });
                }
                per_file_attr_pairs.entry(path.clone()).or_default().push((old_ident.clone(), new_name.clone()));
            }
        }
        Ok((per_file, per_file_attr_pairs))
    }

    fn apply_replacements_for_file(
        &mut self, session: &SymbolIndex, path: &PathBuf, replacements: &mut Vec<SpanReplacement>, attr_pairs: &[(String, String)], touched: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        replacements.sort_by(|a, b| a.span.lo.cmp(&b.span.lo).then_with(|| a.span.hi.cmp(&b.span.hi)).then_with(|| a.replacement.cmp(&b.replacement)));
        replacements.dedup_by(|a, b| a.span.lo == b.span.lo && a.span.hi == b.span.hi && a.replacement == b.replacement);
        remove_nested_spans(replacements);
        for window in replacements.windows(2) {
            let a = &window[0];
            let b = &window[1];
            if a.span.lo == b.span.lo && a.span.hi == b.span.hi && a.replacement != b.replacement {
                return Err(anyhow!("conflicting replacements at {}..{} in {}", a.span.lo, a.span.hi, path.display()));
            }
        }
        let Some(source) = self.registry.sources.get(path).cloned() else {
            return Ok(());
        };
        let source = session.normalized_source(path).cloned().unwrap_or(source);
        let mut updated = syn_patcher::patch_file(&source, replacements)?;
        let mut changed = updated != source;
        if let Ok(ast) = syn::parse_file(&updated) {
            let mut ast = ast;
            let mut pairs = attr_pairs.to_vec();
            pairs.sort();
            pairs.dedup();
            let mut attr_changed = false;
            for (old_name, new_name) in &pairs {
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
            touched.insert(path.to_path_buf());
        }
        Ok(())
    }

    fn apply_delete_symbol(&mut self, handle: &SymbolHandle, symbol_id: &str) -> Result<HashSet<PathBuf>> {
        let mut touched = HashSet::new();
        let file = handle.file.clone();
        let ast = self.registry.asts.get_mut(&file).ok_or_else(|| anyhow!("missing AST for {}", file.display()))?;
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
        self.rewrite_paths(RewriteMode::Full { old_full, new_full }, &mut touched);
        Ok(touched)
    }

    fn apply_module_rename(&mut self, old_module_path: &str, new_name: &str) -> Result<(HashSet<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        let uses_crate_prefix = self.registry.module_files.keys().any(|k| k.starts_with("crate::"));
        let normalized = crate::edit::helper::normalize_module_path(old_module_path, uses_crate_prefix);
        let old_segments = split_module_path(&normalized);
        if old_segments.is_empty() {
            return Err(anyhow!("module path must include name"));
        }
        let parent_segments = &old_segments[..old_segments.len() - 1];
        let parent_module_path = join_module_path(parent_segments);
        let old_name = old_segments.last().unwrap().to_string();
        let mut new_segments = parent_segments.to_vec();
        new_segments.push(new_name.to_string());
        let module_file = self.resolve_module_file(&normalized)?;
        let module_move = self.compute_module_move(&module_file, new_name)?;
        let plan = ModuleRenamePlan { old_segments, new_segments, parent_module_path, old_name, new_name: new_name.to_string(), module_move: Some(module_move) };
        self.apply_module_path_rename(plan)
    }

    fn resolve_module_file(&self, old_module_path: &str) -> Result<PathBuf> {
        if let Some(path) = self.registry.module_files.get(old_module_path) {
            return Ok(path.clone());
        }
        Err(anyhow!("no file for module path {old_module_path}"))
    }

    fn compute_module_move(&self, module_file: &Path, new_name: &str) -> Result<(PathBuf, PathBuf)> {
        let file_name = module_file.file_name().and_then(|n| n.to_str());
        if file_name == Some("mod.rs") {
            let parent = module_file.parent().ok_or_else(|| anyhow!("mod.rs missing parent dir"))?;
            let grand = parent.parent().ok_or_else(|| anyhow!("mod.rs missing grandparent dir"))?;
            return Ok((parent.to_path_buf(), grand.join(new_name)));
        }
        let new_file_name = format!("{new_name}.rs");
        Ok((module_file.to_path_buf(), module_file.with_file_name(new_file_name)))
    }

    fn update_parent_mod_decl(&mut self, parent_file: &Path, old_name: &str, new_name: Option<&str>, new_path: Option<&str>) -> bool {
        let Some(ast) = self.registry.asts.get_mut(parent_file) else {
            return false;
        };
        let mut changed = false;
        if let Some(new_name) = new_name {
            if rename_mod_decl(ast, old_name, new_name) {
                changed = true;
            }
        }
        if let Some(new_path) = new_path {
            if update_mod_path_attr(ast, old_name, new_path) {
                changed = true;
            }
        } else if strip_mod_path_attr(ast, old_name) {
            changed = true;
        }
        changed
    }

    fn rewrite_paths(&mut self, mode: RewriteMode, touched: &mut HashSet<PathBuf>) {
        for (path, ast) in self.registry.asts.iter_mut() {
            let mut rewriter = match &mode {
                RewriteMode::Prefix { old_segments, new_segments } => PathRewriter::replace_prefix(old_segments, new_segments),
                RewriteMode::Full { old_full, new_full } => PathRewriter::replace_full(old_full, new_full),
            };
            if rewriter.visit_file(ast) {
                touched.insert(path.clone());
            }
        }
    }

    fn apply_module_path_rename(&mut self, plan: ModuleRenamePlan) -> Result<(HashSet<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        let mut touched = HashSet::new();
        let mut file_moves = Vec::new();

        let parent_file = self.registry.module_files.get(&plan.parent_module_path).cloned().ok_or_else(|| anyhow!("no file for parent module path {}", plan.parent_module_path))?;

        if self.update_parent_mod_decl(&parent_file, &plan.old_name, Some(&plan.new_name), None) {
            touched.insert(parent_file.clone());
        }
        if let Some((_, new_file)) = &plan.module_move {
            let base_dir = parent_file.parent().unwrap_or_else(|| Path::new(""));
            let rel = new_file.strip_prefix(base_dir).unwrap_or(new_file);
            let rel = rel.to_string_lossy().replace('\\', "/");
            if self.update_parent_mod_decl(&parent_file, &plan.old_name, None, Some(rel.as_str())) {
                touched.insert(parent_file);
            }
        }

        self.rewrite_paths_and_collect(&plan.old_segments, &plan.new_segments, &mut touched);

        if let Some(mv) = plan.module_move {
            file_moves.push(mv);
        }
        Ok((touched, file_moves))
    }

    fn rewrite_paths_and_collect(&mut self, old_segments: &[String], new_segments: &[String], touched: &mut HashSet<PathBuf>) {
        self.rewrite_paths(RewriteMode::Prefix { old_segments: old_segments.to_vec(), new_segments: new_segments.to_vec() }, touched);
        let uses_crate_prefix = self.registry.module_files.keys().any(|k| k.starts_with("crate::"));
        if !uses_crate_prefix {
            let mut old_prefixed = Vec::with_capacity(old_segments.len() + 1);
            old_prefixed.push("crate".to_string());
            old_prefixed.extend_from_slice(old_segments);
            let mut new_prefixed = Vec::with_capacity(new_segments.len() + 1);
            new_prefixed.push("crate".to_string());
            new_prefixed.extend_from_slice(new_segments);
            self.rewrite_paths(RewriteMode::Prefix { old_segments: old_prefixed, new_segments: new_prefixed }, touched);
        }

        if let Some(crate_name) = crate::edit::helper::infer_crate_name(&self.project_root) {
            let (old_tail, new_tail) = if old_segments.first().map(|s| s.as_str()) == Some("crate") { (&old_segments[1..], &new_segments[1..]) } else { (old_segments, new_segments) };
            if old_tail.first().map(|s| s.as_str()) != Some(crate_name.as_str()) {
                let mut old_prefixed = Vec::with_capacity(old_tail.len() + 1);
                old_prefixed.push(crate_name.clone());
                old_prefixed.extend_from_slice(old_tail);
                let mut new_prefixed = Vec::with_capacity(new_tail.len() + 1);
                new_prefixed.push(crate_name);
                new_prefixed.extend_from_slice(new_tail);
                self.rewrite_paths(RewriteMode::Prefix { old_segments: old_prefixed, new_segments: new_prefixed }, touched);
            }
        }
    }

    fn apply_dir_rename(&mut self, old_dir: &Path, new_dir: &Path) -> Result<(HashSet<PathBuf>, Vec<(PathBuf, PathBuf)>)> {
        let old_segments = dir_to_module_segments(&self.source_root, old_dir)?;
        let new_segments = dir_to_module_segments(&self.source_root, new_dir)?;
        let old_name = old_segments.last().unwrap().to_string();
        let new_name = new_segments.last().unwrap().to_string();
        let parent_segments = &old_segments[..old_segments.len() - 1];
        let parent_module_path = join_module_path(parent_segments);
        let plan = ModuleRenamePlan { old_segments, new_segments, parent_module_path, old_name, new_name, module_move: Some((old_dir.to_path_buf(), new_dir.to_path_buf())) };
        self.apply_module_path_rename(plan)
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
        Ok(())
    }
}

fn dir_to_module_segments(source_root: &Path, dir: &Path) -> Result<Vec<String>> {
    let dir = canonicalize_relative(dir, source_root)?;
    let segments = module_path_for_dir(source_root, &dir)?;
    if segments.len() < 2 {
        return Err(anyhow!("directory rename must be under source root"));
    }
    Ok(segments)
}

struct ModuleRenamePlan {
    old_segments: Vec<String>,
    new_segments: Vec<String>,
    parent_module_path: String,
    old_name: String,
    new_name: String,
    module_move: Option<(PathBuf, PathBuf)>,
}

enum RewriteMode {
    Prefix { old_segments: Vec<String>, new_segments: Vec<String> },
    Full { old_full: Vec<String>, new_full: Vec<String> },
}

fn remove_nested_spans(replacements: &mut Vec<SpanReplacement>) {
    if replacements.len() < 2 {
        return;
    }
    let mut keep = vec![true; replacements.len()];
    for i in 0..replacements.len() {
        for j in 0..replacements.len() {
            if i == j {
                continue;
            }
            let a = &replacements[i];
            let b = &replacements[j];
            let a_lo = a.span.lo;
            let a_hi = a.span.hi;
            let b_lo = b.span.lo;
            let b_hi = b.span.hi;
            if a_lo <= b_lo && b_hi <= a_hi && (a_lo < b_lo || b_hi < a_hi) {
                keep[i] = false;
            }
        }
    }
    let mut idx = 0usize;
    replacements.retain(|_| {
        let k = keep[idx];
        idx += 1;
        k
    });
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

fn update_mod_path_attr(ast: &mut syn::File, old_name: &str, new_path: &str) -> bool {
    let mut changed = false;
    for item in &mut ast.items {
        let syn::Item::Mod(item_mod) = item else { continue };
        if item_mod.ident == old_name {
            for attr in &mut item_mod.attrs {
                if !attr.path().is_ident("path") {
                    continue;
                }
                if let syn::Meta::NameValue(name_value) = &mut attr.meta {
                    let lit = syn::LitStr::new(new_path, proc_macro2::Span::call_site());
                    name_value.value = syn::Expr::Lit(syn::ExprLit { attrs: Vec::new(), lit: syn::Lit::Str(lit) });
                    changed = true;
                }
            }
        }
    }
    changed
}
