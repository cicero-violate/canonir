# Git-Like Version Control System with Object Store, Indexing, and Branching

This project implements a simplified Git-like version control system in Rust that supports content-addressable storage, commits, branching, and diffs. It models core Git concepts such as blobs, trees, commits, and references while providing a CLI for interacting with repositories. The system is interesting because it combines hashing, immutable data structures, DAG modeling, and file system interactions into a powerful distributed version control mechanism.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/git_like_vcs`

## Requirements

1. Implement a Rust binary crate structured into modules such as `object`, `blob`, `tree`, `commit`, `hash`, `store`, `index`, `diff`, `repo`, `reference`, `branch`, `log`, `cli`, and `errors`.
2. Design a content-addressable object store where blobs, trees, and commits are stored using SHA-1 or SHA-256 hashes.
3. Implement blob objects for file contents and tree objects for directory structures.
4. Build commit objects that reference trees, parent commits, author metadata, and messages.
5. Implement an index (staging area) that tracks changes before committing.
6. Support basic commands such as `init`, `add`, `commit`, `status`, and `log`.
7. Implement branching and references, allowing creation and switching of branches.
8. Build a diff engine to compare file versions and display changes between commits or working directory.
9. Support checkout functionality to restore working directory state from a given commit or branch.
10. Implement simple merge functionality with conflict detection (no need for full conflict resolution UI).
11. Provide persistence of all objects and metadata using filesystem storage and `serde` where appropriate.
12. Integrate structured logging with `tracing` to trace object creation, hashing, commit history traversal, and repository operations, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.