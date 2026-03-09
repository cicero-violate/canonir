#![cfg(feature = "canon_capture_compat")]
#![allow(dead_code)]

use anyhow::Result;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::intravisit::{self, Visitor};
use rustc_middle::ty::TyCtxt;
use rustc_span::source_map::SourceMap;
use rustc_span::{FileName, Span};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// NOTE: This is a verbatim compatibility copy of canon-capture's span/symbol
// collection logic, gated behind the `canon_capture_compat` feature so it
// does not affect default builds. We keep it here to compare symbol naming
// with the UPG extractor and debug mismatches.

pub fn collect_spans_and_symbols(
    tcx: TyCtxt<'_>,
    output_dir: &Path,
    crate_name: &str,
) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;
    let writer: BufWriter<Box<dyn Write>> = BufWriter::new(Box::new(std::io::sink()));
    let mut collector = SpanCollector::new(writer, crate_name.to_string(), HashSet::new());
    collector.collect(tcx)?;
    collector.finalize()?;
    let symbols_path = output_dir.join("symbols.json");
    write_symbols_json(&symbols_path, &collector.symbol_kinds)?;
    Ok(())
}

struct SpanCollector {
    def_id_to_symbol: HashMap<rustc_hir::def_id::DefId, String>,
    symbol_kinds: HashMap<String, String>,
    out: BufWriter<Box<dyn Write>>,
    span_count: usize,
    emitted_source_files: HashSet<PathBuf>,
    crate_name: String,
}

impl SpanCollector {
    fn new(
        out: BufWriter<Box<dyn Write>>,
        crate_name: String,
        emitted_source_files: HashSet<PathBuf>,
    ) -> Self {
        Self {
            def_id_to_symbol: HashMap::new(),
            symbol_kinds: HashMap::new(),
            out,
            span_count: 0,
            emitted_source_files,
            crate_name,
        }
    }

    fn collect(&mut self, tcx: TyCtxt<'_>) -> Result<()> {
        let source_map = tcx.sess.source_map();
        self.def_id_to_symbol.clear();

        let idx = crate::index::build_index(tcx);
        for def_id in idx.def_ids {
            let path = tcx.def_path_str(def_id);
            let symbol_id = if path == "crate" {
                "crate".to_string()
            } else {
                format!("crate::{path}")
            };
            let kind = def_kind_to_symbol_kind(tcx.def_kind(def_id));
            self.symbol_kinds
                .entry(symbol_id.clone())
                .or_insert_with(|| kind.to_string());
            self.def_id_to_symbol.insert(def_id, symbol_id.clone());
        }

        let mut visitor = PathVisitor {
            source_map,
            sink: self,
            tcx,
        };
        tcx.hir_visit_all_item_likes_in_crate(&mut visitor);

        // Always include definition spans so renames touch the defining item.
        let def_items: Vec<_> = self
            .def_id_to_symbol
            .iter()
            .map(|(def_id, symbol_id)| (*def_id, symbol_id.clone()))
            .collect();
        for (def_id, symbol_id) in def_items {
            let span = tcx.def_span(def_id);
            self.emit_def_span(&symbol_id, source_map, span);
        }
        Ok(())
    }

    fn finalize(&mut self) -> Result<()> {
        let line = json!({
            "type": "done",
            "symbol_count": self.symbol_kinds.len(),
            "span_count": self.span_count
        })
        .to_string();
        writeln!(self.out, "{line}")?;
        self.out.flush()?;
        Ok(())
    }

