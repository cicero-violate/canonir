
class HeuristicPlanner:
    '''
    Scores task plans based on heuristic complexity.
    '''

    def score(self, proposal):
        score = 0
        for task in proposal.get("tasks", []):
            score += len(str(task.get("payload", {})))
        return score


class ConstraintPlanner:
    '''
    Validates proposal against simple constraints.
    '''

    def validate(self, proposal):
        ids = set()
        for task in proposal.get("tasks", []):
            if task["id"] in ids:
                return False
            ids.add(task["id"])
        return True


class SpeculativeExecutor:
    '''
    Multi-branch speculative execution simulation.
    '''

    def branch(self, proposal):
        # Simulate 2 speculative branches
        return [proposal.copy(), proposal.copy()]
