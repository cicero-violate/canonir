use anyhow::{bail, Result};
use canon_ir::ir::CanonIR;
use canon_ir::node::{CanonNodeKind, CfgOp};
use canon_ir::node::flags;

pub fn validate(canon: &CanonIR) -> Result<()> {
    validate_no_alloc_artifacts(canon)?;
    validate_no_malformed_paths(canon)?;
    validate_global_csr(canon)?;
    validate_cfg_switch_targets(canon)?;
    validate_graph_integrity(canon)?;
    validate_module_ownership(canon)?;
    validate_fn_bodies(canon)?;
    Ok(())
}

fn validate_no_alloc_artifacts(canon: &CanonIR) -> Result<()> {
    let mut bad_count = 0usize;
    let mut examples: Vec<String> = Vec::new();
    for s in &canon.name_intern.vec {
        if is_alloc_artifact_name(s) {
            bad_count += 1;
            if examples.len() < 5 {
                examples.push(s.clone());
            }
        }
    }
    if bad_count > 0 {
        let sample = examples.join(", ");
        bail!(
            "Invariant violation: MIR alloc/debug artifact leaked into Canon name interner (count={bad_count}) examples=[{sample}]"
        );
    }
    Ok(())
}

fn is_alloc_artifact_name(name: &str) -> bool {
    matches!(
        name,
        "__rust_alloc"
            | "__rust_dealloc"
            | "__rust_realloc"
            | "__rust_alloc_zeroed"
            | "__rust_alloc_error_handler"
            | "__rdl_alloc"
            | "__rdl_dealloc"
            | "__rdl_realloc"
            | "__rdl_alloc_zeroed"
    ) || name.contains("{{")
        || name.contains('\0')
        || (name.starts_with("__") && name.contains("alloc"))
        || name.contains("promoted[")
        || name.contains("{alloc")
}

fn validate_no_malformed_paths(canon: &CanonIR) -> Result<()> {
    let mut bad_count = 0usize;
    let mut examples: Vec<String> = Vec::new();
    for p in &canon.path_intern.vec {
        if p.split("::").any(|seg| seg.is_empty() || seg == "_" || seg.starts_with('_')) {
            bad_count += 1;
            if examples.len() < 5 {
                examples.push(p.clone());
            }
        }
    }
    if bad_count > 0 {
        let sample = examples.join(", ");
        bail!(
            "Invariant violation: malformed/private helper path segment in Canon path interner (count={bad_count}) examples=[{sample}]"
        );
    }
    Ok(())
}

fn validate_global_csr(canon: &CanonIR) -> Result<()> {
    if let Err(msg) = canon.validate_global_csr() {
        bail!("Invariant violation: {msg}");
    }
    Ok(())
}

fn validate_cfg_switch_targets(canon: &CanonIR) -> Result<()> {
    for node in &canon.nodes {
        let CanonNodeKind::Body { blocks } = &node.kind else {
            continue;
        };
        let block_count = blocks.len() as u32;
        for (idx, block_id) in blocks.iter().enumerate() {
            let block = canon.node(*block_id);
            let CanonNodeKind::BasicBlock { ops, .. } = &block.kind else {
                bail!(
                    "Invariant violation: body block list references non-BasicBlock node body_index={idx} block_id={:?}",
                    block_id
                );
            };
            for op in ops {
                if let CfgOp::Switch { targets, otherwise, .. } = op {
                    for (value_id, target) in targets {
                        if *target >= block_count {
                            bail!(
                                "Invariant violation: switch target out of bounds block_index={idx} value_local={:?} target={target} block_count={block_count}",
                                value_id
                            );
                        }
                    }
                    if let Some(target) = otherwise {
                        if *target >= block_count {
                            bail!(
                                "Invariant violation: switch otherwise out of bounds block_index={idx} target={target} block_count={block_count}"
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_graph_integrity(canon: &CanonIR) -> Result<()> {
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for node in &canon.nodes {
        if !seen.insert(node.id.0) {
            bail!("Invariant violation: duplicate CanonId detected id={}", node.id.0);
        }
    }
    let node_count = canon.nodes.len() as u32;
    let graphs = [
        ("name_graph", &canon.name_graph),
        ("type_graph", &canon.type_graph),
        ("call_graph", &canon.call_graph),
        ("module_graph", &canon.module_graph),
        ("cfg_graph", &canon.cfg_graph),
        ("region_graph", &canon.region_graph),
        ("value_graph", &canon.value_graph),
        ("macro_graph", &canon.macro_graph),
    ];
    for (label, graph) in graphs {
        if graph.node_data.len() as u32 != node_count {
            bail!("Invariant violation: {label} node_data length mismatch node_count={node_count} node_data={}", graph.node_data.len());
        }
        if graph.row_ptr.len() != graph.node_data.len() + 1 {
            bail!("Invariant violation: {label} row_ptr length mismatch row_ptr={} node_data={}", graph.row_ptr.len(), graph.node_data.len());
        }
        if graph.col_idx.len() != graph.edge_data.len() {
            bail!("Invariant violation: {label} edge payload length mismatch col_idx={} edge_data={}", graph.col_idx.len(), graph.edge_data.len());
        }
        for &dst in &graph.col_idx {
            if dst >= node_count {
                bail!("Invariant violation: {label} edge dst out of bounds dst={dst} node_count={node_count}");
            }
        }
    }
    Ok(())
}

fn validate_fn_bodies(canon: &CanonIR) -> Result<()> {
    for node in &canon.nodes {
        if let CanonNodeKind::Fn { body, flags: f, .. } = &node.kind {
            if body.is_none() && (f & flags::EXTERN == 0) {
                bail!("Invariant violation: function missing body without EXTERN flag node_id={:?}", node.id);
            }
            if let Some(body_id) = body {
                let body_node = canon.node(*body_id);
                if !matches!(body_node.kind, CanonNodeKind::Body { .. }) {
                    bail!("Invariant violation: function body points to non-Body node fn_id={:?} body_id={:?}", node.id, body_id);
                }
            }
        }
    }
    Ok(())
}

fn validate_module_ownership(canon: &CanonIR) -> Result<()> {
    let node_count = canon.nodes.len();
    let mut has_parent = vec![false; node_count];
    for src_idx in 0..canon.module_graph.node_data.len() {
        let start = canon.module_graph.row_ptr[src_idx] as usize;
        let end = canon.module_graph.row_ptr[src_idx + 1] as usize;
        for edge_idx in start..end {
            if canon.module_graph.edge_data[edge_idx] != canon_ir::edge::EdgeKind::Contains {
                continue;
            }
            let dst_idx = canon.module_graph.col_idx[edge_idx] as usize;
            if dst_idx < has_parent.len() {
                has_parent[dst_idx] = true;
            }
        }
    }
    for (idx, node) in canon.nodes.iter().enumerate() {
        if matches!(node.kind, CanonNodeKind::Module { .. }) && !has_parent[idx] {
            bail!("Invariant violation: module missing parent contains edge node_id={:?}", node.id);
        }
    }
    Ok(())
}
