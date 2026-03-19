#!/usr/bin/env python3
import re
import time
import os

DIR = "/workspace/ai_sandbox/canon/canon-utils/state/event_log/event.tlog.d"

def latest_log():
    files = [f for f in os.listdir(DIR) if f.endswith(".log")]
    if not files:
        return None
    return os.path.join(DIR, max(files))
# ANSI COLORS
RESET = "\033[0m"
COLORS = {
    "ts": "\033[90m",          # gray
    "source": "\033[94m",      # blue
    "kind": "\033[92m",        # green
    "error": "\033[91m",       # red
    "json": "\033[96m",        # cyan
    "default": "\033[97m",     # white
}

def colorize(line: str) -> str:
    # highlight json blocks
    if "{" in line and "}" in line:
        line = f"{COLORS['json']}{line}{RESET}"

    # key highlights
    line = re.sub(r'("ts":\s*\d+)', lambda m: f"{COLORS['ts']}{m.group(1)}{RESET}", line)
    line = re.sub(r'("source":\s*"[^"]+")', lambda m: f"{COLORS['source']}{m.group(1)}{RESET}", line)
    line = re.sub(r'("kind":\s*"[^"]+")', lambda m: f"{COLORS['kind']}{m.group(1)}{RESET}", line)

    # errors / warnings
    if "error" in line.lower() or "fail" in line.lower():
        line = f"{COLORS['error']}{line}{RESET}"

    return line

def extract_strings(data):
    matches = re.findall(rb"[ -~]{8,}", data)
    for m in matches:
        try:
            line = m.decode("ascii")
            print(colorize(line))
        except:
            pass

def watch_file():
    current = latest_log()
    if not current:
        print("no logs found")
        return

    f = open(current, "rb")
    f.seek(0, os.SEEK_END)

    while True:
        new_latest = latest_log()
        if new_latest and new_latest != current:
            f.close()
            current = new_latest
            f = open(current, "rb")
            f.seek(0, os.SEEK_END)
            print(f"\n--- switched to {current} ---\n")

        pos = f.tell()
        chunk = f.read()

        if not chunk:
            time.sleep(0.2)
            f.seek(pos)
        else:
            extract_strings(chunk)

if __name__ == "__main__":
    watch_file()
