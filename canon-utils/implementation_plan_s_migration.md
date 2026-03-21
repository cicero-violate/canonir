# Implementation Plan: S-Score Migration (J → 1, C → 1, E → 1)

## Current State

**Build is BROKEN.** Phase 3 was partially applied — struct fields updated, call sites
updated, but `LlmWork.name: String` in llm_executor.rs was not changed. `.as_str()` on a
`String` gives `&str` bounded to the local variable's lifetime, not `&'static str` as the
updated `CapabilityCompleted.capability` and `CapabilityFailed.capability` fields require.

Progress:
- **Phase 1** — ✅ complete (`NodeReady.args` removed)
- **Phase 2** — ✅ complete (`CapabilityResult` typed, `canon-builder` updated)
- **Phase 3** — 🔴 in progress — struct defs done, llm_executor.rs blocked (see below)
- **Phase 4** — not started

Scores after Phases 1 and 2:
- **E ≈ 0.95** — `CapabilityCompleted.result: CapabilityResult` typed; LLM response body stays `Value`
- **C ≈ 0.6** — `CapabilityCompleted`/`CapabilityFailed` use `&'static str`; `CapabilityInvoked` still `name: String`
- **J ≈ 0.6** — `NodeReady.args` gone, `CapabilityResult` typed; analysis channel and LLM response body remain
- **R ≈ 0.4** — unchanged

Target: finish Phase 3 (unblock build), then Phase 4.

---

## Scope

| Phase | Status     | Score impact | Files touched                                     |
|-------+------------+--------------+---------------------------------------------------|
|     1 | ✅ done    | J ↑          | `NodeReady` — `args` removed                      |
|     2 | ✅ done    | J ↑, E ↑     | `CapabilityCompleted.result: CapabilityResult`    |
|     3 | 🔴 blocked | C ↑          | `LlmWork.name` lifetime fix + `CapabilityInvoked` |
|     4 | pending    | J ↑          | analysis internal channel                         |

---

## ~~Phase 1~~ — ✅ Complete

`NodeReady.args: serde_json::Value` removed. No action needed.

---

## ~~Phase 2~~ — ✅ Complete

`CapabilityResult` type added to events.rs. `CapabilityCompleted.result: CapabilityResult`.
`canon-builder/src/executor/capabilities.rs` updated. No action needed.

---

## Phase 3 — 🔴 Fix `LlmWork.name` lifetime + finish `CapabilityInvoked`

### What is done

- `CapabilityCompleted { request_id: String, capability: &'static str, result: CapabilityResult }` — struct updated
- `CapabilityFailed { request_id: String, capability: &'static str, error: String }` — struct updated
- All `CapabilityCompleted`/`CapabilityFailed` construction sites use `capability:` field name

### What is blocking — `LlmWork.name: String`

**File: `canon-utils/canon-runtime/src/consumers/llm_executor.rs`**

`LlmWork` still has `name: String` (line 25). Three call sites pass `name.as_str()` to
`capability: &'static str` fields — this fails because `name.as_str()` has the lifetime of
`name`, not `'static`.

**Two-line fix:**

**Line 25** — change the field type:
```rust
// Before:
struct LlmWork {
    request_id: String,
    name: String,
    prompt: String,
    role: Option<String>,
    raw: bool,
    emitter: EventEmitterHandle,
}

// After:
struct LlmWork {
    request_id: String,
    name: &'static str,
    prompt: String,
    role: Option<String>,
    raw: bool,
    emitter: EventEmitterHandle,
}
```

**Line 454** — change the send site:
```rust
// Before:
if self.work_tx.send(LlmWork { request_id: request.request_id, name: "llm.call".to_string(), prompt, role, raw: false, emitter }).is_err() {

// After:
if self.work_tx.send(LlmWork { request_id: request.request_id, name: self.name(), prompt, role, raw: false, emitter }).is_err() {
```

`self.name()` already returns `&'static str` — no leaking, no allocation.

**Lines 113, 249, 288** — remove `.as_str()` (name is already `&'static str`):
```rust
// Before:
capability: name.as_str()

// After:
capability: name
```

**Lines 108, 284** — `name.clone()` in `json!()` log blocks. `&'static str` is `Copy` — `.clone()`
compiles but is unnecessary. Leave as-is or remove; either is correct.

### What remains after the fix — `CapabilityInvoked`

**File: `canon-utils/canon-runtime-events/src/events.rs`**

`CapabilityInvoked` at line 252 still has `name: String`:
```rust
// Current:
canon_event_struct!(CapabilityInvoked { capability_id: String, name: String, node_id: String });

// After:
canon_event_struct!(CapabilityInvoked { capability_id: String, capability: &'static str, node_id: String });
```

