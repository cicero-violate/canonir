#!/usr/bin/env bash

files=(
src/graph/reachability.rs
src/graph/reachability.cu
src/graph/max_flow.rs
src/graph/max_flow.cu
src/constraints/ac3.rs
src/constraints/forward_checking.rs
src/constraints/constraints.cu
src/sorting/gpu.rs
src/sorting/bitonic_sort.cu
src/searching/gpu.rs
src/searching/linear_search.cu
src/numerical/gpu.rs
src/numerical/matrix_multiply.cu
src/numerical/sieve.cu
src/string_algorithms/gpu.rs
src/string_algorithms/rabin_karp.cu
src/cryptography/merkle_tree_gpu.rs
src/cryptography/merkle_tree.cu
src/graph/gpu.rs
src/graph/csr.rs
Cargo.toml
)

for f in "${files[@]}"; do
    echo "=== $f ==="
    bat -n "$f"
    echo
done
