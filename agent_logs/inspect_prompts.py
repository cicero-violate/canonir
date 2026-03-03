#!/usr/bin/env python3
import os
import glob

def read_tail(path, limit=800):
    with open(path) as f:
        content = f.read().strip()
    return content[-limit:]

def main():
    base = os.path.dirname(__file__)

    candidates = sorted(
        glob.glob(os.path.join(base, "tick_40[5-9]")) +
        glob.glob(os.path.join(base, "tick_41*")),
        key=lambda x: int(os.path.basename(x).split("_")[1])
    )

    for d in candidates:
        n = int(os.path.basename(d).split("_")[1])
        print("=" * 60)
        print(f"TICK {n}")

        prompt_path = os.path.join(d, "prompt.txt")
        if os.path.exists(prompt_path):
            print("--- prompt.txt (tail) ---")
            print(read_tail(prompt_path, 800))

        retry_prompt_path = os.path.join(d, "retry_prompt.txt")
        if os.path.exists(retry_prompt_path):
            print("--- retry_prompt.txt (tail) ---")
            print(read_tail(retry_prompt_path, 500))

if __name__ == "__main__":
    main()

