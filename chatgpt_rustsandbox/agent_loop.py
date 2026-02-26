from collections import defaultdict, deque
from typing import Dict, Any, List
from IPython import get_ipython

class Task:
    def __init__(self, task_id: str, deps: List[str], payload: Dict[str, Any]):
        self.id = task_id
        self.deps = deps
        self.payload = payload

class DAGPlanner:
    def __init__(self):
        self.tasks: Dict[str, Task] = {}

    def add_task(self, task: Task):
        self.tasks[task.id] = task

    def topological_sort(self):
        indegree = defaultdict(int)
        graph = defaultdict(list)

        for t in self.tasks.values():
            for d in t.deps:
                graph[d].append(t.id)
                indegree[t.id] += 1

        queue = deque([tid for tid in self.tasks if indegree[tid] == 0])
        order = []

        while queue:
            node = queue.popleft()
            order.append(node)
            for nbr in graph[node]:
                indegree[nbr] -= 1
                if indegree[nbr] == 0:
                    queue.append(nbr)

        if len(order) != len(self.tasks):
            raise ValueError("Cycle detected in DAG")

        return order

class AgentLoop:
    def __init__(self):
        self.planner = DAGPlanner()
        self.enabled = True

    def propose_task(self, task_id: str, deps: List[str], payload: Dict[str, Any]):
        self.planner.add_task(Task(task_id, deps, payload))

    def execute(self):
        order = self.planner.topological_sort()
        for tid in order:
            print(f"Executing {tid}")
        return order

    def install_hook(self):
        ip = get_ipython()

        def post_run_cell(result=None):
            if not self.enabled:
                return
            print("[agent-loop] ready")

        ip.events.register("post_run_cell", post_run_cell)

def bootstrap():
    from unified_agent_system import UnifiedAgent
    agent = UnifiedAgent()
    return agent
