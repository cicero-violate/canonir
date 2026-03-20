
• Repair Plan

  1. Fix state feedback loop first (no new behavior until this is correct).
  2. Introduce a dedicated goal crate with structured goal state.
  3. Migrate routing/planning to consume structured state+goal, not prompt text blobs.
  4. Add deterministic completion checks in system layer.
  5. Deprecate legacy prompt-driven goal flow fully.

  Phase 1: Close the Feedback Loop

  1. Route state source of truth

  - Stop deriving route state only from watcher-fed Msg::Event.
  - Update route state synchronously from runtime-dispatched events (LoopObserved, LoopPlanned, LoopActed, LoopVerified, LoopRewarded).
  - Keep watcher path for persistence/replay only, not live control state.

  2. State transitions (deterministic)

  - On LoopObserved: set context_ready from goal presence + diagnostics.
  - On LoopPlanned: increment planned_pending; append journal entry.
  - On LoopActed: decrement planned_pending; set acted_unverified=true; set last_action_kind.
  - On LoopVerified: set acted_unverified=false; record pass/fail summary.
  - On LoopRewarded: set halt/completion flags if needed.

  3. Journal correctness

  - Maintain bounded ring buffer (e.g. 32).
  - Must append one line per phase outcome with IDs (plan_id, action_id, tool_call_id, tool_result_id where present).
  - Route prompt must include this journal snapshot.

  4. Guard policy adjustments

  - execute should depend on meaningful readiness (observed context or queued plan), not static false.
  - Add explicit fallback path when route parse/validation fails (not only LLM call failure).

  5. Verification

  - Add a runtime trace test: within N ticks after one observed goal, state fields must change from defaults.
  - Assert no repeated (none) journal across >2 routed cycles when events are flowing.

  Phase 2: Add Separate Goal Crate

  1. New crate: canon-goal

  - Core types:
      - GoalSpec (id, type, target path, constraints/requirements, metadata)
      - GoalStatus (pending/running/satisfied/failed)
      - GoalProgress (per-requirement pass/fail evidence)
  - Serialization: serde + stable schema version.

  2. Goal ingestion

  - Parse AGENT_GOAL.md once into structured GoalSpec (initial parser can be conservative).
  - Persist current goal state in runtime state store / tlog events.

  3. Goal events

  - Add explicit events:
      - goal_loaded
      - goal_progressed
      - goal_satisfied
      - goal_failed
  - Include deterministic evidence payloads.

  Phase 3: System-Owned Completion

  1. Deterministic completion evaluator

  - Implement is_goal_satisfied(state, goal) -> bool in system layer.
  - For your rust-project goal:
      - target dir exists
      - cargo build succeeds there
      - README.md exists and non-empty

  2. Routing integration

  - finish_ready must come from evaluator result, not LLM suggestion.
  - LLM may suggest conclude, but gate only approves when evaluator passes (or explicit override mode).

  Phase 4: Prompt and Routing Cleanup

  1. Route prompt payload

  - Send compact:
      - goal summary (from GoalSpec)
      - live state fields
      - bounded journal
  - Remove full goal markdown blob per tick.

  2. Planner prompt payload

  - Use GoalSpec + latest state delta.
  - Keep LLM focused on next action, not re-deriving goal semantics each cycle.

  Phase 5: Migration/Deprecation

  1. Mark legacy goal-from-prompt path deprecated.
  2. Keep compatibility shim for one release window.
  3. Remove shim after goal crate is stable and tests pass.

  Acceptance Criteria

  1. In a live run, context_ready, planned_pending, acted_unverified, last_action_kind, and journal all change meaningfully over ticks.
  2. No long execute/scan loops caused by stale state.
  3. Route decisions are based on current system state, not model memory.
  4. Goal completion is decided deterministically by system evaluator.
  5. AGENT_GOAL.md remains the single human-authored goal input; structured GoalSpec becomes runtime truth.
