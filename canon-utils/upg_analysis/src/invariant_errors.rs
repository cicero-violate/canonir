#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvariantError {
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
}

impl InvariantError {
    pub fn as_str(&self) -> &'static str {
        match self {
            InvariantError::must_have_contiguous_node_ids => "must_have_contiguous_node_ids",
            InvariantError::must_have_valid_edge_sources => "must_have_valid_edge_sources",
            InvariantError::must_have_valid_edge_destinations => "must_have_valid_edge_destinations",
            InvariantError::must_have_valid_node_kinds => "must_have_valid_node_kinds",
            InvariantError::must_have_valid_edge_kinds => "must_have_valid_edge_kinds",
            InvariantError::must_have_valid_file_ids => "must_have_valid_file_ids",
            InvariantError::must_have_basic_blocks_with_owner => "must_have_basic_blocks_with_owner",
            InvariantError::must_have_callsites_with_owner => "must_have_callsites_with_owner",
            InvariantError::must_have_callsites_with_incoming_edges => "must_have_callsites_with_incoming_edges",
            InvariantError::must_not_have_isolated_nodes => "must_not_have_isolated_nodes",
            InvariantError::must_have_single_module_root => "must_have_single_module_root",
            InvariantError::must_have_module_exports_from_modules => "must_have_module_exports_from_modules",
            InvariantError::must_have_unique_symbol_kind => "must_have_unique_symbol_kind",
            InvariantError::must_have_unique_symbol_kind_per_module => "must_have_unique_symbol_kind_per_module",
            InvariantError::must_have_module_owner => "must_have_module_owner",
            InvariantError::must_have_module_file_mapping => "must_have_module_file_mapping",
            InvariantError::must_have_ordered_spans => "must_have_ordered_spans",
            InvariantError::must_have_consistent_span_file => "must_have_consistent_span_file",
            InvariantError::must_have_connected_function_cfg => "must_have_connected_function_cfg",
            InvariantError::must_not_have_orphan_files => "must_not_have_orphan_files",
            InvariantError::must_have_entry_roots_in_files => "must_have_entry_roots_in_files",
            InvariantError::must_have_files_within_project_root => "must_have_files_within_project_root",
        }
    }
}

impl std::fmt::Display for InvariantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for InvariantError {}
