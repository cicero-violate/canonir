#!/usr/bin/env python3
"""
Generate a concise report from kernel.tlog.

Usage:
  ./report_panic_tlog.py [path/to/kernel.tlog]

Outputs:
  - counts for key top-level fields (def_id, mir_variant, lowering_stage, file, span)
  - top message signatures
  - top kernel frames / failure sites (from frames)
"""
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOG_DEFAULT = ROOT / "state" / "kernel_logs" / "kernel.tlog"

TOP_N = 15
EXAMPLE_N = 3


def load_records(path: Path):
    decoder = json.JSONDecoder()
    records = []
    bad_chunks = 0
    with path.open() as f:
        for raw_line in f:
            line = raw_line.strip()
            if not line:
                continue
            idx = 0
            length = len(line)
            while idx < length:
                while idx < length and line[idx] not in "{[":
                    idx += 1
                if idx >= length:
                    break
                try:
                    obj, next_idx = decoder.raw_decode(line, idx)
                    records.append(obj)
                    idx = next_idx
                except json.JSONDecodeError:
                    bad_chunks += 1
                    break
    return records, bad_chunks


def normalize_message(msg: str) -> str:
    if not msg:
        return "<none>"
    out = msg
    out = re.sub(r"def_id=[^\\s,\\)]+", "def_id=<id>", out)
    out = re.sub(r"mir_variant=[^\\s,\\)]+", "mir_variant=<mir>", out)
    out = re.sub(r"lowering_stage=[^\\s,\\)]+", "lowering_stage=<stage>", out)
    out = re.sub(r"path=[^\\s,\\)]+", "path=<path>", out)
    out = re.sub(r"target=[^\\s,\\)]+", "target=<bb>", out)
    out = re.sub(r"lhs=\\\"[^\\\"]+\\\"", "lhs=\"<lhs>\"", out)
    out = re.sub(r"rvalue=\\{[^}]+\\}", "rvalue={<rvalue>}", out)
    out = re.sub(r"rvalue=[^\\s,\\)]+", "rvalue=<rvalue>", out)
    out = re.sub(r"node_id=[(]?[^\s,)]*", "node_id=<id>", out)
    return out


def kernel_frames(panic):
    out = []
    for f in panic.get("frames", []):
        for sym in f.get("symbols", []):
            s = sym.get("symbol")
            if not s:
                continue
            if "canon_kernel" not in s and "canon::" not in s and "canon_kernel::" not in s:
                continue
            if "install_panic_hook" in s or "panic::" in s:
                continue
            out.append(
                {
                    "symbol": s,
                    "file": sym.get("file"),
                    "line": sym.get("line"),
                }
            )
    return out


def print_top(label, counter, top_n=TOP_N):
    print(f"\n{label}:")
    for key, count in counter.most_common(top_n):
        print(f"  [{count}x] {key}")

def print_stage_distribution(stages, total):
    print("\nStage distribution (C_i = events at stage / E):")
    for stage, count in stages.most_common(TOP_N):
        pct = (count / total * 100.0) if total else 0.0
        print(f"  [{count:4d}x] {pct:5.1f}%  {stage}")

def span_to_file_line(span: str) -> str:
    # span format: "path:line:col"
    if not span or span == "<none>":
        return "<none>"
    parts = span.rsplit(":", 2)
    if len(parts) >= 2:
        return f"{parts[0]}:{parts[1]}"
    return span


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else LOG_DEFAULT
    if not path.exists():
        print(f"kernel.tlog not found at {path}")
        return 1

    records, bad_chunks = load_records(path)
    panics = [r for r in records if r.get("t") == "PANIC"]

    total = len(panics)
    print(f"Total panic events: {total}")
    if bad_chunks:
        print(f"WARNING: skipped {bad_chunks} malformed log chunks")

    # Top-level field distributions
    def_ids = Counter(r.get("def_id") or "<none>" for r in panics)
    mir_variants = Counter(r.get("mir_variant") or "<none>" for r in panics)
    stages = Counter(r.get("lowering_stage") or "<none>" for r in panics)
    files = Counter(r.get("file") or "<none>" for r in panics)
    spans = Counter(r.get("span") or "<none>" for r in panics)

    print_top("Top def_ids", def_ids)
    print_top("Top mir_variants", mir_variants)
    print_top("Top lowering_stages", stages)
    print_top("Top files", files)
    print_top("Top spans", spans)
    print_stage_distribution(stages, total)

    # Failure localization: file:line from span
    span_file_lines = Counter(span_to_file_line(s) for s in spans.elements())
    print_top("Top failure locations (file:line from span)", span_file_lines)

    # Message signatures
    messages = Counter(normalize_message(r.get("message", "")) for r in panics)
    print(f"\nUnique panic signatures: {len(messages)}")
    for msg, count in messages.most_common(TOP_N):
        print(f"  [{count}x] {msg[:200]}")

    # Frame-based hotspots
    innermost = Counter()
    hotspots = Counter()
    for p in panics:
        kf = kernel_frames(p)
        if kf:
            innermost[kf[0]["symbol"]] += 1
            file = kf[0].get("file") or "<no-file>"
            line = kf[0].get("line") or "?"
            hotspots[f"{file}:{line}"] += 1

    print_top("Dominant failure frames (innermost canon_kernel, hook filtered)", innermost)
    print_top("Top failure sites (file:line)", hotspots)

    # Example mapping: message signature -> sample def_ids
    signature_examples = defaultdict(list)
    for p in panics:
        sig = normalize_message(p.get("message", ""))
        if len(signature_examples[sig]) < EXAMPLE_N:
            did = p.get("def_id")
            if did:
                signature_examples[sig].append(did)

    print("\nSignature examples (def_id):")
    for sig, count in messages.most_common(TOP_N):
        examples = ", ".join(signature_examples[sig]) or "<none>"
        print(f"  [{count}x] {sig[:160]} | examples: {examples}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
