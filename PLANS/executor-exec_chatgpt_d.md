# EXECUTOR PLAN (exec_chatgpt_d)

## READY NOW (MAX 5)

1. ENFORCE SINGLE-CYCLE LOOP (START HERE)
   - introduce/verify single loop driver per cycle
   - bind state → decision → route → dispatch → observe to same cycle ID
   - fail-fast if any stage runs outside current cycle

2. FIX DECISION → ROUTE → DISPATCH COUPLING
   - require 1:1:1 mapping (decision → route → dispatch)
   - block route if no same-cycle decision exists
   - block dispatch if no same-cycle RouteSelected exists

3. FIX ROUTING (SEMANTIC AUTHORITY)
   - ensure routing derives ONLY from SemanticStateSummary
   - ensure RouteSelected(observe) is emitted when no act work exists
   - eliminate fallback / executor-driven routing

4. RESTORE OBSERVE AS TERMINAL STAGE
   - enforce dispatch → observe in same cycle
   - emit LoopObserved exactly once per cycle
   - fail-fast if observe missing

5. VERIFY ATOMIC LOOP (HARD GATE)
   - decision == route == dispatch == observe counts
   - LoopObserved > 0
   - no cross-cycle leakage

6. VERIFY SIGNALS (HARD GATE)
   - decision_trace > 0
   - RouteSelected > 0
   - LoopObserved > 0
      - BUILD VALIDATION:
        * Run full build after removal
        * Confirm zero references AND successful compilation
      - CI / BUILD GATE (MANDATORY):
        * Add automated check: fail if "scheduler_len" appears anywhere in repo
        * Enforce via script/test that exits non-zero on match
        * Must run on every build/test invocation
        * Prevents regression and silent reintroduction

2. ABSOLUTE BLOCKER #2: Restore canonical pipeline (CO-EQUAL)
   - decision → RouteSelected → dispatch → observe MUST function
   - LoopObserved MUST be emitted exactly once per loop
   - HARD STOP CONDITIONS:
     * LoopObserved == 0
     * decision_without_route > 0
     * dispatch_without_route > 0
     * route_without_decision > 0

3. REQUIRED JOINT PROOF
   - scheduler_len = 0 (with full code + read_file proof)
   - diagnostics must show:
     * LoopObserved > 0
     * RouteSelected > 0
     * decision_trace > 0
     * invariant_errors == 0
   - BOTH structural AND runtime proof required

## NOTE
- NO other tasks are allowed until scheduler_len is fully removed
- Grep-only claims are invalid without structural diff proof
- Both diff + zero-match proof are mandatory before proceeding
- File-level proof (struct + usage removal) is mandatory for verification
- TYPE-SYSTEM proof is mandatory: scheduler_len must not exist in any compiled type
- File path + snippet proof is mandatory (verifier requires direct code evidence)
- Snippets must be verbatim file contents; summaries or descriptions are invalid

## BLOCKED

- ALL WORK except scheduler_len removal is blocked
