#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;

use canon::CanonIR;
use std::process::Command;
use std::sync::{Arc, Mutex};

use rustc_driver::Compilation;
use rustc_interface::interface::Compiler;

/// Shared slot to pass the captured IR out of the rustc callback.
type IrSlot = Arc<Mutex<Option<CanonIR>>>;

struct CaptureCallbacks {
    ir_slot: IrSlot,
}

impl rustc_driver::Callbacks for CaptureCallbacks {
    fn after_analysis<'tcx>(&mut self, _compiler: &Compiler, tcx: rustc_middle::ty::TyCtxt<'tcx>) -> Compilation {
        match canon_capture::capture(tcx) {
            Ok(ir) => {
                *self.ir_slot.lock().unwrap() = Some(ir);
            }
            Err(e) => {
                eprintln!("canon_capture: capture failed: {e:?}");
            }
        }
        // Do not continue to codegen — we only need analysis.
        Compilation::Stop
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();

    // Usage: canon_capture <real_rustc> <out_json> <rustc_args...>
    // When used as RUSTC_WRAPPER: canon_capture <real_rustc> <rustc_args...>
    // We detect capture mode via CANON_CAPTURE_OUT env var.

    let real_rustc = argv.get(1).cloned().expect("missing real rustc path");
    let rustc_args: Vec<String> = argv.iter().skip(2).cloned().collect();

    // Skip probes / version / print queries — delegate directly to real rustc.
    let is_probe = rustc_args.iter().any(|a| a.starts_with("--print="))
        || rustc_args.iter().any(|a| a == "-")
        || rustc_args.iter().any(|a| a == "-vV" || a == "--version")
        || rustc_args.windows(2).any(|w| w[0] == "--crate-name" && w[1] == "___");
    let is_build_script = rustc_args.windows(2).any(|w| w[0] == "--crate-name" && w[1] == "build_script_build");

    if is_probe || is_build_script {
        let status = Command::new(&real_rustc).args(&rustc_args).status().expect("failed to exec real rustc (probe)");
        std::process::exit(status.code().unwrap_or(1));
    }

    // For non-primary packages (deps), just run real rustc.
    let is_primary = std::env::var_os("CARGO_PRIMARY_PACKAGE").is_some();
    if !is_primary {
        let status = Command::new(&real_rustc).args(&rustc_args).status().expect("failed to exec real rustc (dep)");
        std::process::exit(status.code().unwrap_or(1));
    }

    // Step 1: run real rustc so cargo bookkeeping stays consistent.
    let status = Command::new(&real_rustc).args(&rustc_args).status().expect("failed to exec real rustc (primary)");
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    // Step 2: run capture pass via rustc_driver on the same args.
    let out_path = std::env::var("CANON_CAPTURE_OUT").unwrap_or_else(|_| "canon_ir_captured.json".to_string());

    let ir_slot: IrSlot = Arc::new(Mutex::new(None));
    let mut callbacks = CaptureCallbacks { ir_slot: Arc::clone(&ir_slot) };

    // rustc_driver::RunCompiler takes argv[0] + rustc_args (no real_rustc prefix).
    let mut driver_args: Vec<String> = vec![argv[0].clone()];
    driver_args.extend(rustc_args.iter().cloned());

    // Suppress rustc output during capture pass.
    let _ = rustc_driver::catch_fatal_errors(|| {
        rustc_driver::run_compiler(&driver_args, &mut callbacks);
    });

    // Step 3: serialize IR to JSON.
    if let Some(ir) = ir_slot.lock().unwrap().take() {
        // Only write once — if the file already exists from a lib capture, skip the bin.
        if std::path::Path::new(&out_path).exists() {
            std::process::exit(0);
        }
        match serde_json::to_string_pretty(&ir) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&out_path, json) {
                    eprintln!("canon_capture: failed to write {out_path}: {e}");
                } else {
                    eprintln!("canon_capture: wrote {out_path}");
                }
            }
            Err(e) => eprintln!("canon_capture: JSON serialize failed: {e}"),
        }
    }

    std::process::exit(0);
}
