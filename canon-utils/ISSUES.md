# Canon Runtime — Observed Issues

Inferred from tlog (`state/event_log/event.tlog.d/00000000000000000000.log`) and run script logs
(`.run_script_logs/canon_*.log`). Issues are semantic — behavioral problems observed from
event sequences, not compile errors.

---

## ISSUE-1: Router Fires Before Goal Is Available — Mission Is "(unknown goal)"

**Component**: `canon-route` / `RouteContext`
**Severity**: High

The first `RouteSelected` event (event_id=40, tick=16) shows `Mission: (unknown goal)` in the
prompt. The router's first LLM call fires before `LoopObserved` has populated `mission_summary`
in `RouteContext`. The LLM has no idea what the goal is when making the first routing decision.

**Observed**: `RouteSelected [40]` prompt contains `Mission:\n(unknown goal)`.

---

## ISSUE-2: Mission Summary Is "goal_id=agent_goal" — Not the Actual Goal

**Component**: `canon-route` / `mission_summary` field / `summarize_goal`
**Severity**: High

After `LoopObserved` delivers the goal text, subsequent `RouteSelected` events (e.g. event_ids
65, 91) show `Mission: goal_id=agent_goal` — a bare identifier, not the actual mission
description. The router LLM sees a meaningless ID instead of what it needs to do.

The actual goal text is present in `LoopObserved.goal_text` but the mission summary being
passed to the router prompt is either an ID extracted from the goal markdown front-matter or
the result of a summarization function that strips the content.

**Observed**: `RouteSelected [65]` prompt contains `Mission:\ngoal_id=agent_goal`.

---

## ISSUE-3: LoopRewarded Fires With halt=False Even When Compiler Is Clean

**Component**: `canon-loop` reward stage
**Severity**: Critical

`LoopVerified` at tick=72 reports `compiler_clean=True`. The conclude route then fires and
emits `LoopRewarded`, but with `halt=False`. The loop does not stop. After this point the
runtime continues ticking indefinitely with only `LoopObserved` + `RouteTick` events.

There is no `LoopRewarded { halt: true }` anywhere in the log. The goal satisfaction check
(`evaluate_goal_satisfied`) is apparently returning false even when `compiler_clean=True`,
causing conclude to reward without halting.

**Observed**: `LoopRewarded [207] tick=72 halt=False` — no halt=True follows.

---

## ISSUE-4: Infinite Observe Loop After LoopRewarded(halt=False)

**Component**: Runtime scheduler / router post-reward behavior
**Severity**: Critical

After `LoopRewarded(halt=False)` at tick=72, the log shows 80+ more `RouteTick` +
`LoopObserved` events with no `LoopPlanned(act)`, `LoopVerified`, or `LoopRewarded(halt=True)`
ever appearing. The agent is alive but doing nothing useful.

The router does fire one more LLM call (completed around tick=80) which produces
`LoopPlanned { action_kind: no_op }` — indicating the planner also considers nothing to do.
But neither the router nor the planner breaks the cycle, and conclude is never selected again.

**Observed**: Events [208]–[234] are exclusively `RouteTick` and `LoopObserved`.

---

## ISSUE-5: Router Goes Dark for ~47 Ticks (No RouteSelected Events)

**Component**: `canon-route` / `RouteExecutor::try_dispatch_route`
**Severity**: High

Between `RouteSelected [91]` (tick=33) and the LLM call completing around tick=80, there are
zero `RouteSelected` events across ~47 ticks. `RouteTick` fires every tick, and the idle
guard (`planned_pending == 0 && pending_tool_result_ids.is_empty()`) should trigger
`try_dispatch_route` on each tick when idle.

Either `pending_request_id` is non-None (blocking dispatch) during this entire window, or
the idle condition is evaluating incorrectly. The total log has only 6 `RouteSelected` events
for 83+ ticks — the router is making far fewer decisions than it should.

**Observed**: 83 `RouteTick` events, 6 `RouteSelected` events.

---

## ISSUE-6: Agent Plans `cargo new` on an Existing Directory

**Component**: Plan consumer / LLM planner prompt
**Severity**: High

`LoopActed [174]` fails with stderr containing:
```
Creating binary (application) `test_rust_project_v3` package
error: destination already exists
```

The agent ran `cargo new test_rust_project_v3` on a directory that already existed. The
planner has no awareness that the target path exists before choosing `cargo new` vs
`cargo init`.

---

## ISSUE-7: Destructive Command Attempted in Planned Batch