    fn emit_span(&mut self, symbol_id: &str, source_map: &SourceMap, span: Span) {
        if span.from_expansion() {
            return;
        }
        let lo = source_map.lookup_byte_offset(span.lo());
        let hi = source_map.lookup_byte_offset(span.hi());
        if !Arc::ptr_eq(&lo.sf, &hi.sf) {
            return;
        }
        let filename = &lo.sf.name;
        let FileName::Real(real_path) = filename else { return };
        let Some(path) = real_path.local_path().map(|p| p.to_path_buf()) else { return };
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let kind = self
            .symbol_kinds
            .get(symbol_id)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        let lo_pos = lo.pos.0 as usize;
        let hi_pos = hi.pos.0 as usize;

        if self.emitted_source_files.insert(path.clone()) {
            if let Some(src) = lo.sf.src.as_deref() {
                let src_line = json!({
                    "type": "source",
                    "file": path.display().to_string(),
                    "src": src
                })
                .to_string();
                let _ = writeln!(self.out, "{src_line}");
            }
        }

        let line = json!({
            "symbol_id": symbol_id,
            "kind": kind,
            "file": path.display().to_string(),
            "lo": lo_pos,
            "hi": hi_pos
        })
        .to_string();
        let _ = writeln!(self.out, "{line}");
        self.span_count += 1;
    }

    fn emit_def_span(&mut self, symbol_id: &str, source_map: &SourceMap, span: Span) {
        if span.from_expansion() {
            return;
        }
        let lo = source_map.lookup_byte_offset(span.lo());
        let hi = source_map.lookup_byte_offset(span.hi());
        if !Arc::ptr_eq(&lo.sf, &hi.sf) {
            return;
        }
        let filename = &lo.sf.name;
        let FileName::Real(real_path) = filename else { return };
        let Some(path) = real_path.local_path().map(|p| p.to_path_buf()) else { return };
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let kind = self
            .symbol_kinds
            .get(symbol_id)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        let lo_pos = lo.pos.0 as usize;
        let hi_pos = hi.pos.0 as usize;

        if self.emitted_source_files.insert(path.clone()) {
            if let Some(src) = lo.sf.src.as_deref() {
                let src_line = json!({
                    "type": "source",
                    "file": path.display().to_string(),
                    "src": src
                })
                .to_string();
                let _ = writeln!(self.out, "{src_line}");
            }
        }

        let line = json!({
            "symbol_id": symbol_id,
            "kind": kind,
            "file": path.display().to_string(),
            "lo": lo_pos,
            "hi": hi_pos
        })
        .to_string();
        let _ = writeln!(self.out, "{line}");
        self.span_count += 1;
    }
}

struct PathVisitor<'a> {
    tcx: TyCtxt<'a>,
    source_map: &'a SourceMap,
    sink: &'a mut SpanCollector,
}

impl<'a> intravisit::Visitor<'a> for PathVisitor<'a> {
    fn visit_path(&mut self, path: &'a rustc_hir::Path<'a>, id: rustc_hir::HirId) {
        if let Some(res) = path.res {
            match res {
                Res::Def(kind, def_id) => {
                    if is_renameable_def_kind(kind) {
                        if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id) {
                            self.sink.emit_span(symbol_id, self.source_map, path.span);
                        }
                    }
                }
                _ => {}
            }
        }
        intravisit::walk_path(self, path);
        if let Some(parent) = self.tcx.hir_parent_iter(id).next() {
            if let rustc_hir::Node::Item(item) = parent.1 {
                if let rustc_hir::ItemKind::Use(_, _) = item.kind {
                    if let Res::Def(kind, def_id) = path.res {
                        if is_renameable_def_kind(kind) {
                            if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id) {
                                self.sink.emit_span(symbol_id, self.source_map, path.span);
                            }
                        }
                    }
                }
            }
        }
    }

    fn visit_qpath(&mut self, qpath: &'a rustc_hir::QPath<'a>, id: rustc_hir::HirId, span: Span) {
        if let Some(res) = qpath_res(qpath, self.tcx, id) {
            if let Res::Def(kind, def_id) = res {
                if is_renameable_def_kind(kind) {
                    if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id) {
                        self.sink.emit_span(symbol_id, self.source_map, span);
                    }
                }
            }
        }
        intravisit::walk_qpath(self, qpath, id, span);
    }
}

