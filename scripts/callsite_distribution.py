#!/usr/bin/env python3
import json
import sys
from collections import Counter

TLOG_DEFAULT = "/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog"


def read_events(path: str):
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            if obj.get("t") != "PANIC":
                continue
            yield obj


def frame_symbols(ev):
    frames = ev.get("frames") or []
    for frame in frames:
        if not isinstance(frame, dict):
            continue
        syms = frame.get("symbols") or []
        for s in syms:
            if isinstance(s, dict):
                sym = s.get("symbol")
                if sym:
                    yield sym


def top_frame_symbol(ev):
    frames = ev.get("frames") or []
    if not frames or not isinstance(frames[0], dict):
        return None
    syms = frames[0].get("symbols") or []
    if not syms:
        return None
    s0 = syms[0]
    return s0.get("symbol") if isinstance(s0, dict) else None


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 else TLOG_DEFAULT
    top_frame_counts = Counter()
    all_frame_counts = Counter()

    total = 0
    for ev in read_events(path):
        total += 1
        top = top_frame_symbol(ev) or "<no_frame>"
        top_frame_counts[top] += 1
        for sym in frame_symbols(ev):
            all_frame_counts[sym] += 1

    print(f"total_panics: {total}")
    print("top_frame_distribution:")
    for sym, count in top_frame_counts.most_common(25):
        print(f"- count={count} top_frame={sym}")
    print("all_frame_distribution:")
    for sym, count in all_frame_counts.most_common(25):
        print(f"- count={count} frame={sym}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
