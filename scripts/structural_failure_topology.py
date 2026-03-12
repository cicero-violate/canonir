#!/usr/bin/env python3
import json
import sys
from collections import Counter, defaultdict

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


def top_frame_symbol(ev):
    frames = ev.get('frames') or []
    if not frames or not isinstance(frames[0], dict):
        return '<no_frame>'
    syms = frames[0].get('symbols') or []
    if not syms:
        return '<no_frame>'
    s0 = syms[0]
    return s0.get('symbol') if isinstance(s0, dict) and s0.get('symbol') else '<no_frame>'


def top_frame_file(ev):
    frames = ev.get('frames') or []
    if not frames or not isinstance(frames[0], dict):
        return '<no_file>'
    syms = frames[0].get('symbols') or []
    if not syms:
        return '<no_file>'
    s0 = syms[0]
    return s0.get('file') if isinstance(s0, dict) and s0.get('file') else '<no_file>'


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 else TLOG_DEFAULT
    edge_counts = Counter()
    def_to_symbols = defaultdict(Counter)

    for ev in read_events(path):
        did = ev.get('def_id', '') or '<unknown>'
        sym = top_frame_symbol(ev)
        fil = ev.get('file') or top_frame_file(ev)
        edge_counts[(did, sym)] += 1
        def_to_symbols[did][sym] += 1
        edge_counts[(did, fil)] += 1

    total_edges = sum(sum(c.values()) for c in def_to_symbols.values())
    print(f"total_edges(def_id->top_frame): {total_edges}")
    print("top_def_id_to_symbol_edges:")
    for (did, sym), count in edge_counts.most_common(25):
        if sym.endswith('.rs') or sym == '<no_file>':
            continue
        print(f"- count={count} def_id={did} symbol={sym}")
    print("top_def_id_to_file_edges:")
    for (did, fil), count in edge_counts.most_common(25):
        if not (fil.endswith('.rs') or fil == '<no_file>'):
            continue
        print(f"- count={count} def_id={did} file={fil}")
    print("per_def_id_symbol_spread:")
    for did, c in sorted(def_to_symbols.items(), key=lambda kv: sum(kv[1].values()), reverse=True)[:10]:
        top = ", ".join([f"{sym}({cnt})" for sym, cnt in c.most_common(5)])
        print(f"- def_id={did} top_symbols=[{top}]")
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
