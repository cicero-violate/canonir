import sys
from typing import Dict, Any, List, Set
import subprocess
from collections import defaultdict, deque

sys.path.append("/mnt/data")

from unified_agent_system import bootstrap
from diagnostics_engine import DiagnosticsEngine
from revision_engine import RevisionEngine
from state_snapshot import ProjectState


# ============================================================
# SAT-LIKE BOOLEAN CONSTRAINT SOLVER
# ============================================================

class ProposalSATValidator:

    def __init__(self, tasks: List[Dict[str, Any]]):
        self.tasks = tasks
        self.constraints = []
        self.results = {}

    def add_constraint(self, name: str, fn):
        self.constraints.append((name, fn))

    def solve(self):
        for name, fn in self.constraints:
            result = fn()
            self.results[name] = result
            if not result:
                raise ValueError(f"SATSolver: constraint '{name}' failed")
        return True


# ============================================================
# INVARIANT ENGINE
# ============================================================

class InvariantEngine:

    def __init__(self, root: str):
        self.root = root

    def verify(self, invariants: List[Dict[str, Any]], proposal: Dict[str, Any]):
        for inv in invariants:
            name = inv["name"]
            if name == "acyclic_dag":
                self._check_acyclic(proposal["tasks"])
            elif name == "no_duplicate_ids":
                self._check_duplicate_ids(proposal["tasks"])
            elif name == "deps_defined":
                self._check_deps_defined(proposal["tasks"])
            elif name == "non_empty_payload":
                self._check_payload(proposal["tasks"])
            elif name == "compile_clean":
                self._check_compile()
            else:
                raise ValueError(f"Unknown invariant '{name}'")

        print("[Invariant Verification Passed]")

    def _check_duplicate_ids(self, tasks):
        ids = [t["id"] for t in tasks]
        if len(ids) != len(set(ids)):
            raise ValueError("Invariant failed: duplicate task IDs")

    def _check_deps_defined(self, tasks):
        ids = {t["id"] for t in tasks}
        for t in tasks:
            for d in t.get("deps", []):
                if d not in ids:
                    raise ValueError(f"Invariant failed: dependency '{d}' undefined")

    def _check_payload(self, tasks):
        for t in tasks:
            if not t.get("payload"):
                raise ValueError(f"Invariant failed: empty payload in task '{t['id']}'")

    def _check_acyclic(self, tasks):
        graph = defaultdict(list)
        indegree = defaultdict(int)

        for t in tasks:
            indegree[t["id"]] = 0

        for t in tasks:
            for d in t.get("deps", []):
                graph[d].append(t["id"])
                indegree[t["id"]] += 1

        q = deque([n for n in indegree if indegree[n] == 0])
        visited = 0

        while q:
            node = q.popleft()
            visited += 1
            for neighbor in graph[node]:
                indegree[neighbor] -= 1
                if indegree[neighbor] == 0:
                    q.append(neighbor)

        if visited != len(tasks):
            raise ValueError("Invariant failed: DAG contains cycle")

    def _check_compile(self):
        result = subprocess.run(
            ["cargo", "check"],
            cwd=self.root,
            capture_output=True,
            text=True
        )
        if result.returncode != 0:
            raise ValueError("Invariant failed: compile not clean")


# ============================================================
# ADAPTIVE POLICY LAYER
# ============================================================

class AdaptiveExecutionLoop:

    def __init__(
        self,
        root: str,
        task_budget: int,
        resource_budget: int,
    ):
        self.root = root
        self.agent = bootstrap(
            root=root,
            task_budget=task_budget,
            resource_budget=resource_budget,
        )
        self.invariant_engine = InvariantEngine(root)
        self.diagnostics_engine = DiagnosticsEngine()
        self.revision_engine = RevisionEngine()
        self.state = ProjectState(root)

    # --------------------------------------------------
    # SAT + INVARIANT VALIDATION
    # --------------------------------------------------

    def validate_with_sat(self, proposal: Dict[str, Any]):
        tasks = proposal["tasks"]
        solver = ProposalSATValidator(tasks)

        solver.add_constraint(
            "no_duplicate_ids",
            lambda: len({t["id"] for t in tasks}) == len(tasks)
        )

        solver.add_constraint(
            "deps_defined",
            lambda: all(
                d in {t["id"] for t in tasks}
                for t in tasks
                for d in t.get("deps", [])
            )
        )

        solver.solve()
        print("[SAT Validation Passed]")

    # --------------------------------------------------
    # EXECUTION WITH CHECKPOINTS
    # --------------------------------------------------

    def execute_with_checkpoints(
        self,
        proposal: Dict[str, Any],
        invariants: List[Dict[str, Any]]
    ):
        self.validate_with_sat(proposal)

        self.invariant_engine.verify(invariants, proposal)

        self.agent.propose(proposal)

        while True:
            result = self.agent._controller.advance()
            print("[Checkpoint]", result)

            if result.get("done"):
                break

        # post-run compile verification with structured diagnostics
        import subprocess
        proc = subprocess.run(
            ["cargo", "check"],
            cwd=self.root,
            capture_output=True,
            text=True
        )

        diagnostics = self.diagnostics_engine.parse(proc.stderr)

        if self.diagnostics_engine.has_errors(diagnostics):
            print("[Diagnostics detected errors]")
            revised = self.revision_engine.propose_fix(diagnostics, proposal)
            print("[Revision proposal generated]")
            self.agent.propose(revised)
