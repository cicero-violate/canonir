use serde::{Deserialize, Serialize};

use crate::invariant_errors::InvariantError;

pub type InvariantFn = fn(&InvariantReport) -> Result<(), InvariantError>;

#[derive(Debug, Serialize, Deserialize)]
pub struct InvariantReport {
    pub generated_at_epoch_ms: u128,
    pub ok: bool,
    pub node_count: usize,
    pub spans_count: usize,
    pub defs_count: usize,
    pub missing_def_nodes: usize,
    pub spans_match_nodes: bool,
    pub span_ids_match_nodes: bool,
    pub edges_with_missing_src: usize,
    pub edges_with_missing_dst: usize,
    pub invalid_node_kinds: usize,
    pub invalid_edge_kinds: usize,
    pub edge_kind_mismatch: usize,
    pub bad_file_id_nodes: usize,
    pub bb_without_has_block: usize,
    pub call_without_has_block: usize,
    pub callsite_no_incoming: usize,
    pub isolated_nodes: usize,
    pub module_count: usize,
    pub module_root_like: usize,
    pub export_src_not_module: usize,
    pub duplicate_symbol_kind: usize,
    pub duplicate_symbol_kind_module: usize,
    pub missing_module_owner: usize,
    pub span_order_violations: usize,
    pub span_file_mismatch: usize,
    pub span_file_inconsistent: usize,
    pub function_cfg_disconnected: usize,
    pub orphan_files: usize,
    pub missing_entry_roots: usize,
    pub files_outside_project_root: usize,
    pub must_have_contiguous_node_ids: bool,
    pub must_have_valid_edge_sources: bool,
    pub must_have_valid_edge_destinations: bool,
    pub must_have_valid_node_kinds: bool,
    pub must_have_valid_edge_kinds: bool,
    pub must_have_valid_file_ids: bool,
    pub must_have_basic_blocks_with_owner: bool,
    pub must_have_callsites_with_owner: bool,
    pub must_have_callsites_with_incoming_edges: bool,
    pub must_not_have_isolated_nodes: bool,
    pub must_have_single_module_root: bool,
    pub must_have_module_exports_from_modules: bool,
    pub must_have_unique_symbol_kind: bool,
    pub must_have_unique_symbol_kind_per_module: bool,
    pub must_have_module_owner: bool,
    pub must_have_module_file_mapping: bool,
    pub must_have_ordered_spans: bool,
    pub must_have_consistent_span_file: bool,
    pub must_have_connected_function_cfg: bool,
    pub must_not_have_orphan_files: bool,
    pub must_have_entry_roots_in_files: bool,
    pub must_have_files_within_project_root: bool,
}

impl InvariantReport {
    pub fn summary(&self) -> String {
        format!(
            "nodes={} spans={} defs={} missing_defs={} missing_edges(src={}, dst={}) invalid_kinds(node={}, edge={}) edge_kind_mismatch={} bad_file_id={} bb_no_block={} call_no_block={} callsite_no_incoming={} isolated={} module_root_like={} export_src_not_module={} dup_symbol_kind={} dup_symbol_kind_module={} missing_module_owner={} span_order_violations={} span_file_mismatch={} span_file_inconsistent={} function_cfg_disconnected={} orphan_files={} missing_entry_roots={} files_outside_project_root={}",
            self.node_count,
            self.spans_count,
            self.defs_count,
            self.missing_def_nodes,
            self.edges_with_missing_src,
            self.edges_with_missing_dst,
            self.invalid_node_kinds,
            self.invalid_edge_kinds,
            self.edge_kind_mismatch,
            self.bad_file_id_nodes,
            self.bb_without_has_block,
            self.call_without_has_block,
            self.callsite_no_incoming,
            self.isolated_nodes,
            self.module_root_like,
            self.export_src_not_module,
            self.duplicate_symbol_kind,
            self.duplicate_symbol_kind_module,
            self.missing_module_owner,
            self.span_order_violations,
            self.span_file_mismatch,
            self.span_file_inconsistent,
            self.function_cfg_disconnected,
            self.orphan_files,
            self.missing_entry_roots,
            self.files_outside_project_root
        )
    }
}

pub fn must_have_contiguous_node_ids(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_contiguous_node_ids {
        Ok(())
    } else {
        Err(InvariantError::must_have_contiguous_node_ids)
    }
}

pub fn must_have_valid_edge_sources(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_valid_edge_sources {
        Ok(())
    } else {
        Err(InvariantError::must_have_valid_edge_sources)
    }
}

