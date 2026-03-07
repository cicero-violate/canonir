#!/usr/bin/env python3
import argparse
import json
from collections import Counter
from pathlib import Path

def load_attempts(report_path: Path):
    with report_path.open() as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            d = json.loads(line)
            if d.get("type") != "attempt":
                continue
            yield d

def summarize_failures(attempts):
    reasons = Counter()
    error_codes = Counter()
    compile_invoked = Counter()
    error_totals = Counter()
    for d in attempts:
        reason = d.get("decision", {}).get("reason")
        if not reason:
            continue
        reasons[reason] += 1
        compile_invoked[d.get("compile", {}).get("invoked")] += 1
        error_totals[d.get("compile", {}).get("error_total_after")] += 1
        if reason == "introduced_errors":
            for code in d.get("compile", {}).get("error_types_after", {}):
                error_codes[code] += 1
    return reasons, error_codes, compile_invoked, error_totals

def sample_failures(attempts):
    samples = {}
    for d in attempts:
        if d.get("decision", {}).get("reason") != "introduced_errors":
            continue
        codes = list(d.get("compile", {}).get("error_types_after", {}).keys())
        code = codes[0] if codes else "unknown"
        if code not in samples:
            samples[code] = d
    return samples

def print_attempt_summaries(attempts):
    for d in attempts:
        if d.get("type") != "attempt":
            continue
        tr = d.get("transform", {})
        compile_block = d.get("compile", {})
        verify = tr.get("verification", {})
        reason = d.get("decision", {}).get("reason")
        symbol_id = tr.get("symbol_id")
        new_name = tr.get("new_name")
        print(f"Attempt {d.get('attempt_id')}: {symbol_id} -> {new_name}")
        print(f"  reason={reason} rename_applied={tr.get('rename_applied')} touched_files={len(tr.get('touched_files') or [])}")
        print(f"  compile.invoked={compile_block.get('invoked')} error_total_after={compile_block.get('error_total_after')}")
        if verify:
            print(f"  verify.method={verify.get('method')} pairs_checked={verify.get('pairs_checked')} pairs_changed={verify.get('pairs_changed')}")
        print()

def default_report_path() -> Path:
    base_dir = Path("/workspace/ai_sandbox/canon/canon-utils/rename")
    candidates = sorted(
        base_dir.glob("rename_report_*.jsonl"),
        key=lambda p: p.stat().st_mtime,
        reverse=True,
    )
    if candidates:
        return candidates[0]
    return base_dir / "rename_report.jsonl"

def main():
    parser = argparse.ArgumentParser(
        description="Summarize rename_report.jsonl failures and sample errors"
    )
    parser.add_argument(
        "report",
        nargs="?",
        default=None,
        help="Path to rename_report*.jsonl (defaults to newest in canon-utils/rename)",
    )
    parser.add_argument(
        "--report",
        dest="report_flag",
        default=None,
        help="Path to rename_report*.jsonl (overrides positional)",
    )
    parser.add_argument(
        "--no-samples",
        action="store_true",
        help="Skip printing sample failures",
    )
    args = parser.parse_args()

    report_arg = args.report_flag or args.report
    report_path = Path(report_arg) if report_arg else default_report_path()
    if not report_path.exists():
        raise SystemExit(f"report file not found: {report_path}")

    attempts = list(load_attempts(report_path))
    reasons, error_codes, compile_invoked, error_totals = summarize_failures(iter(attempts))

    print("Failure reasons:")
    for reason, count in sorted(reasons.items(), key=lambda x: -x[1]):
        print(f"  {reason}: {count}")
    print()
    print("Compile invoked:")
    for k, v in sorted(compile_invoked.items(), key=lambda x: -x[1]):
        print(f"  {k}: {v}")
    print()
    print("Error totals after:")
    for k, v in sorted(error_totals.items(), key=lambda x: -x[1]):
        print(f"  {k}: {v}")
    print()
    print("Error codes in failing renames:")
    for code, count in sorted(error_codes.items(), key=lambda x: -x[1]):
        print(f"  {code}: {count}")

    print()
    print("Attempt details:")
    print_attempt_summaries(iter(attempts))

    if args.no_samples:
        return

    print("Samples (introduced_errors only):")
    samples = sample_failures(iter(attempts))
    for code, d in samples.items():
        print(f"=== {code} ===")
        tr = d.get("transform", {})
        compile_block = d.get("compile", {})
        verify = tr.get("verification", {})
        print(f"  {tr.get('symbol_id')} -> {tr.get('new_name')}")
        print(f"  compile.invoked={compile_block.get('invoked')} error_total_after={compile_block.get('error_total_after')}")
        if verify:
            print(f"  verify.method={verify.get('method')} pairs_checked={verify.get('pairs_checked')} pairs_changed={verify.get('pairs_changed')}")
        msgs = d.get("compile", {}).get("messages", [])
        for m in msgs[:2]:
            print(
                f"  [{m.get('level')}] {m.get('message')} @ {m.get('file')}:{m.get('line')}"
            )
        print()

if __name__ == "__main__":
    main()
