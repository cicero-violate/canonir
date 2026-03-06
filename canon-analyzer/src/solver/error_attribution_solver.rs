use crate::solver::constraint_solver::{build_problem, is_subset_valid};
use crate::solver::search_optimizer_solver::optimize;
use algorithms::optimization::delta_debug::ddmin;
use anyhow::Result;
use canon::node::CanonNodeKind;
use canon::CanonIR;
use std::collections::HashSet;

const MAX_ATTRIBUTIONS: usize = 8;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let problem = build_problem(ir);
    if problem.transforms.is_empty() {
        return Ok(());
    }

    let selected = optimize(&problem, 32);
    let selected_set: HashSet<usize> = selected.iter().copied().collect();
    let rejected: Vec<usize> = (0..problem.transforms.len())
        .filter(|i| !selected_set.contains(i))
        .collect();

    let mut emitted = 0usize;
    for rid in rejected.into_iter().take(MAX_ATTRIBUTIONS) {
        let transform = &problem.transforms[rid];
        let self_invalid = !is_subset_valid(&problem, &[rid]);
        let blockers = if self_invalid {
            Vec::new()
        } else {
            ddmin(&selected, |subset| {
                let mut trial = subset.to_vec();
                trial.push(rid);
                !is_subset_valid(&problem, &trial)
            })
        };

        let reason = if self_invalid {
            "self_invalid"
        } else if blockers.is_empty() {
            "heuristic_rejected"
        } else {
            "blocked_by_constraints"
        };

        let mut details = format!(
            "error_attribution:{} {}->{}",
            reason, transform.old_name, transform.new_name
        );
        if !blockers.is_empty() {
            let labels: Vec<String> = blockers
                .iter()
                .filter_map(|&bid| problem.transforms.get(bid))
                .map(|t| format!("{}->{}", t.old_name, t.new_name))
                .collect();
            details.push_str(&format!(" blockers=[{}]", labels.join(", ")));
        }
        emit_diag(ir, &details);
        emitted += 1;
    }

    emit_diag(ir, &format!("error_attribution_solver:emitted={}", emitted));
    Ok(())
}

fn emit_diag(ir: &mut CanonIR, label: &str) {
    let name_id = ir.intern_name(label);
    let id = ir.push_node(CanonNodeKind::TypeRef { name_id });
    ir.emit_order.push(id);
}
