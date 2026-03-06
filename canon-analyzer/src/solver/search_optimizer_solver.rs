use crate::solver::constraint_solver::{build_problem, is_selected_vector_valid, RenameConstraintProblem};
use algorithms::optimization::beam_search::beam_search;
use anyhow::Result;
use canon::node::CanonNodeKind;
use canon::CanonIR;

const DEFAULT_BEAM_WIDTH: usize = 32;

#[derive(Clone)]
struct SearchState {
    pos: usize,
    selected: Vec<bool>,
    score: i64,
}

pub fn solve(ir: &mut CanonIR) -> Result<()> {
    let problem = build_problem(ir);
    if problem.transforms.is_empty() {
        return Ok(());
    }
    let best = optimize(&problem, DEFAULT_BEAM_WIDTH);
    emit_diag(
        ir,
        &format!(
            "search_optimizer:selected={}/{} beam={}",
            best.len(),
            problem.transforms.len(),
            DEFAULT_BEAM_WIDTH
        ),
    );
    Ok(())
}

pub fn optimize(problem: &RenameConstraintProblem, beam_width: usize) -> Vec<usize> {
    let n = problem.transforms.len();
    if n == 0 {
        return Vec::new();
    }
    let order = if problem.topo_order.len() == n {
        problem.topo_order.clone()
    } else {
        (0..n).collect()
    };

    let conflict_degree = build_conflict_degree(problem, n);
    let dep_out_degree = build_dep_out_degree(problem, n);

    let initial = SearchState {
        pos: 0,
        selected: vec![false; n],
        score: 0,
    };

    let best = beam_search(
        vec![initial],
        beam_width,
        n,
        |state, _level| {
            if state.pos >= n {
                return vec![state.clone()];
            }
            let idx = order[state.pos];
            let mut out = Vec::with_capacity(2);

            let mut skip = state.clone();
            skip.pos += 1;
            out.push(skip);

            let mut take = state.clone();
            take.selected[idx] = true;
            take.pos += 1;
            if is_selected_vector_valid(problem, &take.selected) {
                let quality = 100 - (conflict_degree[idx] as i64 * 10) - (dep_out_degree[idx] as i64 * 5);
                take.score += quality.max(1);
                out.push(take);
            }
            out
        },
        |state| state.score,
    );

    best.map(selected_indices).unwrap_or_default()
}

fn selected_indices(state: SearchState) -> Vec<usize> {
    state
        .selected
        .iter()
        .enumerate()
        .filter_map(|(i, &v)| if v { Some(i) } else { None })
        .collect()
}

fn build_conflict_degree(problem: &RenameConstraintProblem, n: usize) -> Vec<usize> {
    let mut out = vec![0usize; n];
    for &(a, b) in &problem.conflicts {
        out[a] += 1;
        out[b] += 1;
    }
    out
}

fn build_dep_out_degree(problem: &RenameConstraintProblem, n: usize) -> Vec<usize> {
    let mut out = vec![0usize; n];
    for &(a, _) in &problem.dependencies {
        out[a] += 1;
    }
    out
}

fn emit_diag(ir: &mut CanonIR, label: &str) {
    let name_id = ir.intern_name(label);
    let id = ir.push_node(CanonNodeKind::TypeRef { name_id });
    ir.emit_order.push(id);
}