**Component**: Plan consumer / ActConsumer destructive-command guard
**Severity**: Medium

`LoopActed [165]` shows `stderr: 'rejected_destructive_command'`. The agent planned a
destructive shell command as part of the same batch as the `cargo new` attempt. The guard
blocked it correctly, but the planner has no feedback that this class of action is forbidden
— so it will generate destructive commands again on the next plan cycle.

---

## ISSUE-8: Single Action Failure Aborts Entire Planned Batch

**Component**: `canon-act` / batch execution policy
**Severity**: Medium

After the first two failures (events 165, 174), all remaining planned actions —
`run_command`, `apply_patch`, and `done` — emit `stderr: 'skipped:batch_aborted'`. A
6-action plan is entirely voided by the failure of one command.

The batch-abort cascade means partial progress is never made: if a plan has 6 steps and step 2
fails, steps 3–6 are never attempted even if they are independent. The planner must re-plan
from scratch with no signal about which steps could have succeeded.

**Observed**: Events [175], [176], [177] all `success=False stderr='skipped:batch_aborted'`.

---

## ISSUE-9: Capture Invariant Violations Silently Ignored — IR May Be Corrupt

**Component**: `rustc_capture` / canon path interner / name interner
**Severity**: High (canon pipeline)

During IR capture, 6 invariant violations are logged but capture completes and reports
success with 5145 nodes:

- 5× `"malformed/private helper path segment in Canon path interner"`
- 1× `"MIR alloc/debug artifact leaked into Canon name interner"`

These are non-fatal — capture writes the JSON and exits 0 — but the resulting IR may contain
malformed path or name entries. Downstream orchestration and emission operate on this
potentially corrupt IR without any warning.

---

## ISSUE-10: Emitted Source Fails cargo build With Zero Errors Reported

**Component**: Orchestration pipeline / build reporter
**Severity**: High (canon pipeline)

`canon_orchestration.log` reports:
```
cargo build result: FAILED
  error count: 0
  warning count: 0
```

The emitted source fails to build, but the build reporter records zero errors and warnings.
The actual failure cause is not captured anywhere. A FAILED result with 0 errors provides no
actionable signal for debugging or automated repair.

---

## ISSUE-11: Emitted Source Contains Unreachable Code and goto Placeholder

**Component**: Canon IR emitter
**Severity**: Medium (canon pipeline)

Structural surface scan of emitted source reports:
```
unreachable count: 1
// goto count: 1
```

The emitter produced at least one unreachable block and one `// goto` comment — a placeholder
for an unimplemented control-flow construct. This is likely the direct cause of the build
failure in ISSUE-10, since `// goto` is not valid Rust.

---

## ISSUE-12: Provenance Solver Name Shadowing — 10 Instances

**Component**: Orchestration / `provenance_solver`
**Severity**: Medium (canon pipeline)

10 name-shadowing warnings during IR analysis. `canon::CanonId`, `canon::NameId`, and
`canon::PathId` are shadowed within the same module by multiple nodes across 7 modules
(244, 1392, 1962, 2046, 2882, 2946, 4089).

Name shadowing means symbol lookups during emission may resolve to the wrong node, producing
incorrect type annotations or call targets in the emitted source.

---

## Summary Table

| ID       | Component               | Description                                          | Severity |
|----------|-------------------------|------------------------------------------------------|----------|
| ISSUE-1  | canon-route             | Router fires before goal is available (unknown goal) | High     |
| ISSUE-2  | canon-route/summarize   | Mission summary is bare ID, not goal description     | High     |
| ISSUE-3  | canon-loop reward       | halt=False even when compiler clean                  | Critical |
| ISSUE-4  | Runtime scheduler       | Infinite observe loop post-reward                    | Critical |
| ISSUE-5  | RouteExecutor           | Router goes dark for ~47 ticks                       | High     |
| ISSUE-6  | Plan consumer           | cargo new on existing directory                      | High     |
| ISSUE-7  | Plan consumer           | Destructive command in plan batch                    | Medium   |
| ISSUE-8  | canon-act batch         | Single failure aborts entire planned batch           | Medium   |
| ISSUE-9  | rustc_capture           | Invariant violations silently ignored                | High     |
| ISSUE-10 | Build reporter          | FAILED build reports 0 errors                        | High     |
| ISSUE-11 | Canon IR emitter        | Unreachable code + goto in emitted source            | Medium   |
| ISSUE-12 | provenance_solver       | 10 name-shadowing instances in IR                    | Medium   |
