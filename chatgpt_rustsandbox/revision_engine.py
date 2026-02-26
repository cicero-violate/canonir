import os
import re
from typing import List, Dict, Any

class RevisionEngine:

    UNRESOLVED_IMPORT = re.compile(r"cannot find (?:function|struct|type) `?(\w+)`?")
    MISSING_SEMICOLON = re.compile(r"expected `;`")

    def propose_fix(
        self,
        diagnostics: List[Any],
        original_proposal: Dict[str, Any]
    ) -> Dict[str, Any]:

        repair_tasks = []

        for diag in diagnostics:
            message = diag.message

            # ----------------------------
            # Rule 1: Missing semicolon
            # ----------------------------
            if self.MISSING_SEMICOLON.search(message):
                repair_tasks.append({
                    "id": "repair_semicolon",
                    "deps": [],
                    "payload": {
                        "type": "bash",
                        "command": "echo 'Semantic repair: semicolon required.'"
                    }
                })

            # ----------------------------
            # Rule 2: Unresolved symbol
            # ----------------------------
            m = self.UNRESOLVED_IMPORT.search(message)
            if m:
                symbol = m.group(1)
                repair_tasks.append({
                    "id": f"repair_missing_{symbol}",
                    "deps": [],
                    "payload": {
                        "type": "write_file",
                        "path": "src/lib.rs",
                        "content": f"// Auto-repair inserted stub\npub struct {symbol};"
                    }
                })

        if not repair_tasks:
            # fallback: no-op to avoid infinite loop
            repair_tasks.append({
                "id": "noop_revision",
                "deps": [],
                "payload": {
                    "type": "bash",
                    "command": "echo 'No repair rule matched.'"
                }
            })

        return {"tasks": repair_tasks}

