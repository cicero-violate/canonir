use canon::ir::CanonIR;

#[derive(Debug, Default)]
pub struct ProjectionValidationReport {
    pub errors: Vec<String>,
}

pub fn compute_projection_validation(ir: &CanonIR) -> ProjectionValidationReport {
    let mut report = ProjectionValidationReport::default();

    // invariant: node ids must be unique
    let mut seen = std::collections::HashSet::new();
    for node in ir.nodes.iter() {
        if !seen.insert(node.id) {
            report.errors.push(format!("duplicate NodeId detected: {:?}", node.id));
        }
    }

    // invariant: modules must have valid parents
    for node in ir.nodes.iter() {
        if let Some(parent) = node.parent {
            if !ir.nodes.iter().any(|n| n.id == parent) {
                report.errors.push(format!("missing parent module for node {:?}", node.id));
            }
        }
    }

    report
}
