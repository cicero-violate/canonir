# Binary Patch and Diff Tool with Rolling Hash and Delta Encoding

This project implements a binary diff and patch system in Rust that computes efficient differences between files and applies patches to reconstruct new versions. It uses rolling hash algorithms (e.g., Rabin-Karp) to detect matching blocks and generates compact delta instructions similar to rsync or bsdiff. The system supports large files, streaming processing, and verification of patch integrity. This project is interesting because it combines algorithms for diffing, hashing, compression, and streaming I/O into a highly practical tool for synchronization, updates, and storage optimization.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/binary_diff_tool`

## Requirements

1. Implement a Rust binary crate structured into modules such as `hash`, `rolling`, `chunk`, `index`, `matcher`, `delta`, `patch`, `encoder`, `decoder`, `stream`, `io`, `engine`, `cli`, and `errors`.
2. Design a rolling hash implementation (e.g., Rabin-Karp) for efficient detection of matching byte sequences across files.
3. Implement chunking logic that splits input files into variable or fixed-size blocks for comparison.
4. Build an index of source file chunks to enable fast lookup during diff computation.
5. Implement a diff algorithm that produces delta instructions (copy, insert) representing differences between source and target files.
6. Design a compact binary format for encoding patch data using `serde` or custom encoding.
7. Implement a patch application engine that reconstructs the target file from the source file and delta instructions.
8. Support streaming processing for large files without loading entire contents into memory.
9. Implement integrity verification using checksums (e.g., SHA-256) to ensure correctness of patches.
10. Provide performance optimizations such as skipping unchanged regions and minimizing redundant comparisons.
11. Provide a CLI using `clap` with commands like `diff <old> <new>`, `patch <old> <patch>`, and `verify`.
12. Integrate structured logging with `tracing` to trace hashing, matching, delta generation, and patch application, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.