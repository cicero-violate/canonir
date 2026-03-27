use crate::artifacts::{emit_graph_artifact_summary, write_graph_artifact, CaptureMode};
use crate::runtime::flags::{
    find_flag_value, find_flag_values, is_cargo_registry_path, workspace_root_from_output_dir,
};
use crate::runtime::crate_runtime::should_capture_crate;
use crate::log::{append_rustc_log, emit_ir_tlog, install_panic_hook, set_panic_log_root, TlogWriter};
use crate::capture::{collect_spans_and_symbols, collect_symbol_spans, SymbolSpanBundle};
use rustc_driver::{Callbacks, Compilation};
use rustc_interface::interface::Compiler;
use rustc_middle::ty::TyCtxt;
use rustc_span::FileName;
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
                Ok(ir) => {
                    let bundle = collect_spans_and_symbols(
                        tcx,
                        &self.output_dir,
                        crate_name,
                    )
                    .unwrap_or_else(|_| SymbolSpanBundle {
                        spans_by_symbol: collect_symbol_spans(tcx),
                        kinds: std::collections::HashMap::new(),
                    });
                    if let Ok(summary) = write_graph_artifact(&workspace_root, crate_name, &ir, Some(&bundle)) {
                        let _ = emit_graph_artifact_summary(&tlog_path, &summary);
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
                    if matches!(std::env::var("CANON_RUSTC_STRICT").as_deref(), Ok("1" | "true" | "TRUE")) {
                        std::process::exit(1);
                    }
                }
            };

        }

        Compilation::Continue
    }
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
