#!/usr/bin/env python3
import re
import time
import os

DIR = "/workspace/ai_sandbox/canon/canon-utils/state/event_log/event.tlog.d"

# ANSI COLORS
RESET = "\033[0m"
COLORS = {
    "ts":    "\033[90m",   # gray
    "source": "\033[94m",  # blue
    "kind":   "\033[92m",  # green
    "error":  "\033[91m",  # red
    "json":   "\033[96m",  # cyan
    "default": "\033[97m", # white
}

def colorize(s: str) -> str:
    line = s
    # Highlight complete JSON-looking lines
    if "{" in line and "}" in line:
        line = f"{COLORS['json']}{line}{RESET}"

    # Key highlights
    line = re.sub(r'("ts":\s*\d+)',     lambda m: f"{COLORS['ts']}{m.group(1)}{RESET}",    line)
    line = re.sub(r'("source":\s*"[^"]+")', lambda m: f"{COLORS['source']}{m.group(1)}{RESET}", line)
    line = re.sub(r'("kind":\s*"[^"]+")',   lambda m: f"{COLORS['kind']}{m.group(1)}{RESET}",   line)

    # Errors / warnings
    if "error" in line.lower() or "fail" in line.lower():
        line = f"{COLORS['error']}{line}{RESET}"

    return line


def latest_log_path() -> str | None:
    files = [f for f in os.listdir(DIR) if f.endswith(".log")]
    if not files:
        return None
    return os.path.join(DIR, max(files))


def print_preview(s: str, preview_len: int = 30) -> None:
    if len(s) < 8:
        return
    preview = s[:preview_len].rstrip()
    print(colorize(preview + ("…" if len(s) > preview_len else "")))


def watch_file():
    current_file = None
    f = None
    seen_previews = set()

    while True:
        new_file = latest_log_path()
        if not new_file:
            print("No logs found in directory.")
            time.sleep(2)
            continue

        if new_file != current_file:
            if f:
                f.close()
            current_file = new_file
            try:
                f = open(current_file, "rb")
                f.seek(0, os.SEEK_END)
                print(f"\n--- switched to {current_file} ---\n")
            except Exception as e:
                print(f"Cannot open {current_file}: {e}")
                time.sleep(2)
                continue

        pos = f.tell()
        chunk = f.read(4096)           # reasonable chunk size

        if not chunk:
            time.sleep(0.2)
            continue

        text = chunk.decode("ascii", errors="ignore")

        buffer = ""
        depth = 0

        for ch in text:
            buffer += ch

            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0 and buffer.strip():
                    preview_key = buffer[:30].strip()
                    if preview_key not in seen_previews:
                        seen_previews.add(preview_key)
                        print_preview(buffer)
                    buffer = ""

        # If we ended mid-object, keep the buffer for next read
        if depth > 0:
            # Very rare, but helps continuity across chunks
            pass
        else:
            buffer = ""


if __name__ == "__main__":
    try:
        watch_file()
    except KeyboardInterrupt:
        print("\nStopped by user.")
    except Exception as e:
        print(f"Watcher crashed: {e}")
