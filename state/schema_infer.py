#!/usr/bin/env python3
"""
Schema = Map(kind → Map(variant → shape))

Rule: |keys(payload)| = 1  →  payload is a tagged-union container.
      The single key IS the variant; its value IS the body.

Old mistake: union(all keys across all events) under $.payload.*
Fix:         per-kind, per-variant shape — never merged across variants.
"""
import json, os, sys, time
from collections import defaultdict

LOG_PATH = "/workspace/ai_sandbox/canon/canon-utils/state/event_log/event.tlog.d/00000000000000000000.log"

# ── data model ───────────────────────────────────────────────────────────────
#
#  variants_by_kind[kind][variant][dotted_field] = {
#      types: set[str], examples: set[str],
#      array_item_types: set[str], present: int
#  }
#  variant_counts[kind][variant] = int   (events with this variant)
#  kind_counts[kind]             = int   (total events for kind)

def _new_field():
    return {"types": set(), "examples": set(), "array_item_types": set(), "present": 0}

variants_by_kind  = defaultdict(lambda: defaultdict(lambda: defaultdict(_new_field)))
variant_counts    = defaultdict(lambda: defaultdict(int))
kind_counts       = defaultdict(int)
total_objects     = 0


def _classify(v):
    if v is None:            return "null"
    if isinstance(v, bool):  return "bool"
    if isinstance(v, int):   return "int"
    if isinstance(v, float): return "float"
    if isinstance(v, str):   return "string"
    if isinstance(v, list):  return "array"
    if isinstance(v, dict):  return "object"
    return "?"


def _add_example(field, value):
    if isinstance(value, (str, int, float, bool, type(None))):
        if len(field["examples"]) < 5:
            s = repr(value)
            field["examples"].add(s[:50] if len(s) > 50 else s)


def _infer_shape(obj, shape, prefix=""):
    """Recursively walk body dict; record dotted field paths into shape."""
    if not isinstance(obj, dict):
        f = shape[prefix or "_value"]
        f["types"].add(_classify(obj))
        f["present"] += 1
        _add_example(f, obj)
        return
    for k, v in obj.items():
        path = f"{prefix}.{k}" if prefix else k
        f = shape[path]
        f["types"].add(_classify(v))
        f["present"] += 1
        _add_example(f, v)
        if isinstance(v, list):
            for item in v:
                f["array_item_types"].add(_classify(item))
        elif isinstance(v, dict):
            _infer_shape(v, shape, path)


def ingest(obj):
    global total_objects
    total_objects += 1

    kind    = obj.get("kind", "__unknown__")
    payload = obj.get("payload")
    kind_counts[kind] += 1

    if isinstance(payload, dict) and len(payload) == 1:
        # ── tagged union: exactly one key = variant name ──────────────────
        variant, body = next(iter(payload.items()))
    elif isinstance(payload, dict) and len(payload) == 0:
        variant, body = "__empty__", {}
    else:
        # payload has multiple keys or is not a dict: treat as flat struct
        variant, body = "__flat__", payload

    variant_counts[kind][variant] += 1
    _infer_shape(body, variants_by_kind[kind][variant])


# ── rendering ────────────────────────────────────────────────────────────────

def _card(present, total):
    if total == 0: return "   "
    f = present / total
    if f >= 0.98: return " * "
    if f >= 0.75: return " + "
    if f >= 0.30: return " ~ "
    return " - "


def render():
    os.system("clear")
    print("═" * 70)
    print(f"  Event Schema  ·  {total_objects:,} events  ·  {len(kind_counts)} kind(s)")
    print(f"  Model: Schema = Map(kind → Map(variant → shape))")
    print(f"  Rule:  |keys(payload)| = 1  →  variant = that key")
    print("═" * 70)
    if total_objects == 0:
        print("  (waiting for data…)")
        return
    print()

    for kind in sorted(kind_counts):
        k_total  = kind_counts[kind]
        variants = variant_counts[kind]
        v_items  = sorted(variants.items(), key=lambda x: -x[1])

        print(f"  ┌─ {kind}  ({k_total:,})")
        for vi, (vname, vcount) in enumerate(v_items):
            is_last_v = (vi == len(v_items) - 1)
            v_branch  = "└──" if is_last_v else "├──"
            print(f"  │  {v_branch} {vname}  ({vcount:,})")

            shape  = variants_by_kind[kind][vname]
            fields = sorted(shape.items(), key=lambda x: (-x[1]["present"], x[0]))
            inner  = "       " if is_last_v else "   │   "

            for fi, (fname, info) in enumerate(fields):
                is_last_f = (fi == len(fields) - 1)
                f_branch  = "└──" if is_last_f else "├──"
                t   = ",".join(sorted(info["types"]))
                arr = (f" → [{','.join(sorted(info['array_item_types']))}]"
                       if info["array_item_types"] else "")
                ex  = (f"  ex: {', '.join(sorted(info['examples']))}"
                       if info["examples"] else "")
                req = _card(info["present"], vcount)
                print(f"  │  {inner}{f_branch} {fname}{req}{t}{arr}{ex}")

        print(f"  └{'─' * 50}")
        print()


# ── streaming tail ────────────────────────────────────────────────────────────

def tail_and_parse(path, from_start=False):
    if not os.path.exists(path):
        print(f"  file not found: {path}")
        return
    with open(path, "rb") as f:
        if not from_start:
            f.seek(0, os.SEEK_END)
        remainder = b""
        while True:
            chunk = f.readline()
            if not chunk:
                time.sleep(0.1)
                continue
            buf = remainder + chunk
            remainder = b""
            try:
                text = buf.decode("utf-8", errors="replace").rstrip("\r\n")
                if not text.strip():
                    continue
                start = text.find("{")
                if start == -1:
                    continue
                obj = json.loads(text[start:])
                ingest(obj)
                render()
            except json.JSONDecodeError:
                remainder = buf
            except Exception as e:
                print(f"  error: {e.__class__.__name__}  {str(e)[:80]}")


if __name__ == "__main__":
    args      = sys.argv[1:]
    from_start = "--from-start" in args
    paths      = [a for a in args if not a.startswith("--")]
    path       = paths[0] if paths else LOG_PATH
    print(f"Watching: {path}  ({'from start' if from_start else 'tail'})")
    try:
        tail_and_parse(path, from_start)
    except KeyboardInterrupt:
        print("\nStopped.")
        render()
