#!/usr/bin/env python3
import argparse
import json
from collections import Counter, defaultdict
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

def load_degenerate(report_path: Path):
    pairs = []
    with report_path.open() as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            d = json.loads(line)
            if d.get("type") != "skipped" or d.get("reason") != "degenerate":
                continue
            for item in d.get("pairs", []):
                old = item.get("old_name") or item.get("symbol_id") or ""
                new = item.get("new_name") or ""
                if old or new:
                    pairs.append((old, new))
    return pairs

def summarize_failures(attempts):
    reasons = Counter()
    error_codes_introduced = Counter()
    error_codes_after = Counter()
    error_codes_delta = Counter()
    compile_invoked = Counter()
    error_totals = Counter()
    for d in attempts:
        reason = d.get("decision", {}).get("reason")
        if not reason:
            continue
        reasons[reason] += 1
        compile_block = d.get("compile", {})
        compile_invoked[compile_block.get("invoked")] += 1
        error_totals[compile_block.get("error_total_after")] += 1

        for code, count in (compile_block.get("error_types_after") or {}).items():
            error_codes_after[code] += int(count)
            if reason == "introduced_errors":
                error_codes_introduced[code] += int(count)

        if reason == "introduced_errors":
            for code, count in (d.get("delta", {}).get("delta_error_types") or {}).items():
                if int(count) > 0:
                    error_codes_delta[code] += int(count)
    return reasons, error_codes_introduced, error_codes_after, error_codes_delta, compile_invoked, error_totals

def sample_failures(attempts, limit):
    samples = defaultdict(list)
    for d in attempts:
        codes = list((d.get("compile", {}) or {}).get("error_types_after", {}).keys())
        if not codes:
            continue
        for code in codes:
            if len(samples[code]) < limit:
                samples[code].append(d)
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
    base_dir = Path("/workspace/ai_sandbox/canon/canon-utils/canon-editor")
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
        help="Path to rename_report*.jsonl (defaults to newest in canon-utils/canon-editor)",
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
    parser.add_argument(
        "--example-limit",
        type=int,
        default=2,
        help="Max example attempts per error code",
    )
    args = parser.parse_args()

    report_arg = args.report_flag or args.report
    report_path = Path(report_arg) if report_arg else default_report_path()
    if not report_path.exists():
        raise SystemExit(f"report file not found: {report_path}")

    attempts = list(load_attempts(report_path))
    degenerate_pairs = load_degenerate(report_path)
    (
        reasons,
        error_codes_introduced,
        error_codes_after,
        error_codes_delta,
        compile_invoked,
        error_totals,
    ) = summarize_failures(iter(attempts))

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
    print("Error codes (introduced_errors only):")
    for code, count in sorted(error_codes_introduced.items(), key=lambda x: -x[1]):
        print(f"  {code}: {count}")
    print()
    print("Error codes (all compile attempts):")
    for code, count in sorted(error_codes_after.items(), key=lambda x: -x[1]):
        print(f"  {code}: {count}")
    if error_codes_delta:
        print()
        print("Error codes (delta_error_types, introduced only):")
        for code, count in sorted(error_codes_delta.items(), key=lambda x: -x[1]):
            print(f"  {code}: {count}")
    if degenerate_pairs:
        print()
        print(f"Degenerate renames: {len(degenerate_pairs)}")
        for old, new in degenerate_pairs:
            print(f"  {old} -> {new}")

    if args.no_samples:
        return

    print("Examples by error code:")
    samples = sample_failures(iter(attempts), args.example_limit)
    printed_msg_hint = False
    for code, entries in samples.items():
        print(f"=== {code} ===")
        for d in entries:
            tr = d.get("transform", {})
            compile_block = d.get("compile", {})
            verify = tr.get("verification", {})
            delta = d.get("delta", {})
            print(f"  attempt_id={d.get('attempt_id')} reason={d.get('decision', {}).get('reason')}")
            print(f"  {tr.get('symbol_id')} -> {tr.get('new_name')}")
            print(
                f"  touched_files={len(tr.get('touched_files') or [])} "
                f"compile.invoked={compile_block.get('invoked')} "
                f"error_total_after={compile_block.get('error_total_after')} "
                f"delta_total={delta.get('delta_total')}"
            )
            if verify:
                print(
                    f"  verify.method={verify.get('method')} "
                    f"pairs_checked={verify.get('pairs_checked')} "
                    f"pairs_changed={verify.get('pairs_changed')}"
                )
            msgs = d.get("compile", {}).get("messages", [])
            if msgs:
                for m in msgs[:2]:
                    print(
                        f"  [{m.get('level')}] {m.get('message')} @ {m.get('file')}:{m.get('line')}"
                    )
            elif not printed_msg_hint:
                print("  (no compiler messages captured in report)")
                printed_msg_hint = True
        print()

if __name__ == "__main__":
    main()
