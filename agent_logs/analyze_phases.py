#!/usr/bin/env python3
import os
import glob
from collections import Counter

def main():
    base = os.path.dirname(__file__)
    tick_dirs = sorted(
        glob.glob(os.path.join(base, "tick_*")),
        key=lambda x: int(os.path.basename(x).split("_")[1])
    )

    ticks = []
    for d in tick_dirs:
        n = int(os.path.basename(d).split("_")[1])
        info = {"tick": n}

        phase_path = os.path.join(d, "phase.txt")
        if os.path.exists(phase_path):
            with open(phase_path) as f:
                info["phase"] = f.read().strip()

        info["act_error"] = os.path.exists(os.path.join(d, "act_error.txt"))
        info["act_retry_ok"] = os.path.exists(os.path.join(d, "act_retry_ok.txt"))
        info["has_bash"] = os.path.exists(os.path.join(d, "bash_output.txt"))
        info["has_retry"] = os.path.exists(os.path.join(d, "retry_response.json"))

        bo = os.path.join(d, "bash_output.txt")
        if os.path.exists(bo):
            with open(bo) as f:
                lines = f.readlines()
            info["bash_tail"] = "".join(lines[-5:]).strip()

        ticks.append(info)

    phases = Counter(t.get("phase", "?") for t in ticks)

    print("=== PHASE DISTRIBUTION ===")
    for ph, cnt in phases.most_common():
        print(f"{ph}: {cnt}")

    print("\n=== LAST 20 TICKS ===")
    for t in ticks[-20:]:
        print(
            f"tick_{t['tick']:03d} "
            f"phase={t.get('phase','?')} "
            f"act_error={t['act_error']} "
            f"retry={t['has_retry']} "
            f"bash={t['has_bash']}"
        )
        if "bash_tail" in t and t["act_error"]:
            print("  bash_tail:")
            print(t["bash_tail"][:300])

if __name__ == "__main__":
    main()

