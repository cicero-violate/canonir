# Implementation Plan 10 — Hierarchical CANON.md Context Files

## Goal

Agents automatically receive scope-appropriate context from hierarchical `CANON.md`
files without any prompt engineering. A global `CANON.md` describes the runtime
architecture. A project-level `CANON.md` describes the current goal and workspace
state. Path-specific `CANON.md` files inject module-level context when a capability
targets a particular directory. All context is auto-injected into every outgoing
`LlmCall` before the user prompt.

---

## Scope hierarchy (highest priority first)

| Scope | File location | Injected when |
|---|---|---|
| Path-specific | `.canon/{path_prefix}/CANON.md` | Capability targets a file under `path_prefix` |
| Project | `CANON.md` at workspace root | Always |
| Global | `/workspace/ai_sandbox/canon/CANON.md` | Always |

Lower-priority scopes are prepended first; higher-priority scopes append after.
Total injected context is capped at 2000 tokens (≈8000 chars) — truncate from the
global scope upward if needed.

---

## New crate: `canon-utils/canon-context`

### `Cargo.toml`

```toml
[package]
name = "canon-context"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow.workspace = true
once_cell.workspace = true
parking_lot.workspace = true
walkdir.workspace = true
```

### `src/lib.rs`

```rust
pub struct ContextLoader {
    workspace_root: PathBuf,
    global_path: PathBuf,   // /workspace/ai_sandbox/canon/CANON.md
    cache: parking_lot::RwLock<HashMap<PathBuf, (String, std::time::SystemTime)>>,
}

impl ContextLoader {
    pub fn new(workspace_root: PathBuf) -> Self;

    /// Returns the merged context block for a capability targeting `target_path`.
    /// `target_path` may be None (no path-specific context injected).
    ///
    /// Output format:
    /// ## Canon Context
    ///
    /// ### Global
    /// {global CANON.md contents}
    ///
    /// ### Project
    /// {workspace_root/CANON.md contents}
    ///
    /// ### Path: {matched prefix}    ← only if a path-specific file matches
    /// {path-specific CANON.md contents}
    ///
    pub fn load_context(&self, target_path: Option<&Path>) -> String;

    /// Reads a CANON.md file, using a mtime-based cache. Returns empty string
    /// if file doesn't exist.
    fn read_cached(&self, path: &Path) -> String;

    /// Scans `.canon/` directory for path-specific CANON.md files.
    /// Returns the best match for `target_path` (longest prefix match wins).
    fn find_path_specific(&self, target_path: &Path) -> Option<(PathBuf, PathBuf)>;
}

/// Global singleton. Initialized on first access from the workspace root
/// environment variable `CANON_WORKSPACE_ROOT`, falling back to
/// `/workspace/ai_sandbox/canon`.
pub fn global_loader() -> &'static ContextLoader;
```

Cache invalidation: if `mtime` of the file has changed since last read, re-read.
This means prompts update automatically when `CANON.md` is edited.

---

## Path-specific file discovery

The `.canon/` directory at the workspace root stores path-specific overrides:

```
/workspace/ai_sandbox/canon/
  .canon/
    CANON.md                              ← global overrides (alias for workspace root)
    canon-utils/canon-runtime/CANON.md   ← injected for any capability targeting canon-runtime
    canon-utils/canon-loop/CANON.md      ← injected for canon-loop capabilities
    algorithms/CANON.md                  ← injected for algorithms
```

`find_path_specific` walks `.canon/` entries, strips the `.canon/` prefix, and
checks if `target_path` starts with the remainder. The longest matching prefix wins.

---

## Inject context into every `LlmCall`

### Option A — In `capability_executor.rs` (preferred)

In `CapabilityExecutor::on_event`, when the event is `RuntimeEvent::Llm(call)`:
- Extract `target_path` from the `call.prompt` if it contains a known file reference,
  OR from a new optional field `target_path: Option<String>` on `LlmCall`.
- Call `canon_context::global_loader().load_context(target_path.as_deref())`.
- If context is non-empty, prepend it to `call.prompt`:
  ```
  {context_block}

  ---

  {original_prompt}
  ```
- Re-emit the mutated `LlmCall`.

### Add `target_path` to `LlmCall`

In `canon-runtime-events/src/events.rs`, add to `LlmCall`:

```rust
#[serde(default)]
pub target_path: Option<String>,
```

Consumers that know their target path set this field:
- `capability_executor.rs`: set from the `FileEvent` or `BashInvoke` that preceded the call.
- `analyst_consumer.rs`: leave `None` (no specific path target).
- `goal_gen_consumer.rs`: leave `None`.

For the planner, extract from `LoopPlanned.action_payload["path"]` if present.

---

## Create initial CANON.md files

### `/workspace/ai_sandbox/canon/CANON.md` (global)

```markdown
# Canon Runtime

Canon is a multi-agent Rust runtime. Key facts for agents:

- **Event bus:** `canon-utils/canon-runtime/src/bus.rs` — dispatches `RuntimeEvent` variants
- **Loop stages:** LoopObserved → LoopPlanned → LoopActed → LoopVerified → LoopRewarded
- **Consumers:** all in `canon-utils/canon-runtime/src/consumers/`
- **Capabilities:** Cargo, File, Bash, Llm — dispatched via `canon-exec`
- **Tlog:** append-only NDJSON event log at `$CANON_REPORTS_TLOG`
- **Build:** `cargo check --workspace` from `/workspace/ai_sandbox/canon`
- **Config:** `canon-agent-prompts/capability_config.toml`

Never modify `.cargo/config.toml`. Never use `#[allow(dead_code)]`.
```

### `CANON.md` at workspace root (project-level)

This file is written dynamically by `goal_gen_consumer` after goal generation:

After `write_prompt_loaded_to_tlog`, also write a `CANON.md` at workspace root:

```markdown
# Current Goal

{first 500 chars of goal content}

## Status
Generated at: {timestamp}
Retries: {retries}
```

Update this file every time a new goal is successfully validated.

### `.canon/canon-utils/canon-runtime/CANON.md`

```markdown
# canon-runtime

Core event bus and consumer orchestration. Key files:
- `src/bus.rs` — event dispatch, hook chain
- `src/consumers/` — all EventConsumer implementations
- `src/bin/event_runtime.rs` — process entrypoint, consumer registration

The `#[must_emit]` proc-macro is required on all `on_event` implementations.
All RuntimeEvent variants must be exhaustively matched.
```

---

## Wire into `event_runtime.rs`

```rust
// Initialize context loader — happens once at startup, no async needed
let _ = canon_context::global_loader(); // warms cache
```

Add `canon-context` to `canon-exec/Cargo.toml` or `canon-runtime/Cargo.toml`
(whichever is closer to `LlmCall` injection point).

---

## Verification

```
cargo check --workspace
```

1. Create `/workspace/ai_sandbox/canon/CANON.md` with content `# TEST CONTEXT\ntest_marker_xyz`.
2. Run the runtime until any `Llm` event is emitted.
3. Search the tlog for the Llm event's prompt.
4. Confirm `test_marker_xyz` appears in the prompt.
5. Edit `CANON.md` to change the content (no restart needed).
6. Trigger another `Llm` event.
7. Confirm the updated content appears (cache invalidation via mtime).