pub fn must_have_valid_edge_destinations(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_valid_edge_destinations {
        Ok(())
    } else {
        Err(InvariantError::must_have_valid_edge_destinations)
    }
}

pub fn must_have_valid_node_kinds(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_valid_node_kinds {
        Ok(())
    } else {
        Err(InvariantError::must_have_valid_node_kinds)
    }
}

pub fn must_have_valid_edge_kinds(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_valid_edge_kinds {
        Ok(())
    } else {
        Err(InvariantError::must_have_valid_edge_kinds)
    }
}

pub fn must_have_valid_file_ids(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_valid_file_ids {
        Ok(())
    } else {
        Err(InvariantError::must_have_valid_file_ids)
    }
}

pub fn must_have_basic_blocks_with_owner(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_basic_blocks_with_owner {
        Ok(())
    } else {
        Err(InvariantError::must_have_basic_blocks_with_owner)
    }
}

pub fn must_have_callsites_with_owner(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_callsites_with_owner {
        Ok(())
    } else {
        Err(InvariantError::must_have_callsites_with_owner)
    }
}

pub fn must_have_callsites_with_incoming_edges(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_callsites_with_incoming_edges {
        Ok(())
    } else {
        Err(InvariantError::must_have_callsites_with_incoming_edges)
    }
}

pub fn must_not_have_isolated_nodes(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_not_have_isolated_nodes {
        Ok(())
    } else {
        Err(InvariantError::must_not_have_isolated_nodes)
    }
}

pub fn must_have_single_module_root(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_single_module_root {
        Ok(())
    } else {
        Err(InvariantError::must_have_single_module_root)
    }
}

pub fn must_have_module_exports_from_modules(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_module_exports_from_modules {
        Ok(())
    } else {
        Err(InvariantError::must_have_module_exports_from_modules)
    }
}

pub fn must_have_unique_symbol_kind(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_unique_symbol_kind {
        Ok(())
    } else {
        Err(InvariantError::must_have_unique_symbol_kind)
    }
}

pub fn must_have_unique_symbol_kind_per_module(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_unique_symbol_kind_per_module {
        Ok(())
    } else {
        Err(InvariantError::must_have_unique_symbol_kind_per_module)
    }
}

pub fn must_have_module_owner(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_module_owner {
        Ok(())
    } else {
        Err(InvariantError::must_have_module_owner)
    }
}

pub fn must_have_module_file_mapping(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_module_file_mapping {
        Ok(())
    } else {
        Err(InvariantError::must_have_module_file_mapping)
    }
}

pub fn must_have_ordered_spans(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_ordered_spans {
        Ok(())
    } else {
        Err(InvariantError::must_have_ordered_spans)
    }
}

pub fn must_have_consistent_span_file(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_consistent_span_file {
        Ok(())
    } else {
        Err(InvariantError::must_have_consistent_span_file)
    }
}

pub fn must_have_connected_function_cfg(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_connected_function_cfg {
        Ok(())
    } else {
        Err(InvariantError::must_have_connected_function_cfg)
    }
}

pub fn must_not_have_orphan_files(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_not_have_orphan_files {
        Ok(())
    } else {
        Err(InvariantError::must_not_have_orphan_files)
    }
}

pub fn must_have_entry_roots_in_files(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_entry_roots_in_files {
        Ok(())
    } else {
        Err(InvariantError::must_have_entry_roots_in_files)
    }
}

pub fn must_have_files_within_project_root(report: &InvariantReport) -> Result<(), InvariantError> {
    if report.must_have_files_within_project_root {
        Ok(())
    } else {
        Err(InvariantError::must_have_files_within_project_root)
    }
}

pub const INVARIANTS: &[InvariantFn] = &[
    must_have_contiguous_node_ids,
    must_have_valid_edge_sources,
    must_have_valid_edge_destinations,
    must_have_valid_node_kinds,
    must_have_valid_edge_kinds,
    must_have_valid_file_ids,
    must_have_basic_blocks_with_owner,
    must_have_callsites_with_owner,
    must_have_callsites_with_incoming_edges,
    must_not_have_isolated_nodes,
    must_have_single_module_root,
    must_have_module_exports_from_modules,
    must_have_unique_symbol_kind,
    must_have_unique_symbol_kind_per_module,
    must_have_module_owner,
    must_have_module_file_mapping,
    must_have_ordered_spans,
    must_have_consistent_span_file,
    must_have_connected_function_cfg,
    must_not_have_orphan_files,
    must_have_entry_roots_in_files,
    must_have_files_within_project_root,
];
