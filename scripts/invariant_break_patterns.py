#!/usr/bin/env python3
import json
import sys
from collections import Counter, defaultdict

TLOG_DEFAULT = '/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog'

PATTERNS = [
    ('invariant_violation', ['Invariant violation', 'invariant violation']),
    ('canon_invariant', ['canon invariant', 'canon-capture invariant', 'canon structural']),
    ('missing', ['missing', 'not found', 'undefined']),
    ('unresolved', ['unresolved', 'unknown']),
    ('panic', ['panic']),
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
            if obj.get('event') != 'panic':
                continue
            yield obj


def classify_message(msg: str):
    hits = []
    for label, keys in PATTERNS:
        for k in keys:
            if k in msg:
                hits.append(label)
                break
    if not hits:
        hits.append('other')
    return hits


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 else TLOG_DEFAULT
    counts = Counter()
    samples = defaultdict(list)

    for ev in read_events(path):
        msg = ev.get('message', '')
        labels = classify_message(msg)
        for label in labels:
            counts[label] += 1
            if len(samples[label]) < 5 and msg:
                samples[label].append(msg)

    total = sum(counts.values())
    print(f"total_panics: {total}")
    print("pattern_counts:")
    for label, count in counts.most_common():
        msgs = " | ".join(samples.get(label, []))
        print(f"- label={label} count={count} samples={msgs}")
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
