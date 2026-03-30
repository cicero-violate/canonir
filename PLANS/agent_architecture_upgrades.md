
  Architecture: 8/10
  The event-driven runtime with a typed event log, deterministic routing fallbacks, and causal parent tracking is genuinely well-designed. The separation between the mini-agent (isolated process, browser-driven) and the canon runtime (full
   event loop with planner/router/executor lanes) is clean. Most systems this ambitious end up as a tangled monolith — this one doesn't.

  Robustness: 5/10
  The stuck-loop bug we fixed today is a good example of a whole class of issues: the system has no circuit breakers for LLM failure cascades. One timeout shouldn't spin forever. The consecutive_llm_plan_failures fix helps but there are
  probably more like it. The repair server being unreachable (port 9102 refused) silently degrading instead of alerting is another sign of this.

  Agent quality: 6/10
  The planner produces good high-level structure but the executor makes repeated patch format errors (line number prefixes, bare blank lines, single-context @@ anchors). That's partly prompt quality, partly the agents not having a tight
  enough feedback loop when a patch fails. The auto-read-on-anchor-miss in the mini-agent is a good fix for this.

  Observability: 8/10
  The event log is excellent — being able to watch event.tlog.d and trace exactly what happened (tlog pending_set, tlog pending_discharged, route_suppressed) is rare. Most agent systems are black boxes. Yours isn't.

  Operational maturity: 5/10
  Port conflicts, hardcoded addresses, single-connection extension bottleneck, main_1.rs floating unlinked — these are signs the system is still being built out rather than hardened. Normal for this stage.

  Overall: 7/10
  The core ideas are solid and the execution is further along than most. The gaps are operational polish and failure handling, not architecture.

❯ what would put it up there, so that we can make a difference like a claude system. but on budget

● Honest breakdown of the gaps and what closes them cheaply:

  ---
  1. Reliable LLM I/O — biggest ROI

  Your biggest tax right now is the browser automation layer. ChatGPT via Chromium extension is brittle — timeouts, tab state, rate limits, the whole stuck-loop bug we fixed today. Switching the planner/executor to a direct API (Anthropic
  or OpenAI) removes that entire failure class. Structured outputs (JSON mode) eliminates the patch format errors completely — the model physically cannot output malformed JSON. This alone would probably double your task completion rate.

  Cost: API calls are cheaper than you think for the volume an agent loop generates.

  ---
  2. Context before it hits the LLM

  Your planner times out partly because the context it receives is huge and unfiltered. The discovery agent is a step toward this but the real fix is a pre-LLM compression pass — summarize the event history, strip redundant tool results,
  keep only what's relevant to the current objective. A local model (Ollama + Qwen or Mistral) can do this cheaply. The expensive model only sees a tight, relevant context window.

  ---
  3. Persistent memory across sessions

  Right now every session starts cold. A lightweight vector store (even just SQLite + local embeddings) that persists what the agents have tried, what failed, and what the codebase looks like would compound value fast. The planner would
  stop re-discovering the same things every cycle.

  ---
  4. Inner-loop self-check

  Before any apply_patch hits the file system, run a syntax check on the patch itself and the resulting file. You already do cargo check after — move some of that validation earlier. A fast local linter as a pre-commit gate catches 80% of
  executor errors before they waste a full LLM round trip.

  ---
  5. Cheap model for cheap decisions

  Your router is LLM-disabled right now — that's why you get the deterministic fallback loop. A small fast model (Haiku, GPT-4o-mini, or a local 7B) is good enough for routing and planning decisions. Reserve the big model for actual code
  generation. This cuts cost and latency at the same time.

  ---
  6. Tests

  You have near-zero test coverage on the runtime itself. Every change you make has unknown blast radius. Even 20-30 integration tests on the event loop, routing logic, and patch parser would give you confidence to move faster. The patch
  parser fix today — we added one test. That's the right instinct, just needs to be systematic.

  ---
  Priority order if budget is tight:

  1. API-based LLM calls + structured outputs (kills the browser automation fragility)
  2. Context compression before planner call (kills timeouts)
  3. Cheap model for routing (re-enables the router, breaks the deterministic loop permanently)
  4. Persistent memory (compounds over time)
  5. Tests (enables faster iteration without fear)

  The gap between your system and a production one isn't the architecture — it's the reliability of the I/O layer and the feedback loops. Those are fixable without rebuilding anything.
