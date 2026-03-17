#!/usr/bin/env python3
import json, time, os
from collections import defaultdict, Counter

P = "/workspace/ai_sandbox/canon/canon-utils/state/event_log/event.tlog.d/00000000000000000000.log"

schema = defaultdict(lambda: {
    "types": set(),
    "present": 0,
    "examples": set(),
    "array_types": set(),
})
total_objects = 0

def classify(v):
    if isinstance(v, bool): return "bool"
    if isinstance(v, int) and not isinstance(v, bool): return "int"
    if isinstance(v, float): return "float"
    if isinstance(v, str): return "string"
    if v is None: return "null"
    if isinstance(v, list): return "array"
    if isinstance(v, dict): return "object"
    return type(v).__name__

def add_example(entry, v):
    if isinstance(v, (str, int, float, bool)) or v is None:
        if len(entry["examples"]) < 5:
            entry["examples"].add(repr(v))

def update_schema(v, path="$"):
    entry = schema[path]
    t = classify(v)
    entry["types"].add(t)
    entry["present"] += 1
    add_example(entry, v)

    if isinstance(v, dict):
        for k, subv in v.items():
            child = f"{path}.{k}"
            update_schema(subv, child)

    elif isinstance(v, list):
        for i, item in enumerate(v):
            it = classify(item)
            entry["array_types"].add(it)
            update_schema(item, f"{path}[]")

def extract_json_objects_from_buffer(buf):
    out = []
    start = None
    depth = 0
    in_string = False
    escape = False

    i = 0
    while i < len(buf):
        b = buf[i]
        ch = chr(b)

        if start is None:
            if ch == "{":
                start = i
                depth = 1
                in_string = False
                escape = False
            i += 1
            continue

        if in_string:
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif ch == '"':
                in_string = False
        else:
            if ch == '"':
                in_string = True
            elif ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    raw = buf[start:i+1]
                    try:
                        out.append(json.loads(raw.decode("utf-8")))
                    except Exception:
                        pass
                    start = None
        i += 1

    remainder = buf[start:] if start is not None else b""
    return out, remainder

def render():
    os.system("clear")
    print("=== INFERRED SCHEMA ===")
    print(f"objects_seen: {total_objects}\n")
    for path in sorted(schema):
        entry = schema[path]
        types = sorted(entry["types"])
        required = (entry["present"] == total_objects) if total_objects else False
        line = f"{path}: types={types}, required={required}, seen={entry['present']}/{total_objects}"
        if entry["array_types"]:
            line += f", elem_types={sorted(entry['array_types'])}"
        if entry["examples"]:
            line += f", examples={sorted(entry['examples'])}"
        print(line)

def watch(path):
    global total_objects
    buf = b""
    with open(path, "rb") as f:
        f.seek(0, os.SEEK_END)
        while True:
            pos = f.tell()
            chunk = f.read()
            if not chunk:
                time.sleep(0.2)
                f.seek(pos)
                continue

            buf += chunk
            objs, buf = extract_json_objects_from_buffer(buf)
            for obj in objs:
                total_objects += 1
                update_schema(obj, "$")
            render()

if __name__ == "__main__":
    watch(P)
