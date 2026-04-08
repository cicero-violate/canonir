use crate::artifacts::{
    emit_capture_completed, emit_capture_failed, emit_capture_started,
    emit_graph_artifact_summary_with_parents, write_graph_artifact, CaptureMode,
};
use crate::runtime::flags::{
    find_flag_value, find_flag_values, is_cargo_registry_path, workspace_root_from_output_dir,
};
use crate::runtime::crate_runtime::should_capture_crate;
use crate::log::{append_rustc_log, emit_ir_tlog, install_panic_hook, set_panic_log_root, TlogWriter};
use crate::capture::{collect_spans_and_symbols, collect_symbol_spans, SymbolSpanBundle};
use canon_ir::{csr_graph::CsrGraph, ir::CanonCsr, CanonIR};
use rustc_driver::{Callbacks, Compilation};
use rustc_interface::interface::Compiler;
use rustc_middle::ty::TyCtxt;
use rustc_span::FileName;
use std::collections::BTreeSet;
use std::path::PathBuf;

pub struct RustcCaptureCallbacks {
    output_dir: PathBuf,
    crate_name: Option<String>,
    crate_types: Vec<String>,
    capture_mode: CaptureMode,
}

impl RustcCaptureCallbacks {
    pub fn new(argv: &[String]) -> Self {
        let crate_name = find_flag_value(argv, "--crate-name");
        let crate_types = find_flag_values(argv, "--crate-type");
        let output_dir = find_flag_value(argv, "--out-dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        Self {
            output_dir,
            crate_name,
            crate_types,
            capture_mode: CaptureMode::current(),
        }
    }
}

impl Callbacks for RustcCaptureCallbacks {
    fn after_analysis<'tcx>(&mut self, _compiler: &Compiler, tcx: TyCtxt<'tcx>) -> Compilation {
        if is_cargo_registry_path(&self.output_dir) {
            return Compilation::Continue;
        }

        let workspace_root = workspace_root_from_output_dir(&self.output_dir);

        if should_capture_crate(self.crate_name.as_deref(), &self.crate_types)
            && is_workspace_crate(tcx, &workspace_root)
        {
            set_panic_log_root(workspace_root.clone());
            install_panic_hook();
            let crate_name = self.crate_name.as_deref().unwrap_or("unknown");
            let tlog_path = workspace_root.join("state/event_log/event.tlog");
            crate::log::append_rustc_warning_with_root(
                &workspace_root,
                &format!(
                    "canon_kernel: capture_mode={} env_CANON_RUSTC_CAPTURE_MODE={:?}",
                    self.capture_mode.as_str(),
                    std::env::var("CANON_RUSTC_CAPTURE_MODE").ok()
                ),
            );
            let capture_started_id = emit_capture_started(&tlog_path, crate_name, self.capture_mode).ok();
            if let Ok(mut writer) = TlogWriter::open(&tlog_path) {
                if let Err(err) = writer.write_session(crate_name) {
                    append_rustc_log(
                        &self.output_dir,
                        &format!("canon_kernel: session emit failed: {err:?}"),
                    );
                }
            } else {
                append_rustc_log(
                    &self.output_dir,
                    "canon_kernel: session emit failed: tlog open error",
                );
            }
            match crate::capture::capture(tcx) {
                Ok(mut ir) => {
                    if self.capture_mode == CaptureMode::Sparse {
                        prune_ir_for_sparse(&mut ir);
                    }
                    let bundle = collect_spans_and_symbols(
                        tcx,
                        &self.output_dir,
                        crate_name,
                    )
                    .unwrap_or_else(|_| SymbolSpanBundle {
                        spans_by_symbol: collect_symbol_spans(tcx),
                        kinds: std::collections::HashMap::new(),
                    });
                    let file_count = workspace_file_count(tcx, &workspace_root);
                    if let Ok(summary) =
                        write_graph_artifact(&workspace_root, crate_name, &ir, Some(&bundle), Some(file_count))
                    {
                        let artifact_event_id = emit_graph_artifact_summary_with_parents(
                            &tlog_path,
                            &summary,
                            capture_started_id.clone().into_iter().collect(),
                        )
                        .ok();
                        let parents = artifact_event_id.into_iter().collect();
                        let _ = emit_capture_completed(&tlog_path, crate_name, &summary.artifact_id, parents);
                    }
                    if self.capture_mode.emits_structural_events()
                        && let Err(err) = emit_ir_tlog(&ir, &tlog_path, crate_name, Some(&bundle))
                    {
                        append_rustc_log(
                            &self.output_dir,
                            &format!("canon_kernel: tlog emit failed: {err:?}"),
                        );
                    }
                    if let Ok(mut writer) = TlogWriter::open(&tlog_path) {
                        let _ = writer.write_compilation_unit_finished(crate_name);
                    }
                }
                Err(err) => {
                    let def_id = self.crate_name.as_deref().unwrap_or("unknown");
                    let message = format!("capture error: {err:?}");
                    crate::log::append_panic_record(def_id, &message);
                    append_rustc_log(
                        &self.output_dir,
                        &format!("canon_kernel: capture failed: {err:?}"),
                    );
                    let mut parents = Vec::new();
                    if let Some(id) = capture_started_id {
                        parents.push(id);
                    }
                    let _ = emit_capture_failed(&tlog_path, crate_name, &message, parents);
                    if matches!(std::env::var("CANON_RUSTC_STRICT").as_deref(), Ok("1" | "true" | "TRUE")) {
                        std::process::exit(1);
                    }
                }
            };

        }

        Compilation::Continue
    }
}

