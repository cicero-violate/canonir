#!/usr/bin/env python3
import json
import sys
from collections import Counter

TLOG_DEFAULT = '/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog'


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


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 else TLOG_DEFAULT
    counts = Counter()
    for ev in read_events(path):
        did = ev.get('def_id', '') or '<unknown>'
        counts[did] += 1

    total = sum(counts.values())
    print(f"total_panics: {total}")
    print(f"unique_def_ids: {len(counts)}")
    print("top_def_ids:")
    for did, count in counts.most_common(25):
        print(f"- count={count} def_id={did}")
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
