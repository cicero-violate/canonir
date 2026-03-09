# upg_analysis

This crate produces the UPG analysis directory for a Rust crate. The output is fully self-contained and intended to be consumed without reading source code.

**How To Run**

```bash
RUSTC_WRAPPER=/workspace/ai_sandbox/canon/target/debug/analysis_capture cargo check
```

This runs the `analysis_capture` rustc wrapper, which invokes `upg_analysis` and writes the analysis directory at:

```
<crate_root>/analysis/
```

**Analysis Directory Layout**

```
analysis/
  nodes.csv
  edges.csv
  files.txt
  node_kinds.txt
  edge_kinds.txt
  spans.bin
  symbols.json
  csr_row_ptr.bin
  csr_col_idx.bin
  metadata.json
  upg_invariants.json
```

**Key Files**

- `nodes.csv`: semantic nodes.
  - Columns: `node_id,node_kind,symbol,file_id,line,column,parent`
- `edges.csv`: relationships between nodes.
  - Columns: `src_id,dst_id,edge_kind`
- `files.txt`: file table.
  - Columns: `file_id,path`
- `spans.bin`: fixed-width span table, one record per node.
  - Record: `node_id(u32), file_id(u32), lo(u32), hi(u32)` (little-endian)
- `node_kinds.txt` / `edge_kinds.txt`: canonical kind strings.
- `csr_row_ptr.bin` / `csr_col_idx.bin`: CSR adjacency for fast graph traversal.
- `metadata.json`: generation metadata.
- `upg_invariants.json`: invariant report written on every run.

**Invariant Enforcement**

The analysis writer validates invariants after emitting files and fails the run if any are violated. The report is saved as `analysis/upg_invariants.json`.

Core invariants:
- Node IDs are contiguous.
- Every edge endpoint exists.
- Kinds are valid.
- `spans.bin` aligns 1:1 with `nodes.csv`.
- Every `BASIC_BLOCK` and `CALL_SITE` has incoming `HAS_BLOCK`.
- No isolated nodes.
- Exactly one module root.

**Consumption Notes**

- All analysis decisions should be derived from `analysis/` only.
- `spans.bin` + `files.txt` give fast source mapping without `spans.jsonl`.
- `symbols.json` is present for compatibility during migration.

**Python Examples**

Load core tables:

```python
from pathlib import Path
import csv, struct

root = Path("analysis")
nodes_path = root / "nodes.csv"
edges_path = root / "edges.csv"
files_path = root / "files.txt"
spans_path = root / "spans.bin"

# files.txt
files = []
with files_path.open() as f:
    for i, line in enumerate(f):
        if i == 0 or not line.strip():
            continue
        parts = line.rstrip("\n").split(",")
        fid = int(parts[0])
        path = ",".join(parts[1:])
        if len(files) <= fid:
            files.extend([""] * (fid + 1 - len(files)))
        files[fid] = path

# nodes.csv
nodes = {}
with nodes_path.open() as f:
    r = csv.reader(f)
    next(r, None)
    for row in r:
        if not row:
            continue
        node_id = int(row[0])
        nodes[node_id] = {
            "kind": row[1],
            "symbol": row[2],
            "file_id": int(row[3]),
            "line": int(row[4]),
            "col": int(row[5]),
        }

# edges.csv
edges = []
with edges_path.open() as f:
    r = csv.reader(f)
    next(r, None)
    for row in r:
        if not row:
            continue
        edges.append((int(row[0]), int(row[1]), row[2]))
```

Map a node to file + span:

```python
def load_spans(path):
    data = Path(path).read_bytes()
    out = {}
    for i in range(0, len(data), 16):
        node_id = int.from_bytes(data[i:i+4], "little")
        file_id = int.from_bytes(data[i+4:i+8], "little")
        lo = int.from_bytes(data[i+8:i+12], "little")
        hi = int.from_bytes(data[i+12:i+16], "little")
        out[node_id] = (file_id, lo, hi)
    return out

spans = load_spans(spans_path)

node_id = 42
file_id, lo, hi = spans[node_id]
print(files[file_id], lo, hi)
```

Find all functions and their callsites:

```python
call_sites = [nid for nid, n in nodes.items() if n["kind"] == "CALL_SITE"]
functions = [nid for nid, n in nodes.items() if n["kind"] == "FUNCTION"]

# CALL_SITE --CALL--> FUNCTION
call_edges = [(src, dst) for (src, dst, kind) in edges if kind == "CALL"]
print("call edges:", len(call_edges))
```

Quick invariant check (analysis-only, no source):

```python
edge_src = {s for s,_,_ in edges}
edge_dst = {d for _,d,_ in edges}
isolated = [nid for nid in nodes if nid not in edge_src and nid not in edge_dst]
print("isolated nodes:", len(isolated))
```
