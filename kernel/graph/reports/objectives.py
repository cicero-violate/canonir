#!/usr/bin/env python3
import json

def load(path):
    with open(path) as f:
        return json.load(f)

# ── load ──────────────────────────────────────────────────────────────────────
hotspots   = load("structural_hotspots_report.json")
pressure   = load("branch_pressure_report.json")
fanout     = load("dataflow_fanout_report.json")
centrality = load("callgraph_centrality_report.json")
redundancy = load("path_redundancy_report.json")
cycles     = load("dependency_cycle_report.json")
merges     = load("merge_candidates_report.json")
complexity = load("branch_complexity_report.json")

# ── index by symbol ───────────────────────────────────────────────────────────
h_idx = {r['symbol']: r['score']            for r in hotspots}
f_idx = {r['symbol']: r['outgoing_edges']   for r in fanout}
p_idx = {r['symbol']: r['branch_pressure']  for r in pressure}
c_idx = {r['symbol']: r['centrality_score'] for r in centrality}

SEP  = "─" * 120
SEP2 = "═" * 120

def section(title):
    print(f"\n{SEP2}")
    print(f"  {title}")
    print(SEP2)

def loc(r, key='file'):
    return r[key]

# ══ 1. DEPENDENCY CYCLES ══════════════════════════════════════════════════════
section("OBJECTIVE 1 — SEVER DEPENDENCY CYCLES  (cut first: makes downstream a DAG)")
print(f"  Total cycles: {len(cycles)}")
for r in cycles:
    print(f"\n  cycle_id={r['cycle_id']}  length={r['cycle_length']}")
    for n, fi in zip(r['nodes'], r['files']):
        print(f"    node : {n}")
        print(f"    file : {fi}")

# ══ 2. STRUCTURAL HOTSPOTS ════════════════════════════════════════════════════
section("OBJECTIVE 2 — COLLAPSE STRUCTURAL HOTSPOTS  (score = branch_count × duplicate_blocks)")
top_h = sorted(hotspots, key=lambda x: x['score'], reverse=True)[:15]
print(f"  {'SCORE':>10}  {'BRANCHES':>8}  {'DUPS':>6}  {'CALLERS':>7}  SYMBOL")
print(f"  {SEP}")
for r in top_h:
    callers = len(r['callers']) if isinstance(r['callers'], list) else r['callers']
    print(f"  {r['score']:>10}  {r['branch_count']:>8}  {r['duplicate_blocks']:>6}  {callers:>7}  {r['symbol']}")
    print(f"  {'':>10}  {'':>8}  {'':>6}  {'':>7}  └─ {r['file']}:{r['line']}")

# ══ 3. BRANCH PRESSURE ════════════════════════════════════════════════════════
section("OBJECTIVE 3 — RELIEVE BRANCH PRESSURE  (pressure = nodes × branch density)")
top_p = sorted(pressure, key=lambda x: x['branch_pressure'], reverse=True)[:15]
print(f"  {'PRESSURE':>10}  {'NODES':>6}  SYMBOL")
print(f"  {SEP}")
for r in top_p:
    print(f"  {r['branch_pressure']:>10.1f}  {r['branch_nodes']:>6}  {r['symbol']}")
    print(f"  {'':>10}  {'':>6}  └─ {r['file']}:{r['line']}")

# ══ 4. DATAFLOW FANOUT ════════════════════════════════════════════════════════
section("OBJECTIVE 4 — CAP DATAFLOW FANOUT  (outgoing edges = state propagation surface)")
top_f = sorted(fanout, key=lambda x: x['outgoing_edges'], reverse=True)[:15]
print(f"  {'OUT_EDGES':>9}  {'MUT':>5}  {'IO':>4}  SYMBOL")
print(f"  {SEP}")
for r in top_f:
    print(f"  {r['outgoing_edges']:>9}  {r['mutation_edges']:>5}  {r['io_edges']:>4}  {r['symbol']}")
    print(f"  {'':>9}  {'':>5}  {'':>4}  └─ {r['file']}:{r['line']}")

