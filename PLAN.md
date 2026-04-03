# PLAN: Prove Global Validate -> Append -> Dispatch, Enforce End-to-End Fail-Fast, Then Prove Semantic-State-Only Routing

## A. Authoritative Context

### Current State
- `SemanticStateSummary` is the single source of truth for routing and control-flow correctness.
- `scheduler_len`, `planned_pending`, and similar counters are not root truth for routing.
- The latest verifier confirms partial progress, but the system is still non-compliant because correctness is not yet enforced uniformly across the full runtime pipeline.

### Verified By Latest Verifier
- EventBus single-instance wiring previously established.
- `validate_before_append` occurs before dispatch in the targeted path reviewed by the verifier.

### Still Unverified Or False
- global enforcement of `validate -> append -> dispatch` across all event paths
- end-to-end fail-fast propagation across dispatch, routing, and loop
- routing strictly derived from `SemanticStateSummary`
- consistent fail-fast emission because `emit_tick` is still partially ignored elsewhere
- full invariant enforcement across the pipeline
- guarantee of successful event processing per tick

### Canonical Evidence
- `PLANS/SPEC.md` defines Canon as lawful event-sourced control: semantic state -> judgment -> lawful transition -> event log.
- `PLANS/INVARIANTS.md` defines append-only, deterministic replay, no-hidden-state, deterministic routing, lawful transition, and control/effect invariants.
- `VIOLATIONS.md` says:
  - ordering is not proven end-to-end
  - fail-fast is still inconsistent
  - downstream consumers are not proven invariant-gated
  - per-tick successful control propagation is not guaranteed
  - strict `SemanticStateSummary` routing is still not proven
- `PLANS/agent_0/diagnostics-agent_0.md` says:
  - global validate -> append -> dispatch is still not enforced across all live paths
  - ignored `emit_tick` and ignored dispatch outcomes still exist
  - invariant rejection is still not globally equivalent to hard prevention of unlawful control advancement
  - semantic-only routing intent exists, but global semantic-state authority is still not proven

### Source Evidence
- `canon-utils/canon-runtime/src/lib.rs`
  - contains multiple dispatch and append surfaces
  - targeted ordering fix exists on one path
  - file still documents divergence if a control event is dropped after dispatch
  - `LoopObserved` still requires explicit re-evaluation under uniform invariant rules
- `canon-utils/canon-runtime/src/bin/event_runtime.rs`
  - live loop still contains at least one ignored `runtime.emit_tick()` result
  - live loop still contains ignored flush behavior adjacent to critical emission
- `canon-utils/canon-runtime/src/invariants.rs`
  - invariant rejection semantics still need to be made globally authoritative
- `canon-utils/canon-route/src/policy.rs` and `canon-utils/canon-route/src/executor.rs`
  - semantic-only routing intent exists, but verifier still does not accept global proof of strict semantic-state authority
- `canon-utils/canon-loop/src/executor.rs` and `canon-utils/canon-loop/src/stage/plan.rs`
  - downstream pipeline surfaces still need proof that only lawful appended events advance control

### Planning Rule
- First prove global runtime correctness across all live event paths, not just one targeted fix.
- Then make fail-fast semantics uniform across emit, validate, append, dispatch, routing, and loop surfaces.
- Then guarantee each tick yields successfully appended and consumed control progress or terminates the runtime.
- Then prove routing is strictly `f(SemanticStateSummary)` with no queue-truth fallback.

## B. Ranked Root Failures

### 0. GLOBAL VALIDATE -> APPEND -> DISPATCH IS NOT PROVEN (PRIMARY BLOCKER)
Evidence:
- verifier only accepts a targeted path fix
- diagnostics still report multiple dispatch and append surfaces
- `canon-utils/canon-runtime/src/lib.rs` still contains code and comments describing divergence when dispatch advances before lawful persistence is guaranteed

Required outcome:
- every live event path must satisfy one authoritative rule: no control/FSM/consumer advancement before validation and write admission are known lawful

### 1. FAIL-FAST IS STILL NOT END-TO-END
Evidence:
- ignored `emit_tick` result remains in the live loop
- diagnostics report ignored dispatch outcomes still exist
- verifier marks full pipeline fail-fast enforcement as false

Required outcome:
- every critical runtime progression step must either succeed or terminate the runtime immediately

### 2. DOWNSTREAM CONSUMERS ARE NOT YET PROVEN INVARIANT-GATED
Evidence:
- `VIOLATIONS.md` explicitly says downstream consumers are not proven to reject invalid state
- targeted ordering repair alone does not prove all consumer execution is gated by lawful persisted state

Required outcome:
- dispatch layer, route layer, and loop layer must only advance from events that are already lawful under the invariant system

### 3. PER-TICK SUCCESSFUL CONTROL PROGRESSION IS NOT GUARANTEED
Evidence:
- verifier still marks per-tick successful event processing as false
- diagnostics still see invariant-violation strings in fresh canonical segments

Required outcome:
- each runtime tick must produce at least one successfully appended and consumed control event or fail fast

### 4. STRICT `SemanticStateSummary` ROUTING AUTHORITY IS STILL NOT PROVEN
Evidence:
- verifier still marks this unverified
- diagnostics still describe queue-counter terminology across runtime and loop surfaces

Required outcome:
- route choice must be globally enforced as semantic-state-derived with no heuristic or queue-truth fallback

