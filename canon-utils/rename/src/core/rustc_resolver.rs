#![cfg(feature = "rustc_frontend")]

use anyhow::{anyhow, Result};
use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def::Res;
use rustc_hir::intravisit::{self, Visitor};
use rustc_interface::interface;
use rustc_span::source_map::SourceMap;
use rustc_span::{FileName, Span};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct SpanRange {
    pub lo: usize,
    pub hi: usize,
}

pub struct RustcResolver {
    project_root: PathBuf,
    source_root: PathBuf,
    crate_name: String,
    sysroot: PathBuf,
}

impl RustcResolver {
    pub fn new(project_root: &Path) -> Result<Self> {
        let source_root = determine_source_root(project_root);
        let crate_name = infer_crate_name(project_root)?;
        let sysroot = find_sysroot()?;
        Ok(Self { project_root: project_root.to_path_buf(), source_root, crate_name, sysroot })
    }

    pub fn collect_occurrences(&self, symbol_id: &str) -> Result<HashMap<PathBuf, Vec<SpanRange>>> {
        let input = self.source_root.join("lib.rs");
        if !input.exists() {
            return Err(anyhow!("expected lib.rs under {}", self.source_root.display()));
        }
        let mut callback = CollectorCallbacks::new(symbol_id.to_string());
        let args = vec![
            "rustc".to_string(),
            input.display().to_string(),
            "--crate-name".to_string(),
            self.crate_name.clone(),
            "--crate-type".to_string(),
            "lib".to_string(),
            "--edition".to_string(),
            "2021".to_string(),
            "--sysroot".to_string(),
            self.sysroot.display().to_string(),
        ];
        rustc_driver::run_compiler(&args, &mut callback);
        callback.into_result()
    }
}

fn determine_source_root(project: &Path) -> PathBuf {
    let src = project.join("src");
    if src.is_dir() {
        src
    } else {
        project.to_path_buf()
    }
}

fn infer_crate_name(project_root: &Path) -> Result<String> {
    let cargo_toml = project_root.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml)?;
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("name") {
            if let Some(eq_idx) = rest.find('=') {
                let value = rest[eq_idx + 1..].trim().trim_matches('"');
                if !value.is_empty() {
                    return Ok(value.to_string());
                }
            }
        }
    }
    Err(anyhow!("could not infer crate name from {}", cargo_toml.display()))
}

fn find_sysroot() -> Result<PathBuf> {
    if let Ok(sysroot) = std::env::var("RUSTC_SYSROOT") {
        return Ok(PathBuf::from(sysroot));
    }
    let output = std::process::Command::new("rustc")
        .arg("--print")
        .arg("sysroot")
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("failed to run rustc --print sysroot"));
    }
    let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sysroot.is_empty() {
        return Err(anyhow!("empty sysroot from rustc --print sysroot"));
    }
    Ok(PathBuf::from(sysroot))
}

struct CollectorCallbacks {
    target_symbol: String,
    occurrences: HashMap<PathBuf, Vec<SpanRange>>,
    errors: Vec<String>,
}

impl CollectorCallbacks {
    fn new(target_symbol: String) -> Self {
        Self { target_symbol, occurrences: HashMap::new(), errors: Vec::new() }
    }

    fn into_result(self) -> Result<HashMap<PathBuf, Vec<SpanRange>>> {
        if self.errors.is_empty() {
            Ok(self.occurrences)
        } else {
            Err(anyhow!(self.errors.join("\n")))
        }
    }

    fn record_span(&mut self, source_map: &SourceMap, span: Span) {
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
        let range = SpanRange { lo: lo.pos.0 as usize, hi: hi.pos.0 as usize };
        self.occurrences.entry(path).or_default().push(range);
    }
}

impl Callbacks for CollectorCallbacks {
    fn config(&mut self, config: &mut interface::Config) {
        config.opts.unstable_opts.incremental_ignore_spans = true;
    }

    fn after_analysis<'tcx>(&mut self, _compiler: &interface::Compiler, tcx: rustc_middle::ty::TyCtxt<'tcx>) -> Compilation {
        if let Err(err) = self.collect(tcx) {
            self.errors.push(err.to_string());
        }
        Compilation::Stop
    }
}

impl CollectorCallbacks {
    fn collect<'tcx>(&mut self, tcx: rustc_middle::ty::TyCtxt<'tcx>) -> Result<()> {
        let source_map = tcx.sess.source_map();
        let target_def = find_def_id(tcx, &self.target_symbol);
        let Some(target_def) = target_def else {
            return Err(anyhow!("symbol not found via rustc: {}", self.target_symbol));
        };
        let def_span = tcx.def_span(target_def);
        self.record_span(source_map, def_span);
        let mut visitor = PathVisitor { tcx, target_def, source_map, sink: self };
        let hir = tcx.hir();
        for id in hir.items() {
            let item = hir.item(id);
            visitor.visit_item(item);
        }
        for id in hir.body_owners() {
            let body = hir.body(id);
            visitor.visit_body(body);
        }
        Ok(())
    }
}

fn find_def_id<'tcx>(tcx: rustc_middle::ty::TyCtxt<'tcx>, target: &str) -> Option<rustc_hir::def_id::DefId> {
    let hir = tcx.hir();
    for item_id in hir.items() {
        let def_id = hir.local_def_id(item_id);
        let path = tcx.def_path_str(def_id.to_def_id());
        if path == target {
            return Some(def_id.to_def_id());
        }
    }
    None
}

struct PathVisitor<'a, 'tcx> {
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    target_def: rustc_hir::def_id::DefId,
    source_map: &'a SourceMap,
    sink: &'a mut CollectorCallbacks,
}

impl<'a, 'tcx> Visitor<'tcx> for PathVisitor<'a, 'tcx> {
    fn visit_path(&mut self, path: &rustc_hir::Path<'_>, _id: rustc_hir::HirId) {
        if let Res::Def(_, def_id) = path.res {
            if def_id == self.target_def {
                if let Some(seg) = path.segments.last() {
                    self.sink.record_span(self.source_map, seg.ident.span);
                }
            }
        }
        intravisit::walk_path(self, path);
    }
}
