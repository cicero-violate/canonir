## Implementation Plan: rustc-once / syn-mutate / cargo-check delta loop

---

### Variables

$$
P = \text{project root path}
$$
$$
\mathcal{R} = \{(s_i^{\text{old}}, s_i^{\text{new}}) \mid i = 1 \dots n\} \quad \text{rename pairs}
$$
$$
\Sigma = \text{symbol registry: } \{(\text{id}, \text{kind})\} \quad \text{built once via rustc}
$$
$$
\mathcal{O}(s) = \{\,(f, [\text{lo}, \text{hi}])\,\} \quad \text{byte-span occurrences of symbol } s \text{, built once via rustc}
$$
$$
E_0 = \text{baseline error count from } \texttt{cargo check}
$$
$$
E_k = \text{error count after attempt } k
$$
$$
\Delta_k = E_k - E_0
$$

---

### Latent Equations

$$
\text{accept}(k) = \mathbb{1}[\Delta_k = 0]
$$
$$
\mathcal{O}(s) \xleftarrow{\text{rustc, once}} \text{CollectorCallbacks}(s, \text{args})
$$
$$
f'_j = \texttt{syn::parse\_file}(f_j) \xrightarrow{\text{visit spans}} \texttt{Ident::rename}(\mathcal{O}(s) \cap f_j)
$$
$$
\Delta_k = \texttt{cargo\_check}(P \mid f'_j) - E_0
$$

---

### The Core Bug — Why rustc Panics

The panic is:

```
forcing query with already existing DepNode
```

**Root cause:** `rustc_driver::run_compiler` is being called **once per rename attempt** inside `run_incremental_attempt` → `rename::rename_symbol_pairs` → `collect_occurrences`. This re-enters the rustc query engine in the same process with leftover global state from the prior invocation. rustc's dep-graph is process-global and cannot be re-entered.

---

### Architecture: Three-Phase Separation

```
Phase 1 [rustc, ONCE]     Phase 2 [syn, per-attempt]     Phase 3 [cargo check, per-accept]
─────────────────────     ──────────────────────────     ─────────────────────────────────
build Σ (symbol ids)  →   apply byte-span patches    →   measure Δ, accept or reject
build O(s) for all s      via syn token rewriting         git restore on reject
emit: SpanIndex            no rustc invoked               no rustc invoked
```

---

### Phase 1 — `RustcSession` (one-shot, process-lifetime)

**File:** `src/core/rustc_session.rs`

- Call `cargo_rustc_args` once to get compiler args.
- Call `rustc_driver::run_compiler` **once** with a `BulkCollectorCallbacks` that:
  - Builds `Σ`: full symbol id → kind map
  - Builds `SpanIndex`: `HashMap<symbol_id, HashMap<PathBuf, Vec<SpanRange>>>`  for **all** symbols in one pass
- Store result in `RustcSession { span_index, symbol_catalog }`.
- **Never call `run_compiler` again** in the process.

$$
\text{RustcSession} = \texttt{run\_compiler}(\text{args}, \texttt{BulkCollector}) \quad \text{exactly once}
$$

---

### Phase 2 — `SynPatcher` (per-attempt, pure)

**File:** `src/core/syn_patcher.rs`

- Input: `SpanIndex` slice for symbol `s`, source file bytes.
- Use `syn::parse_file` to parse.
- Walk the AST, match idents by byte offset against `SpanRange` list.
- Rewrite matching `Ident` nodes to `new_ident`.
- Emit modified source via `prettyplease` or `quote`.
- **No rustc. No cargo. Pure text transformation.**

---

### Phase 3 — Delta Debugger (per-attempt)

**File:** `src/core/delta_checker.rs` (thin wrapper, already mostly exists in example)

- Write patched files to disk.
- Run `cargo check --message-format=json` as subprocess (already correct).
- Compute `Δ = E_k - E_0`.
- `accept = Δ == 0`.
- On reject: `git restore src`.
- On accept: leave files dirty, proceed to next rename.

---

### Changes Required

**`src/core/rustc_session.rs`** — new file
- `BulkCollectorCallbacks`: single rustc pass, collects all def-paths + all spans for all symbols
- `RustcSession::build(project) -> Result<RustcSession>`
- `RustcSession::spans_for(&self, symbol_id) -> Option<&HashMap<PathBuf, Vec<SpanRange>>>`
- `RustcSession::symbol_catalog(&self) -> Vec<(String, String)>`

**`src/core/rustc_resolver.rs`** — gutted
- Remove `collect_occurrences` (replaced by session lookup)
- Keep `cargo_rustc_args`, `ensure_sysroot`, `absolutize_input_paths` as free functions
- Remove `run_rustc_in_dir` and `CollectorCallbacks` (moved to session)

**`src/core/project_editor.rs`** — refactor
- `ProjectEditor::load_with_rustc` calls `RustcSession::build` once
- Stores `session: RustcSession`
- `queue_by_id` looks up spans from `session.spans_for(symbol_id)`, never calls rustc

**`src/core/syn_patcher.rs`** — new file
- `patch_file(src: &str, spans: &[SpanRange], new_ident: &str) -> Result<String>`
- Uses `syn` + `proc_macro2` for ident rewriting by byte offset

**`examples/rename_self.rs`**
- `load_symbol_ids`: call `RustcSession::build` once, cache it
- Pass session into `run_incremental_attempt`
- Remove any path that calls `rename_symbol_pairs` in a loop (each call currently re-invokes rustc)

---

### Sequencing for the Agent

```
1. src/core/rustc_session.rs        — new, BulkCollector + RustcSession
2. src/core/syn_patcher.rs          — new, byte-offset ident rewriter
3. src/core/rustc_resolver.rs       — strip per-call rustc, keep arg-building utilities
4. src/core/project_editor.rs       — wire RustcSession in, remove rustc re-entry
5. src/core/mod.rs                  — pub mod rustc_session; pub mod syn_patcher;
6. examples/rename_self.rs          — hoist session to top of main, thread into loop
```

**Invariant to enforce throughout:** `rustc_driver::run_compiler` appears in exactly **one** call site in the entire codebase, inside `RustcSession::build`.
