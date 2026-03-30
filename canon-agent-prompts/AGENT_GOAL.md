# Filesystem Snapshot Diff and Replay Engine

This project implements a Rust-based filesystem snapshot engine that captures directory states, computes diffs between snapshots, and replays changes to reconstruct or transform filesystem states, along with a coverage discovery system that identifies untested diff scenarios, edge-case file operations, and replay inconsistencies. It is interesting because filesystem state transitions involve complex combinations of operations (create, delete, move, modify), ordering constraints, and edge cases that are difficult to fully test without systematic coverage exploration.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/fs-snapshot-diff`

## Requirements

1. Implement a Rust binary crate organized into modules such as `fs`, `node`, `file`, `directory`, `metadata`, `snapshot`, `diff`, `patch`, `operation`, `replay`, `path`, `walker`, `hash`, `store`, `runtime`, `trace`, `coverage`, `analysis`, `generator`, `report`, `cli`, and `errors`.
2. Design an in-memory representation of a filesystem including directories, files, metadata (timestamps, permissions), and content hashes.
3. Implement snapshot functionality that captures the full state of a filesystem tree from a given root path.
4. Develop a diff engine that computes changes between two snapshots, including file creation, deletion, modification, renaming, and directory restructuring.
5. Represent diffs as ordered operations and implement a patch/replay engine that can apply diffs to reconstruct target states from a base snapshot.
6. Support detection of content changes using hashing (e.g., SHA-256) and structural differences using tree comparisons.
7. Handle edge cases such as empty directories, deeply nested trees, cyclic symbolic links (simulated), permission changes, identical content with different paths, and conflicting operations.
8. Provide a CLI using `clap` to snapshot directories, compute diffs, apply patches, and inspect filesystem states.
9. Create a trace system that records traversal steps, diff decisions, operation ordering, and replay execution.
10. Build a coverage tracking system that records which filesystem operations, diff cases, path structures, and replay scenarios have been exercised.
11. Develop an analysis module that identifies untested scenarios such as rare rename patterns, deep directory mutations, conflicting operations, and edge-case metadata changes, and implement a generator that produces synthetic filesystem trees and mutation sequences targeting uncovered behaviors.
12. Include reporting features such as operation counts, diff sizes, replay correctness checks, coverage summaries, and uncovered scenarios, ensuring the implementation spans at least 800 lines of Rust code across modules and compiles successfully with `cargo check`.