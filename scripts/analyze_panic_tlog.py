import json
import re
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOG = ROOT / "state" / "kernel_logs" / "kernel.tlog"

if not LOG.exists():
    raise SystemExit(f"kernel.tlog not found at {LOG}")

def load_records(path: Path):
    decoder = json.JSONDecoder()
    records = []
    bad_chunks = 0
    with path.open() as f:
        for raw_line in f:
            line = raw_line.strip()
            if not line:
                continue
            idx = 0
            length = len(line)
            while idx < length:
                # Skip non-JSON leading chars (e.g., stray log prefixes)
                while idx < length and line[idx] not in "{[":
                    idx += 1
                if idx >= length:
                    break
                try:
                    obj, next_idx = decoder.raw_decode(line, idx)
                    records.append(obj)
                    idx = next_idx
                except json.JSONDecodeError:
                    bad_chunks += 1
                    break
    return records, bad_chunks


records, bad_chunks = load_records(LOG)

panics = [r for r in records if r.get("t") == "PANIC"]

TOP_N = 10
EXAMPLE_N = 3

print(f"Total panic events: {len(panics)}")
if bad_chunks:
    print(f"WARNING: skipped {bad_chunks} malformed log chunks")

def normalize_message(msg: str) -> str:
    if not msg:
        return "<none>"
    out = msg
    out = re.sub(r"def_id=[^\\s,\\)]+", "def_id=<id>", out)
    out = re.sub(r"mir_variant=[^\\s,\\)]+", "mir_variant=<mir>", out)
    out = re.sub(r"lowering_stage=[^\\s,\\)]+", "lowering_stage=<stage>", out)
    out = re.sub(r"path=[^\\s,\\)]+", "path=<path>", out)
    out = re.sub(r"target=[^\\s,\\)]+", "target=<bb>", out)
    out = re.sub(r"lhs=\\\"[^\\\"]+\\\"", "lhs=\"<lhs>\"", out)
    out = re.sub(r"rvalue=\\{[^}]+\\}", "rvalue={<rvalue>}", out)
    out = re.sub(r"rvalue=[^\\s,\\)]+", "rvalue=<rvalue>", out)
    out = re.sub(r"node_id=\(?[^\s,\)]+\)?", "node_id=<id>", out)
    return out

messages = Counter(normalize_message(r.get("message", "")) for r in panics)
print(f"\nUnique panic signatures: {len(messages)}")
for msg, count in messages.most_common(TOP_N):
    print(f"  [{count}x] {msg[:200]}")

def_ids = Counter(r.get("def_id") for r in panics)
print(f"\nTop def_ids: {min(len(def_ids), TOP_N)} of {len(def_ids)}")
for did, count in def_ids.most_common(TOP_N):
    print(f"  [{count}x] {did}")

mir_variants = Counter(r.get("mir_variant") or "<none>" for r in panics)
print(f"\nTop mir_variants: {min(len(mir_variants), TOP_N)} of {len(mir_variants)}")
for mv, count in mir_variants.most_common(TOP_N):
    print(f"  [{count}x] {mv}")

stages = Counter(r.get("lowering_stage") or "<none>" for r in panics)
print(f"\nTop lowering_stages: {min(len(stages), TOP_N)} of {len(stages)}")
for st, count in stages.most_common(TOP_N):
    print(f"  [{count}x] {st}")

panic_sites = Counter()
panic_files = Counter()
for p in panics:
    span = p.get("span")
    file = p.get("file")
    if span:
        panic_sites[span] += 1
    if file:
        panic_files[file] += 1

if panic_sites:
    print(f"\nTop panic sites (span):")
    for span, count in panic_sites.most_common(TOP_N):
        print(f"  [{count}x] {span}")
if panic_files:
    print(f"\nTop panic files:")
    for file, count in panic_files.most_common(TOP_N):
        print(f"  [{count}x] {file}")

def kernel_frames(panic):
    out = []
    for f in panic.get("frames", []):
        for sym in f.get("symbols", []):
            s = sym.get("symbol")
            if not s:
                continue
            if "canon_kernel" not in s and "canon::" not in s and "canon_kernel::" not in s:
                continue
            if "install_panic_hook" in s:
                continue
            if "panic::" in s:
                continue
            out.append({
                "symbol": s,
                "file": sym.get("file"),
                "line": sym.get("line"),
            })
    return out

