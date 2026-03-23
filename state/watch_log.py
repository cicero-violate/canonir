#!/usr/bin/env python3
import json
import os
import time
from typing import Optional


DEFAULT_DIR = "/workspace/ai_sandbox/canon/state/event_log/event.tlog.d"
DIR = os.environ.get("CANON_WATCH_TLOG_DIR", DEFAULT_DIR)

# ANSI COLORS
RESET = "\033[0m"
COLORS = {
    "ts": "\033[90m",      # gray
    "source": "\033[94m",  # blue
    "kind": "\033[92m",    # green
    "error": "\033[91m",   # red
    "json": "\033[96m",    # cyan
}


def latest_log() -> Optional[str]:
    try:
        files = [f for f in os.listdir(DIR) if f.endswith(".log")]
    except FileNotFoundError:
        return None
    if not files:
        return None
    files.sort()
    return os.path.join(DIR, files[-1])


def colorize(line: str) -> str:
    line = f"{COLORS['json']}{line}{RESET}"
    if '"ts":' in line:
        line = line.replace('"ts":', f"{COLORS['ts']}\"ts\":{RESET}")
    if '"source":' in line:
        line = line.replace('"source":', f"{COLORS['source']}\"source\":{RESET}")
    if '"kind":' in line:
        line = line.replace('"kind":', f"{COLORS['kind']}\"kind\":{RESET}")
    if "error" in line.lower() or "fail" in line.lower():
        line = f"{COLORS['error']}{line}{RESET}"
    return line


def should_skip(obj: dict) -> bool:
    # Requested: filter rustc events from console output.
    return obj.get("source") == "rustc" or obj.get("kind") == "rustc_event"


def process_line(line: str) -> None:
    line = line.strip()
    if not line:
        return
    try:
        obj = json.loads(line)
    except json.JSONDecodeError:
        return
    if not isinstance(obj, dict):
        return
    if should_skip(obj):
        return
    rendered = json.dumps(obj, separators=(",", ":"), ensure_ascii=False)
    print(colorize(rendered), flush=True)


def open_tail(path: str):
    f = open(path, "r", encoding="utf-8", errors="replace")
    f.seek(0, os.SEEK_END)
    return f


def watch_file() -> None:
    def wait_for_log() -> str:
        print(f"waiting for logs in {DIR} ...", flush=True)
        while True:
            current_log = latest_log()
            if current_log:
                return current_log
            time.sleep(0.2)

    current = latest_log() or wait_for_log()

    print(f"watching {DIR} (filtering rustc events)")
    f = open_tail(current)
    inode = os.fstat(f.fileno()).st_ino

    while True:
        new_latest = latest_log()
        if new_latest and new_latest != current:
            f.close()
            current = new_latest
            f = open_tail(current)
            inode = os.fstat(f.fileno()).st_ino
            print(f"\n--- switched to {current} ---\n", flush=True)

        # Detect truncation/recreation of current file.
        try:
            st = os.stat(current)
            if st.st_ino != inode or f.tell() > st.st_size:
                f.close()
                f = open_tail(current)
                inode = os.fstat(f.fileno()).st_ino
        except FileNotFoundError:
            time.sleep(0.2)
            continue

        line = f.readline()
        if not line:
            time.sleep(0.2)
            continue
        process_line(line)


if __name__ == "__main__":
    watch_file()
