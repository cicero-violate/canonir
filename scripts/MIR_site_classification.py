#!/usr/bin/env python3
import json
import sys
from collections import Counter, defaultdict

TLOG_DEFAULT = '/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog'

KEYWORDS = [
    ('mir', ['::mir', ' MIR ', ' mir::', 'mir_']),
    ('hir', ['::hir', ' HIR ', ' hir::']),
    ('typeck', ['typeck', 'type check', 'typeck::']),
    ('borrowck', ['borrowck', 'borrow check']),
    ('canon', ['canon', 'canon_']),
]


def read_events(path: str):
    with open(path, 'r', encoding='utf-8') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            if obj.get('t') != 'PANIC':
                continue
            yield obj


def collect_text(ev):
    parts = []
    msg = ev.get('message', '')
    if msg:
        parts.append(msg)
    frames = ev.get('frames') or []
    for frame in frames:
        if not isinstance(frame, dict):
            continue
        for s in frame.get('symbols') or []:
            if isinstance(s, dict):
                sym = s.get('symbol')
                if sym:
                    parts.append(sym)
    return " ".join(parts)


def classify(ev):
    text = collect_text(ev)
    for label, keys in KEYWORDS:
        for k in keys:
            if k in text:
                return label
    return 'other'


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 else TLOG_DEFAULT
    counts = Counter()
    samples = defaultdict(list)

    for ev in read_events(path):
        label = classify(ev)
        counts[label] += 1
        if len(samples[label]) < 5:
            did = ev.get('def_id', '')
            if did:
                samples[label].append(did)

    total = sum(counts.values())
    print(f"total_panics: {total}")
    print("classification:")
    for label, count in counts.most_common():
        defs = ", ".join(samples.get(label, []))
        print(f"- label={label} count={count} sample_defs=[{defs}]")
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
