use anyhow::Result;
use rustc_hir::def::{DefKind, Res};
use rustc_hir::intravisit::{self, Visitor};
use rustc_middle::ty::TyCtxt;
use rustc_span::source_map::SourceMap;
use rustc_span::{FileName, Span};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn collect_spans_and_symbols(
    tcx: TyCtxt<'_>,
    output_dir: &Path,
    crate_name: &str,
) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;
    let spans_path = output_dir.join("spans.jsonl");
    let emitted = load_emitted_source_files(&spans_path);
    let file = OpenOptions::new().create(true).append(true).open(&spans_path)?;
    let writer = BufWriter::new(file);
    let mut collector = SpanCollector::new(writer, crate_name.to_string(), emitted);
    collector.collect(tcx)?;
    collector.finalize()?;
    let symbols_path = output_dir.join("symbols.json");
    write_symbols_json(&symbols_path, &collector.symbol_kinds)?;
    Ok(())
}

struct SpanCollector {
    def_id_to_symbol: HashMap<rustc_hir::def_id::DefId, String>,
    symbol_kinds: HashMap<String, String>,
    out: BufWriter<File>,
    span_count: usize,
    emitted_source_files: HashSet<PathBuf>,
    crate_name: String,
}

impl SpanCollector {
    fn new(out: BufWriter<File>, crate_name: String, emitted_source_files: HashSet<PathBuf>) -> Self {
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
        if writeln!(self.out, "{line}").is_ok() {
            self.span_count += 1;
        }
    }

    fn emit_def_span(&mut self, symbol_id: &str, source_map: &SourceMap, span: Span) {
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
        if writeln!(self.out, "{line}").is_ok() {
            self.span_count += 1;
        }
    }
}

fn def_kind_to_symbol_kind(kind: DefKind) -> &'static str {
    match kind {
        DefKind::Fn | DefKind::AssocFn => "fn",
        DefKind::Struct => "struct",
        DefKind::Enum => "enum",
        DefKind::Const | DefKind::AssocConst => "const",
        DefKind::Static { .. } => "static",
        DefKind::TyAlias | DefKind::AssocTy => "type",
        DefKind::Trait => "trait",
        DefKind::Mod => "module",
        _ => "unknown",
    }
}

fn def_id_symbol_for_item(
    sink: &mut SpanCollector,
    item: &rustc_hir::Item<'_>,
) -> Option<String> {
    let def_id = item.owner_id.to_def_id();
    sink.def_id_to_symbol.get(&def_id).cloned()
}

fn item_ident_span(item: &rustc_hir::Item<'_>) -> Option<Span> {
    item.kind.ident().map(|ident| ident.span)
}

struct PathVisitor<'sm, 'cb, 'v> {
    source_map: &'sm SourceMap,
    sink: &'cb mut SpanCollector,
    tcx: TyCtxt<'v>,
}

