#!/usr/bin/env python3
import os
import glob
import json

def read_file(path, limit=800):
    with open(path) as f:
        return f.read().strip()[:limit]

def main():
    base = os.path.dirname(__file__)

    candidates = sorted(
        glob.glob(os.path.join(base, "tick_3[6-9]*")) +
        glob.glob(os.path.join(base, "tick_4[0-1][0-9]")),
        key=lambda x: int(os.path.basename(x).split("_")[1])
    )

    for d in candidates:
        n = int(os.path.basename(d).split("_")[1])
        print("=" * 60)
        print(f"TICK {n}")

        for fname in ["phase.txt", "act_error.txt", "bash_output.txt"]:
            fp = os.path.join(d, fname)
            if os.path.exists(fp):
                print(f"--- {fname} ---")
                print(read_file(fp, 500))

        for rname in ["response.json", "retry_response.json"]:
            rp = os.path.join(d, rname)
            if os.path.exists(rp):
                try:
                    data = json.load(open(rp))
                    print(f"--- {rname} (keys: {list(data.keys())}) ---")
                    print(json.dumps(data)[:800])
                except Exception as e:
                    print(f"ERROR reading {rname}: {e}")

if __name__ == "__main__":
    main()

