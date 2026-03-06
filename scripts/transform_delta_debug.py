#!/usr/bin/env python3
"""
Incremental transform delta-debug runner.

Math model:
  S_i = T_i(S_{i-1})
  E_i = C(S_i)
  ΔE_i = E_i - E_{i-1}
  if ΔE_i > 0 and rollback enabled => rollback(T_i)
"""

from __future__ import annotations

import argparse
import json
import re
import shlex
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple


ERROR_CODE_RE = re.compile(r"\bE\d{4}\b")


@dataclass
class CompileSummary:
    total_errors: int
    by_code: Dict[str, int]
    raw_errors: List[Tuple[str, str]]


def run_shell(cmd: str, cwd: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        cmd,
        cwd=str(cwd),
        shell=True,
        text=True,
        capture_output=True,
    )


def parse_compile_output(stdout: str, stderr: str) -> CompileSummary:
    by_code: Dict[str, int] = {}
    raw_errors: List[Tuple[str, str]] = []
    total = 0

    # First pass: cargo --message-format=json style output.
    for line in stdout.splitlines() + stderr.splitlines():
        line = line.strip()
        if not line:
            continue
        if line.startswith("{") and "\"reason\"" in line:
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                obj = None
            if not isinstance(obj, dict):
                continue
            if obj.get("reason") != "compiler-message":
                continue
            msg = obj.get("message", {})
            if not isinstance(msg, dict):
                continue
            if msg.get("level") != "error":
                continue
            code_obj = msg.get("code")
            code = "UNKNOWN"
            if isinstance(code_obj, dict) and isinstance(code_obj.get("code"), str):
                code = code_obj["code"]
            rendered = msg.get("rendered", "")
            total += 1
            by_code[code] = by_code.get(code, 0) + 1
            raw_errors.append((code, rendered if isinstance(rendered, str) else ""))

    # Fallback: plain text parse for rustc-style lines:
    #   error[E0308]: ...
    #   error: ...
    if total == 0:
        for line in (stdout + "\n" + stderr).splitlines():
            s = line.strip()
            if not (s.startswith("error[") or s.startswith("error:")):
                continue
            m = ERROR_CODE_RE.search(s)
            code = m.group(0) if m else "UNKNOWN"
            total += 1
            by_code[code] = by_code.get(code, 0) + 1
            raw_errors.append((code, s))

    return CompileSummary(total_errors=total, by_code=by_code, raw_errors=raw_errors)


def delta_counts(prev: Dict[str, int], cur: Dict[str, int]) -> Dict[str, int]:
    keys = set(prev) | set(cur)
    out: Dict[str, int] = {}
    for k in sorted(keys):
        d = cur.get(k, 0) - prev.get(k, 0)
        if d != 0:
            out[k] = d
    return out


def load_plan(path: Path) -> dict:
    data = json.loads(path.read_text())
    if not isinstance(data, dict):
        raise ValueError("plan json must be an object")
    if "compile_cmd" not in data or "steps" not in data:
        raise ValueError("plan json requires `compile_cmd` and `steps`")
    if not isinstance(data["steps"], list):
        raise ValueError("plan.steps must be a list")
    return data


def main() -> int:
    ap = argparse.ArgumentParser(description="Incremental transform delta debugger")
    ap.add_argument("--plan", required=True, help="Path to JSON plan file")
    ap.add_argument("--workdir", required=True, help="Directory where commands run")
    ap.add_argument("--output", required=True, help="Output report JSON path")
    ap.add_argument(
        "--rollback-on-regression",
        action="store_true",
        help="Run rollback_cmd for a step when total errors increase",
    )
    args = ap.parse_args()

    plan_path = Path(args.plan).resolve()
    workdir = Path(args.workdir).resolve()
    output_path = Path(args.output).resolve()

    plan = load_plan(plan_path)
    compile_cmd = plan["compile_cmd"]
    steps = plan["steps"]

    started = time.time()

    baseline_proc = run_shell(compile_cmd, workdir)
    baseline = parse_compile_output(baseline_proc.stdout, baseline_proc.stderr)

    report = {
        "plan": str(plan_path),
        "workdir": str(workdir),
        "compile_cmd": compile_cmd,
        "baseline": {
            "exit_code": baseline_proc.returncode,
            "total_errors": baseline.total_errors,
            "by_code": baseline.by_code,
        },
        "steps": [],
    }

    prev = baseline

    for idx, raw_step in enumerate(steps, start=1):
        if not isinstance(raw_step, dict):
            raise ValueError(f"step {idx} must be an object")
        name = str(raw_step.get("name", f"step_{idx}"))
        cmd = raw_step.get("cmd")
        rollback_cmd = raw_step.get("rollback_cmd")
        if not isinstance(cmd, str) or not cmd.strip():
            raise ValueError(f"step {idx} is missing non-empty `cmd`")

        apply_proc = run_shell(cmd, workdir)
        compile_proc = run_shell(compile_cmd, workdir)
        cur = parse_compile_output(compile_proc.stdout, compile_proc.stderr)

        deltas = delta_counts(prev.by_code, cur.by_code)
        total_delta = cur.total_errors - prev.total_errors
        regressed = total_delta > 0

        rolled_back = False
        rollback_exit: Optional[int] = None
        if regressed and args.rollback_on_regression and isinstance(rollback_cmd, str) and rollback_cmd.strip():
            rb = run_shell(rollback_cmd, workdir)
            rollback_exit = rb.returncode
            rolled_back = rb.returncode == 0
            if rolled_back:
                compile_after_rb = run_shell(compile_cmd, workdir)
                cur = parse_compile_output(compile_after_rb.stdout, compile_after_rb.stderr)
                deltas = delta_counts(prev.by_code, cur.by_code)
                total_delta = cur.total_errors - prev.total_errors
                regressed = total_delta > 0

        report["steps"].append(
            {
                "index": idx,
                "name": name,
                "transform_cmd": cmd,
                "transform_exit_code": apply_proc.returncode,
                "compile_exit_code": compile_proc.returncode,
                "errors_total": cur.total_errors,
                "errors_by_code": cur.by_code,
                "delta_total": total_delta,
                "delta_by_code": deltas,
                "regressed": regressed,
                "rolled_back": rolled_back,
                "rollback_exit_code": rollback_exit,
            }
        )

        prev = cur

    report["elapsed_sec"] = round(time.time() - started, 3)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(report, indent=2, sort_keys=True))

    # concise human-readable summary
    print(f"baseline errors: {report['baseline']['total_errors']}")
    for step in report["steps"]:
        print(
            f"{step['index']:02d} {step['name']}: "
            f"errors={step['errors_total']} Δ={step['delta_total']} regressed={step['regressed']}"
        )
    print(f"report: {output_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
