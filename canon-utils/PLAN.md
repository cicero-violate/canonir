# Canon Agent Loop — Architecture Plan

## Problem

The current agent loop is not working. The root causes:

1. **No real Observe step** — state is scattered across `AgentWorkerState` fields, inferred ad-hoc from events
2. **Plan is too complex** — full task graph patching via LLM, multiple retry strategies, hardcoded fallbacks
3. **Act has too many failure modes** — executor deltas, graph patches, LLM parse retries all interleaved
4. **No Verify step** — nothing checks whether the action actually worked
5. **No Reward step** — no signal drives convergence; the loop has no exit condition except manual termination

## Design

Five crates, one responsibility each. The loop is:

```
Observe → Plan → Act → Verify → Reward → (repeat)
```

Each iteration is a single synchronous pass. The runtime drives the loop by emitting a `Tick` event. No goal tracking, no task graphs, no speculation.

---

## Crate: `canon-observe`

**Responsibility**: Read exact current state. Return a flat, serializable snapshot.

### What it reads
- **tlog**: last N events (errors, capability results, compiler output)
- **filesystem**: target files listed in scope (not recursive scan)
- **compiler**: run `cargo check --message-format=json` on workspace, parse diagnostics
- **metrics**: count of errors by kind in current tlog window

### Output type
```rust
pub struct Snapshot {
    pub tick: u64,
    pub errors: Vec<CompilerError>,      // from cargo check
    pub tlog_tail: Vec<RawEvent>,        // last 50 events
    pub files: HashMap<PathBuf, String>, // contents of scoped files
    pub error_count: usize,
    pub warning_count: usize,
}
```

### Rules
- No inference, no LLM, no mutation
- Reads are bounded (last 50 tlog events, max 10 files)
- Snapshot is written to a temp file so other crates can read it without re-running

---

## Crate: `canon-plan`

**Responsibility**: Given a snapshot, produce one concrete next action. Nothing else.

### Input
- `Snapshot` from `canon-observe`

### Output type
```rust
pub enum Action {
    RunCommand { cmd: String, args: Vec<String>, cwd: PathBuf },
    WriteFile { path: PathBuf, content: String },
    PatchFile { path: PathBuf, old: String, new: String },
    NoOp { reason: String },
}
```

### Planning strategy
1. If `snapshot.errors` is non-empty → pick the first compiler error → produce the minimal `PatchFile` or `WriteFile` to fix it
2. If `snapshot.errors` is empty and `snapshot.warning_count > 0` → produce `NoOp` (warnings are not failures)
3. If everything is clean → produce `NoOp { reason: "clean" }`

### LLM use (optional, single call)
- Only invoked for step 1 when the error requires code generation
- Prompt contains: error message + file content around the error span + instruction to return only the patch
- Response must be `{"old": "...", "new": "..."}` — if it is not, plan falls back to `NoOp`
- One attempt. No retries. Retry is the next loop iteration.

### Rules
- One action per plan call
- No task graphs, no dependency resolution
- If uncertain, emit `NoOp` and let Reward signal stagnation

---

## Crate: `canon-act`

**Responsibility**: Execute the action from `canon-plan`. Apply a concrete delta.

### Execution map
| Action | Implementation |
|---|---|
| `RunCommand` | spawn subprocess, capture stdout/stderr, timeout 30s |
| `WriteFile` | `std::fs::write` |
| `PatchFile` | find `old` in file, replace with `new`, write atomically |
| `NoOp` | emit event, do nothing |

### Output type
```rust
pub struct ActResult {
    pub action: Action,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub success: bool,
}
```

### Rules
- No LLM calls
- Patch application fails if `old` string not found exactly once (prevents silent partial edits)
- All results written to tlog as a `capability_result` event
- Timeout kills the subprocess; `success = false`

---

## Crate: `canon-verify`

**Responsibility**: After Act, check whether the system state improved or is at least valid.

### Checks (in order)
1. `cargo check` — must exit 0
2. Assert tlog invariants — no new `ErrorOccurred` events since Act started
3. If `ActResult.action` was `PatchFile` or `WriteFile` — confirm file on disk matches expected content

### Output type
```rust
pub struct VerifyResult {
    pub passed: bool,
    pub compiler_clean: bool,
    pub tlog_clean: bool,
    pub file_correct: bool,
    pub diagnostics: Vec<String>,
}
```

### Rules
- `passed = compiler_clean && tlog_clean && file_correct`
- Does not fix anything — only reports
- Writes result to tlog as a `verify_result` event

---

## Crate: `canon-reward`

**Responsibility**: Compare snapshots before and after the loop iteration. Emit a scalar reward signal.

### Inputs
- `Snapshot` before Act (from Observe)
- `Snapshot` after Verify (re-run Observe)
- `VerifyResult`

### Reward computation
```
reward = (errors_before - errors_after)        // positive = improvement
       + (warnings_before - warnings_after) * 0.1
       - (1 if verify failed else 0)           // penalty for broken state
```

### Output type
```rust
pub struct RewardSignal {
    pub reward: f32,
    pub errors_before: usize,
    pub errors_after: usize,
    pub stagnant_ticks: u32,   // incremented if reward == 0.0
    pub halt: bool,            // true if stagnant_ticks > threshold
}
```

### Rules
- If `stagnant_ticks > 5` → set `halt = true` → runtime stops the loop
- Reward written to tlog as a `reward_signal` event
- No LLM calls

---

## Runtime Integration

The existing `canon-runtime` drives the loop. One tick = one full pass:

```
Tick
 └─ Observe::run(scope) → Snapshot
     └─ Plan::run(snapshot) → Action
         └─ Act::run(action) → ActResult
             └─ Verify::run(act_result) → VerifyResult
                 └─ Reward::run(before, after, verify) → RewardSignal
                     └─ if halt → stop; else → wait for next Tick
```

The `AgentConsumer` in `canon-runtime/src/consumers/agent/mod.rs` is replaced with a single `LoopConsumer` that calls these five crates in sequence on each `Tick`. No task graph, no LLM parse retries, no executor delta tracking.

---

## What Gets Deleted

| Current code | Reason |
|---|---|
| `TaskGraph` / `TaskGraphPatch` | Replaced by single `Action` enum |
| `handle_llm_parse_failed()` retry logic | Single LLM call in Plan; failure = NoOp |
| `executor_delta` / `delta_to_node` | Replaced by `canon-act` RunCommand |
| `plan_if_stalled()` stall detection | Replaced by `canon-reward` stagnation counter |
| `AgentGoal` / AGENT_GOAL.md watcher | Scope is the workspace; no goal file needed |
| `schedule_next()` / `apply_result()` | Replaced by sequential loop in LoopConsumer |

---

## Constraints

- No crate imports another loop crate (observe, plan, act, verify, reward are peers)
- All crates expose a single `run()` function
- All inter-crate data is plain serializable structs (no Arc, no channels)
- The loop is synchronous within a tick; async only at the `canon-runtime` boundary
