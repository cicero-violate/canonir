# scripts/audit_rvalue_coverage.py
"""
Reads the kernel.tlog and emits:
  1. Rvalue discriminant frequency table from panic messages
  2. Cast kind frequency table
  3. Constant operand source frequency table
  4. Per-discriminant: first 3 concrete def_id examples
Cross-referenced against a hardcoded table of known-handled variants.
"""
import json, re
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOG  = ROOT / "state" / "kernel_logs" / "kernel.tlog"

# ── known-handled Rvalue variants (from mir_rvalue_expr match arms) ─────────
HANDLED = {
    "Use",
    "BinaryOp",          # partial — only non-const operands confirmed
    "UnaryOp",
    "Ref",
    "AddressOf",
    "Aggregate::Struct",
    "Aggregate::Adt",
    "Aggregate::Array",
    "Aggregate::Tuple",
    "Aggregate::Closure",
    "Discriminant",
    "Len",
    "NullaryOp::SizeOf",
    "NullaryOp::AlignOf",
    "Cast::IntToInt",
    "Cast::FloatToInt",
    "Cast::IntToFloat",
    "Cast::FloatToFloat",
}

def load_panics(path: Path):
    dec = json.JSONDecoder()
    panics = []
    with path.open() as f:
        for raw in f:
            line = raw.strip()
            if not line:
                continue
            idx = 0
            while idx < len(line):
                while idx < len(line) and line[idx] not in "{[":
                    idx += 1
                if idx >= len(line):
                    break
                try:
                    obj, nxt = dec.raw_decode(line, idx)
                    if obj.get("t") == "PANIC":
                        panics.append(obj)
                    idx = nxt
                except json.JSONDecodeError:
                    break
    return panics

# ── extraction patterns ──────────────────────────────────────────────────────
PAT_RVALUE     = re.compile(r'rvalue=(\w[\w::<>, ]*?)(?:\s|,|\))')
PAT_CAST_KIND  = re.compile(r'cast_kind=(\w+)')
PAT_CONST_SRC  = re.compile(r'const_source=(\w+)')
PAT_OPERAND    = re.compile(r'operand_kind=(\w+)')
PAT_SERDE_PRIV = re.compile(r'serde-private def filtered')

panics = load_panics(LOG)
print(f"Loaded {len(panics)} panic events\n")

rvalue_ctr   = Counter()
cast_ctr     = Counter()
const_ctr    = Counter()
operand_ctr  = Counter()
stage_ctr    = Counter()
serde_count  = 0
examples     = defaultdict(list)

for p in panics:
    msg = p.get("message", "") or ""
    did = p.get("def_id", "<?>")

    if PAT_SERDE_PRIV.search(msg):
        serde_count += 1
        continue                 # serde-private is a separate filter issue

    mir_variant = p.get("mir_variant")
    if mir_variant:
        rvalue_ctr[mir_variant] += 1
        if len(examples[("rvalue", mir_variant)]) < 3:
            examples[("rvalue", mir_variant)].append(did)
    else:
        for m in PAT_RVALUE.finditer(msg):
            key = m.group(1).strip()
            rvalue_ctr[key] += 1
            if len(examples[("rvalue", key)]) < 3:
                examples[("rvalue", key)].append(did)

    cast_kind = p.get("cast_kind")
    if cast_kind:
        cast_ctr[cast_kind] += 1
        if len(examples[("cast", cast_kind)]) < 3:
            examples[("cast", cast_kind)].append(did)
    else:
        for m in PAT_CAST_KIND.finditer(msg):
            key = m.group(1)
            cast_ctr[key] += 1
            if len(examples[("cast", key)]) < 3:
                examples[("cast", key)].append(did)

    const_source = p.get("const_source")
    if const_source:
        const_ctr[const_source] += 1
    else:
        for m in PAT_CONST_SRC.finditer(msg):
            key = m.group(1)
            const_ctr[key] += 1

    operand_kind = p.get("operand_kind")
    if operand_kind:
        operand_ctr[operand_kind] += 1
    else:
        for m in PAT_OPERAND.finditer(msg):
            key = m.group(1)
            operand_ctr[key] += 1

    lowering_stage = p.get("lowering_stage")
    if lowering_stage:
        stage_ctr[lowering_stage] += 1

print(f"Serde-private panics (separate filter issue): {serde_count}")
print(f"Non-serde panics analysed: {len(panics) - serde_count}\n")

def report(title, ctr, handled_set=None):
    print(f"── {title} ({'handled' if handled_set else 'all'}) ──")
    if not ctr:
        print("  (none captured in messages — enrich messages or run next phase)\n")
        return
    for k, n in ctr.most_common():
        tag = "✓" if (handled_set and k in handled_set) else "✗ GAP"
        exs = ", ".join(examples.get((title.split()[0].lower(), k), []))
        print(f"  [{n:4d}x] {tag:6s} {k}")
        if exs:
            print(f"           examples: {exs}")
    print()

report("Rvalue variants",  rvalue_ctr,  HANDLED)
report("Cast kinds",       cast_ctr,    None)
report("Const sources",    const_ctr,   None)
report("Operand kinds",    operand_ctr, None)
report("Lowering stages",  stage_ctr,   None)

# ── static gap table: full MIR Rvalue enum vs HANDLED ───────────────────────
ALL_RVALUE_VARIANTS = [
    # Use / scalars
    "Use", "Repeat", "Ref", "ThreadLocalRef", "AddressOf",
    "Len", "Cast", "BinaryOp", "NullaryOp", "UnaryOp",
    "Discriminant", "Aggregate", "ShallowInitBox", "CopyForDeref",
    # Cast sub-kinds (nightly)
    "Cast::IntToInt", "Cast::IntToFloat", "Cast::FloatToInt", "Cast::FloatToFloat",
    "Cast::FnPtrToPtr", "Cast::PtrToPtr", "Cast::PointerCoercion",
    "Cast::PointerExposeAddress", "Cast::PointerWithExposedProvenance",
    "Cast::Transmute",
    # Aggregate sub-kinds
    "Aggregate::Array", "Aggregate::Tuple", "Aggregate::Adt",
    "Aggregate::Closure", "Aggregate::Coroutine", "Aggregate::RawPtr",
]

ALL_CAST_KINDS = [
    "IntToInt", "IntToFloat", "FloatToInt", "FloatToFloat",
    "FnPtrToPtr", "PtrToPtr", "PointerCoercion",
    "PointerExposeAddress", "PointerWithExposedProvenance",
    "Transmute",
]

print("── Static gap table (MIR → HANDLED) ──")
for v in ALL_RVALUE_VARIANTS:
    status = "✓ handled" if v in HANDLED else "✗ GAP"
    print(f"  {status:12s}  {v}")

print("\n── Cast kind gap table ──")
for ck in ALL_CAST_KINDS:
    full = f"Cast::{ck}"
    status = "✓" if full in HANDLED else "✗ GAP"
    print(f"  {status}  {ck}")
