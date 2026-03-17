#!/usr/bin/env python3
import json
import os
import time
from collections import defaultdict

P = "/workspace/ai_sandbox/canon/canon-utils/state/event_log/event.tlog.d/00000000000000000000.log"

schema = defaultdict(lambda: {
    "types": set(),
    "present": 0,
    "examples": set(),
    "array_types": set(),
    "children": set(),
    "parents": set(),
})

total_objects = 0


def classify(v):
    if isinstance(v, bool):
        return "bool"
    if isinstance(v, int) and not isinstance(v, bool):
        return "int"
    if isinstance(v, float):
        return "float"
    if isinstance(v, str):
        return "string"
    if v is None:
        return "null"
    if isinstance(v, list):
        return "array"
    if isinstance(v, dict):
        return "object"
    return type(v).__name__


def add_example(entry, v):
    if isinstance(v, (str, int, float, bool)) or v is None:
        if len(entry["examples"]) < 5:
            entry["examples"].add(repr(v))


def child_path(path, key):
    if path == "$":
        return f"$.{key}"
    return f"{path}.{key}"


def link(parent, child):
    schema[parent]["children"].add(child)
    schema[child]["parents"].add(parent)


def update_schema(v, path="$", parent=None):
    entry = schema[path]
    t = classify(v)
    entry["types"].add(t)
    entry["present"] += 1
    add_example(entry, v)

    if parent is not None:
        link(parent, path)

    if isinstance(v, dict):
        for k, subv in v.items():
            child = child_path(path, k)
            update_schema(subv, child, path)

    elif isinstance(v, list):
        for item in v:
            entry["array_types"].add(classify(item))
            child = f"{path}[*]"
            update_schema(item, child, path)


def extract_json_objects_from_buffer(buf):
    out = []
    start = None
    depth = 0
    in_string = False
    escape = False

    i = 0
    while i < len(buf):
        ch = chr(buf[i])

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
                    raw = buf[start:i + 1]
                    try:
                        out.append(json.loads(raw.decode("utf-8")))
                    except Exception:
                        pass
                    start = None
        i += 1

    remainder = buf[start:] if start is not None else b""
    return out, remainder


def node_name(path):
    if path == "$":
        return "."
    return path.split(".")[-1]


def render_tree(path="$", prefix="", is_last=True):
    entry = schema[path]
    name = node_name(path)

    types = sorted(entry["types"])
    required = (entry["present"] == total_objects) if total_objects else False

    meta = f"[{','.join(types)}]"
    if required:
        meta += " *"

    connector = "└── " if is_last else "├── "
    print(prefix + connector + f"{name} {meta}")

    children = sorted(schema[path]["children"])
    if not children:
        return

    new_prefix = prefix + ("    " if is_last else "│   ")

    for i, child in enumerate(children):
        render_tree(child, new_prefix, i == len(children) - 1)


def render():
    os.system("clear")
    print("=== TREE SCHEMA ===")
    print(f"objects_seen: {total_objects}\n")

    if "$" not in schema:
        print("(no data)")
        return

    print(".")
    children = sorted(schema["$"]["children"])
    for i, child in enumerate(children):
        render_tree(child, "", i == len(children) - 1)


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
                update_schema(obj, "$", None)

            render()


if __name__ == "__main__":
    watch(P)
