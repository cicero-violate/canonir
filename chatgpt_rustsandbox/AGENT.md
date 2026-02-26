
# 🧠 Unified Agent Protocol (ChatGPT Execution Contract)

This document defines EXACTLY how ChatGPT must operate in this sandbox.

The system now uses:

- UnifiedAgent (planner + controller)
- Compile gate (cargo check)
- Budget manager
- Deterministic DAG ordering
- Optional Jupyter hook

---

# 1️⃣ Bootstrap (Run Once Per Session)

import sys
sys.path.append("/mnt/data")

from unified_agent_system import bootstrap

agent = bootstrap(
    root="model_ir_project",
    task_budget=50,
    resource_budget=5000
)

agent.propose({
    "tasks": [
        {"id": "create", "deps": [], "payload": {"action": "cargo_new"}},
    ]
})

agent.run()

---

# 2️⃣ Proposal Format (MANDATORY)

proposal = {
    "tasks": [
        {
            "id": "task_id",
            "deps": ["optional_dependency_ids"],
            "payload": {
                "type": "cargo_init" | "write_file",
                "path": "src/file.rs",
                "content": "...rust code..."
            }
        }
    ]
}

Rules:
- IDs must be unique
- Dependencies must reference existing IDs
- No cycles allowed
- Project must compile after every step

---

# 3️⃣ Load Proposal

agent.propose(proposal)

---

# 4️⃣ Execution Modes

Step Mode:
agent.step()

Full Run:
agent.run()

---

# 5️⃣ Compile Gate

After EACH task:
- cargo check runs
- If compile fails → execution stops
- Fix via new proposal

---

# 6️⃣ Budget Rules

Each task consumes:
- 1 task unit
- Resource cost proportional to payload size

If budget exceeded → execution halts.

---

# 7️⃣ Determinism Requirements

ChatGPT MUST:
- Keep tasks minimal and ordered
- Avoid breaking compilation
- Respect dependency graph
- Avoid side effects outside project root

---

# 7.1️⃣ Mandatory Multi-Call Execution

You MUST use multiple python tool call invocations.

Rule:
- Each invocation should execute a bounded proposal (recommended 1–5 tasks).
- After each invocation, re-check state (files/compile/diagnostics) and decide the next proposal.
- Prefer many small convergent steps over one big plan.

---

# 7.2️⃣ Mandatory Reasoning / Introspection

Before each proposal:
- State assumptions (what you believe is true right now)
- State unknowns (what might be wrong)
- State what evidence you will collect in the next invocation (sanity check)

Do not add new JSON schema fields; embed these inside existing PLAN fields.

---

# 8️⃣ Canonical Execution Pattern

BOOTSTRAP
→ propose
→ step()
→ step()
→ done

---

# 9️⃣ Sandbox Summary

- Rust toolchain: /mnt/data/rust-sandbox/bin
- Cargo mirror configured
- CPU only
- No public GitHub
- apply_patch at /opt/apply_patch/bin/apply_patch

---

This file replaces AGENT.md as the authoritative execution contract.
