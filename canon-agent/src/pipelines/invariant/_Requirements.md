## Generic Agent Loop — Design Criteria

### 1. Deterministic Phase Machine

* Fixed phases: `Observe → Plan → Act → Verify`
* One phase per tick
* Enforced phase transitions
* No implicit side effects

---

### 2. Stateless Decision Surface

* Each tick derives from:

  * `cwd`
  * `last_output`
  * `last_error`
  * `rationale_history`
* No hidden domain memory
* No implicit reward shaping

---

### 3. Strict Delta Model

* All mutations expressed as structured deltas
* No free-form shell execution
* Phase-restricted capabilities:

  * Observe → read-only
  * Act → controlled mutation
  * Verify → read-only execution

---

### 4. Verifiable Environment Boundary

* All environment interaction via:

  * Explicit commands
  * Explicit patch application
* Every side effect observable
* Every tick reproducible

---

### 5. Explicit Exit Condition

* External `exit_check_command`
* Boolean success condition
* No heuristic success detection

---

### 6. Parseable LLM Contract

* Structured JSON response
* Required fields:

  * `phase`
  * `deltas`
  * `rationale`
* Strict validation before execution

---

### 7. Minimal Persistent State

Only:

* `current_phase`
* `tick`
* `rationale_history`

No:

* reward shaping
* stagnation heuristics
* domain classification
* scoring layers

---

### 8. Idempotent Act Layer

* Re-applying same delta must not corrupt state
* Safe failure semantics
* No partial mutation leakage

---

### 9. Transparent Logging

* Each tick produces:

  * input snapshot
  * LLM output
  * applied deltas
  * verification output
  * exit_check_output
  * state_snapshot
* Fully replayable

* Log root directory:

  `/workspace/ai_sandbox/canon/agent_logs`

* Required log layout (columnar by artifact type):

  agent_logs/
    input_prompt/<tick>.md
    llm_response/<tick>.json
    deltas_applied/<tick>.json
    act_output/<tick>.txt
    verify_output/<tick>.txt
    exit_check_output/<tick>.txt
    state_snapshot/<tick>.json

* Replay invariant:

  For each tick t:

    Log_t = { I_t, R_t, D_t, O_t, V_t }

  The complete state trajectory must be reconstructible using only:

    - input_prompt/t
    - llm_response/t
    - deltas_applied/t
    - exit_check_output/t

---

### 10. Domain Agnostic

* No Cargo assumptions
* No compiler coupling
* No invariant model
* No project-specific parsing

---

## Core Equation

Let:

* ( S_t ) = observable system state
* ( P_t ) = phase
* ( A_t ) = agent decision (deltas)
* ( V(S) ) = exit verification

### Transition

[
S_{t+1} = \text{Apply}(S_t, A_t)
]

### Loop Condition

[
\neg V(S_t) \Rightarrow t = t + 1
]

---

A generic agent loop is:

> A deterministic phase machine that transforms observable state through structured deltas until an externally verifiable condition is satisfied.
