# Implementation Plan: Capability Collapse + Canon Introspection

## Status: Complete except two compiler fixes

All structural work is done and verified. Two diagnostic fixes remain before the build is clean.

---

## Verification Results

| Check                                        | Status | Evidence                                  |
|----------------------------------------------+--------+-------------------------------------------|
| `CapabilityRequested` removed                | ✅     | Zero grep matches across all .rs files    |
| `canon_event_struct!` derives `Default`      | ✅     | Line 7 of canon-macros/src/lib.rs         |
| `canon_event_enum!` generates `sample_all()` | ✅     | Lines 33-43 of canon-macros/src/lib.rs    |
| `canon-introspection` crate exists           | ✅     | Crate + lib.rs present                    |
| `assert_all_routes_safe` used in tests       | ✅     | canon-capability/src/tests.rs             |
| `decode.rs` deleted                          | ✅     | File does not exist                       |
| `ArgSpec` / `ArgKind` removed                | ✅     | Zero grep matches across all .rs files    |
| `registry.route()` type-based dispatch       | ✅     | Uses `matches!` on typed variants         |
| `CapabilityExecutor` typed filter            | ✅     | `EventFilter::All` + typed guard          |
| Sub-enum `Default` impls                     | ✅     | All 4 impls in events.rs:314-336          |
| `anyhow` import in tools-editor              | ❌     | Used but not imported — compiler error    |
| Unused imports in builder capabilities       | ❌     | Stale imports — compiler warning as error |

---

## Fix 1 — Missing `anyhow` import

**File: `canon-utils/canon-tools-editor/src/lib.rs`**

`anyhow::anyhow!()` is used on lines ~77-78 but `anyhow` is not imported.

Add to the top of the file:
```rust
use anyhow::anyhow;
```

Or if the full path form `anyhow::anyhow!()` is preferred, ensure `anyhow` is listed in `canon-tools-editor/Cargo.toml` dependencies. If it is already there, adding `use anyhow;` at the top of the file resolves the scope error.

---

## Fix 2 — Remove Unused Imports

**File: `canon-utils/canon-builder/src/executor/capabilities.rs`**

Line 3 imports `CargoEvent`, `FileEvent`, `LlmCall` (and possibly `BashInvoke`, `CargoBuild`, `CargoCheck`, `CargoRun`, `FilePatch`, `FileRead`, `FileWrite`) but these types are not directly referenced by name — they are accessed through `CanonEvent::Cargo(...)`, `CanonEvent::File(...)` etc., which does not require the inner types to be imported.

Remove the unused symbols from the import line. Keep only what is actually referenced by name in the function bodies.

Example — if the only directly-used imports are `CanonEvent` and `CapabilityCompleted`:
```rust
// Before (stale):
use canon_event::{CanonEvent, CapabilityCompleted, CargoEvent, FileEvent, LlmCall, BashInvoke, CargoBuild, CargoCheck, CargoRun, FilePatch, FileRead, FileWrite};

// After (clean):
use canon_event::{CanonEvent, CapabilityCompleted};
```

Read the file first to confirm exactly which names are referenced before trimming the import list.

---

## Verify After Fixes

```bash
cargo check --workspace
```

Expected: zero errors, zero warnings.

---

## Final State

Once the two fixes are applied, the system satisfies all invariants:

```
∀ e ∈ CanonEvent, safe(route(e))
```

- `CanonEvent::sample_all()` — macro-generated, zero maintenance
- `assert_all_routes_safe(registry)` — single call covers all variants
- New event variant added → `sample_all()` auto-includes it → test auto-covers it
- No JSON args, no string routing, no decode bridge, no `CapabilityRequested`
