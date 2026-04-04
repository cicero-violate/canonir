# Diagnostics Report

## Inputs Scanned
- Event log segments reviewed repeatedly from `state/event_log/event.tlog.d`, with freshest focus on `00000000000000012399.log` through `00000000000000012444.log`.
- Fresh failure-bearing segments observed in the current cycle: `00000000000000012400.log` and `00000000000000012437.log`.
- Violations reviewed: `VIOLATIONS.md`.
- Source areas reviewed:
  - `canon-utils/canon-route/src/decision.rs`
  - `canon-utils/canon-route/src/executor.rs`
  - `canon-utils/canon-route/src/lib.rs`
  - `canon-utils/canon-route/src/context.rs`
  - `canon-utils/canon-loop/src/stage/plan.rs`
  - `canon-utils/canon-runtime/src/bin/harness_repair.rs`
  - `canon-utils/canon-mini-agent/src/main.rs`
- Commands run:
  - Multiple structured Python scans over the latest event-log segments.
  - Source-term scans for `SemanticStateSummary`, `scheduler_len`, `planned_pending`, `pending_act`, `RequestDispatch`, `RouteController`, and `decide_from_json`.
  - Targeted line-snippet extraction for the route, loop, and harness files above.

## Ranked Failures

1. Impact: high
   Signal: Queue-local scheduler state still drives control recovery in the plan stage.
   Evidence:
   - `canon-utils/canon-loop/src/stage/plan.rs:173-179`:
     - `execute_complete(...)` uses `ctx.scheduler.len()`.
     - On `pending_plan.take()` = `None`, it treats `ctx.scheduler.len() == 0` as the trigger to inject a fallback task.
   - `canon-utils/canon-loop/src/stage/plan.rs:643-645`:
     - `let scheduler_len_after = ctx.scheduler.len();`
     - `assert!(scheduler_len_after > 0, "Plan produced zero tasks — deadlock risk");`
   - Canonical law says `SemanticStateSummary` is the routing/control authority; `scheduler_len` is not authoritative unless proven as a derived mirror.
   - Latest event-log scans did **not** show fresh `RouteSelected`, `LoopObserved`, `LoopVerified`, or `LoopRewarded` markers in the sampled newest segments, so the strongest live control-flow evidence is source-side authority drift, not a contradictory fresh route trace.
   Repair Targets:
   - `canon-utils/canon-loop/src/stage/plan.rs::execute_complete`
   - Remove queue-count-driven fallback injection and queue-count assertions as control authority.
   - Replace `scheduler_len` gating with semantic-state-derived outcomes (`SemanticStateSummary`, planning status, explicit no-work / blocked / repair-required semantics).
   - Ensure zero-task handling is emitted as canonical semantic/control events, not hidden scheduler seeding.

2. Impact: high
   Signal: Fresh rustc capture invariant violations are active now and are rooted in debug/fallback string generation inside harness repair logic.
   Evidence:
   - Current-cycle event-log scans repeatedly found only `invariant violation` hits in:
     - `state/event_log/event.tlog.d/00000000000000012400.log`
     - `state/event_log/event.tlog.d/00000000000000012437.log`
   - Fresh violation sample from latest logs:
     - `invariant violation: alloc/debug artifact leaked into name interner name="|| extract_primary_file_line(prompt).map(|(path, line)| format!(\"{\\\"action\\\":\\\"read_file\\\",...`.
   - Source root cause found in `canon-utils/canon-runtime/src/bin/harness_repair.rs`:
     - `355-383` in `local_planner_fallback(...)`.
     - `366`: `let prompt_primary = extract_primary_file_line(prompt)...`
     - `377-379`: fallback action is synthesized with `extract_primary_file_line(prompt).map(|(path, line)| format!("{\"action\":\"read_file\",...`.
   - The leaked closure/debug string in the logs matches this source shape.
   Repair Targets:
   - `canon-utils/canon-runtime/src/bin/harness_repair.rs::local_planner_fallback`
   - Stop emitting or capturing closure/formatted fallback strings in a way that can enter the rustc capture name interner.
   - Precompute/sanitize fallback action strings before any debug/capture path, or avoid logging/capturing closure-shaped expressions entirely.
   - Audit the capture boundary so debug artifacts from fallback generation cannot become interner names.

