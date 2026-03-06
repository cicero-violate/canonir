use crate::solver::constraint_solver::build_problem;
use anyhow::Result;
use canon::node::CanonNodeKind;
use canon::CanonIR;

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let problem = build_problem(ir);
    if problem.transforms.is_empty() {
        return Ok(());
    }

    let cyclic: Vec<&Vec<usize>> = problem.sccs.iter().filter(|c| c.len() > 1).collect();
    if !cyclic.is_empty() {
        for component in cyclic {
            let names: Vec<String> = component
                .iter()
                .filter_map(|&vid| problem.transforms.get(vid))
                .map(|t| format!("{}->{}", t.old_name, t.new_name))
                .collect();
            emit_diag(
                ir,
                &format!("graph_dependency_solver:cycle {}", names.join(" | ")),
            );
        }
    } else {
        emit_diag(
            ir,
            &format!(
                "graph_dependency_solver:ok vars={} order={}",
                problem.transforms.len(),
                problem.topo_order.len()
            ),
        );
    }

    Ok(())
}

fn emit_diag(ir: &mut CanonIR, label: &str) {
    let name_id = ir.intern_name(label);
    let id = ir.push_node(CanonNodeKind::TypeRef { name_id });
    ir.emit_order.push(id);
}