fn prune_ir_for_sparse(ir: &mut CanonIR) {
    // Keep module/call/cfg graphs to populate callgraph/cfg outputs,
    // but drop other structural graphs to keep sparse captures lean.
    ir.name_graph = CsrGraph::empty();
    ir.type_graph = CsrGraph::empty();
    ir.region_graph = CsrGraph::empty();
    ir.value_graph = CsrGraph::empty();
    ir.macro_graph = CsrGraph::empty();
    ir.graph_csr = CanonCsr::default();
    ir.graph_csr_rev = CanonCsr::default();
}

fn is_workspace_crate(tcx: TyCtxt<'_>, workspace_root: &PathBuf) -> bool {
    let workspace_root = workspace_root.canonicalize().unwrap_or_else(|_| workspace_root.clone());
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let manifest_dir_path = PathBuf::from(&manifest_dir)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(&manifest_dir));
        if !manifest_dir_path.starts_with(&workspace_root) {
            return false;
        }
        if manifest_dir_path.starts_with(workspace_root.join("target")) {
            return false;
        }
        return true;
    }
    let source_map = tcx.sess.source_map();
    source_map
        .files()
        .iter()
        .filter_map(|f| match &f.name {
            FileName::Real(rn) => rn.local_path().map(|p| p.to_path_buf()),
            _ => None,
        })
        .any(|path| {
            let abs = path.canonicalize().unwrap_or(path);
            abs.starts_with(&workspace_root)
        })
}

fn workspace_file_count(tcx: TyCtxt<'_>, workspace_root: &PathBuf) -> usize {
    let workspace_root = workspace_root.canonicalize().unwrap_or_else(|_| workspace_root.clone());
    let target_root = workspace_root.join("target");
    let source_map = tcx.sess.source_map();
    let mut files = BTreeSet::new();
    for path in source_map.files().iter().filter_map(|f| match &f.name {
        FileName::Real(real_name) => real_name.local_path().map(|p| p.to_path_buf()),
        _ => None,
    }) {
        if is_cargo_registry_path(&path) {
            continue;
        }
        let abs = path.canonicalize().unwrap_or(path);
        if abs.starts_with(&workspace_root) && !abs.starts_with(&target_root) {
            files.insert(abs);
        }
    }
    files.len()
}
