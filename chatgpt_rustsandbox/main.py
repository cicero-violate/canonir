import sys
import json
import difflib
from typing import Dict, Any

from adaptive_execution_loop import AdaptiveExecutionLoop
from prompt_contract import SYSTEM_PROMPT
import os


class RustSandboxSystem:

    def __init__(
        self,
        root: str = "workspace",
        task_budget: int = 50,
        resource_budget: int = 5000,
        plan_token_budget: int = 1500,
        max_revisions: int = 3,
    ):
        print(SYSTEM_PROMPT)

        # Ensure root exists
        if not os.path.exists(root):
            os.makedirs(root, exist_ok=True)

        # Preflight: warn if not a Cargo project
        cargo_path = os.path.join(root, "Cargo.toml")
        if not os.path.exists(cargo_path):
            print(f"[Preflight Warning] No Cargo.toml found in '{root}'.")
            print("compile_clean invariant will fail until a Rust project exists.")

        self.loop = AdaptiveExecutionLoop(
            root=root,
            task_budget=task_budget,
            resource_budget=resource_budget,
        )

        self.plan_token_budget = plan_token_budget
        self.max_revisions = max_revisions
        self.previous_plan = None

    # --------------------------------------------------
    # VALIDATION
    # --------------------------------------------------

    def validate_plan(self, plan: Dict[str, Any], proposal: Dict[str, Any]):
        required = [
            "objective",
            "steps",
            "ordering_rationale",
            "risk_analysis",
            "task_mapping",
            "invariants"
        ]

        for r in required:
            if r not in plan:
                raise ValueError(f"PLAN missing '{r}'")

        for t in proposal["tasks"]:
            if t["id"] not in plan["task_mapping"]:
                raise ValueError(
                    f"PLAN.task_mapping missing task '{t['id']}'"
                )

        token_estimate = len(json.dumps(plan).split())
        if token_estimate > self.plan_token_budget:
            raise ValueError("PLAN exceeds token budget")

    # --------------------------------------------------
    # PLAN DIFF
    # --------------------------------------------------

    def analyze_plan_diff(self, plan: Dict[str, Any]):
        serialized = json.dumps(plan, indent=2)

        if self.previous_plan:
            print("\n[PLAN DIFF]")
            diff = difflib.unified_diff(
                self.previous_plan.splitlines(),
                serialized.splitlines(),
                lineterm=""
            )
            for line in diff:
                print(line)

        self.previous_plan = serialized

    # --------------------------------------------------
    # EXECUTION
    # --------------------------------------------------

    def run(self, data: Dict[str, Any]):
        revision = 0

        while revision <= self.max_revisions:
            try:
                plan = data["plan"]
                proposal = data["proposal"]

                print("\n--- PLAN ---")
                print(json.dumps(plan, indent=2))

                self.validate_plan(plan, proposal)
                self.analyze_plan_diff(plan)

                print("\n--- EXECUTION ---")
                self.loop.execute_with_checkpoints(
                    proposal,
                    plan["invariants"]
                )

                print("\nExecution Successful.")
                return

            except Exception as e:
                print("\nExecution failed:", str(e))
                revision += 1

                if revision > self.max_revisions:
                    raise RuntimeError(
                        "Maximum PLAN revision attempts exceeded"
                    )

                print("\n--- PLAN REVISION REQUIRED ---")
                raise


if __name__ == "__main__":
    print("RustSandboxSystem loaded.")
    print("No automatic execution is performed.")
    print("Provide structured PLAN + PROPOSAL to system.run(...)")
