#!/usr/bin/env python3

import re
import json
import time
import os
from collections import defaultdict

P = "/workspace/ai_sandbox/canon/canon-utils/state/event_log/event.tlog.d/00000000000000000000.log"

schema = defaultdict(set)

def classify(v):
    if isinstance(v, bool): return "bool"
    if isinstance(v, int): return "int"
    if isinstance(v, float): return "float"
    if isinstance(v, list): return "list"
    if isinstance(v, dict): return "object"
    if v is None: return "null"
    return "string"

def update_schema(obj, prefix=""):
    for k, v in obj.items():
        key = f"{prefix}.{k}" if prefix else k
        schema[key].add(classify(v))
        if isinstance(v, dict):
            update_schema(v, key)

def extract_json(data):
    matches = re.findall(rb"\{.*?\}", data)
    for m in matches:
        try:
            yield json.loads(m.decode("utf-8"))
        except:
            pass

def watch(path):
    with open(path, "rb") as f:
        f.seek(0, os.SEEK_END)

        while True:
            pos = f.tell()
            chunk = f.read()

            if not chunk:
                time.sleep(0.2)
                f.seek(pos)
                continue

            for obj in extract_json(chunk):
                update_schema(obj)

            os.system("clear")
            print("=== INFERRED SCHEMA ===")
            for k, v in sorted(schema.items()):
                print(f"{k}: {list(v)}")

if __name__ == "__main__":
    watch(P)
