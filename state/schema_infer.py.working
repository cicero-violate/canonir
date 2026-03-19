#!/usr/bin/env python3
import json
import os
import time
from collections import defaultdict, Counter

LOG_PATH = "/workspace/ai_sandbox/canon/canon-utils/state/event_log/event.tlog.d/00000000000000000000.log"

# ────────────────────────────────────────────────
# Schema structure
# ────────────────────────────────────────────────

schema = defaultdict(lambda: {
    "types":        set(),          # set of seen scalar types
    "array_item_types": set(),      # if this path points to array
    "present_count": 0,             # how many objects had this field/path
    "examples":     set(),          # up to ~5 example values (repr)
    "children":     set(),          # direct child paths
    "parents":      set(),
})

total_objects = 0


def classify_value(v):
    if v is None:               return "null"
    if isinstance(v, bool):     return "bool"
    if isinstance(v, int):      return "int"
    if isinstance(v, float):    return "float"
    if isinstance(v, str):      return "string"
    if isinstance(v, list):     return "array"
    if isinstance(v, dict):     return "object"
    return f"?{type(v).__name__}?"


def add_example(entry, value):
    if isinstance(value, (str, int, float, bool, type(None))):
        if len(entry["examples"]) < 5:
            if isinstance(value, str) and len(value) > 50:
                value = value[:50]
            entry["examples"].add(repr(value))


def child_path(parent_path: str, key: str | int) -> str:
    if parent_path == "$":
        return f"$.{key}"
    if isinstance(key, int):
        return f"{parent_path}[*]"
    return f"{parent_path}.{key}"


def link(parent_path, child_path):
    schema[parent_path]["children"].add(child_path)
    schema[child_path]["parents"].add(parent_path)


def update_schema(value, path: str = "$", parent_path=None):
    entry = schema[path]
    t = classify_value(value)
    entry["types"].add(t)
    entry["present_count"] += 1

    add_example(entry, value)

    if parent_path is not None:
        link(parent_path, path)

    if isinstance(value, dict):
        for k, subv in value.items():
            child = child_path(path, k)
            update_schema(subv, child, path)

    elif isinstance(value, list):
        for item in value:
            entry["array_item_types"].add(classify_value(item))
            # We only go one level deeper with [*]
            # (you can make this recursive if you want deep array-of-array inference)
            item_path = child_path(path, "[*]")
            update_schema(item, item_path, path)


def print_schema_tree(path="$", prefix="", is_last=True, depth=0, max_depth=6):
    if depth > max_depth:
        print(prefix + ("└── " if is_last else "├── ") + "(depth limit)")
        return

    entry = schema[path]
    name = "." if path == "$" else path.split(".")[-1].split("[")[-1]

    types = sorted(entry["types"])
    array_types = sorted(entry["array_item_types"])

    # Very naive "cardinality"
    frac = entry["present_count"] / total_objects if total_objects > 0 else 0
    card = "   "               # always / structurally required
    if frac >= 0.98:    card = " * "     # almost always
    elif frac >= 0.75:  card = " + "     # very often
    elif frac >= 0.30:  card = " ~ "     # sometimes
    elif frac > 0:      card = " - "     # rarely
    else:               card = "   "

    type_str = ",".join(types)
    if array_types:
        type_str += f" → [{','.join(array_types)}]"

    if entry["examples"]:
        ex = ", ".join(sorted(entry["examples"]))
        type_str += f"  ex: {ex}"

    connector = "└── " if is_last else "├── "
    print(f"{prefix}{connector}{name}{card}{type_str}")

    children = sorted(
        schema[path]["children"],
        key=lambda p: (-schema[p]["present_count"], p)
    )

    new_prefix = prefix + ("    " if is_last else "│   ")
    for i, child in enumerate(children):
        print_schema_tree(
            child,
            new_prefix,
            i == len(children) - 1,
            depth + 1,
            max_depth
        )


def render():
    os.system("clear")
    print("═" * 70)
    print(f"  Combined Schema Tree (streaming union of event structures)   —   {total_objects:,} objects seen")
    print("═" * 70)
    if total_objects == 0 or "$" not in schema:
        print("  (waiting for data)")
        return

    print_schema_tree("$", "", True)


def tail_and_parse(path):
    global total_objects

    if not os.path.exists(path):
        print(f"File not found: {path}")
        return

    with open(path, "rb") as f:
        f.seek(0, os.SEEK_END)   # start at end

        remainder = b""

        while True:
            line_bytes = f.readline()

            if not line_bytes:
                time.sleep(0.1)
                continue

            buf = remainder + line_bytes
            remainder = b""

            # Naive: assume one JSON per line after some prefix junk
            # You can replace this with proper streaming JSON decoder if needed
            try:
                text = buf.decode("utf-8", errors="replace").rstrip("\r\n")
                if not text.strip():
                    continue

                # Find first {
                start = text.find("{")
                if start == -1:
                    continue

                json_part = text[start:]
                obj = json.loads(json_part)

                total_objects += 1
                update_schema(obj, "$", None)
                render()

            except json.JSONDecodeError:
                # Maybe multi-line or broken → keep remainder
                remainder = buf
            except Exception as e:
                print(f"  error: {e.__class__.__name__}  {str(e)[:80]}")
                continue


if __name__ == "__main__":
    print("Watching:", LOG_PATH)
    try:
        tail_and_parse(LOG_PATH)
    except KeyboardInterrupt:
        print("\nStopped.")
        render()   # final render