innermost = Counter()
hotspots = Counter()
for p in panics:
    kf = kernel_frames(p)
    if kf:
        innermost[kf[0]["symbol"]] += 1
        file = kf[0].get("file") or "<no-file>"
        line = kf[0].get("line") or "?"
        hotspots[f"{file}:{line}"] += 1

print(f"\nDominant failure frames (innermost canon_kernel, hook filtered):")
for sym, count in innermost.most_common(TOP_N):
    print(f"  [{count}x] {sym}")

print(f"\nTop failure sites (file:line):")
for loc, count in hotspots.most_common(TOP_N):
    print(f"  [{count}x] {loc}")

edge_counter = Counter()
for p in panics:
    kf = kernel_frames(p)
    for i in range(len(kf) - 1):
        a = kf[i]["symbol"].split("::")[-1]
        b = kf[i + 1]["symbol"].split("::")[-1]
        edge_counter[(a, b)] += 1

print(f"\nDominant call chain edges:")
for (a, b), count in edge_counter.most_common(TOP_N):
    print(f"  [{count}x] {a} -> {b}")

signature_examples = defaultdict(list)
for p in panics:
    sig = normalize_message(p.get("message", ""))
    if len(signature_examples[sig]) < EXAMPLE_N:
        signature_examples[sig].append(p.get("def_id"))

print(f"\nSignature examples (def_id):")
for sig, count in messages.most_common(TOP_N):
    examples = ", ".join(d for d in signature_examples[sig] if d) or "<none>"
    print(f"  [{count}x] {sig[:160]} | examples: {examples}")

rvalue_pattern = re.compile(r"rvalue=(\{[^}]+\}|\S+)")
rvalues = Counter()
for p in panics:
    m = rvalue_pattern.search(p.get("message", ""))
    if m:
        rvalues[m.group(1)] += 1

print(f"\nDistinct failing rvalues:")
for rv, count in rvalues.most_common(TOP_N):
    print(f"  [{count}x] {rv}")

lhs_pattern = re.compile(r"lhs=\"([^\"]+)\"")
lhs_names = Counter()
for p in panics:
    m = lhs_pattern.search(p.get("message", ""))
    if m:
        lhs_names[m.group(1)] += 1

print(f"\nDistinct failing lhs locals:")
for lhs, count in lhs_names.most_common(TOP_N):
    print(f"  [{count}x] {lhs}")

path_pattern = re.compile(r"invalid path for path_intern: (.+?) @")
bad_paths = Counter()
for p in panics:
    m = path_pattern.search(p.get("message", ""))
    if m:
        bad_paths[m.group(1)] += 1

print(f"\nDistinct invalid paths:")
for path, count in bad_paths.most_common(TOP_N):
    print(f"  [{count}x] {path}")

malformed_pattern = re.compile(r"malformed/private helper path segment.*examples=\[(.+)\]")
malformed = Counter()
for p in panics:
    m = malformed_pattern.search(p.get("message", ""))
    if m:
        for frag in m.group(1).split(", "):
            if frag:
                malformed[frag] += 1

print(f"\nMalformed path examples:")
for frag, count in malformed.most_common(TOP_N):
    print(f"  [{count}x] {frag}")

alloc_pattern = re.compile(r'artifact leaked into Canon name interner[^\n]*\n?.*?name="?([^"\\s,\\)]+)', re.DOTALL)
artifact_names = Counter()
for p in panics:
    m = alloc_pattern.search(p.get("message", ""))
    if m:
        artifact_names[m.group(1)] += 1

print(f"\nLeaked artifact names:")
for name, count in artifact_names.most_common(TOP_N):
    print(f"  [{count}x] {name}")

alloc_messages = [p for p in panics if "alloc/debug artifact" in (p.get("message", "") or "")]
if alloc_messages:
    print(f"\nFull messages (alloc artifact class, first {EXAMPLE_N}):")
    for p in alloc_messages[:EXAMPLE_N]:
        msg = p.get("message", "")
        print(f"  def_id: {p.get('def_id')}")
        print(f"  message: {msg}")
        print()