impl<'sm, 'cb, 'v> Visitor<'v> for PathVisitor<'sm, 'cb, 'v> {
    type NestedFilter = rustc_middle::hir::nested_filter::All;

    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt {
        self.tcx
    }

    fn visit_item(&mut self, item: &'v rustc_hir::Item<'v>) {
        if item.span.from_expansion() {
            return;
        }
        if let Some(symbol_id) = def_id_symbol_for_item(&mut *self.sink, item) {
            if let Some(ident_span) = item_ident_span(item) {
                self.sink.emit_span(&symbol_id, self.source_map, ident_span);
            }
        }
        if let rustc_hir::ItemKind::Use(path, use_kind) = &item.kind {
            match use_kind {
                rustc_hir::UseKind::Single(ident) => {
                    for res in path.res.present_items() {
                        if let Res::Def(_, def_id) = res {
                            if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                                self.sink
                                    .emit_span(&symbol_id, self.source_map, ident.span);
                            }
                        }
                    }
                }
                rustc_hir::UseKind::Glob => {
                    if let Some(seg) = path.segments.last() {
                        for res in path.res.present_items() {
                            if let Res::Def(_, def_id) = res {
                                if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                                    self.sink
                                        .emit_span(&symbol_id, self.source_map, seg.ident.span);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        intravisit::walk_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'v rustc_hir::ImplItem<'v>) {
        if item.span.from_expansion() {
            return;
        }
        let def_id = item.owner_id.to_def_id();
        if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id).cloned() {
            self.sink
                .emit_span(&symbol_id, self.source_map, item.ident.span);
        }
        intravisit::walk_impl_item(self, item);
    }

    fn visit_trait_item(&mut self, item: &'v rustc_hir::TraitItem<'v>) {
        if item.span.from_expansion() {
            return;
        }
        let def_id = item.owner_id.to_def_id();
        if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id).cloned() {
            self.sink
                .emit_span(&symbol_id, self.source_map, item.ident.span);
        }
        intravisit::walk_trait_item(self, item);
    }

    fn visit_ty(&mut self, ty: &'v rustc_hir::Ty<'v, rustc_hir::AmbigArg>) {
        if let rustc_hir::TyKind::Path(qpath) = &ty.kind {
            self.emit_qpath_span(qpath);
        }
        intravisit::walk_ty(self, ty);
    }

    fn visit_expr(&mut self, expr: &'v rustc_hir::Expr<'v>) {
        match &expr.kind {
            rustc_hir::ExprKind::Struct(qpath, ..) => {
                self.emit_qpath_span(qpath);
            }
            rustc_hir::ExprKind::Path(qpath) => match qpath {
                rustc_hir::QPath::TypeRelative(_, segment) => {
                    let hir_id = expr.hir_id;
                    let res = self
                        .tcx
                        .typeck(hir_id.owner.def_id)
                        .qpath_res(qpath, hir_id);
                    if let Res::Def(_, def_id) = res {
                        if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                            self.sink
                                .emit_span(&symbol_id, self.source_map, segment.ident.span);
                        }
                    }
                }
                _ => {
                    self.emit_qpath_span(qpath);
                }
            },
            rustc_hir::ExprKind::MethodCall(segment, _receiver, _, _) => {
                let hir_id = expr.hir_id;
                if let Some(def_id) = self
                    .tcx
                    .typeck(hir_id.owner.def_id)
                    .type_dependent_def_id(hir_id)
                {
                    if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                        self.sink
                            .emit_span(&symbol_id, self.source_map, segment.ident.span);
                    }
                }
            }
            _ => {}
        }
        intravisit::walk_expr(self, expr);
    }

    fn visit_use(
        &mut self,
        path: &'v rustc_hir::UsePath<'v>,
        hir_id: rustc_hir::HirId,
    ) {
        intravisit::walk_use(self, path, hir_id);
    }

    fn visit_variant(&mut self, v: &'v rustc_hir::Variant<'v>) {
        if v.span.from_expansion() {
            return;
        }
        let def_id = v.def_id.to_def_id();
        if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
            self.sink
                .emit_span(&symbol_id, self.source_map, v.ident.span);
        }
        intravisit::walk_variant(self, v);
    }

    fn visit_pat(&mut self, pat: &'v rustc_hir::Pat<'v>) {
        match &pat.kind {
            rustc_hir::PatKind::TupleStruct(qpath, _, _) => {
                self.emit_qpath_span(qpath);
            }
            rustc_hir::PatKind::Struct(qpath, _, _) => {
                self.emit_qpath_span(qpath);
            }
            rustc_hir::PatKind::Expr(pat_expr) => {
                if let rustc_hir::PatExprKind::Path(qpath) = pat_expr.kind {
                    self.emit_qpath_span(&qpath);
                }
            }
            _ => {}
        }
        intravisit::walk_pat(self, pat);
    }
}

impl<'sm, 'cb, 'v> PathVisitor<'sm, 'cb, 'v> {
    fn symbol_id_for_def(&mut self, def_id: rustc_hir::def_id::DefId) -> Option<String> {
        if let Some(symbol_id) = self.sink.def_id_to_symbol.get(&def_id).cloned() {
            return Some(symbol_id);
        }
        let path = self.tcx.def_path_str(def_id);
        let local_path = if def_id.is_local() {
            Some(path.as_str())
        } else {
            let crate_name = self.sink.crate_name.as_str();
            let normalized = crate_name.replace('-', "_");
            let prefix = format!("{crate_name}::");
            let prefix_norm = format!("{normalized}::");
            if path == crate_name || path == normalized {
                Some("crate")
            } else if path.starts_with(&prefix) {
                Some(&path[prefix.len()..])
            } else if path.starts_with(&prefix_norm) {
                Some(&path[prefix_norm.len()..])
            } else {
                None
            }
        };
        let Some(local_path) = local_path else { return None };
        let symbol_id = if local_path == "crate" {
            "crate".to_string()
        } else {
            format!("crate::{local_path}")
        };
        let kind = def_kind_to_symbol_kind(self.tcx.def_kind(def_id));
        self.sink
            .symbol_kinds
            .entry(symbol_id.clone())
            .or_insert_with(|| kind.to_string());
        self.sink.def_id_to_symbol.insert(def_id, symbol_id.clone());
        Some(symbol_id)
    }

