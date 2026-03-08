//! Build script for analysis_capture wrapper
//! Usage: rustc build_wrapper.rs && ./build_wrapper

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let status = Command::new("cargo")
        .args(["build", "-p", "upg_analysis"])
        .env("RUSTC_WRAPPER", "")
        .env("CARGO_BUILD_RUSTC_WRAPPER", "")
        .current_dir(".")
        .status()
        .expect("failed to build upg_analysis");
    if !status.success() {
        eprintln!("cargo build failed");
        std::process::exit(1);
    }

    let _target_dir = PathBuf::from("../../target/debug");

    let status = Command::new("cargo")
        .args(["build", "-p", "analysis_capture"])
        .env("RUSTC_WRAPPER", "")
        .env("CARGO_BUILD_RUSTC_WRAPPER", "")
        .current_dir(".")
        .status()
        .expect("failed to build analysis_capture");

    if status.success() {
        println!("✓ Built analysis_capture at ../../target/debug/analysis_capture");
    } else {
        eprintln!("cargo build analysis_capture failed");
        std::process::exit(1);
    }
}
