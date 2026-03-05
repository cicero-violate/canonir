import json
import sys

if len(sys.argv) < 2:
    print("[]")
    sys.exit(0)

path = sys.argv[1]
max_lines = int(sys.argv[2]) if len(sys.argv) > 2 else 50

first_obj = None

with open(path, "r", encoding="utf-8") as f:
    for line in f:
        if not line.strip():
            continue
        first_obj = json.loads(line)
        break

if not isinstance(first_obj, dict):
    print("[]")
    sys.exit(0)

scalar_key = None
string_key = None
string_value = None
object_key = None
nested_scalar_key = None
array_key = None

for k, v in first_obj.items():
    if scalar_key is None and isinstance(v, (str, int, float, bool)):
        scalar_key = k
    if string_key is None and isinstance(v, str):
        string_key = k
        string_value = v
    if object_key is None and isinstance(v, dict):
        for nk, nv in v.items():
            if isinstance(nv, (str, int, float, bool)):
                object_key = k
                nested_scalar_key = nk
                break
    if array_key is None and isinstance(v, list) and len(v) > 0:
        array_key = k

queries = []

if scalar_key:
    queries.append(f"$.{scalar_key}")
if object_key and nested_scalar_key:
    queries.append(f"$.{object_key}.{nested_scalar_key}")
if array_key:
    queries.append(f"$.{array_key}[0]")
    queries.append(f"$.{array_key}[0:3]")
if string_key and string_value is not None:
    safe = string_value.replace("'", "\\'")
    queries.append(f"$.{string_key}[?(@ == '{safe}')]" )

print(json.dumps(queries))
