# Implementation Plan: Split runner.rs (1374 LOC) into focused modules

## Current structure

All logic lives in `src/runner.rs`. Seven distinct responsibilities are tangled together:

| Responsibility | Functions |
|---|---|
| Entry points / config | `run_rename_self_from_env`, `run_rename_self`, `RenameSelfConfig`, `SuggestConfig`, `RenameSelfMode` |
| Attempt execution | `run_incremental_attempt`, `run_bulk_attempt`, `restore_project_src` |
| Rename group solver | `build_rename_groups`, `trait_method_key_from_impl`, `extract_trait_from_impl_symbol`, `is_known_external_trait_method`, `classify_rename_safety`, `is_degenerate_rename`, `to_snake` |
| Verification | `verify_renames_applied`, `VerifySummary` |
| Compile / check | `run_cargo_check_json`, `CargoCheckJson`, `accumulate_error_counts_json`, `summarize_error_messages`, `compute_delta_error_counts`, `merge_counts`, `sum_counts`, `sum_counts_i64` |
| Symbols I/O | `parse_symbols_json`, `load_symbol_ids`, `load_symbols_entries`, `write_symbols_entries` |
| LLM suggest | `run_suggest_names`, `SuggestConfig`, `group_pending_by_file`, `primary_file_for_symbol`, `build_prompt`, `call_llm_for_suggestions`, `parse_llm_response`, `apply_suggestions_from_stdin`, `is_valid_rust_ident`, `is_rust_keyword` |
| Report / timing | `append_report_line`, `git_head_commit`, `now_unix_secs`, `now_iso_utc`, `now_compact_utc`, `KindStats`, `update_kind_stats`, `SolverPlan` |
| Shell helpers | `run_cmd`, `run_capture`, `project_from_args` |

---

## Target layout

```
src/
  runner/
    mod.rs              ← re-exports public API: run_rename_self_from_env, run_rename_self,
                          RenameSelfConfig, RenameSelfMode, RenameSelfResult
    config.rs           ← RenameSelfConfig, SuggestConfig, RenameSelfMode, RenameSelfResult,
                          project_from_args
    attempt.rs          ← run_incremental_attempt, run_bulk_attempt, IncrementalOutcome,
                          BulkOutcome, restore_project_src
    solver.rs           ← build_rename_groups, trait_method_key_from_impl,
                          extract_trait_from_impl_symbol, is_known_external_trait_method,
                          classify_rename_safety, is_degenerate_rename, to_snake
    verify.rs           ← verify_renames_applied, VerifySummary
    check.rs            ← run_cargo_check_json, CargoCheckJson, accumulate_error_counts_json,
                          summarize_error_messages, compute_delta_error_counts,
                          merge_counts, sum_counts, sum_counts_i64
    symbols.rs          ← parse_symbols_json, load_symbol_ids, load_symbols_entries,
                          write_symbols_entries
    suggest.rs          ← run_suggest_names, group_pending_by_file, primary_file_for_symbol,
                          build_prompt, call_llm_for_suggestions, parse_llm_response,
                          apply_suggestions_from_stdin, is_valid_rust_ident, is_rust_keyword
    report.rs           ← append_report_line, git_head_commit, now_unix_secs, now_iso_utc,
                          now_compact_utc, KindStats, update_kind_stats, SolverPlan
    shell.rs            ← run_cmd, run_capture
```

`src/lib.rs` changes `pub mod runner;` — no external API change since `mod.rs` re-exports
the same public symbols.

---

## Steps

### Step 1 — Create `src/runner/` directory, move runner.rs to runner/mod.rs

No code changes. Just move the file. Verify `cargo check` still passes.

### Step 2 — Extract `shell.rs`

Move `run_cmd`, `run_capture`. No dependencies on other runner internals.
Add `mod shell;` to `mod.rs`, replace usages with `shell::run_cmd` etc or keep as `use`.

### Step 3 — Extract `report.rs`

Move `append_report_line`, `git_head_commit`, `now_unix_secs`, `now_iso_utc`,
`now_compact_utc`, `KindStats`, `update_kind_stats`, `SolverPlan`.
Depends only on `std` and `serde_json`. No internal deps.

### Step 4 — Extract `check.rs`

Move `run_cargo_check_json`, `CargoCheckJson`, `accumulate_error_counts_json`,
`summarize_error_messages`, `compute_delta_error_counts`, `merge_counts`,
`sum_counts`, `sum_counts_i64`.
Depends on `shell.rs` for `run_cmd` if refactored, otherwise self-contained.

### Step 5 — Extract `symbols.rs`

Move `parse_symbols_json`, `load_symbol_ids`, `load_symbols_entries`,
`write_symbols_entries`.
Depends on `RustcSession` — import from `crate::core::rustc_session`.

### Step 6 — Extract `verify.rs`

Move `verify_renames_applied`, `VerifySummary`.
Depends on `RustcSession`, `ProjectEditor`, `normalize_symbol_id`.

### Step 7 — Extract `solver.rs`

Move `build_rename_groups`, `trait_method_key_from_impl`,
`extract_trait_from_impl_symbol`, `is_known_external_trait_method`,
`classify_rename_safety`, `is_degenerate_rename`, `to_snake`.
Depends on `RustcSession`.

### Step 8 — Extract `suggest.rs`

Move `run_suggest_names`, `group_pending_by_file`, `primary_file_for_symbol`,
`build_prompt`, `call_llm_for_suggestions`, `parse_llm_response`,
`apply_suggestions_from_stdin`, `is_valid_rust_ident`, `is_rust_keyword`.
Depends on `symbols.rs`, `RustcSession`, `shell.rs`.

### Step 9 — Extract `attempt.rs`

Move `run_incremental_attempt`, `run_bulk_attempt`, `IncrementalOutcome`,
`BulkOutcome`, `restore_project_src`.
Depends on `check.rs`, `verify.rs`, `shell.rs`, `ProjectEditor`, `RustcSession`.

### Step 10 — Extract `config.rs`

Move `RenameSelfConfig`, `SuggestConfig`, `RenameSelfMode`, `RenameSelfResult`,
`project_from_args`.
No internal deps — pure config structs and env reads.

### Step 11 — Slim down `mod.rs`

`mod.rs` retains only `run_rename_self_from_env` and `run_rename_self`.
These are the orchestrators — they import from all other submodules.
Target size: under 150 LOC.

---

## Dependency graph between new modules

```
mod.rs
  ├── config.rs
  ├── attempt.rs ──── check.rs
  │               ├── verify.rs
  │               └── shell.rs
  ├── solver.rs
  ├── suggest.rs ──── symbols.rs
  │               └── shell.rs
  └── report.rs
```

No cycles. Each module has a single clear responsibility.

---

## Verification after each step

```bash
cargo check -p rename 2>&1 | grep '^error'
```

Must return empty after every step. Do not proceed to the next step if errors exist.