fn qpath_res<'a>(qpath: &'a rustc_hir::QPath<'a>, tcx: TyCtxt<'a>, id: rustc_hir::HirId) -> Option<Res> {
    match qpath {
        rustc_hir::QPath::Resolved(_, path) => path.res,
        rustc_hir::QPath::TypeRelative(_, segment) => segment.res,
        rustc_hir::QPath::LangItem(lang_item, _) => {
            Some(Res::Def(DefKind::Struct, tcx.require_lang_item(*lang_item, None)))
        }
    }
}

fn is_renameable_def_kind(kind: DefKind) -> bool {
    matches!(
        kind,
        DefKind::Mod
            | DefKind::Fn
            | DefKind::Struct
            | DefKind::Enum
            | DefKind::Trait
            | DefKind::TyAlias
            | DefKind::Impl
            | DefKind::AssocFn
            | DefKind::AssocConst
            | DefKind::AssocTy
            | DefKind::Static(_)
            | DefKind::Const
            | DefKind::Ctor(_, _)
    )
}

fn def_kind_to_symbol_kind(kind: DefKind) -> &'static str {
    match kind {
        DefKind::Mod => "module",
        DefKind::Fn => "fn",
        DefKind::Struct => "struct",
        DefKind::Enum => "enum",
        DefKind::Trait => "trait",
        DefKind::TyAlias => "type_alias",
        DefKind::Impl => "impl",
        DefKind::AssocFn => "assoc_fn",
        DefKind::AssocConst => "assoc_const",
        DefKind::AssocTy => "assoc_type",
        DefKind::Static(_) => "static",
        DefKind::Const => "const",
        DefKind::Ctor(_, _) => "ctor",
        _ => "unknown",
    }
}

fn write_symbols_json(path: &Path, symbol_kinds: &HashMap<String, String>) -> Result<()> {
    let mut merged: HashMap<String, String> = load_existing_symbol_kinds(path);
    for (symbol_id, kind) in symbol_kinds {
        merged.entry(symbol_id.clone()).or_insert_with(|| kind.clone());
    }
    let mut entries = Vec::new();
    for (symbol_id, kind) in &merged {
        let new_name = symbol_id.rsplit("::").next().unwrap_or(symbol_id.as_str());
        let safety = classify_rename_safety(symbol_id, kind);
        entries.push(json!({
            "symbol_id": symbol_id,
            "new_name": new_name,
            "kind": kind,
            "rename_safe": safety == "safe",
            "rename_skip_reason": if safety == "safe" { "" } else { safety }
        }));
    }
    entries.sort_by(|a, b| {
        let sa = a.get("rename_safe").and_then(|v| v.as_bool()).unwrap_or(false);
        let sb = b.get("rename_safe").and_then(|v| v.as_bool()).unwrap_or(false);
        sb.cmp(&sa).then_with(|| {
            a.get("symbol_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .cmp(
                    b.get("symbol_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                )
        })
    });
    let content = serde_json::to_string_pretty(&entries)?;
    std::fs::write(path, format!("{content}\n"))?;
    Ok(())
}

fn load_existing_symbol_kinds(path: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return out,
    };
    let parsed = match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let Some(arr) = parsed.as_array() else { return out };
    for item in arr {
        let Some(symbol_id) = item.get("symbol_id").and_then(|v| v.as_str()) else { continue };
        let Some(kind) = item.get("kind").and_then(|v| v.as_str()) else { continue };
        out.insert(symbol_id.to_string(), kind.to_string());
    }
    out
}

fn classify_rename_safety(symbol_id: &str, _kind: &str) -> &'static str {
    if symbol_id.contains(" as ") {
        if let Some(trait_part) = extract_trait_from_impl_symbol(symbol_id) {
            if trait_part.contains("as") || trait_part.contains("dyn") || trait_part.contains("::") {
                return "safe";
            }
            return "skip: impl trait rename would break trait name";
        }
    }
    if symbol_id.contains("{use#") {
        return "skip: rename use binding is unsafe";
    }
    "safe"
}

fn extract_trait_from_impl_symbol(symbol_id: &str) -> Option<String> {
    let impl_part = symbol_id.split(" as ").nth(1)?;
    let trait_part = impl_part.split(" for ").next()?;
    Some(trait_part.to_string())
}
