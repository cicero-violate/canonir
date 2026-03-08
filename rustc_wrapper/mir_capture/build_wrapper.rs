//! Build script for mir_capture wrapper
//! Usage: rustc build_wrapper.rs && ./build_wrapper

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let sysroot_output = Command::new("rustc")
        .args(&["--print", "sysroot"])
        .output()
        .expect("failed to get rustc sysroot");
    let sysroot = String::from_utf8(sysroot_output.stdout)
        .expect("invalid utf8 from rustc")
        .trim()
        .to_string();

    let status = Command::new("cargo")
        .args(["build", "-p", "mir_analysis"])
        .current_dir(".")
        .status()
        .expect("failed to build mir_analysis");
    if !status.success() {
        eprintln!("cargo build failed");
        std::process::exit(1);
    }

    let target_dir = PathBuf::from("../../target/debug");
    let mir_analysis_rlib = target_dir.join("libmir_analysis.rlib");
    let deps_dir = target_dir.join("deps");

    let find_dep = |prefix: &str, suffix: &str| {
        std::fs::read_dir(&deps_dir)
            .expect("failed to read deps dir")
            .filter_map(Result::ok)
            .find(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with(prefix) && name.ends_with(suffix)
            })
            .map(|e| e.path())
            .unwrap_or_else(|| panic!("missing dependency {}*{} in deps", prefix, suffix))
    };

    let anyhow_dep = find_dep("libanyhow-", ".rlib");
    let serde_dep = find_dep("libserde-", ".rlib");
    let serde_derive = find_dep("libserde_derive-", ".so");
    let serde_json_dep = find_dep("libserde_json-", ".rlib");

    let rustc_driver = std::fs::read_dir(format!("{}/lib", sysroot))
        .expect("failed to read sysroot/lib")
        .filter_map(Result::ok)
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("librustc_driver-")
                && e.file_name().to_string_lossy().ends_with(".so")
        })
        .expect("no rustc_driver.so found")
        .path();
    let rustc_interface = std::fs::read_dir(format!("{}/lib/rustlib/x86_64-unknown-linux-gnu/lib", sysroot))
        .expect("failed to read sysroot rustlib lib")
        .filter_map(Result::ok)
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("librustc_interface-")
                && e.file_name().to_string_lossy().ends_with(".rmeta")
        })
        .expect("no rustc_interface rmeta found")
        .path();
    let rustc_middle = std::fs::read_dir(format!("{}/lib/rustlib/x86_64-unknown-linux-gnu/lib", sysroot))
        .expect("failed to read sysroot rustlib lib")
        .filter_map(Result::ok)
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("librustc_middle-")
                && e.file_name().to_string_lossy().ends_with(".rmeta")
        })
        .expect("no rustc_middle rmeta found")
        .path();
    let rustc_session = std::fs::read_dir(format!("{}/lib/rustlib/x86_64-unknown-linux-gnu/lib", sysroot))
        .expect("failed to read sysroot rustlib lib")
        .filter_map(Result::ok)
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("librustc_session-")
                && e.file_name().to_string_lossy().ends_with(".rmeta")
        })
        .expect("no rustc_session rmeta found")
        .path();

    let status = Command::new("rustc")
        .args(&[
            "-Z",
            "unstable-options",
            "-C",
            "prefer-dynamic",
            "-L",
            target_dir.to_str().unwrap(),
            "-L",
            deps_dir.to_str().unwrap(),
            "-L",
            &format!("{}/lib", sysroot),
            "--extern",
            &format!("mir_analysis={}", mir_analysis_rlib.display()),
            "--extern",
            &format!("rustc_driver={}", rustc_driver.display()),
            "--extern",
            &format!("rustc_interface={}", rustc_interface.display()),
            "--extern",
            &format!("rustc_middle={}", rustc_middle.display()),
            "--extern",
            &format!("rustc_session={}", rustc_session.display()),
            "--extern",
            &format!("anyhow={}", anyhow_dep.display()),
            "--extern",
            &format!("serde={}", serde_dep.display()),
            "--extern",
            &format!("serde_derive={}", serde_derive.display()),
            "--extern",
            &format!("serde_json={}", serde_json_dep.display()),
            "--edition",
            "2024",
            "src/main.rs",
            "-o",
            target_dir.join("mir_capture").to_str().unwrap(),
        ])
        .status()
        .expect("failed to compile mir_capture");

    if status.success() {
        println!("✓ Built mir_capture at ../../target/debug/mir_capture");
    } else {
        eprintln!("rustc compilation failed");
        std::process::exit(1);
    }
}
