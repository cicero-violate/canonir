import json
import os

class PersistentMemory:
    def __init__(self, path=".agent_memory.json"):
        self.path = path
        self.state = self._load()

    def _load(self):
        if os.path.exists(self.path):
            with open(self.path, "r") as f:
                return json.load(f)
        return {"history": []}

    def save(self):
        with open(self.path, "w") as f:
            json.dump(self.state, f, indent=2)

    def record(self, entry):
        self.state["history"].append(entry)
        self.save()

    def get_history(self):
        return self.state["history"]
