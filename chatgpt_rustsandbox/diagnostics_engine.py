import re
from typing import List, Dict

class Diagnostic:
    def __init__(self, level: str, message: str):
        self.level = level
        self.message = message

class DiagnosticsEngine:

    ERROR_PATTERN = re.compile(r"error\[.*?\]: (.*)")

    def parse(self, stderr: str) -> List[Diagnostic]:
        diagnostics = []
        for line in stderr.splitlines():
            m = self.ERROR_PATTERN.search(line)
            if m:
                diagnostics.append(Diagnostic("error", m.group(1)))
        return diagnostics

    def has_errors(self, diagnostics: List[Diagnostic]) -> bool:
        return any(d.level == "error" for d in diagnostics)