Then grep all `CapabilityInvoked {` construction sites and update `name:` → `capability:`, values
from `.to_string()` to the `&'static str` directly.

```bash
grep -r "CapabilityInvoked {" canon-utils --include="*.rs" -n
```

**Verify after all Phase 3 changes:** `cargo check --workspace` — zero errors.

---

## Phase 4 — Analysis internal channel — remove `serde_json::Value`

**Problem:** `AnalysisWork` enum in `canon-tools-analysis/src/capabilities/run.rs` uses
`serde_json::Value` as the channel message type. This is internal (not an event), but contributes
to the JSON footprint.

```rust
// Before:
enum AnalysisWork {
    Crate(serde_json::Value),
    Workspace(serde_json::Value),
}
```

**Fix:** Replace with typed structs that match what the sender provides.

```rust
struct CrateWork {
    crate_name: String,
    batch_id: Option<String>,
}

enum AnalysisWork {
    Crate(CrateWork),
    Workspace,
}
```

**Update sender** (in `AnalysisRunCapability::handle`):
```rust
// Before:
let args = serde_json::json!({ "crate": crate_name, "batch_id": batch_id });
let _ = self.work_tx.send(AnalysisWork::Crate(args));

// After:
let _ = self.work_tx.send(AnalysisWork::Crate(CrateWork { crate_name, batch_id }));
```

**Update worker** (in `spawn_analysis_worker`):

```rust
AnalysisWork::Crate(work) => {
    // Pass work.crate_name and work.batch_id directly to runner.
    // runner::run_full_analysis still takes serde_json::Value for now — construct it here,
    // as close to the runner boundary as possible.
    let args = serde_json::json!({ "crate": work.crate_name, "batch_id": work.batch_id });
    match crate::capabilities::runner::run_full_analysis(&args) { ... }
}
AnalysisWork::Workspace => {
    let args = serde_json::json!({});
    match crate::capabilities::runner::run_workspace_analysis(&args) { ... }
}
```

This keeps the JSON construction at the runner boundary (one call site) rather than scattered
through the channel and the worker. If `runner::run_full_analysis` is later typed, the `json!`
call here becomes the last removal.

**Verify:** `cargo check --workspace` — zero errors.

---

## Score Projection

| Variable | Phase 1+2 done | After Phase 3 | After Phase 4 |
|----------|---------------|---------------|---------------|
| E | 0.95 | 0.95 | 0.95 |
| C | 0.6 | 0.90 — all lifecycle events use `&'static str` | 0.90 |
| J | 0.6 | 0.65 | 0.75 — `AnalysisWork` typed |
| R | 0.4 | 0.4 | 0.4 — unchanged |

```
S after Phase 3 = min(0.95, 0.90, 0.65, 0.4) = 0.4
S after Phase 4 = min(0.95, 0.90, 0.75, 0.4) = 0.4
```

R is the floor. Raising S beyond 0.4 requires replacing the string-keyed `HashMap<String, Arc<dyn
CapabilityHandler>>` in `CapabilityRegistry` with type-native dispatch. That is a separate plan item.

---

## Execution Order

Run phases in order. After each phase:
```bash
cargo check --workspace
```
Must produce zero errors and zero warnings before proceeding to the next phase.

Do not batch multiple phases into one commit. One phase = one clean build = one commit.

---

## Files Modified

| Phase | Status  | File                                           | Change                                                                                     |
|-------+---------+------------------------------------------------+--------------------------------------------------------------------------------------------|
|     1 | ✅      | `canon-runtime-events/src/events.rs`           | `NodeReady.args` removed                                                                   |
|     2 | ✅      | `canon-runtime-events/src/events.rs`           | `ProcessResult`, `LlmResult`, `CapabilityResult` added; `CapabilityCompleted.result` typed |
|     2 | ✅      | `canon-builder/src/executor/capabilities.rs`   | Helpers + capability impls updated                                                         |
|     2 | ✅      | `canon-runtime/src/consumers/llm_executor.rs`  | `CapabilityCompleted` emit updated                                                         |
|     3 | 🔴      | `canon-runtime/src/consumers/llm_executor.rs`  | `LlmWork.name: &'static str`; remove `.as_str()` at lines 113, 249, 288                    |
|     3 | pending | `canon-runtime-events/src/events.rs`           | `CapabilityInvoked.name: String` → `capability: &'static str`                              |
|     3 | pending | All `CapabilityInvoked {` construction sites   | `name:` → `capability:`                                                                    |
|     4 | pending | `canon-tools-analysis/src/capabilities/run.rs` | Type `AnalysisWork` channel                                                                |