3. Impact: medium
   Signal: Semantic authority is mostly restored, but legacy decision API/wiring still carries non-semantic scaffolding and stale naming.
   Evidence:
   - `canon-utils/canon-route/src/decision.rs:19-35`:
     - `decide_from_json(semantic: &SemanticStateSummary, _model_json: &str, prompt: String, _controller: &mut RouteController)`.
     - Decision routing now derives from `SemanticStateSummary`, but the function name, `_model_json`, and `_controller` preserve obsolete influence paths.
   - `canon-utils/canon-route/src/decision.rs:1-5` still imports `RouteController`.
   - Prior source scan found:
     - `canon-utils/canon-route/src/executor.rs:849` calls `decide_from_json(&self.ctx.semantic_summary, &semantic_json, prompt.clone(), &mut self.controller)`.
     - `canon-utils/canon-route/src/lib.rs:9` re-exports `decide_from_json`.
   - This is no longer a proof of current semantic-authority failure, but it is still architectural drift and regression surface.
   Repair Targets:
   - `canon-utils/canon-route/src/decision.rs`
   - `canon-utils/canon-route/src/executor.rs`
   - `canon-utils/canon-route/src/lib.rs`
   - Rename to a semantic-state authority API such as `decide_from_semantic_state`.
   - Remove `RouteController` and `_model_json` from the decision interface.
   - Remove residual controller threading through executor/lib exports where it no longer contributes to semantic routing.

4. Impact: medium
   Signal: `VIOLATIONS.md` is stale and materially misstates current architecture, which can misdirect planner effort.
   Evidence:
   - `VIOLATIONS.md:3-12` claims `decide_from_json(ctx: &RouteContext, ...)` still accepts full `RouteContext`.
   - Current source disproves that claim:
     - `canon-utils/canon-route/src/decision.rs:19` now accepts `semantic: &SemanticStateSummary` directly.
   - `VIOLATIONS.md:34-41` claims semantic-source population is unverified.
   - Latest verifier summary supplied in the prompt says verified items include:
     - `decision input restricted to SemanticStateSummary at type level`
     - `semantic_summary assigned from LoopObserved event`
     - `no additional mutation sites for semantic_summary found`
   Repair Targets:
   - `VIOLATIONS.md`
   - Any generator/prompt source that injects stale violation text into diagnostics/planner prompts (likely `canon-utils/canon-mini-agent/src/main.rs` diagnostics prompt assembly).
   - Split stale architectural claims from live runtime failures.
   - Regenerate violations from current code truth before using them as planner authority.

## Planner Handoff
- Highest-value repair targets, in order:
  1. `canon-utils/canon-loop/src/stage/plan.rs`
     - Remove `scheduler_len` as control authority for fallback injection and deadlock assertions.
     - Make zero-task / blocked / repair-required outcomes semantic-state-driven.
  2. `canon-utils/canon-runtime/src/bin/harness_repair.rs`
     - Eliminate the active capture/interner leak caused by `extract_primary_file_line(prompt)` fallback/debug string generation.
  3. `canon-utils/canon-route/src/decision.rs`
     - Rename and simplify the decision entrypoint to semantic-state-only authority.
  4. `canon-utils/canon-route/src/executor.rs` and `canon-utils/canon-route/src/lib.rs`
     - Remove residual `RouteController` / `_model_json` threading and old API exports.
  5. `VIOLATIONS.md` plus diagnostics prompt generation in `canon-utils/canon-mini-agent/src/main.rs`
     - Remove stale claims so the planner stops chasing already-fixed architecture issues.

- Blockers / missing evidence:
  - In the newest sampled event-log segments for this cycle, I did **not** observe fresh `RouteSelected` / `LoopObserved` / `LoopVerified` / `LoopRewarded` markers; the sampled fresh logs were dominated by rustc capture invariant violations instead.
  - Because of that, the current ranking is anchored primarily on source-truth plus fresh capture-failure logs, not on a contradictory fresh runtime route trace.
  - If a later cycle produces fresh runtime route events in the latest segments, re-run diagnostics to confirm whether any queue-driven routing survives beyond the plan-stage source evidence above.
