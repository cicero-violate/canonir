import os
from typing import Dict

class ProjectState:

    def __init__(self, root: str):
        self.root = root

    def snapshot_files(self) -> Dict[str, str]:
        state = {}
        for dirpath, _, filenames in os.walk(self.root):
            for f in filenames:
                if f.endswith(".rs"):
                    full = os.path.join(dirpath, f)
                    with open(full, "r") as fh:
                        state[full] = fh.read()
        return state

