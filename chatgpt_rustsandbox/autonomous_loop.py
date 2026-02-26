
from state_snapshot import ProjectState
from persistent_memory import PersistentMemory
from llm_reflector import LLMReflector
from planner_extensions import HeuristicPlanner, ConstraintPlanner, SpeculativeExecutor


class AutonomousLoop:

    def __init__(self, controller, diagnostics_engine, revision_engine, root="."):
        self.controller = controller
        self.diagnostics_engine = diagnostics_engine
        self.revision_engine = revision_engine
        self.snapshot_engine = ProjectState(root)
        self.memory = PersistentMemory()
        self.reflector = LLMReflector()
        self.heuristic = HeuristicPlanner()
        self.constraint = ConstraintPlanner()
        self.speculative = SpeculativeExecutor()
        self.goal_verified = False

    def _embed_state(self, result):
        snapshot = self.snapshot_engine.snapshot_files()
        return {
            "last_task": result.get("executed_task"),
            "snapshot_size": len(snapshot),
            "budget": result.get("budget")
        }

    def _verify_goal(self, result):
        return result.get("remaining", 0) == 0

    def run_cycle(self, proposal):
        if not self.constraint.validate(proposal):
            return {"status": "constraint_failed"}

        score = self.heuristic.score(proposal)

        branches = self.speculative.branch(proposal)

        for branch in branches:
            while True:
                result = self.controller.advance()
                if result.get("done"):
                    break

                structured_state = self._embed_state(result)
                reflection = self.reflector.reflect(structured_state, self.memory.get_history())

                self.memory.record({
                    "state": structured_state,
                    "reflection": reflection,
                    "score": score
                })

                if self._verify_goal(result):
                    self.goal_verified = True

        return {
            "status": "completed",
            "goal_verified": self.goal_verified
        }
