#!/usr/bin/env python3

import re
import time
import os

P = "/workspace/ai_sandbox/canon/canon-utils/state/event_log/event.tlog.d/00000000000000000000.log"

def extract_strings(data):
    matches = re.findall(rb"[ -~]{8,}", data)
    for m in matches:
        try:
            print(m.decode("ascii"))
        except:
            pass

def watch_file(path):
    with open(path, "rb") as f:
        f.seek(0, os.SEEK_END)  # start at end (like tail -f)

        while True:
            pos = f.tell()
            chunk = f.read()

            if not chunk:
                time.sleep(0.2)
                f.seek(pos)
            else:
                extract_strings(chunk)

if __name__ == "__main__":
    watch_file(P)
