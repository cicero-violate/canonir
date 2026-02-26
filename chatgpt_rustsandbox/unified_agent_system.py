import os
import tarfile
from typing import Dict, Any

from agent_loop import DAGPlanner, Task
from external_reasoning_controller_extended import ExternalReasoningController

REGISTRY_URL = "sparse+https://<user>:<pass>@packages.applied-caas-gateway1.internal.api.openai.org/artifactory/api/cargo/cargo-public/index/"

# Project-local Rust toolchain configuration (no /mnt/data dependency)
PROJECT_ROOT = os.path.abspath(os.getcwd())
RUST_ARCHIVE = os.path.join(PROJECT_ROOT, "rust-sandbox-minimal.tar.gz")
RUST_ROOT = "rust-sandbox"
RUST_EXTRACT_PATH = os.path.join(PROJECT_ROOT, ".rust_env")


# ------------------------------------------------------------
# Rust Environment Setup (lazy, deterministic)
# ------------------------------------------------------------

def _ensure_rust_sandbox():
    archive = RUST_ARCHIVE
    extract_root = RUST_EXTRACT_PATH
    rust_bin = os.path.join(extract_root, RUST_ROOT, "bin")

    if not os.path.exists(os.path.join(rust_bin, "rustc")):
        if not os.path.exists(archive):
            raise RuntimeError("rust-sandbox-minimal.tar.gz not found")
        os.makedirs(extract_root, exist_ok=True)
        with tarfile.open(archive, "r:gz") as tar:
            tar.extractall(path=extract_root)

    os.environ["PATH"] = rust_bin + ":" + os.environ.get("PATH", "")
    os.environ["CARGO_HOME"] = os.path.abspath("./.cargo")


def _configure_cargo_registry(root: str):
    cargo_dir = os.path.join(root, ".cargo")
    os.makedirs(cargo_dir, exist_ok=True)

    config = f"""
[source.crates-io]
replace-with = "mirror"

[source.mirror]
registry = "{REGISTRY_URL}"
"""

    with open(os.path.join(cargo_dir, "config.toml"), "w") as f:
        f.write(config)


# ------------------------------------------------------------
# Unified Agent
# ------------------------------------------------------------

class UnifiedAgent:

    def __init__(self, root="workspace", task_budget=100, resource_budget=10000):
        _ensure_rust_sandbox()
        _configure_cargo_registry(root)

        self._root = root
        self._controller = ExternalReasoningController(
            root,
            task_budget,
            resource_budget
        )
        self._planner = DAGPlanner()
        self._initialized = False
        self._task_order = []
        self._cursor = 0

    # --------------------------------------------------------
    # PROPOSE
    # --------------------------------------------------------

    def propose(self, proposal: Dict[str, Any]):
        if "tasks" not in proposal:
            raise RuntimeError("Invalid proposal")

        self._planner = DAGPlanner()

        for t in proposal["tasks"]:
            self._planner.add_task(
                Task(t["id"], t.get("deps", []), t.get("payload", {}))
            )

        self._task_order = self._planner.topological_sort()
        self._cursor = 0

        sorted_tasks = []
        for tid in self._task_order:
            task = self._planner.tasks[tid]
            sorted_tasks.append({
                "id": task.id,
                "deps": task.deps,
                "payload": task.payload
            })

        self._controller.ingest_proposal({"tasks": sorted_tasks})
        self._initialized = True

    # --------------------------------------------------------
    # STEP (required by AGENT.md)
    # --------------------------------------------------------

    def step(self):
        if not self._initialized:
            raise RuntimeError("No proposal loaded")

        result = self._controller.advance()
        print(result)

        if result.get("done"):
            return True
        return False

    # --------------------------------------------------------
    # RUN
    # --------------------------------------------------------

    def run(self):
        if not self._initialized:
            raise RuntimeError("No proposal loaded")

        while True:
            done = self.step()
            if done:
                break


# ------------------------------------------------------------
# Bootstrap Entry
# ------------------------------------------------------------

def bootstrap(root="workspace", task_budget=100, resource_budget=10000):
    return UnifiedAgent(root, task_budget, resource_budget)
