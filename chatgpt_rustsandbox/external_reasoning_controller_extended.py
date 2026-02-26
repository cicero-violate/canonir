import os
import subprocess
from collections import defaultdict, deque
from typing import Dict, Any

class BudgetManager:
    def __init__(self, task_budget=100, resource_budget=5000):
        self.task_budget = task_budget
        self.resource_budget = resource_budget
        self.tasks_used = 0
        self.resources_used = 0

    def charge(self, cost: int):
        self.tasks_used += 1
        self.resources_used += cost

    def verify(self):
        if self.tasks_used > self.task_budget:
            return False, "Exceeded task budget"
        if self.resources_used > self.resource_budget:
            return False, "Exceeded resource budget"
        return True, "OK"

    def snapshot(self):
        return {
            "tasks_used": self.tasks_used,
            "task_budget": self.task_budget,
            "resources_used": self.resources_used,
            "resource_budget": self.resource_budget
        }

class ResumableDAG:
    def __init__(self):
        self.tasks = {}
        self.order = []
        self.index = 0

    def add_task(self, task_id, deps, payload):
        self.tasks[task_id] = {"deps": deps, "payload": payload}

    def build(self):
        indegree = defaultdict(int)
        graph = defaultdict(list)

        for tid, t in self.tasks.items():
            for d in t["deps"]:
                graph[d].append(tid)
                indegree[tid] += 1

        queue = deque([tid for tid in self.tasks if indegree[tid] == 0])
        order = []

        while queue:
            n = queue.popleft()
            order.append(n)
            for nbr in graph[n]:
                indegree[nbr] -= 1
                if indegree[nbr] == 0:
                    queue.append(nbr)

        if len(order) != len(self.tasks):
            raise RuntimeError("Cycle detected")

        self.order = order

    def next(self):
        if self.index >= len(self.order):
            return None
        t = self.order[self.index]
        self.index += 1
        return t

class ExternalReasoningController:

    def __init__(self, root="workspace", task_budget=100, resource_budget=5000):
        self.root = root
        self.engine = ResumableDAG()
        self.budget = BudgetManager(task_budget, resource_budget)
        self.initialized = False

    def ingest_proposal(self, proposal: Dict[str, Any]):
        for t in proposal.get("tasks", []):
            self.engine.add_task(t["id"], t.get("deps", []), t.get("payload", {}))
        self.engine.build()
        self.initialized = True

    def _write_file(self, path, content):
        full = os.path.join(self.root, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w") as f:
            f.write(content)

    def _run_bash(self, command: str):
        result = subprocess.run(command, shell=True, cwd=self.root)
        if result.returncode != 0:
            raise RuntimeError("Bash failed")

    def _compile_check(self):
        if not os.path.exists(os.path.join(self.root, "Cargo.toml")):
            return True
        result = subprocess.run(["cargo", "check"], cwd=self.root)
        return result.returncode == 0

    def advance(self):
        if not self.initialized:
            raise RuntimeError("Not initialized")

        task_id = self.engine.next()
        if task_id is None:
            return {"done": True, "budget": self.budget.snapshot()}

        payload = self.engine.tasks[task_id]["payload"]
        cost = len(str(payload))
        self.budget.charge(cost)

        ok, msg = self.budget.verify()
        if not ok:
            raise RuntimeError(msg)

        if payload.get("type") == "cargo_init":
            os.makedirs(self.root, exist_ok=True)
            subprocess.run(["cargo", "init", "--lib", self.root])

        elif payload.get("type") == "write_file":
            self._write_file(payload["path"], payload["content"])

        elif payload.get("type") == "bash":
            self._run_bash(payload["command"])

        if not self._compile_check():
            raise RuntimeError("Compile failed")

        return {
            "done": False,
            "executed_task": task_id,
            "remaining": len(self.engine.order) - self.engine.index,
            "budget": self.budget.snapshot()
        }
