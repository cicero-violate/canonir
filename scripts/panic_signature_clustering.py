#!/usr/bin/env python3
import json
import re
import sys
from collections import Counter, defaultdict

TLOG_DEFAULT = "/workspace/ai_sandbox/canon/state/kernel_logs/kernel.tlog"

_num_re = re.compile(r"\\b\\d+\\b")
_hex_re = re.compile(r"0x[0-9a-fA-F]+")
_loc_re = re.compile(r"\\s@\\s.+?:\\d+:\\d+$")


def normalize_message(msg: str) -> str:
    if not msg:
        return ""
    msg = _loc_re.sub("", msg)
    msg = _hex_re.sub("<hex>", msg)
    msg = _num_re.sub("<n>", msg)
    return msg.strip()


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


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 else TLOG_DEFAULT
    clusters = Counter()
    sample_def_ids = defaultdict(list)
    sample_msgs = {}
    sample_top_frame = {}

    total = 0
    for ev in read_events(path):
        total += 1
        msg = ev.get("message", "")
        norm = normalize_message(msg)
        clusters[norm] += 1
        if len(sample_def_ids[norm]) < 3:
            did = ev.get("def_id", "")
            if did:
                sample_def_ids[norm].append(did)
        if norm and norm not in sample_msgs:
            sample_msgs[norm] = msg
        if norm and norm not in sample_top_frame:
            frames = ev.get("frames") or []
            top = None
            if frames and isinstance(frames, list):
                syms = frames[0].get("symbols") if isinstance(frames[0], dict) else None
                if syms and isinstance(syms, list) and syms:
                    top = syms[0].get("symbol") if isinstance(syms[0], dict) else None
            sample_top_frame[norm] = top or "<no_frame>"

    print(f"total_panics: {total}")
    print(f"cluster_count: {len(clusters)}")
    print("top_clusters:")
    for norm, count in clusters.most_common(25):
        msg = sample_msgs.get(norm, norm)
        top = sample_top_frame.get(norm, "<no_frame>")
        defs = ", ".join(sample_def_ids.get(norm, []))
        print(f"- count={count} top_frame={top} defs=[{defs}] msg={msg}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
