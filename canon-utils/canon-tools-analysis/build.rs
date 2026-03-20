use std::env;
use std::path::PathBuf;
use std::process::Command;

// All .cu files grouped by domain directory.
// Each entry: (source path relative to crate root, object name)
const CUDA_SOURCES: &[(&str, &str)] = &[
    ("resources/kernels/create_combined_escape_carry_newline_count_index.cu", "create_combined_escape_carry_newline_count_index"),
    ("resources/kernels/create_combined_escape_newline_index.cu", "create_combined_escape_newline_index"),
    ("resources/kernels/create_escape_carry_index.cu", "create_escape_carry_index"),
    ("resources/kernels/create_escape_index.cu", "create_escape_index"),
    ("resources/kernels/create_leveled_bitmaps.cu", "create_leveled_bitmaps"),
    ("resources/kernels/create_leveled_bitmaps_carry_index.cu", "create_leveled_bitmaps_carry_index"),
    ("resources/kernels/create_newline_index.cu", "create_newline_index"),
    ("resources/kernels/create_quote_index.cu", "create_quote_index"),
    ("resources/kernels/create_string_index.cu", "create_string_index"),
    ("resources/kernels/count_newlines.cu", "count_newlines"),
    ("resources/kernels/find_value.cu", "find_value"),
    ("resources/kernels/user_lang_query.cu", "user_lang_query"),
    ("cuda/gpjson_wrappers.cu", "gpjson_wrappers"),
];

fn main() {
    if std::env::var("CARGO_FEATURE_CUDA").is_err() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let cuda_home = env::var("CUDA_HOME").unwrap_or_else(|_| "/opt/cuda".into());
    let nvcc = PathBuf::from(&cuda_home).join("bin/nvcc");
    let ar = env::var("AR").unwrap_or_else(|_| "ar".into());

    let mut obj_paths: Vec<PathBuf> = Vec::new();

    for (src, name) in CUDA_SOURCES {
        println!("cargo:rerun-if-changed={}", src);

        let obj = out_dir.join(format!("{}.o", name));

        // nvcc: compile .cu -> .o
        let status =
            Command::new(&nvcc).args([*src, "-c", "-o"]).arg(&obj).args(["-Xcompiler", "-fPIC", "-std=c++17", "-ccbin", "/usr/bin/g++-11", "-include", "assert.h"]).status().unwrap_or_else(|err| {
                eprintln!("nvcc failed to start for {}: {}", src, err);
                std::process::exit(1);
            });
        if !status.success() {
            eprintln!("nvcc compilation failed for {}", src);
            std::process::exit(1);
        }

        obj_paths.push(obj);
    }

    // ar: pack all .o files into a single libgpjson_gpu.a
    let lib = out_dir.join("libgpjson_gpu.a");
    let mut ar_cmd = Command::new(&ar);
    ar_cmd.args(["crus", lib.to_str().unwrap()]);
    for obj in &obj_paths {
        ar_cmd.arg(obj);
    }
    let status = match ar_cmd.status() {
        Ok(status) => status,
        Err(err) => {
            eprintln!("ar failed to start: {}", err);
            std::process::exit(1);
        }
    };
    if !status.success() {
        eprintln!("ar failed");
        std::process::exit(1);
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=gpjson_gpu");
    println!("cargo:rustc-link-search=native={}/lib64", cuda_home);
    println!("cargo:rustc-link-lib=dylib=cudart");
    println!("cargo:rustc-link-lib=dylib=curand");
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
