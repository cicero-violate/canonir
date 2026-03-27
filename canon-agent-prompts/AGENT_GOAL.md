# Multi-Version Concurrency Control (MVCC) Key-Value Store with Snapshot Isolation and Transaction Manager

This project implements a key-value storage engine in Rust using Multi-Version Concurrency Control (MVCC) to support concurrent transactions with snapshot isolation. Each write produces a new version, and readers can access consistent snapshots without blocking writers. The system includes a transaction manager, version visibility rules, garbage collection, and conflict detection. This project is interesting because it combines concurrency control, versioned storage, transaction semantics, and performance trade-offs into a core database mechanism used in modern systems.

## Target
- Project path: `/workspace/ai_sandbox/canon/test_projects/goalgen/mvcc_kv_store`

## Requirements

1. Implement a Rust binary crate structured into modules such as `key`, `value`, `version`, `entry`, `storage`, `memtable`, `index`, `transaction`, `timestamp`, `visibility`, `snapshot`, `conflict`, `gc`, `engine`, `cli`, and `errors`.
2. Design a versioned storage model where each key maps to multiple versions, each tagged with a timestamp or transaction ID.
3. Implement a transaction manager that assigns timestamps, tracks active transactions, and manages commit/abort states.
4. Support transaction operations including BEGIN, GET, PUT, DELETE, COMMIT, and ABORT with snapshot isolation semantics.
5. Implement visibility rules so transactions only see committed versions that are valid for their snapshot.
6. Detect write-write conflicts during commit and enforce isolation guarantees.
7. Build an in-memory index (e.g., BTreeMap) mapping keys to version chains for efficient lookup.
8. Implement garbage collection that removes obsolete versions no longer visible to any active transaction.
9. Support read-only transactions that can operate without locking and still observe consistent snapshots.
10. Provide persistence using file-based storage with `serde`, including recovery of committed versions and transaction state.
11. Provide a CLI using `clap` with commands like `begin`, `get`, `put`, `delete`, `commit`, `abort`, and `scan`.
12. Integrate structured logging with `tracing` to trace transaction lifecycle, version visibility checks, conflict detection, and garbage collection, and ensure the implementation spans at least 800 lines of real Rust code across modules and compiles successfully with `cargo check` without requiring external services.