    fn emit_qpath_span(&mut self, qpath: &rustc_hir::QPath<'_>) {
        match qpath {
            rustc_hir::QPath::Resolved(_, path) => {
                if let Res::Def(_, def_id) = path.res {
                    if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                        let short = symbol_id.rsplit("::").next().unwrap_or("");
                        for seg in path.segments.iter() {
                            if seg.ident.as_str() == short {
                                self.sink
                                    .emit_span(&symbol_id, self.source_map, seg.ident.span);
                            }
                        }
                    }
                    let def_kind = self.tcx.def_kind(def_id);
                    let mut enum_def_id = None;
                    match def_kind {
                        rustc_hir::def::DefKind::Variant => {
                            enum_def_id = Some(self.tcx.parent(def_id));
                        }
                        rustc_hir::def::DefKind::Ctor(_, _) => {
                            let variant_def_id = self.tcx.parent(def_id);
                            if self.tcx.def_kind(variant_def_id)
                                == rustc_hir::def::DefKind::Variant
                            {
                                enum_def_id = Some(self.tcx.parent(variant_def_id));
                            }
                        }
                        _ => {}
                    }
                    if let Some(enum_def_id) = enum_def_id {
                        if let Some(enum_symbol_id) = self.symbol_id_for_def(enum_def_id) {
                            let short = enum_symbol_id.rsplit("::").next().unwrap_or("");
                            if path.segments.len() >= 2 {
                                let seg = &path.segments[path.segments.len() - 2];
                                if seg.ident.as_str() == short {
                                    self.sink
                                        .emit_span(&enum_symbol_id, self.source_map, seg.ident.span);
                                }
                            }
                        }
                    }
                }
            }
            rustc_hir::QPath::TypeRelative(_, segment) => {
                if let Res::Def(_, def_id) = segment.res {
                    if let Some(symbol_id) = self.symbol_id_for_def(def_id) {
                        self.sink
                            .emit_span(&symbol_id, self.source_map, segment.ident.span);
                    }
                }
            }
            _ => {}
        }
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

fn load_emitted_source_files(spans_path: &Path) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    let file = match File::open(spans_path) {
        Ok(f) => f,
        Err(_) => return out,
    };
    let reader = BufReader::new(file);
    for line in reader.lines().flatten() {
        if !line.contains("\"type\":\"source\"") {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
            if value.get("type").and_then(|v| v.as_str()) != Some("source") {
                continue;
            }
            if let Some(file) = value.get("file").and_then(|v| v.as_str()) {
                out.insert(PathBuf::from(file));
            }
        }
    }
    out
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
            if !trait_part.starts_with("crate::") {
                return "external_trait_impl";
            }
        }
    }
    if is_known_external_trait_method(symbol_id) {
        return "external_trait_impl";
    }
    "safe"
}

fn extract_trait_from_impl_symbol(symbol_id: &str) -> Option<&str> {
    let as_pos = symbol_id.find(" as ")?;
    let after_as = &symbol_id[as_pos + 4..];
    let end = after_as.find('>')?;
    Some(&after_as[..end])
}

fn is_known_external_trait_method(symbol_id: &str) -> bool {
    const EXTERNAL_TRAITS: &[&str] = &[
        "std::fmt::Display",
        "std::fmt::Debug",
        "std::fmt::Write",
        "std::fmt::LowerHex",
        "std::fmt::UpperHex",
        "std::convert::From",
        "std::convert::Into",
        "std::convert::TryFrom",
        "std::convert::TryInto",
        "std::convert::AsRef",
        "std::convert::AsMut",
        "std::clone::Clone",
        "std::default::Default",
        "std::ops::Add",
        "std::ops::Sub",
        "std::ops::Mul",
        "std::ops::Div",
        "std::ops::Neg",
        "std::ops::Not",
        "std::ops::Index",
        "std::ops::IndexMut",
        "std::ops::Deref",
        "std::ops::DerefMut",
        "std::ops::Drop",
        "std::iter::Iterator",
        "std::iter::IntoIterator",
        "std::iter::FromIterator",
        "std::cmp::PartialEq",
        "std::cmp::Eq",
        "std::cmp::PartialOrd",
        "std::cmp::Ord",
        "std::hash::Hash",
        "std::error::Error",
        "std::str::FromStr",
        "std::io::Read",
        "std::io::Write",
        "std::io::Seek",
        "serde::Serialize",
        "serde::Deserialize",
        "async_trait",
    ];
    EXTERNAL_TRAITS.iter().any(|name| symbol_id.contains(name))
}
