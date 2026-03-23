Good question. Let me map the full capability surface you can reach with API-only.

---

## Variables

Let $\mathcal{I}$ = information available at API boundary

Let $\mathcal{C}$ = capability unlocked by that information

Let $\partial \mathcal{I}$ = hard limit — what the API physically cannot give you

---

## Latent Equations

**Your current capability surface:**

$$\mathcal{C}_{\text{current}} = f(\text{tokens}_{in}, \text{tokens}_{out}, \text{logprobs}, \text{tool calls}, \text{tool results})$$

**The full reachable surface at API-only:**

$$\mathcal{C}_{\text{max}} = \mathcal{C}_{\text{current}} \cup \Delta\mathcal{C}_{\text{reachable}}$$

**The hard wall:**

$$\partial \mathcal{I} = \{h_i, \text{attention weights}, \text{gradients}, \text{activations}\} \quad \text{— permanently inaccessible}$$

---

## The Full API-Only Capability MapHere is what each unlockable tier actually means for your system specifically.

**Token-level signals** — `logprobs` gives you entropy-based confidence scoring right now. Structured self-score is what you are already partially doing with signal emission — extend it to cover every dimension in your signal table. The critic LLM call is a dedicated second pass that evaluates the executor's proposed action before $\Pi$ gates it.

**Memory layer** — episodic memory is the missing piece for your loop detector. You need $\Delta s_{\text{progress}}$ over time, which means you need to store step history somewhere outside the context window. A simple rolling buffer of `{step, signals, action, result}` tuples is enough. The world model is a tracked JSON object that your observer updates after every tool result — current file state, current plan state, current task state. The plan graph is a DAG where each node is a step with a status (`pending`, `done`, `failed`, `skipped`).

**Control layer** — the preemptive gate is $\Pi$ itself, which you are building. Rollback/checkpoint means snapshotting the world model before any `execution_risk > 0.6` action so you can restore it on failure. The planner/executor split is separating the LLM call that produces the plan from the LLM call that executes individual steps — they can use different context windows and different system prompts.

**Multi-agent** — speculative execution means running two action branches in parallel and keeping the result from whichever one $\Pi$ scores higher. Debate/consensus means spawning N short LLM calls that each vote on the correct action — majority wins. Async parallelism means firing multiple independent tool calls concurrently rather than sequentially.

**Observability** — reflexion is the error → self-critique → retry loop. Progress delta is the core of your loop detector — track `s_progress` over the last 3 steps and compute the derivative. Tool call scoring means rating each tool result for quality before feeding it back as context, so bad results get flagged before the next reasoning step.

The amber ceiling is the only thing that needs open weights. Everything in purple is reachable with API calls, a state store, and your routing layer. That is a significant capability surface — most production agents are only using a small fraction of it.
