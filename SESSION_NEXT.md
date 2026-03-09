# Next Session: Schema Versioning + EdgeKind Deduplication

## Goal
Fix two structural gaps identified in the analysis pipeline:

1. `analysis/` has no schema version — format changes silently break consumers
2. `parse_edge_kind` is duplicated in `loader.rs` and `augment.rs` — one canonical table required

## Prompt
We are working on a Rust compiler analysis pipeline that extracts a UPG
(Universal Program Graph) from every crate during cargo build via a rustc
wrapper. The graph lives in `analysis/` as immutable source of truth.
A background engine reads it and writes derived outputs to `analysis/post_analysis/`.

Two structural gaps need fixing today:

### Gap 1 — No schema version in analysis/
`analysis/metadata.json` exists but has no schema version field.
If `nodes.csv`, `edges.csv`, or `files.txt` format changes, consumers
(analysis-engine, agents) silently read corrupt data.
We need a `schema_version` integer in `metadata.json`, written by the
UPG extractor and validated by the engine before loading.
Engine must hard-fail if schema version is missing or mismatched.

### Gap 2 — Duplicated parse_edge_kind
`parse_edge_kind` exists independently in:
- `canon-utils/analysis-engine/src/loader.rs`
- `canon-utils/analysis-engine/src/augment.rs`
They can drift. When a new EdgeKind is added, both must be updated manually
(we hit this bug today). There must be one canonical parse table.

Follow AGENT.md invariant-first procedure throughout.
Use analysis/ as source of truth before reading source files.

## Files to Review First

```bash
# Schema version gap
python3 -c "import json; d=json.load(open('canon-utils/analysis-engine/analysis/metadata.json')); print(d)"
rg 'metadata|schema' canon-utils/upg_analysis/src/types.rs canon-utils/upg_analysis/src/emit.rs -n
rg 'Metadata' canon-utils/analysis-engine/src/loader.rs -n -A 8

# EdgeKind duplication gap
rg 'fn parse_edge_kind|fn edge_kind_str' canon-utils/analysis-engine/src/ -n
rg 'enum EdgeKind' canon-utils/analysis-engine/src/loader.rs -A 25
```

## Key Files
- `canon-utils/upg_analysis/src/types.rs` — Metadata struct definition (extractor side)
- `canon-utils/upg_analysis/src/emit.rs` — writes metadata.json
- `canon-utils/analysis-engine/src/loader.rs` — reads metadata.json, parse_edge_kind #1
- `canon-utils/analysis-engine/src/augment.rs` — parse_edge_kind #2, edge_kind_str
- `canon-utils/analysis-engine/src/main.rs` — engine entry point, schema validation goes here
- `analysis/metadata.json` — current schema (no version field)
- `AGENT.md` — invariant protocol, must be followed
