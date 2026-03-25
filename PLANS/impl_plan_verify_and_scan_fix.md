# Repair Plan: Fix Verify Livelock + Scan Stage Dead Code

## Status

Two structural bugs cause the loop to never complete a cycle:

| # | Bug                                              | File                             | Line   | Impact                                                  |
|---+--------------------------------------------------+----------------------------------+--------+---------------------------------------------------------|
| 1 | `run_cargo_check` propagates OS `ENOENT` via `?` | `canon-loop/src/stage/verify.rs` | 30, 61 | 0 LoopVerified events — loop permanently stuck          |
| 2 | `Scan` route arm returns `Noop`                  | `canon-loop/src/stage/mod.rs`    | 29     | "observe" route silently ignored — LoopObserved starves |

Fix 1 first, then Fix 2. Run `cargo check -p canon-loop` after each.

---

## Fix 1 — `verify.rs`: guard `run_cargo_check` against missing target path

**File:** `canon-utils/canon-loop/src/stage/verify.rs`

**Root cause:** `run_cargo_check` calls
`Command::new("cargo").arg("check").current_dir(workspace).output()?`.
When `workspace` is the goal's target path (e.g.
`/workspace/ai_sandbox/canon/test_projects/goalgen/<slug>`) and that directory has
not been created yet, the OS rejects `chdir` with `ENOENT`. `.output()` returns
`Err("No such file or directory (os error 2)")`. The `?` propagates the error
out of `verify::execute`, which the loop executor catches and re-emits as
`ErrorOccurred` — **`LoopVerified` is never emitted**.

**Fix:** Add a path-existence check at the top of `run_cargo_check`. If the
directory does not exist, return a controlled `Ok((false, reason))` instead of
propagating an OS error. Do not use `?` to exit — the verify stage must always
produce a `LoopVerified` event.

### Change 1: `run_cargo_check` function (lines 60–65)

Replace:
```rust
fn run_cargo_check(workspace: &Path) -> anyhow::Result<(bool, String)> {
    let output = Command::new("cargo").arg("check").current_dir(workspace).output()?;
    let success = output.status.success();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((success, stderr))
}
```

With:
```rust
fn run_cargo_check(workspace: &Path) -> anyhow::Result<(bool, String)> {
    if !workspace.exists() {
        return Ok((false, format!("target path does not exist: {}", workspace.display())));
    }
    let output = Command::new("cargo").arg("check").current_dir(workspace).output()?;
    let success = output.status.success();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((success, stderr))
}
```

### Change 2: update the diagnostic pushed on cargo check failure (line 33)

The existing code pushes `"cargo_check_failed"` and then the raw stderr. When the
target path is missing, stderr is empty but the failure reason is in the `ok` string.
The current push of `stderr` is still correct — `run_cargo_check` now puts the
reason in the second tuple element, so it flows through unchanged.

No other changes needed in `verify.rs`.

---

## Fix 2 — `stage/mod.rs`: `Scan` arm must call `observe::execute`

**File:** `canon-utils/canon-loop/src/stage/mod.rs`

**Root cause:** When the router selects route `"observe"`, `TryFrom<RuntimeEvent>`
returns `LoopStageEvent::Scan(rs)`, and `LoopStageEvent::execute` matches:

```rust
LoopStageEvent::Scan(_rs) => Ok(LoopStageResult::Noop),
```

This is dead code — the "observe" route does nothing. `LoopObserved` only fires
reactively (on `ErrorOccurred`, `PromptLoaded`, first `Tick`) instead of on demand.
The watchdog fires on "observed" stall because the router cannot trigger observation
by routing to it.

**Fix:** Replace the `Noop` return with a call to `observe::execute`.

### Change: `LoopStageEvent::execute` match arm (line 29)

Replace:
```rust
LoopStageEvent::Scan(_rs) => Ok(LoopStageResult::Noop),
```

With:
```rust
LoopStageEvent::Scan(_rs) => observe::execute(ctx),
```

`observe::execute` already has dedup logic — if state hasn't changed it returns
`LoopStageResult::Noop` safely. No new behavior is introduced when state is
unchanged; when state has changed the observe fires as expected.

---

## Verification

```
cargo check -p canon-loop
cargo test --workspace
```

Then run the runtime and confirm in the tlog:
1. `LoopVerified` events appear after each `LoopActed`
2. `LoopRewarded` events appear after successful `LoopVerified`
3. `ErrorOccurred` count drops from ~93% of events to a small minority
4. Watchdog stall errors stop flooding (no more 2,000+ error bursts)
5. When route "observe" is selected, a `LoopObserved` event follows in the tlog