## C. Dependency Order
1. Audit all live event paths for global `validate -> append -> dispatch` enforcement.
2. Remove every ignored `Result` on critical runtime progression paths.
3. Gate downstream consumer execution on lawful persisted state only.
4. Add a per-tick successful control progression invariant.
5. Remove exceptional invariant-bypass behavior unless explicitly lawful in `PLANS/SPEC.md` and `PLANS/INVARIANTS.md`.
6. Prove strict semantic-state-only routing across runtime, route, and loop layers.
7. Only then advance broader decision, observe, plan, act, verify, and reward restoration.

## D. READY NOW

### Executor: executor_pool
1. **Audit and patch all live event paths in `canon-utils/canon-runtime/src/lib.rs` for global ordering.**
   - Read every occurrence of `bus.dispatch`, `append_runtime_event`, `validate_before_append`, `handle_runtime_event_located_with_parents`, replay handling, and any other emission path.
   - Patch `canon-utils/canon-runtime/src/lib.rs` so every live event path obeys one rule: validate first, then append or commit lawful admission, then dispatch or an equally lawful atomic equivalent.
   - Remove any path where control/FSM state can advance before lawful write admission is known.
   - Rebuild and verify no live path can dispatch before validation and lawful write admission.

2. **Patch fail-fast semantics in the live runtime loop in `canon-utils/canon-runtime/src/bin/event_runtime.rs`.**
   - Read the main loop around the remaining `let _ = runtime.emit_tick();` and adjacent ignored flush behavior.
   - Replace ignored critical `Result` handling with fail-fast `?` propagation.
   - Rebuild and verify the live loop no longer ignores tick or flush failure.

3. **Audit all critical runtime progression calls for ignored `Result`s.**
   - Read `canon-utils/canon-runtime/src/bin/event_runtime.rs`, `canon-utils/canon-runtime/src/lib.rs`, and any related runtime helpers.
   - Search for ignored `Result` patterns on emission, flush, drain, dispatch, and related critical progression operations.
   - Patch all critical paths so failures terminate the runtime instead of being dropped.
   - Rebuild and verify no ignored critical path remains.

4. **Patch downstream invariant gating across dispatch, route, and loop layers.**
   - Read `canon-utils/canon-runtime/src/bus.rs`, `canon-utils/canon-runtime/src/invariants.rs`, `canon-utils/canon-route/src/executor.rs`, `canon-utils/canon-loop/src/executor.rs`, and `canon-utils/canon-loop/src/stage/plan.rs`.
   - Patch these surfaces so downstream consumers only execute on events that have already passed lawful validation and admission.
   - Add guards preventing invalid or rejected events from advancing route or loop state.
   - Rebuild and verify invalid state cannot propagate downstream.

5. **Patch invariant rejection semantics to be uniform and non-bypassable.**
   - Read `canon-utils/canon-runtime/src/invariants.rs` and the invariant-handling branches in `canon-utils/canon-runtime/src/lib.rs`.
   - Remove special-case persistence or execution behavior unless `PLANS/SPEC.md` and `PLANS/INVARIANTS.md` explicitly define it as lawful.
   - Re-evaluate the `LoopObserved` override under the same rule.
   - Rebuild and verify invariant rejection semantics are uniform.

6. **Add a per-tick successful control progression guarantee.**
   - Read the live loop in `canon-utils/canon-runtime/src/bin/event_runtime.rs` plus append and drain surfaces in `canon-utils/canon-runtime/src/lib.rs`.
   - Patch the runtime so each tick either produces at least one successfully appended and consumed control event or fails immediately.
   - Make the guarantee observable in canonical artifacts, runtime state, or tests.
   - Rebuild and verify silent zero-progress ticks are no longer possible.

7. **Patch `PLANS/SPEC.md` and `PLANS/INVARIANTS.md` to make global ordering and fail-fast law explicit.**
   - Read both files together.
   - Patch them so lawful runtime behavior explicitly requires global `validate -> append -> dispatch` ordering or a clearly defined lawful atomic equivalent.
   - Patch them so critical emission, append, dispatch, and per-tick control progression failures must terminate the runtime.
   - Re-read both files to confirm the law is explicit and mandatory.

8. **Add runtime ordering, fail-fast, and downstream-gating tests.**
   - Read runtime tests and invariant harnesses such as `canon-utils/canon-invariant/src/control_harness.rs` and `canon-utils/canon-invariant/src/request_lifecycle_harness.rs`.
   - Add tests that fail when:
     - any live path dispatches before validation and lawful append
     - critical `Result`s are ignored
     - downstream consumers execute on invalid state
     - a tick does not yield successfully appended and consumed control progress
     - invariant rejection is bypassed without lawful documentation
   - Run the relevant test targets.

9. **Inspect fresh canonical artifacts with Python after enforcement patches.**
   - Read only the newest files under `state/event_log/event.tlog.d`.
   - Verify current artifacts show lawful control progress, fail-fast behavior, and reduced invariant-violation noise.
   - Do not advance if fresh artifacts still indicate silent fail-open behavior or unlawful progression.

10. **Re-check strict `SemanticStateSummary` routing authority only after runtime correctness is global.**
   - Read `canon-utils/canon-route/src/policy.rs`, `canon-utils/canon-route/src/executor.rs`, `canon-utils/canon-loop/src/executor.rs`, and any runtime surfaces that still carry queue-counter terminology.
   - Patch route choice so queue-local mirrors cannot alter outcome unless proven semantic-state-derived.
   - Rebuild and run route-policy tests or targeted runtime checks.

## E. BLOCKED / NOT READY YET
- Any broader pipeline restoration before global `validate -> append -> dispatch` enforcement is proven.
- Any routing work that assumes runtime correctness while ignored critical `Result`s or downstream invalid-state propagation remain.
- Any reliance on observational logs or targeted-path success while global correctness is still unverified.
