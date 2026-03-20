use canon_event_store::{read_any_events_from_path, AnyEvent};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Default, Clone)]
struct FieldInfo {
    types: BTreeSet<String>,
    examples: BTreeSet<String>,
    array_item_types: BTreeSet<String>,
    present: u64,
}

#[derive(Default)]
struct SchemaState {
    variants_by_kind: BTreeMap<String, BTreeMap<String, BTreeMap<String, FieldInfo>>>,
    variant_counts: BTreeMap<String, BTreeMap<String, u64>>,
    kind_counts: BTreeMap<String, u64>,
    total_objects: u64,
}

pub fn write_event_schema_report(tlog_path: &Path, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut state = SchemaState::default();
    let events = read_any_events_from_path(tlog_path)?;
    for event in events {
        if let AnyEvent::Canon(canon) = event {
            ingest(&mut state, &canon.kind, &canon.payload);
        }
    }
    let text = render(&state);
    std::fs::create_dir_all(out_dir)?;
    let path = out_dir.join("event_schema.txt");
    std::fs::write(&path, text)?;
    Ok(path)
}

fn ingest(state: &mut SchemaState, kind: &str, payload: &Value) {
    state.total_objects = state.total_objects.saturating_add(1);
    *state.kind_counts.entry(kind.to_string()).or_insert(0) += 1;

    let (variant, body) = match payload {
        Value::Object(map) if map.len() == 1 => {
            let (k, v) = map.iter().next().unwrap();
            (k.as_str(), v)
        }
        Value::Object(map) if map.is_empty() => ("__empty__", payload),
        _ => ("__flat__", payload),
    };

    *state.variant_counts.entry(kind.to_string()).or_default().entry(variant.to_string()).or_insert(0) += 1;

    let shape = state.variants_by_kind.entry(kind.to_string()).or_default().entry(variant.to_string()).or_default();
    infer_shape(body, shape, "");
}

fn infer_shape(value: &Value, shape: &mut BTreeMap<String, FieldInfo>, prefix: &str) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let path = if prefix.is_empty() { k.to_string() } else { format!("{prefix}.{k}") };
                let entry = shape.entry(path.clone()).or_default();
                entry.types.insert(classify(v));
                entry.present = entry.present.saturating_add(1);
                add_example(entry, v);
                if let Value::Array(items) = v {
                    for item in items {
                        entry.array_item_types.insert(classify(item));
                    }
                } else if v.is_object() {
                    infer_shape(v, shape, &path);
                }
            }
        }
        _ => {
            let key = if prefix.is_empty() { "_value" } else { prefix };
            let entry = shape.entry(key.to_string()).or_default();
            entry.types.insert(classify(value));
            entry.present = entry.present.saturating_add(1);
            add_example(entry, value);
            if let Value::Array(items) = value {
                for item in items {
                    entry.array_item_types.insert(classify(item));
                }
            }
        }
    }
}

fn classify(value: &Value) -> String {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
    .to_string()
}

fn add_example(field: &mut FieldInfo, value: &Value) {
    if field.examples.len() >= 5 {
        return;
    }
    let s = match value {
        Value::String(v) => format!("'{}'", truncate(v, 50)),
        Value::Bool(v) => format!("{v}"),
        Value::Number(v) => v.to_string(),
        Value::Null => "null".to_string(),
        _ => return,
    };
    field.examples.insert(s);
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

fn presence_card(present: u64, total: u64) -> &'static str {
    if total == 0 {
        return "   ";
    }
    let f = present as f64 / total as f64;
    if f >= 0.98 {
        " * "
    } else if f >= 0.75 {
        " + "
    } else if f >= 0.30 {
        " ~ "
    } else {
        " - "
    }
}

fn render(state: &SchemaState) -> String {
    let mut out = String::new();
    out.push_str("══════════════════════════════════════════════════════════════════════\n");
    out.push_str(&format!("  Event Schema  ·  {} events  ·  {} kind(s)\n", format_u64(state.total_objects), state.kind_counts.len()));
    out.push_str("  Model: Schema = Map(kind → Map(variant → shape))\n");
    out.push_str("  Rule:  |keys(payload)| = 1  →  variant = that key\n");
    out.push_str("══════════════════════════════════════════════════════════════════════\n\n");

    if state.total_objects == 0 {
        out.push_str("  (waiting for data…)\n");
        return out;
    }

    for (kind, k_total) in &state.kind_counts {
        out.push_str(&format!("  ┌─ {}  ({})\n", kind, format_u64(*k_total)));
        let variants = state.variant_counts.get(kind).cloned().unwrap_or_default();
        let mut v_items: Vec<(String, u64)> = variants.into_iter().collect();
        v_items.sort_by(|a, b| b.1.cmp(&a.1));
        for (vi, (vname, vcount)) in v_items.iter().enumerate() {
            let is_last_v = vi + 1 == v_items.len();
            let v_branch = if is_last_v { "└──" } else { "├──" };
            out.push_str(&format!("  │  {v_branch} {vname}  ({})\n", format_u64(*vcount)));
            let inner = if is_last_v { "       " } else { "   │   " };
            let shape = state.variants_by_kind.get(kind).and_then(|v| v.get(vname)).cloned().unwrap_or_default();
            let mut fields: Vec<(String, FieldInfo)> = shape.into_iter().collect();
            fields.sort_by(|a, b| b.1.present.cmp(&a.1.present).then_with(|| a.0.cmp(&b.0)));
            for (fi, (fname, info)) in fields.iter().enumerate() {
                let is_last_f = fi + 1 == fields.len();
                let f_branch = if is_last_f { "└──" } else { "├──" };
                let t = info.types.iter().cloned().collect::<Vec<_>>().join(",");
                let arr = if info.array_item_types.is_empty() { String::new() } else { format!(" → [{}]", info.array_item_types.iter().cloned().collect::<Vec<_>>().join(",")) };
                let ex = if info.examples.is_empty() { String::new() } else { format!("  ex: {}", info.examples.iter().cloned().collect::<Vec<_>>().join(", ")) };
                let req = presence_card(info.present, *vcount);
                out.push_str(&format!("  │  {inner}{f_branch} {fname}{req}{t}{arr}{ex}\n"));
            }
        }
        out.push_str("  └──────────────────────────────────────────────────\n\n");
    }
    out
}

fn format_u64(value: u64) -> String {
    let s = value.to_string();
    let mut out = String::new();
    let mut count = 0usize;
    for ch in s.chars().rev() {
        if count == 3 {
            out.push(',');
            count = 0;
        }
        out.push(ch);
        count += 1;
    }
    out.chars().rev().collect()
}