# ══ 5. CALLGRAPH CENTRALITY ═══════════════════════════════════════════════════
section("OBJECTIVE 5 — ISOLATE HIGH-CENTRALITY HUBS  (hub failure = cascade failure)")
top_c = sorted(centrality, key=lambda x: x['centrality_score'], reverse=True)[:15]
print(f"  {'CENTRALITY':>10}  {'CALLERS':>7}  {'CALLEES':>7}  SYMBOL")
print(f"  {SEP}")
for r in top_c:
    print(f"  {r['centrality_score']:>10.1f}  {r['caller_count']:>7}  {r['callee_count']:>7}  {r['symbol']}")
    print(f"  {'':>10}  {'':>7}  {'':>7}  └─ {r['file']}")

# ══ 6. PATH REDUNDANCY ════════════════════════════════════════════════════════
section("OBJECTIVE 6 — PRUNE REDUNDANT PATHS")
non_trivial = [r for r in redundancy if r['redundancy_ratio'] < 1.0 and r['paths_total'] > 1]
top_r = sorted(non_trivial, key=lambda x: x['paths_total'], reverse=True)[:15]
print(f"  {'RATIO':>7}  {'TOTAL':>6}  {'UNIQUE':>6}  SYMBOL")
print(f"  {SEP}")
if top_r:
    for r in top_r:
        print(f"  {r['redundancy_ratio']:>7.4f}  {r['paths_total']:>6}  {r['paths_unique']:>6}  {r['symbol']}")
        print(f"  {'':>7}  {'':>6}  {'':>6}  └─ {r['file']}:{r['line']}")
else:
    print("  No symbols with ratio < 1.0 and paths_total > 1.")
    top_r2 = sorted(redundancy, key=lambda x: x['paths_total'], reverse=True)[:10]
    for r in top_r2:
        print(f"  {r['redundancy_ratio']:>7.4f}  total={r['paths_total']:>5}  unique={r['paths_unique']:>5}  {r['symbol']}")
        print(f"  {'':>7}  {'':>28}  └─ {r['file']}:{r['line']}")

# ══ 7. COMPOSITE EXPLOSION SCORE ══════════════════════════════════════════════
section("OBJECTIVE 7 — COMPOSITE STATE EXPLOSION RANKING  H(v) × F(v) × P(v)  [triple-overlap only]")
overlap = set(h_idx) & set(f_idx) & set(p_idx)
ranked = sorted(overlap, key=lambda s: h_idx[s] * f_idx[s] * p_idx[s], reverse=True)[:15]
print(f"  {'COMPOSITE':>14}  {'H_SCORE':>9}  {'FANOUT':>7}  {'PRESSURE':>9}  SYMBOL")
print(f"  {SEP}")
for s in ranked:
    composite = h_idx[s] * f_idx[s] * p_idx[s]
    print(f"  {composite:>14.0f}  {h_idx[s]:>9}  {f_idx[s]:>7}  {p_idx[s]:>9.1f}  {s}")

# ══ 8. MERGE CANDIDATES ═══════════════════════════════════════════════════════
section("OBJECTIVE 8 — MERGE CANDIDATE BLOCKS  (largest collapsible block groups)")
top_m = sorted(merges, key=lambda x: len(x['candidate_blocks']), reverse=True)[:15]
print(f"  {'CANDIDATES':>10}  {'SUCCESSORS':>10}  FUNCTION")
print(f"  {SEP}")
for r in top_m:
    print(f"  {len(r['candidate_blocks']):>10}  {len(r['successors']):>10}  {r['function']}")
    print(f"  {'':>10}  {'':>10}  block={r['branch_block']}  successors={r['successors'][:4]}")

# ══ SUMMARY ═══════════════════════════════════════════════════════════════════
section("SUMMARY — SCALE OF STATE EXPLOSION")
print(f"  branch_complexity entries : {len(complexity):>6}   (total branch surfaces in codebase)")
print(f"  structural hotspots       : {len(hotspots):>6}   (scored nodes)")
print(f"  branch pressure nodes     : {len(pressure):>6}")
print(f"  dataflow fanout nodes     : {len(fanout):>6}")
print(f"  callgraph centrality nodes: {len(centrality):>6}")
print(f"  path redundancy nodes     : {len(redundancy):>6}")
print(f"  merge candidate groups    : {len(merges):>6}")
print(f"  dependency cycles         : {len(cycles):>6}")
print(f"  triple-overlap symbols    : {len(overlap):>6}   ← primary deletion targets")
top1    = hotspots[0]
print(f"\n  Worst single node score   : {top1['score']:>9}  {top1['symbol']}")
print(f"  Worst single node file    :            {top1['file']}:{top1['line']}")
