# INTENT
## Objective
Break the infinite planning loop caused by LLM planner timeouts by tracking consecutive failures and routing to observe after a threshold instead of repeatedly selecting plan.
## Constraints
- no build break
- no test failure
## Targets
- canon-utils/canon-route/src/context.rs
- canon-utils/canon-route/src/executor.rs
## Success Criteria
- consecutive LLM planning failures are incremented on llm_failed or timeout and reset on successful planning
- routing switches to observe when failures ≥ 2
- router_disabled_fallback returns observe instead of always selecting plan
- infinite plan → plan loop is eliminated in runtime behavior
- agent execution progresses past repeated LLM timeout scenarios


### **Agent Objective (direct)**

**Goal:**
Restore a **fully reachable, invariant-safe execution loop**.

**Tasks:**

1. Detect all invariant violations in logs
2. Map each violation → missing successor or illegal transition
3. Patch minimal invariant or emitter logic
4. Re-run harness until:

   * no append failures
   * no duplicate control events
   * loop completes at least once

**Success Criteria:**
[
\exists \tau:\ \text{complete loop}(\tau)\land \text{violations}=0
]

---

### **English**

Your system is down because **control flow is not closed**.
Agents should not “improve” anything — only **restore closure**:

* every event leads somewhere valid
* no rejection by writer
* loop runs end-to-end

This is a **repair objective**, not a growth objective.


