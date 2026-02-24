use std::env;
use std::path::PathBuf;
use std::process::Command;

// All .cu files grouped by domain directory.
// Each entry: (source path relative to crate root, object name)
const CUDA_SOURCES: &[(&str, &str)] = &[
    ("src/graph/bfs.cu", "graph_bfs"),
    ("src/graph/bellman_ford.cu", "graph_bellman_ford"),
    ("src/graph/csr_unified.cu", "graph_csr_unified"),
    ("src/control_flow/dominators.cu", "control_flow_dominators"),
    ("src/control_flow/dataflow.cu", "control_flow_dataflow"),
    ("src/cryptography/merkle_tree.cu", "cryptography_merkle_tree"),
    ("src/sorting/bitonic_sort.cu", "sorting_bitonic_sort"),
    ("src/searching/linear_search.cu", "searching_linear_search"),
    ("src/numerical/matrix_multiply.cu", "numerical_matrix_multiply"),
    ("src/numerical/sieve.cu", "numerical_sieve"),
    ("src/string_algorithms/rabin_karp.cu", "string_rabin_karp"),
    ("src/optimization/genetic_algorithm.cu", "optimization_genetic_algorithm"),
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
        let status = Command::new(&nvcc)
            .args([*src, "-c", "-o"])
            .arg(&obj)
            .args(["-Xcompiler", "-fPIC", "-std=c++17", "-ccbin", "/usr/bin/g++-11"])
            .status()
            .unwrap_or_else(|_| panic!("nvcc failed to start for {}", src));
        if !status.success() {
            panic!("nvcc compilation failed for {}", src);
        }

        obj_paths.push(obj);
    }

    // ar: pack all .o files into a single libgpu.a
    let lib = out_dir.join("libgpu.a");
    let mut ar_cmd = Command::new(&ar);
    ar_cmd.args(["crus", lib.to_str().unwrap()]);
    for obj in &obj_paths {
        ar_cmd.arg(obj);
    }
    let status = ar_cmd.status().expect("ar failed to start");
    if !status.success() {
        panic!("ar failed");
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=gpu");
    println!("cargo:rustc-link-search=native={}/lib64", cuda_home);
    println!("cargo:rustc-link-lib=dylib=cudart");
    println!("cargo:rustc-link-lib=dylib=curand");
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
