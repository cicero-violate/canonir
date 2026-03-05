use crate::helpers::dependency_graph::{compute_dependency_graph, compute_emit_order};
use crate::helpers::module_normalization::normalize_module_tree;
use crate::helpers::emit_plan::compute_emit_plan;
use crate::helpers::structural_validation::validate_emitted_rust_structure;

/// High-level deterministic emit pipeline.
/// All steps are pure transformations except the final filesystem write step
/// which occurs in the outer orchestration layer.
pub fn compute_validated_emit_plan(
    crate_name: &str,
    items: &[(String, Vec<String>)],
    modules: &[Vec<String>],
) -> Result<Vec<(String, String)>, String> {
    // 1. Normalize modules
    let normalized_modules = normalize_module_tree(modules);

    // 2. Build dependency graph
    let graph = compute_dependency_graph(items);

    // 3. Compute deterministic order
    let order = compute_emit_order(&graph);

    // 4. Validate structure
    let item_ids: Vec<String> = items.iter().map(|(id, _)| id.clone()).collect();
    let errors = validate_emitted_rust_structure(&item_ids, &graph);

    if !errors.is_empty() {
        return Err(format!("Structural validation failed: {:?}", errors));
    }

    // 5. Prepare ordered items with module assignment
    let ordered_items: Vec<(String, Vec<String>)> = order
        .into_iter()
        .map(|id| {
            let module = normalized_modules.get(0).cloned().unwrap_or_default();
            (id, module)
        })
        .collect();

    // 6. Compute emission plan
    let plan = compute_emit_plan(crate_name, &ordered_items);

    // Convert to simplified output
    Ok(plan.into_iter().map(|e| (e.item, e.path)).collect())
}
